//! Tower `Service` contract regression for `CircuitBreaker`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_core::testing::StatefulInner;

#[tokio::test]
async fn circuitbreaker_drives_readied_instance() {
    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(50.0)
        .build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn circuitbreaker_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(50.0)
        .build();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
