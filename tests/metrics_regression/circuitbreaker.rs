//! Circuit breaker metrics regression tests

use super::helpers::*;
use serial_test::serial;
use std::time::Duration;
use tower::{Service, ServiceExt};
use tower_resilience_circuitbreaker::CircuitBreakerLayer;

#[tokio::test]
#[serial]
async fn circuitbreaker_metrics_exist() {
    init_recorder();

    // Create a circuit breaker with a low threshold to trigger state transitions
    let layer = CircuitBreakerLayer::builder()
        .name("test_cb")
        .failure_rate_threshold(0.5)
        .sliding_window_size(4)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_millis(100))
        .build();

    // Create a test service that can succeed or fail
    let success_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let success_count_clone = success_count.clone();
    let service = tower::service_fn(move |_: u64| {
        let count = success_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        async move {
            if count.is_multiple_of(2) {
                Ok::<_, &'static str>("success")
            } else {
                Err("failure")
            }
        }
    });

    let mut service = layer.layer(service);

    // Make some calls to generate metrics
    for i in 0..6 {
        let _ = service.ready().await.unwrap().call(i).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Verify counter metrics
    assert_counter_exists("circuitbreaker_calls_total");
    assert_metric_has_label("circuitbreaker_calls_total", "circuitbreaker", "test_cb");
    assert_metric_has_label("circuitbreaker_calls_total", "outcome", "success");
    assert_metric_has_label("circuitbreaker_calls_total", "outcome", "failure");

    // Verify transition counter
    assert_counter_exists("circuitbreaker_transitions_total");
    assert_metric_has_label(
        "circuitbreaker_transitions_total",
        "circuitbreaker",
        "test_cb",
    );

    // Verify state gauge
    assert_gauge_exists("circuitbreaker_state");
    assert_metric_has_label("circuitbreaker_state", "circuitbreaker", "test_cb");

    // Verify duration histogram
    assert_histogram_exists("circuitbreaker_call_duration_seconds");
    assert_metric_has_label(
        "circuitbreaker_call_duration_seconds",
        "circuitbreaker",
        "test_cb",
    );
}

#[tokio::test]
#[serial]
async fn circuitbreaker_slow_call_metrics() {
    init_recorder();

    // Create a circuit breaker with slow call detection
    let layer = CircuitBreakerLayer::builder()
        .name("slow_cb")
        .slow_call_duration_threshold(Duration::from_millis(50))
        .slow_call_rate_threshold(0.5)
        .sliding_window_size(4)
        .minimum_number_of_calls(2)
        .build();

    let service = tower::service_fn(move |req: u64| async move {
        if req.is_multiple_of(2) {
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
        Ok::<_, &'static str>("success")
    });

    let mut service = layer.layer(service);

    // Make some calls, some will be slow
    for i in 0..4 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    // Verify slow call counter exists
    assert_counter_exists("circuitbreaker_slow_calls_total");
    assert_metric_has_label(
        "circuitbreaker_slow_calls_total",
        "circuitbreaker",
        "slow_cb",
    );
}

#[tokio::test]
#[serial]
async fn circuitbreaker_state_transition_labels() {
    init_recorder();

    let layer = CircuitBreakerLayer::builder()
        .name("transition_cb")
        .failure_rate_threshold(0.5)
        .sliding_window_size(4)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(1)
        .build();

    let fail_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let fail_count_clone = fail_count.clone();
    let service = tower::service_fn(move |_: u64| {
        let count = fail_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        async move {
            if count < 3 {
                Err::<&'static str, _>("failure")
            } else {
                Ok("success")
            }
        }
    });

    let mut service = layer.layer(service);

    // Generate failures to trigger Closed -> Open transition
    for i in 0..3 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    // Wait for circuit to enter half-open
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Make a successful call to trigger Open -> HalfOpen -> Closed transitions
    let _ = service.ready().await.unwrap().call(100).await;

    // Verify transition labels exist
    assert_metric_has_label("circuitbreaker_transitions_total", "from", "Closed");
    assert_metric_has_label("circuitbreaker_transitions_total", "to", "Open");
}
