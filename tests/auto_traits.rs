//! Auto-trait assertions for every layer service.
//!
//! These tests are compile-time checks. They ensure that every layer in the
//! crate produces a `Send + Sync + 'static` service when wrapped around a
//! `Send + Sync + 'static` inner. This is the surface that tonic, axum,
//! tower::buffer, and other `Arc`-shared service holders require.
//!
//! A regression that drops `Sync` (e.g., by storing a `Pin<Box<dyn Future +
//! Send>>` field without `+ Sync`) will fail to compile here.
//!
//! See #287 for the motivating bug.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tower::{Layer, Service};

/// Inner service that is `Send + Sync + 'static` and whose future is
/// `Send + Sync + 'static`. Any auto-trait loss in the wrapping layer is
/// attributable to the layer itself, not the inner.
#[derive(Clone)]
struct SyncInner;

impl Service<()> for SyncInner {
    type Response = ();
    type Error = std::convert::Infallible;
    type Future =
        Pin<Box<dyn Future<Output = Result<(), std::convert::Infallible>> + Send + Sync + 'static>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: ()) -> Self::Future {
        Box::pin(async { Ok(()) })
    }
}

fn assert_send_sync_static<T: Send + Sync + 'static>(_: &T) {}

#[test]
fn bulkhead_is_send_sync() {
    use tower_resilience_bulkhead::BulkheadLayer;
    let svc = BulkheadLayer::builder()
        .max_concurrent_calls(2)
        .build()
        .layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn bulkhead_backpressure_is_send_sync() {
    use tower_resilience_bulkhead::BulkheadLayer;
    let svc = BulkheadLayer::builder()
        .max_concurrent_calls(2)
        .backpressure()
        .build()
        .layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn circuitbreaker_is_send_sync() {
    use tower_resilience_circuitbreaker::CircuitBreakerLayer;
    let svc = CircuitBreakerLayer::builder().build().layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn ratelimiter_is_send_sync() {
    use tower_resilience_ratelimiter::RateLimiterLayer;
    let svc = RateLimiterLayer::builder()
        .limit_for_period(100)
        .refresh_period(Duration::from_secs(1))
        .build()
        .layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn ratelimiter_backpressure_is_send_sync() {
    use tower_resilience_ratelimiter::RateLimiterLayer;
    let svc = RateLimiterLayer::builder()
        .limit_for_period(100)
        .refresh_period(Duration::from_secs(1))
        .backpressure()
        .build()
        .layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn timelimiter_is_send_sync() {
    use tower_resilience_timelimiter::TimeLimiterLayer;
    let svc = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(1))
        .build()
        .layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn chaos_is_send_sync() {
    use tower_resilience_chaos::ChaosLayer;
    let svc = ChaosLayer::builder().build().layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn adaptive_is_send_sync() {
    use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd};
    let svc = AdaptiveLimiterLayer::new(
        Aimd::builder()
            .initial_limit(8)
            .latency_threshold(Duration::from_secs(1))
            .build(),
    )
    .layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn cache_is_send_sync() {
    use tower_resilience_cache::CacheLayer;
    let svc = CacheLayer::<(), usize>::builder()
        .max_size(16)
        .ttl(Duration::from_secs(60))
        .key_extractor(|_: &()| 0usize)
        .build()
        .unwrap()
        .layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn coalesce_is_send_sync() {
    use tower_resilience_coalesce::CoalesceLayer;
    let svc = CoalesceLayer::new(|_: &()| 0usize).layer(SyncInner);
    assert_send_sync_static(&svc);
}

// `ExecutorLayer::current()` captures `tokio::runtime::Handle::current()`, which
// panics outside a runtime -- so this is the one assertion that runs under
// `#[tokio::test]` rather than `#[test]`. The auto-trait check is identical.
#[tokio::test]
async fn executor_is_send_sync() {
    use tower_resilience_executor::ExecutorLayer;
    let svc = ExecutorLayer::current().layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn fallback_is_send_sync() {
    use tower_resilience_fallback::FallbackLayer;
    let svc = FallbackLayer::<(), (), std::convert::Infallible>::value(()).layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn hedge_is_send_sync() {
    use tower_resilience_hedge::HedgeLayer;
    let svc = HedgeLayer::builder()
        .delay(Duration::from_millis(20))
        .max_hedged_attempts(2)
        .build()
        .layer(SyncInner);
    assert_send_sync_static(&svc);
}

// Healthcheck does not expose a tower `Layer`; `HealthCheckWrapper` monitors a
// set of resources via a `HealthChecker` rather than wrapping an inner service.
// We assert the wrapper itself is `Send + Sync + 'static` over a unit resource.
#[test]
fn healthcheck_is_send_sync() {
    use tower_resilience_healthcheck::{HealthCheckWrapper, HealthChecker, HealthStatus};

    struct UnitChecker;
    impl HealthChecker<()> for UnitChecker {
        async fn check(&self, _: &()) -> HealthStatus {
            HealthStatus::Healthy
        }
    }

    let wrapper = HealthCheckWrapper::builder()
        .with_context((), "inner")
        .with_checker(UnitChecker)
        .build();
    assert_send_sync_static(&wrapper);
}

#[test]
fn outlier_is_send_sync() {
    use tower_resilience_outlier::{OutlierDetectionLayer, OutlierDetector};
    let detector = OutlierDetector::new();
    detector.register("inner", 5);
    let svc = OutlierDetectionLayer::builder()
        .detector(detector)
        .instance_name("inner")
        .build()
        .layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn reconnect_is_send_sync() {
    use tower_resilience_reconnect::{ReconnectConfig, ReconnectLayer, ReconnectPolicy};
    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(5),
            Duration::from_millis(20),
        ))
        .max_attempts(3)
        .build();
    let svc = ReconnectLayer::new(config).layer(SyncInner);
    assert_send_sync_static(&svc);
}

#[test]
fn retry_is_send_sync() {
    use tower_resilience_retry::RetryLayer;
    let layer: RetryLayer<(), (), std::convert::Infallible> =
        RetryLayer::builder().max_attempts(1).build();
    let svc = layer.layer(SyncInner);
    assert_send_sync_static(&svc);
}

// Router exposes a standalone `Service` (`WeightedRouter`), not a `Layer`: it
// fans requests out to weighted backend services. We use `SyncInner` as the
// backend and assert the composed router is `Send + Sync + 'static`.
#[test]
fn router_is_send_sync() {
    use tower_resilience_router::WeightedRouter;
    let router = WeightedRouter::builder().route(SyncInner, 1).build();
    assert_send_sync_static(&router);
}

/// Simulates the tonic / `Arc<T>` server holder pattern from #287. tonic
/// stores `inner: Arc<T>` and dispatches via `Arc::clone` across `.await`
/// points, which requires `T: Send + Sync + 'static`. Before the fix, this
/// did not compile because `Bulkhead<S>: !Sync`.
#[tokio::test]
async fn bulkhead_composes_under_arc_like_tonic() {
    use std::sync::Arc;
    use tower_resilience_bulkhead::BulkheadLayer;

    let svc = BulkheadLayer::builder()
        .max_concurrent_calls(2)
        .backpressure()
        .build()
        .layer(SyncInner);

    // The shape tonic generates: Arc<MyService>, where MyService holds the
    // composed tower stack. Captured across `.await` -- requires Sync.
    struct Server<S> {
        inner: S,
    }

    let server = Arc::new(Server { inner: svc });

    fn requires_send_sync_static<T: Send + Sync + 'static>(_: &T) {}
    requires_send_sync_static(&server);

    // Spawn two tasks sharing the Arc -- mirrors how tonic dispatches.
    let s1 = Arc::clone(&server);
    let s2 = Arc::clone(&server);
    let t1 = tokio::spawn(async move {
        let _ = &s1.inner;
    });
    let t2 = tokio::spawn(async move {
        let _ = &s2.inner;
    });
    t1.await.unwrap();
    t2.await.unwrap();
}
