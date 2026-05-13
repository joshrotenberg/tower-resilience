//! Cross-crate Tower `Service` contract regression for #286.
//!
//! Each layer crate has its own `tests/contract.rs` that runs the same probe
//! against its layer (see `crates/tower-resilience-*/tests/contract.rs`). This
//! umbrella test re-exercises every layer in one place so a composition-time
//! regression -- not just a per-crate one -- gets caught here.
//!
//! Shared probe: [`tower_resilience_core::testing::StatefulInner`].

use std::time::Duration;
use tower::{Service, ServiceExt};
use tower_resilience_core::testing::StatefulInner;

#[tokio::test]
async fn circuitbreaker_drives_readied_instance() {
    use tower_resilience_circuitbreaker::CircuitBreakerLayer;

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
async fn bulkhead_rejection_drives_readied_instance() {
    use tower_resilience_bulkhead::BulkheadLayer;

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
    use tower_resilience_bulkhead::BulkheadLayer;

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

#[tokio::test]
async fn timelimiter_drives_readied_instance() {
    use tower_resilience_timelimiter::TimeLimiterLayer;

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
async fn ratelimiter_rejection_drives_readied_instance() {
    use tower_resilience_ratelimiter::RateLimiterLayer;

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
    use tower_resilience_ratelimiter::RateLimiterLayer;

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
async fn chaos_drives_readied_instance() {
    use tower_resilience_chaos::ChaosLayer;

    let layer = ChaosLayer::builder().build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn fallback_drives_readied_instance() {
    use tower_resilience_fallback::FallbackLayer;

    let layer = FallbackLayer::<(), (), std::convert::Infallible>::value(());
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn retry_drives_readied_instance() {
    use tower_resilience_retry::RetryLayer;

    let layer: tower_resilience_retry::RetryLayer<(), (), std::convert::Infallible> =
        RetryLayer::builder().max_attempts(1).build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn executor_drives_readied_instance() {
    use tower_resilience_executor::ExecutorLayer;

    let layer = ExecutorLayer::current();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn outlier_drives_readied_instance() {
    use tower_resilience_outlier::{OutlierDetectionLayer, OutlierDetector};

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
