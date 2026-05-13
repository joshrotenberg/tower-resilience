//! Regression tests for #286.
//!
//! Tower's `Service` contract permits `call` to panic if the receiver was not
//! first driven to `Poll::Ready(Ok(()))` via `poll_ready`. Earlier versions of
//! every layer in this crate that wraps a clonable inner service moved a fresh
//! clone of `self.inner` (rather than the readied original) into the returned
//! future, violating the contract for any inner service whose `Clone` resets
//! per-instance readiness state.
//!
//! These tests wrap a `StatefulInner` whose `Clone` impl deliberately resets
//! its `ready` flag (matching the documented behavior of
//! `tower::limit::ConcurrencyLimit`, `tower::buffer::Buffer`, etc.) so that any
//! regression to the clone-in-call anti-pattern panics here.
//!
//! See: https://docs.rs/tower-service/0.3.3/tower_service/trait.Service.html#be-careful-when-cloning-inner-services

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tower::{Service, ServiceExt};

/// Service whose `Clone` resets `ready: false`, panicking on `call` until
/// `poll_ready` is observed on that specific instance.
struct StatefulInner {
    ready: bool,
}

impl Clone for StatefulInner {
    fn clone(&self) -> Self {
        // Resets readiness state, matching the documented behavior of stateful
        // tower middleware (ConcurrencyLimit, Buffer, Batch, LoadShed).
        Self { ready: false }
    }
}

impl StatefulInner {
    fn new() -> Self {
        Self { ready: false }
    }
}

impl Service<()> for StatefulInner {
    type Response = ();
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.ready = true;
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        assert!(
            self.ready,
            "Service::call invoked without prior poll_ready -- tower contract violation (#286)"
        );
        // The contract: call consumes the readiness. Next call must re-poll.
        self.ready = false;
        Box::pin(async { Ok(()) })
    }
}

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

    // Rejection mode: poll_ready returns Ready immediately, permit acquired in call.
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

    // Backpressure mode: permit acquired in poll_ready.
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

    // No chaos injection -- we only care about the readiness contract.
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

    // value() strategy returns a fixed Res when inner errors. We only care
    // that the inner is invoked on the readied instance.
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

    // Multiple calls force the contract over the retry boundary as well.
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
