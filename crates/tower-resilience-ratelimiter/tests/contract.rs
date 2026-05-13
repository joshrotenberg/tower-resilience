//! Tower `Service` contract regression for `RateLimiter`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use std::time::Duration;
use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_core::testing::StatefulInner;
use tower_resilience_ratelimiter::RateLimiterLayer;

#[tokio::test]
async fn ratelimiter_rejection_drives_readied_instance() {
    let layer = RateLimiterLayer::builder()
        .limit_for_period(1000)
        .refresh_period(Duration::from_secs(1))
        .build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn ratelimiter_backpressure_drives_readied_instance() {
    let layer = RateLimiterLayer::builder()
        .limit_for_period(1000)
        .refresh_period(Duration::from_secs(1))
        .backpressure()
        .build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn ratelimiter_rejection_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = RateLimiterLayer::builder()
        .limit_for_period(1000)
        .refresh_period(Duration::from_secs(1))
        .build();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn ratelimiter_backpressure_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = RateLimiterLayer::builder()
        .limit_for_period(1000)
        .refresh_period(Duration::from_secs(1))
        .backpressure()
        .build();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
