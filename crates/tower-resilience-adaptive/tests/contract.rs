//! Tower `Service` contract regression for `AdaptiveService`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use std::time::Duration;
use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd};
use tower_resilience_core::testing::StatefulInner;

#[tokio::test]
async fn adaptive_drives_readied_instance() {
    let layer = AdaptiveLimiterLayer::new(
        Aimd::builder()
            .initial_limit(8)
            .latency_threshold(Duration::from_secs(1))
            .build(),
    );
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn adaptive_composes_with_concurrency_limit() {
    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = AdaptiveLimiterLayer::new(
        Aimd::builder()
            .initial_limit(8)
            .latency_threshold(Duration::from_secs(1))
            .build(),
    );
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
