//! Tower `Service` contract regression for `Bulkhead`.
//!
//! Wraps the layer around a [`StatefulInner`] probe whose `Clone` resets
//! readiness, mirroring the documented behavior of compliant stateful
//! middleware (`tower::limit::ConcurrencyLimit`, `tower::buffer::Buffer`, ...).
//! A regression that moves a fresh `self.inner.clone()` into the returned
//! future (instead of the readied original via `std::mem::replace`) panics
//! here with a message naming #286.
//!
//! Both rejection and backpressure modes are exercised.

use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_bulkhead::BulkheadLayer;
use tower_resilience_core::testing::StatefulInner;

#[tokio::test]
async fn bulkhead_rejection_drives_readied_instance() {
    let layer = BulkheadLayer::builder()
        .max_concurrent_calls(4)
        .reject_when_full()
        .build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn bulkhead_backpressure_drives_readied_instance() {
    let layer = BulkheadLayer::builder()
        .max_concurrent_calls(4)
        .backpressure()
        .build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

// Wrapping `tower::limit::ConcurrencyLimit` is the canonical real-world
// composition test for the clone-in-call contract. ConcurrencyLimit's `Clone`
// resets its semaphore permit slot (matching the documented contract); a
// regression that moves a fresh clone into `call` instead of the readied
// original triggers `ConcurrencyLimit::call`'s `expect`/`panic` path.
#[tokio::test]
async fn bulkhead_rejection_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = BulkheadLayer::builder()
        .max_concurrent_calls(4)
        .reject_when_full()
        .build();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn bulkhead_backpressure_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = BulkheadLayer::builder()
        .max_concurrent_calls(4)
        .backpressure()
        .build();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
