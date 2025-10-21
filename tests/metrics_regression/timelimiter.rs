//! Time limiter metrics regression tests

use super::helpers::*;
use serial_test::serial;
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_timelimiter::TimeLimiterLayer;

#[tokio::test]
#[serial]
async fn timelimiter_metrics_exist() {
    init_recorder();

    let layer = TimeLimiterLayer::builder()
        .name("test_timelimiter")
        .timeout_duration(Duration::from_millis(100))
        .build();

    let service = tower::service_fn(|_: u64| async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok::<_, &'static str>("success")
    });

    let mut service = layer.layer(service);

    // Make a successful call that completes before timeout
    let _ = service.ready().await.unwrap().call(1).await;

    // Verify counter metrics
    assert_counter_exists("timelimiter_calls_total");
    assert_metric_has_label("timelimiter_calls_total", "timelimiter", "test_timelimiter");
    assert_metric_has_label("timelimiter_calls_total", "result", "success");

    // Verify histogram metric
    assert_histogram_exists("timelimiter_call_duration_seconds");
    assert_metric_has_label(
        "timelimiter_call_duration_seconds",
        "timelimiter",
        "test_timelimiter",
    );
}

#[tokio::test]
#[serial]
async fn timelimiter_timeout_metrics() {
    init_recorder();

    let layer = TimeLimiterLayer::builder()
        .name("timeout_timelimiter")
        .timeout_duration(Duration::from_millis(30))
        .build();

    let service = tower::service_fn(|_: u64| async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok::<_, &'static str>("success")
    });

    let mut service = layer.layer(service);

    // Make a call that will timeout
    let _ = service.ready().await.unwrap().call(1).await;

    // Verify timeout result label
    assert_metric_has_label("timelimiter_calls_total", "result", "timeout");
}

#[tokio::test]
#[serial]
async fn timelimiter_error_metrics() {
    init_recorder();

    let layer = TimeLimiterLayer::builder()
        .name("error_timelimiter")
        .timeout_duration(Duration::from_millis(100))
        .build();

    let service = tower::service_fn(|_: u64| async { Err::<&'static str, _>("error") });

    let mut service = layer.layer(service);

    // Make a call that will error
    let _ = service.ready().await.unwrap().call(1).await;

    // Verify error result label
    assert_metric_has_label("timelimiter_calls_total", "result", "error");
}
