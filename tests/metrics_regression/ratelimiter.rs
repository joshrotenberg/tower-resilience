//! Rate limiter metrics regression tests

use super::helpers::*;
use serial_test::serial;

use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_ratelimiter::RateLimiterLayer;

#[tokio::test]
#[serial]
async fn ratelimiter_metrics_exist() {
    init_recorder();

    let layer = RateLimiterLayer::builder()
        .name("test_ratelimiter")
        .limit_for_period(10)
        .refresh_period(Duration::from_secs(1))
        .build();

    let service = tower::service_fn(|_: u64| async { Ok::<_, &'static str>("success") });

    let mut service = layer.layer(service);

    // Make some calls
    for i in 0..3 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    // Verify counter metrics
    assert_counter_exists("ratelimiter_calls_total");
    assert_metric_has_label("ratelimiter_calls_total", "ratelimiter", "test_ratelimiter");
    assert_metric_has_label("ratelimiter_calls_total", "result", "permitted");

    // Verify histogram metric
    assert_histogram_exists("ratelimiter_wait_duration_seconds");
    assert_metric_has_label(
        "ratelimiter_wait_duration_seconds",
        "ratelimiter",
        "test_ratelimiter",
    );
}

#[tokio::test]
#[serial]
async fn ratelimiter_rejection_metrics() {
    init_recorder();

    let layer = RateLimiterLayer::builder()
        .name("reject_ratelimiter")
        .limit_for_period(2)
        .refresh_period(Duration::from_secs(10))
        .build();

    let service = tower::service_fn(|_: u64| async { Ok::<_, &'static str>("success") });

    let mut service = layer.layer(service);

    // Make many rapid calls to trigger rejections (more than limit_for_period)
    for i in 0..20 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    // Verify rejection label exists
    assert_metric_has_label("ratelimiter_calls_total", "result", "rejected");
}
