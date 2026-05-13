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
