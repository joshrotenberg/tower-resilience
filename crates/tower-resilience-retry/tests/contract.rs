//! Tower `Service` contract regression for `Retry`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_core::testing::StatefulInner;
use tower_resilience_retry::RetryLayer;

#[tokio::test]
async fn retry_drives_readied_instance() {
    let layer: tower_resilience_retry::RetryLayer<(), (), std::convert::Infallible> =
        RetryLayer::builder().max_attempts(1).build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    // Multiple calls also exercise the retry boundary.
    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn retry_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer: tower_resilience_retry::RetryLayer<(), (), std::convert::Infallible> =
        RetryLayer::builder().max_attempts(1).build();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
