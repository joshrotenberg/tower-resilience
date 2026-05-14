//! Tower `Service` contract regression for `ReconnectService`.
//!
//! Reconnect's retry path stores `self.inner.clone()` inside the
//! `ReconnectFuture` and calls it again after a backoff. The clone's
//! readiness is *not* inherited from the readied original (per the standard
//! stateful-service contract -- see `tower::limit::ConcurrencyLimit`), so
//! every retry must drive `poll_ready` on the stored clone before the
//! retry's `call`. Otherwise the contract is violated against the inner.
//!
//! The first call uses the caller-readied receiver (correct). The probe
//! below fails that first call to push the future into the retry path, then
//! succeeds on the retried call. The probe's `call` asserts that the inner
//! saw a `poll_ready` since its last `Clone`/call -- without the fix this
//! test panics with the same "tower contract violation" message used by the
//! `StatefulInner` probe in #286.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for related
//! tests that exercise the same contract on layer middleware.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tower::{Layer, Service};
use tower_resilience_reconnect::{ReconnectConfig, ReconnectLayer, ReconnectPolicy};

/// An inner [`Service`] whose [`Clone`] resets readiness and which fails the
/// first call (forcing the retry path).
#[derive(Default)]
struct StatefulFailFirst {
    ready: bool,
    calls: Arc<AtomicUsize>,
}

impl StatefulFailFirst {
    fn new() -> Self {
        Self {
            ready: false,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Clone for StatefulFailFirst {
    fn clone(&self) -> Self {
        // Mirrors `Clone for ConcurrencyLimit` / `Buffer`: the new instance
        // does not inherit readiness from the original.
        Self {
            ready: false,
            calls: Arc::clone(&self.calls),
        }
    }
}

impl Service<String> for StatefulFailFirst {
    type Response = String;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<String, io::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.ready = true;
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: String) -> Self::Future {
        assert!(
            self.ready,
            "Service::call invoked without prior poll_ready -- tower contract violation (#293/H)"
        );
        self.ready = false;
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if n == 0 {
                Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    "first attempt fails",
                ))
            } else {
                Ok(req)
            }
        })
    }
}

#[tokio::test]
async fn reconnect_drives_readied_instance_on_retry() {
    use tower::ServiceExt;

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(5),
            Duration::from_millis(20),
        ))
        .max_attempts(3)
        .build();
    let layer = ReconnectLayer::new(config);
    let mut svc = layer.layer(StatefulFailFirst::new());

    // First attempt fails inside the service; the retry path must re-drive
    // poll_ready on the stored clone before calling, or the assert in
    // `StatefulFailFirst::call` panics.
    let response = svc
        .ready()
        .await
        .unwrap()
        .call("hello".into())
        .await
        .unwrap();
    assert_eq!(response, "hello");
}
