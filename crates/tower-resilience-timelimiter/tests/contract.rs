//! Tower `Service` contract regression for `TimeLimiter`.
//!
//! See `crates/tower-resilience-bulkhead/tests/contract.rs` for the rationale.

use std::convert::Infallible;
use std::future::{ready, Future, Ready};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::{sleep, Sleep};
use tower::limit::ConcurrencyLimit;
use tower::{Service, ServiceExt};
use tower_resilience_core::testing::StatefulInner;
use tower_resilience_timelimiter::TimeLimiterLayer;

struct DelayedReady {
    delay_duration: Duration,
    delay: Pin<Box<Sleep>>,
}

impl DelayedReady {
    fn new(delay: Duration) -> Self {
        Self {
            delay_duration: delay,
            delay: Box::pin(sleep(delay)),
        }
    }
}

impl Clone for DelayedReady {
    fn clone(&self) -> Self {
        // TimeLimiter's readied-instance replacement requires Clone. Calls use
        // the instance that poll_ready drove; the replacement is for the next
        // request and gets its own readiness delay.
        Self::new(self.delay_duration)
    }
}

impl Service<()> for DelayedReady {
    type Response = &'static str;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.delay.as_mut().poll(cx).map(|()| Ok(()))
    }

    fn call(&mut self, (): ()) -> Self::Future {
        ready(Ok("ready"))
    }
}

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

#[tokio::test(start_paused = true)]
async fn readiness_wait_is_outside_the_call_timeout() {
    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(10))
        .build();
    let mut svc = tower::ServiceBuilder::new()
        .layer(layer)
        .service(DelayedReady::new(Duration::from_millis(100)));

    let started = tokio::time::Instant::now();
    let result = svc.ready().await.unwrap().call(()).await;

    assert_eq!(result.unwrap(), "ready");
    assert_eq!(started.elapsed(), Duration::from_millis(100));
}
