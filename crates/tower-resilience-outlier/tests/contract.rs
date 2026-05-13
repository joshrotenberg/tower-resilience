//! Tower `Service` contract regression for `OutlierDetectionService`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_core::testing::StatefulInner;
use tower_resilience_outlier::{OutlierDetectionLayer, OutlierDetector};

#[tokio::test]
async fn outlier_drives_readied_instance() {
    let detector = OutlierDetector::new();
    detector.register("inner", 5);

    let layer = OutlierDetectionLayer::builder()
        .detector(detector)
        .instance_name("inner")
        .build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn outlier_composes_with_concurrency_limit() {
    let detector = OutlierDetector::new();
    detector.register("inner", 5);

    let inner = ConcurrencyLimit::new(StatefulInner::new(), 8);
    let layer = OutlierDetectionLayer::builder()
        .detector(detector)
        .instance_name("inner")
        .build();
    let mut svc = tower::ServiceBuilder::new().layer(layer).service(inner);

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
