//! Test helpers for tower-resilience layer crates.
//!
//! This module is gated behind the `testing` feature and is intended for use
//! in `[dev-dependencies]` only. It exposes inner-service probes that exercise
//! tower contract requirements that synthetic `service_fn`-based test doubles
//! cannot reach.
//!
//! # Why
//!
//! Every layer in this workspace implements [`tower::Service`]. The trait has
//! a contract that, if violated, lets compliant downstream middleware panic at
//! runtime:
//!
//! > Implementations are permitted to panic if `call` is invoked without
//! > obtaining `Poll::Ready(Ok(()))` from `poll_ready`.
//!
//! Tests that wrap a layer around `tower::service_fn` or a `MockService` style
//! probe never exercise this -- those inners have no-op `poll_ready` and no
//! per-instance readiness state, so the contract violation is invisible. The
//! `StatefulInner` probe in this module deliberately resets its `ready` flag
//! on `Clone`, mirroring how `tower::limit::ConcurrencyLimit`, `Buffer`,
//! `LoadShed`, and other stateful tower middleware behave in production. Any
//! regression to the clone-in-call anti-pattern (see #286) panics here.
//!
//! # Example
//!
//! ```ignore
//! use tower::{Layer, Service, ServiceExt};
//! use tower_resilience_core::testing::StatefulInner;
//!
//! #[tokio::test]
//! async fn my_layer_drives_readied_instance() {
//!     let layer = MyLayer::builder().build();
//!     let mut svc = tower::ServiceBuilder::new()
//!         .layer(layer)
//!         .service(StatefulInner::new());
//!
//!     for _ in 0..3 {
//!         let _ = svc.ready().await.unwrap().call(()).await;
//!     }
//! }
//! ```
//!
//! See `CONTRIBUTING.md` for the full `Service` impl checklist.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// An inner [`tower::Service`] whose [`Clone`] resets readiness state.
///
/// `poll_ready` flips `ready: true`; `call` asserts `ready` is set, then flips
/// it back to `false`. Crucially, [`Clone`] produces an instance with
/// `ready: false`, mirroring the documented behavior of stateful tower
/// middleware like `tower::limit::ConcurrencyLimit`.
///
/// Wrap a layer around this and drive multiple `ready().await; call(...)`
/// cycles. If the layer moves a fresh `self.inner.clone()` into its returned
/// future (rather than the readied original via `std::mem::replace`), the
/// `call` assertion panics with a message naming #286.
pub struct StatefulInner {
    ready: bool,
}

impl StatefulInner {
    /// Constructs an un-ready probe.
    pub fn new() -> Self {
        Self { ready: false }
    }
}

impl Default for StatefulInner {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for StatefulInner {
    fn clone(&self) -> Self {
        // Mirrors `Clone for ConcurrencyLimit` / `Buffer`: the new instance
        // does not inherit readiness from the original.
        Self { ready: false }
    }
}

impl tower::Service<()> for StatefulInner {
    type Response = ();
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.ready = true;
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        assert!(
            self.ready,
            "Service::call invoked without prior poll_ready -- tower contract violation (#286)"
        );
        // The contract: call consumes readiness. Next call must re-poll.
        self.ready = false;
        Box::pin(async { Ok(()) })
    }
}
