//! Retry metrics regression tests

use super::helpers::*;
use serial_test::serial;
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_retry::RetryLayer;

#[tokio::test]
#[serial]
async fn retry_metrics_exist() {
    init_recorder();

    let layer = RetryLayer::builder()
        .name("test_retry")
        .max_attempts(3)
        .fixed_backoff(Duration::from_millis(10))
        .build();

    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let service = tower::service_fn(move |_: u64| {
        let c = counter_clone.clone();
        async move {
            let count = c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if count < 2 {
                Err::<&'static str, _>("failure")
            } else {
                Ok("success")
            }
        }
    });

    let mut service = layer.layer(service);

    // Make a call that will retry and eventually succeed
    let _ = service.ready().await.unwrap().call(1).await;

    // Verify counter metrics
    assert_counter_exists("retry_calls_total");
    assert_metric_has_label("retry_calls_total", "retry", "test_retry");
    assert_metric_has_label("retry_calls_total", "result", "success");

    assert_counter_exists("retry_attempts_total");
    assert_metric_has_label("retry_attempts_total", "retry", "test_retry");

    // Verify histogram metric
    assert_histogram_exists("retry_attempts");
    assert_metric_has_label("retry_attempts", "retry", "test_retry");
}

#[tokio::test]
#[serial]
async fn retry_exhausted_metrics() {
    init_recorder();

    let layer = RetryLayer::builder()
        .name("exhausted_retry")
        .max_attempts(2)
        .fixed_backoff(Duration::from_millis(10))
        .build();

    let service = tower::service_fn(|_: u64| async { Err::<&'static str, _>("failure") });

    let mut service = layer.layer(service);

    // Make a call that will exhaust retries
    let _ = service.ready().await.unwrap().call(1).await;

    // Verify exhausted result label
    assert_metric_has_label("retry_calls_total", "result", "exhausted");
}
