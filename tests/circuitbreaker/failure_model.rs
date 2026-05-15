//! `FailureModel::ConsecutiveFailures` trip-condition tests.
//!
//! Covers selection, transitions, and interaction with the existing
//! sliding-window slow-call detector. See #283.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service};
use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitState, FailureModel};

/// Helper: a service that fails on call N if `failure_indices` contains N.
fn intermittent_service(
    failure_indices: Arc<Vec<usize>>,
    counter: Arc<AtomicUsize>,
) -> impl tower::Service<
    (),
    Response = (),
    Error = &'static str,
    Future = impl std::future::Future<Output = Result<(), &'static str>> + Send,
> + Clone
+ Send
+ 'static {
    tower::service_fn(move |_req: ()| {
        let idx = counter.fetch_add(1, Ordering::SeqCst);
        let fail = failure_indices.contains(&idx);
        async move { if fail { Err::<(), _>("err") } else { Ok(()) } }
    })
}

#[tokio::test]
async fn consecutive_failures_trip_at_k() {
    let counter = Arc::new(AtomicUsize::new(0));
    let svc = intermittent_service(Arc::new(vec![0, 1, 2]), Arc::clone(&counter));

    let layer = CircuitBreakerLayer::builder()
        .consecutive_failures(3)
        .wait_duration_in_open(Duration::from_secs(10))
        .name("consec-3")
        .build();
    let mut cb = layer.layer(svc);

    let _ = cb.call(()).await;
    assert_eq!(cb.state().await, CircuitState::Closed);
    let _ = cb.call(()).await;
    assert_eq!(cb.state().await, CircuitState::Closed);
    let _ = cb.call(()).await;
    assert_eq!(cb.state().await, CircuitState::Open);
}

#[tokio::test]
async fn consecutive_failures_reset_on_success() {
    let counter = Arc::new(AtomicUsize::new(0));
    // Fail, fail, succeed, fail, fail -- should NOT trip at k=3.
    let svc = intermittent_service(Arc::new(vec![0, 1, 3, 4]), Arc::clone(&counter));

    let layer = CircuitBreakerLayer::builder()
        .consecutive_failures(3)
        .wait_duration_in_open(Duration::from_secs(10))
        .name("consec-3-reset")
        .build();
    let mut cb = layer.layer(svc);

    for _ in 0..5 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Closed);
}

#[tokio::test]
async fn consecutive_failures_ignores_failure_rate_threshold_and_window_size() {
    // With consecutive_failures(2), the trip should fire at the second
    // failure even if failure_rate_threshold / sliding_window_size would
    // have required many more samples first.
    let counter = Arc::new(AtomicUsize::new(0));
    let svc = intermittent_service(Arc::new(vec![0, 1]), Arc::clone(&counter));

    let layer = CircuitBreakerLayer::builder()
        .consecutive_failures(2)
        .failure_rate_threshold(0.99) // would never trip via rate model
        .sliding_window_size(1000) // window would never fill
        .minimum_number_of_calls(1000)
        .wait_duration_in_open(Duration::from_secs(10))
        .name("consec-ignores-rate")
        .build();
    let mut cb = layer.layer(svc);

    let _ = cb.call(()).await;
    let _ = cb.call(()).await;
    assert_eq!(cb.state().await, CircuitState::Open);
}

#[tokio::test]
async fn consecutive_failures_default_model_is_sliding_window() {
    // Without `.consecutive_failures(...)` the breaker uses the default
    // sliding-window model and does not trip just from a few errors when
    // the window is much larger.
    let counter = Arc::new(AtomicUsize::new(0));
    let svc = intermittent_service(Arc::new(vec![0, 1, 2]), Arc::clone(&counter));

    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(100)
        .minimum_number_of_calls(100)
        .wait_duration_in_open(Duration::from_secs(10))
        .name("default-sliding")
        .build();
    let mut cb = layer.layer(svc);

    let _ = cb.call(()).await;
    let _ = cb.call(()).await;
    let _ = cb.call(()).await;
    assert_eq!(cb.state().await, CircuitState::Closed);
}

#[tokio::test]
async fn consecutive_failures_recovers_through_half_open() {
    // 3 failures trip the breaker; after the wait, half-open lets a probe
    // through; success closes the breaker.
    let counter = Arc::new(AtomicUsize::new(0));
    // Calls 0..3 fail, all subsequent succeed.
    let svc = intermittent_service(Arc::new(vec![0, 1, 2]), Arc::clone(&counter));

    let layer = CircuitBreakerLayer::builder()
        .consecutive_failures(3)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(1)
        .name("consec-recover")
        .build();
    let mut cb = layer.layer(svc);

    for _ in 0..3 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    sleep(Duration::from_millis(150)).await;

    // Half-open probe succeeds, breaker closes.
    let _ = cb.call(()).await;
    assert_eq!(cb.state().await, CircuitState::Closed);
}

#[tokio::test]
#[should_panic(expected = "ConsecutiveFailures requires k > 0")]
async fn consecutive_failures_zero_k_panics_on_build() {
    let _ = CircuitBreakerLayer::builder()
        .failure_model(FailureModel::ConsecutiveFailures { k: 0 })
        .build();
}

#[tokio::test]
async fn failure_model_via_failure_model_method_matches_shortcut() {
    let counter = Arc::new(AtomicUsize::new(0));
    let svc = intermittent_service(Arc::new(vec![0, 1, 2]), Arc::clone(&counter));

    let layer = CircuitBreakerLayer::builder()
        .failure_model(FailureModel::ConsecutiveFailures { k: 3 })
        .wait_duration_in_open(Duration::from_secs(10))
        .name("via-failure-model")
        .build();
    let mut cb = layer.layer(svc);

    for _ in 0..3 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);
}
