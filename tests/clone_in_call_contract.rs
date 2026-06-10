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

#[tokio::test]
async fn adaptive_drives_readied_instance() {
    use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd};

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
async fn cache_miss_drives_readied_instance() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower_resilience_cache::CacheLayer;

    // A fresh key per call forces every iteration through the cache-miss path,
    // which is the only path that dispatches to the readied inner.
    let counter = Arc::new(AtomicUsize::new(0));
    let layer = CacheLayer::<(), usize>::builder()
        .max_size(16)
        .ttl(Duration::from_secs(60))
        .key_extractor(move |_: &()| counter.fetch_add(1, Ordering::SeqCst))
        .build()
        .unwrap();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn coalesce_leader_drives_readied_instance() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower_resilience_coalesce::CoalesceLayer;

    // A unique key per call forces every iteration through the leader path,
    // which is the only path that dispatches to the readied inner.
    let counter = Arc::new(AtomicUsize::new(0));
    let layer = CoalesceLayer::new(move |_: &()| counter.fetch_add(1, Ordering::SeqCst));
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn router_drives_readied_instance() {
    use tower_resilience_router::WeightedRouter;

    // `WeightedRouter` is constructed directly rather than as a layer; it
    // readies every backend in `poll_ready`, so the selected backend is a
    // readied instance.
    let mut router = WeightedRouter::builder()
        .route(StatefulInner::new(), 1)
        .route(StatefulInner::new(), 1)
        .build();

    for _ in 0..6 {
        let _ = router.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn hedge_drives_readied_instance() {
    use tower_resilience_hedge::HedgeLayer;

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(20))
        .max_hedged_attempts(2)
        .build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}

#[tokio::test]
async fn reconnect_drives_readied_instance() {
    use tower_resilience_reconnect::{ReconnectConfig, ReconnectLayer, ReconnectPolicy};

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(5),
            Duration::from_millis(20),
        ))
        .max_attempts(3)
        .build();
    let layer = ReconnectLayer::new(config);
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(StatefulInner::new());

    for _ in 0..3 {
        let _ = svc.ready().await.unwrap().call(()).await;
    }
}
