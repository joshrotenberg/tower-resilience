//! Chaos metrics regression tests

use super::helpers::*;
use serial_test::serial;
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_chaos::ChaosLayer;

#[tokio::test]
#[serial]
async fn chaos_error_injection_metrics() {
    init_recorder();

    // Types inferred from closure signature
    let layer = ChaosLayer::builder()
        .name("error_chaos")
        .error_rate(1.0)
        .error_fn(|_req: &u64| "injected_error")
        .build();

    let service = tower::service_fn(|_: u64| async { Ok::<_, &'static str>("success") });

    let mut service = layer.layer(service);

    // Make a call that will have error injected
    let _ = service.ready().await.unwrap().call(1).await;

    // Verify error injection counter
    assert_counter_exists("chaos.errors_injected");
    assert_metric_has_label("chaos.errors_injected", "layer", "error_chaos");
}

#[tokio::test]
#[serial]
async fn chaos_latency_injection_metrics() {
    init_recorder();

    // Latency-only chaos - no type parameters needed!
    let layer = ChaosLayer::builder()
        .name("latency_chaos")
        .latency_rate(1.0)
        .min_latency(Duration::from_millis(10))
        .max_latency(Duration::from_millis(10))
        .build();

    let service = tower::service_fn(|_: u64| async { Ok::<_, &'static str>("success") });

    let mut service = layer.layer(service);

    // Make a call that will have latency injected
    let _ = service.ready().await.unwrap().call(1).await;

    // Verify latency injection metrics
    assert_counter_exists("chaos.latency_injections");
    assert_metric_has_label("chaos.latency_injections", "layer", "latency_chaos");

    assert_histogram_exists("chaos.injected_latency_ms");
    assert_metric_has_label("chaos.injected_latency_ms", "layer", "latency_chaos");
}

#[tokio::test]
#[serial]
async fn chaos_passthrough_metrics() {
    init_recorder();

    // Latency-only chaos with 0% rate - all pass through
    let layer = ChaosLayer::builder()
        .name("passthrough_chaos")
        .latency_rate(0.0)
        .build();

    let service = tower::service_fn(|_: u64| async { Ok::<_, &'static str>("success") });

    let mut service = layer.layer(service);

    // Make a call that will pass through
    let _ = service.ready().await.unwrap().call(1).await;

    // Verify passthrough counter
    assert_counter_exists("chaos.passed_through");
    assert_metric_has_label("chaos.passed_through", "layer", "passthrough_chaos");
}
