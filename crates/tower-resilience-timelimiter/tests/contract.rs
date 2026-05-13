//! Tower `Service` contract regression for `TimeLimiter`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use std::time::Duration;
use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_core::testing::StatefulInner;
use tower_resilience_timelimiter::TimeLimiterLayer;

#[tokio::test]
async fn timelimiter_drives_readied_instance() {
    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(1))
        .build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn timelimiter_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(1))
        .build();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
