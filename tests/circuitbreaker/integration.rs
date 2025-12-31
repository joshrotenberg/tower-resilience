use futures::future::BoxFuture;
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll};
use std::time::Duration;
use tower::{Service, ServiceBuilder, service_fn};
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_circuitbreaker::{CircuitBreakerError, CircuitState};

#[derive(Clone)]
struct FlakyService {
    fail_after: usize,
    counter: Arc<AtomicUsize>,
}

impl FlakyService {
    fn new(fail_after: usize) -> Self {
        Self {
            fail_after,
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Service<()> for FlakyService {
    type Response = &'static str;
    type Error = &'static str;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        let should_fail = count >= self.fail_after;

        Box::pin(async move { if should_fail { Err("fail") } else { Ok("ok") } })
    }
}

#[tokio::test]
async fn circuit_opens_after_consecutive_failures() {
    let service = FlakyService::new(3);

    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(6)
        .wait_duration_in_open(Duration::from_millis(100))
        .permitted_calls_in_half_open(1)
        .build();

    let mut breaker = layer.layer(service);

    let mut results = vec![];
    for _ in 0..6 {
        let result = breaker.call(()).await;
        results.push(result);
    }

    let errors: Vec<_> = results.iter().filter(|r| r.is_err()).collect();
    assert!(errors.len() >= 3, "Expected at least 3 errors");
}

#[tokio::test]
async fn circuit_transitions_through_half_open_and_recovers() {
    let failing_service = FlakyService::new(0);
    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(4)
        .wait_duration_in_open(Duration::from_millis(100))
        .permitted_calls_in_half_open(1)
        .build();

    let mut breaker = layer.layer(failing_service);
    for _ in 0..4 {
        let _ = breaker.call(()).await;
    }

    let succeeding_service = FlakyService::new(100);
    breaker = layer.layer(succeeding_service);

    tokio::time::sleep(Duration::from_millis(150)).await;

    let result = breaker.call(()).await;
    assert!(result.is_ok());

    let result = breaker.call(()).await;
    assert!(result.is_ok());
    assert_eq!(breaker.state().await, CircuitState::Closed);
}

#[tokio::test]
async fn circuit_rejects_when_open() {
    let service = FlakyService::new(0);
    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(2)
        .wait_duration_in_open(Duration::from_secs(1))
        .permitted_calls_in_half_open(1)
        .build();

    let mut breaker = layer.layer(service);
    for _ in 0..2 {
        let _ = breaker.call(()).await;
    }

    let result = breaker.call(()).await;
    assert!(matches!(result, Err(CircuitBreakerError::OpenCircuit)));
}

#[tokio::test]
async fn half_open_fails_and_reopens() {
    let service = FlakyService::new(0); // always fails
    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(1.0)
        .sliding_window_size(2)
        .wait_duration_in_open(Duration::from_millis(100))
        .permitted_calls_in_half_open(1)
        .build();
    let mut breaker = layer.layer(service);

    for _ in 0..2 {
        let _ = breaker.call(()).await;
    }

    tokio::time::sleep(Duration::from_millis(150)).await;

    let _ = breaker.call(()).await;
    assert_eq!(breaker.state().await, CircuitState::Open);
}

#[tokio::test]
async fn does_not_trip_before_window_full() {
    let service = FlakyService::new(1);
    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(1.0)
        .sliding_window_size(10)
        .wait_duration_in_open(Duration::from_secs(1))
        .permitted_calls_in_half_open(1)
        .build();
    let mut breaker = layer.layer(service);

    for _ in 0..9 {
        let _ = breaker.call(()).await;
    }

    assert_eq!(breaker.state().await, CircuitState::Closed);
}

#[tokio::test]
async fn all_successes_keep_circuit_closed() {
    let service = FlakyService::new(100);
    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(0.1)
        .sliding_window_size(5)
        .wait_duration_in_open(Duration::from_secs(1))
        .permitted_calls_in_half_open(1)
        .build();
    let mut breaker = layer.layer(service);

    for _ in 0..99 {
        let _ = breaker.call(()).await;
    }

    assert_eq!(breaker.state().await, CircuitState::Closed);
}

#[tokio::test]
async fn integrates_with_service_builder_layer() {
    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(4)
        .wait_duration_in_open(Duration::from_secs(1))
        .build();

    let mut service: tower_resilience_circuitbreaker::CircuitBreaker<
        _,
        (),
        &'static str,
        &'static str,
    > = ServiceBuilder::new()
        .layer(layer.for_request::<()>())
        .service(service_fn(
            |_: ()| async move { Ok::<_, &'static str>("ok") },
        ));

    let response = service.call(()).await.expect("call should succeed");
    assert_eq!(response, "ok");
}

#[tokio::test]
async fn does_not_trip_if_minimum_not_met() {
    let service = FlakyService::new(0); // always fails
    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(0.1)
        .sliding_window_size(10)
        .minimum_number_of_calls(6)
        .wait_duration_in_open(Duration::from_secs(1))
        .permitted_calls_in_half_open(1)
        .build();
    let mut breaker = layer.layer(service);

    // fewer than 6 calls should not trigger evaluation
    for _ in 0..5 {
        let _ = breaker.call(()).await;
    }

    // The circuit should still be closed
    assert_eq!(breaker.state().await, CircuitState::Closed);
    // but another burst of calls should trigger the circuit
    for _ in 0..5 {
        let _ = breaker.call(()).await;
    }
    assert_eq!(breaker.state().await, CircuitState::Open);
}

#[tokio::test]
async fn closed_open_halfopen_closed_cycle() {
    let service = service_fn(|req: bool| async move { if req { Ok("ok") } else { Err("fail") } });

    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(2)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(2)
        .build();
    let mut breaker = layer.layer(service);

    // 1) Closed → Open
    assert_eq!(breaker.state().await, CircuitState::Closed);
    for _ in 0..2 {
        let _ = breaker.call(false).await;
    }
    assert_eq!(breaker.state().await, CircuitState::Open);

    // 2) wait out the open period
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(breaker.state().await, CircuitState::Open);

    // 3) call again, should HalfOpen the circuit
    breaker.call(true).await.unwrap();
    assert_eq!(breaker.state().await, CircuitState::HalfOpen);

    // 4) and another should close it again
    breaker.call(true).await.unwrap();
    assert_eq!(breaker.state().await, CircuitState::Closed);
}

#[cfg(feature = "metrics")]
#[tokio::test]
async fn metrics_are_emitted() {
    use metrics::set_global_recorder;
    use metrics_util::debugging::DebugValue;
    use metrics_util::debugging::DebuggingRecorder;
    use std::sync::LazyLock;

    static RECORDER: LazyLock<DebuggingRecorder> = LazyLock::new(DebuggingRecorder::default);

    // Register the global recorder (once per test run)
    let _ = set_global_recorder(&*RECORDER);

    let service = FlakyService::new(0); // always fails
    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(1.0)
        .sliding_window_size(1)
        .minimum_number_of_calls(1)
        .wait_duration_in_open(Duration::from_secs(1))
        .permitted_calls_in_half_open(1)
        .build();
    let mut breaker = layer.layer(service);

    let _ = breaker.call(()).await;

    let snapshot_vec = RECORDER.snapshotter().snapshot().into_vec();
    let calls = snapshot_vec.iter().find_map(|(key, _, _, value)| {
        if key.key().name() == "circuitbreaker_calls_total" {
            Some(value)
        } else {
            None
        }
    });

    assert!(
        matches!(calls, Some(DebugValue::Counter(_))),
        "expected circuitbreaker_calls_total metric",
    );

    assert!(matches!(
        snapshot_vec
            .iter()
            .find(|(key, ..)| key.key().name() == "circuitbreaker_calls_total"),
        Some((_, _, _, DebugValue::Counter(_)))
    ));

    assert!(matches!(
        snapshot_vec
            .iter()
            .find(|(key, ..)| key.key().name() == "circuitbreaker_transitions_total"),
        Some((_, _, _, DebugValue::Counter(_)))
    ));

    assert!(matches!(
        snapshot_vec
            .iter()
            .find(|(key, ..)| key.key().name() == "circuitbreaker_state"),
        Some((_, _, _, DebugValue::Gauge(_)))
    ));

    snapshot_vec.iter().any(|record| {
        let (key, _, _, value) = record;
        if key.key().name() == "circuitbreaker_calls_total" {
            if let DebugValue::Counter(_) = value {
                key.key()
                    .labels()
                    .any(|label| label.key() == "outcome" && label.value() == "failure")
            } else {
                false
            }
        } else {
            false
        }
    });
}

#[tokio::test]
async fn fallback_is_called_when_circuit_open() {
    let service = FlakyService::new(0); // always fails

    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(2)
        .wait_duration_in_open(Duration::from_secs(10))
        .build();

    let mut breaker = layer.layer(service).with_fallback(
        |_req: ()| -> BoxFuture<'static, Result<&'static str, &'static str>> {
            Box::pin(async { Ok("fallback response") })
        },
    );

    // First two calls fail and open the circuit
    for _ in 0..2 {
        let _ = breaker.call(()).await;
    }

    // Circuit should be open now
    assert_eq!(breaker.state().await, CircuitState::Open);

    // Next call should use the fallback
    let result = breaker.call(()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "fallback response");
}

#[tokio::test]
async fn no_fallback_returns_error_when_circuit_open() {
    let service = FlakyService::new(0); // always fails

    let layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(2)
        .wait_duration_in_open(Duration::from_secs(10))
        .build();

    let mut breaker = layer.layer(service);

    // First two calls fail and open the circuit
    for _ in 0..2 {
        let _ = breaker.call(()).await;
    }

    // Circuit should be open now
    assert_eq!(breaker.state().await, CircuitState::Open);

    // Next call should return OpenCircuit error since no fallback
    let result = breaker.call(()).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CircuitBreakerError::OpenCircuit
    ));
}

#[tokio::test]
async fn multi_layer_composition_with_timeout() {
    use tower_resilience_timelimiter::TimeLimiterLayer;

    let service = service_fn(|delay_ms: u64| async move {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        Ok::<_, &'static str>("success")
    });

    // Compose timeout + circuit breaker
    let timeout_layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(100))
        .build();

    let circuit_breaker_layer = CircuitBreakerLayer::<
        &'static str,
        tower_resilience_timelimiter::TimeLimiterError<&'static str>,
    >::builder()
    .failure_rate_threshold(0.5)
    .sliding_window_size(4)
    .minimum_number_of_calls(2)
    .wait_duration_in_open(Duration::from_secs(10))
    .build();

    let mut composed = ServiceBuilder::new()
        .layer(circuit_breaker_layer.for_request::<u64>())
        .layer(timeout_layer)
        .service(service);

    // Fast request should succeed
    let result = composed.call(10).await;
    assert!(result.is_ok());

    // Slow requests will timeout and count as failures
    for _ in 0..3 {
        let _ = composed.call(200).await; // Will timeout
    }

    // Circuit should now be open
    let result = composed.call(10).await;
    assert!(matches!(result, Err(CircuitBreakerError::OpenCircuit)));
}

#[tokio::test]
async fn multi_layer_composition_with_retry() {
    use tower_resilience_retry::RetryLayer;

    let attempt_count = Arc::new(AtomicUsize::new(0));
    let attempt_clone = Arc::clone(&attempt_count);

    let service = service_fn(move |_req: ()| {
        let count = attempt_clone.fetch_add(1, Ordering::Relaxed);
        async move {
            // Fail first 2 attempts, then succeed
            if count < 2 {
                Err::<&'static str, _>("transient error")
            } else {
                Ok("success")
            }
        }
    });

    // Compose circuit breaker + retry
    // Note: Retry is inner, so it retries before circuit breaker sees failures
    let circuit_breaker_layer = CircuitBreakerLayer::<&'static str, &'static str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(4)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_secs(10))
        .build();

    let retry_layer = RetryLayer::<(), &'static str>::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::from_millis(10))
        .build();

    let mut composed = ServiceBuilder::new()
        .layer(circuit_breaker_layer.for_request::<()>())
        .layer(retry_layer)
        .service(service);

    // First call: retry will handle the transient errors
    let result = composed.call(()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "success");

    // Should have made 3 attempts (2 failures + 1 success)
    // But circuit breaker only sees 1 success
    assert_eq!(attempt_count.load(Ordering::Relaxed), 3);
}
