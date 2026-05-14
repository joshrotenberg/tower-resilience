//! Tower `Service` contract regression for `Hedge`.
//!
//! Hedge fans a single request out to N parallel attempts. The primary uses
//! the caller-readied receiver (correct). Each hedge attempt operates on a
//! fresh `inner.clone()` whose readiness is not inherited from the original
//! (per `tower::limit::ConcurrencyLimit` etc.) and therefore must drive its
//! own `poll_ready` before calling. See #293.
//!
//! The probe below has a `Clone` that resets readiness and a `call` that
//! asserts the inner saw a `poll_ready` since its last `Clone`/call. The
//! primary sleeps long enough for hedges to fire; without the fix the
//! hedge spawn calls into a fresh clone whose `ready` is `false`, and the
//! assert panics.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for related
//! contract tests on simpler layer middleware.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_hedge::HedgeLayer;

/// An inner [`Service`] whose [`Clone`] resets readiness, and whose first
/// call sleeps long enough for hedge fan-out to fire (so the hedge spawns
/// actually issue calls against fresh clones).
struct StatefulSlowFirst {
    ready: bool,
    calls: Arc<AtomicUsize>,
}

impl StatefulSlowFirst {
    fn new() -> Self {
        Self {
            ready: false,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Clone for StatefulSlowFirst {
    fn clone(&self) -> Self {
        Self {
            ready: false,
            calls: Arc::clone(&self.calls),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProbeError;

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProbeError")
    }
}

impl std::error::Error for ProbeError {}

impl Service<()> for StatefulSlowFirst {
    type Response = ();
    type Error = ProbeError;
    type Future = Pin<Box<dyn Future<Output = Result<(), ProbeError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.ready = true;
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        assert!(
            self.ready,
            "Service::call invoked without prior poll_ready -- tower contract violation (#293)"
        );
        self.ready = false;
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if n == 0 {
                // Primary sleeps so the hedge has time to fire.
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Ok(())
        })
    }
}

// Bounded with `tokio::time::timeout` because a contract regression panics
// inside a `tokio::spawn`'d hedge task -- without the timeout the main test
// would hang waiting for a channel message that never arrives.

#[tokio::test]
async fn hedge_drives_readied_instance_on_attempts() {
    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(20))
        .max_hedged_attempts(2)
        .build();
    let mut svc = layer.layer(StatefulSlowFirst::new());

    let _ = tokio::time::timeout(Duration::from_secs(2), svc.ready().await.unwrap().call(()))
        .await
        .expect("hedge call hung -- likely contract regression in a spawned task");
}

#[tokio::test]
async fn hedge_parallel_mode_drives_readied_instance_on_attempts() {
    let layer = HedgeLayer::builder()
        .no_delay()
        .max_hedged_attempts(3)
        .build();
    let mut svc = layer.layer(StatefulSlowFirst::new());

    let _ = tokio::time::timeout(Duration::from_secs(2), svc.ready().await.unwrap().call(()))
        .await
        .expect("hedge call hung -- likely contract regression in a spawned task");
}
