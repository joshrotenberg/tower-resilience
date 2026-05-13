//! Tower `Service` contract regression for `ExecutorService`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_core::testing::StatefulInner;
use tower_resilience_executor::ExecutorLayer;

#[tokio::test]
async fn executor_drives_readied_instance() {
    let layer = ExecutorLayer::current();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn executor_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = ExecutorLayer::current();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
