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
