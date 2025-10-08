use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::time::sleep;
use tower::Service;
use tower_resilience_circuitbreaker::{CircuitBreakerConfig, CircuitState, SlidingWindowType};

/// Test time-based window + slow call detection + failure classification
#[tokio::test]
async fn time_based_slow_call_failure_classification() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // Mix of fast failures, slow failures, fast successes, slow successes
    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            match count {
                0..=2 => {
                    // Fast failures (30%)
                    sleep(Duration::from_millis(50)).await;
                    Err::<(), _>("fast_error")
                }
                3..=5 => {
                    // Slow failures (30%)
                    sleep(Duration::from_millis(150)).await;
                    Err("slow_error")
                }
                6..=7 => {
                    // Fast successes (20%)
                    sleep(Duration::from_millis(50)).await;
                    Ok(())
                }
                _ => {
                    // Slow successes (20%)
                    sleep(Duration::from_millis(150)).await;
                    Ok(())
                }
            }
        }
    });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_secs(5))
        .failure_rate_threshold(0.7) // 60% failure rate < 70%
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.6) // 50% slow rate < 60%
        .minimum_number_of_calls(10)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("combination-test")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // 6 failures (60%) < 70%, 5 slow (50%) < 60%
    // Both below thresholds, should stay closed
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test count-based + slow call rate + failure rate (both thresholds)
#[tokio::test]
async fn count_based_dual_thresholds() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // All slow, half fail
    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            sleep(Duration::from_millis(150)).await;
            if count < 5 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .sliding_window_type(SlidingWindowType::CountBased)
        .sliding_window_size(10)
        .failure_rate_threshold(0.6) // 50% < 60%, won't trip
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.9) // 100% >= 90%, will trip
        .minimum_number_of_calls(10)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("dual-threshold")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // Slow call rate trips it
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test custom failure classifier + slow call detection
#[tokio::test]
async fn custom_classifier_with_slow_calls() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            // Use more generous timings for Windows - 200ms slow vs 30ms fast
            // This gives Windows timers more margin to correctly classify
            sleep(Duration::from_millis(if count < 5 { 200 } else { 30 })).await;

            if count < 3 {
                Err::<(), _>("retryable_error")
            } else if count < 6 {
                Err("fatal_error")
            } else {
                Ok(())
            }
        }
    });

    // Only count "fatal_error" as failures
    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .sliding_window_size(10)
        .failure_rate_threshold(0.5)
        .slow_call_duration_threshold(Duration::from_millis(80)) // Lower threshold with more margin
        .slow_call_rate_threshold(0.6)
        .minimum_number_of_calls(10)
        .wait_duration_in_open(Duration::from_millis(100))
        .failure_classifier(|result: &Result<(), &str>| {
            result
                .as_ref()
                .err()
                .map(|e| *e == "fatal_error")
                .unwrap_or(false)
        })
        .name("custom-classifier")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // Only 3 "fatal" failures (30%) < 50%
    // But 5 slow calls (50%) < 60%
    // Should stay closed
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test event listeners + all configuration permutations
#[tokio::test]
async fn event_listeners_with_complex_config() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let permitted = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let transitions = Arc::new(AtomicUsize::new(0));
    let slow_calls = Arc::new(AtomicUsize::new(0));

    let c = Arc::clone(&call_count);
    let p = Arc::clone(&permitted);
    let r = Arc::clone(&rejected);
    let t = Arc::clone(&transitions);
    let s = Arc::clone(&slow_calls);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            sleep(Duration::from_millis(if count < 5 { 150 } else { 50 })).await;
            if count < 5 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_secs(10))
        .failure_rate_threshold(0.5)
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.5)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(2)
        .name("event-test")
        .on_call_permitted(move |_state| {
            p.fetch_add(1, Ordering::Relaxed);
        })
        .on_call_rejected(move || {
            r.fetch_add(1, Ordering::Relaxed);
        })
        .on_state_transition(move |_from, _to| {
            t.fetch_add(1, Ordering::Relaxed);
        })
        .on_slow_call(move |_duration| {
            s.fetch_add(1, Ordering::Relaxed);
        })
        .build();

    let mut cb = layer.layer(service);

    // Make calls that will trip circuit
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // Verify events were emitted
    assert!(
        permitted.load(Ordering::Relaxed) > 0,
        "Should have permitted calls"
    );
    assert!(
        slow_calls.load(Ordering::Relaxed) > 0,
        "Should have detected slow calls"
    );
    assert!(
        transitions.load(Ordering::Relaxed) > 0,
        "Should have state transitions"
    );
}

/// Test manual state control during active failure scenarios
#[tokio::test]
async fn manual_override_during_failures() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("manual-override")
        .build();

    let mut cb = layer.layer(service);

    // Cause failures
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Force closed despite failures
    cb.force_closed().await;
    assert_eq!(cb.state().await, CircuitState::Closed);

    // Make more failures (10 to definitely trip it since counters were reset)
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // Should open again due to continued failures (10 failures = 100% >= 50%)
    assert_eq!(cb.state().await, CircuitState::Open);

    // Force closed again
    cb.force_closed().await;
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test custom classifier with different error types
#[tokio::test]
async fn custom_classifier_transient_errors() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count < 8 {
                Err::<String, _>("transient_error")
            } else {
                Ok("success".to_string())
            }
        }
    });

    // Only "transient_error" counts as failure
    let layer = CircuitBreakerConfig::<String, &str>::builder()
        .sliding_window_size(10)
        .failure_rate_threshold(0.7)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(50))
        .failure_classifier(|result: &Result<String, &str>| {
            result
                .as_ref()
                .err()
                .map(|e| *e == "transient_error")
                .unwrap_or(false)
        })
        .name("transient-classifier")
        .build();

    let mut cb = layer.layer(service);

    // Make calls - 8 transient errors (80%) >= 70%, should open
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test time-based with all features enabled
#[tokio::test]
async fn time_based_kitchen_sink() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            // Varying durations and results
            let duration = match count % 4 {
                0 => 50,  // Fast
                1 => 150, // Slow
                2 => 50,  // Fast
                _ => 150, // Slow
            };
            sleep(Duration::from_millis(duration)).await;

            if count < 6 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_secs(10))
        .failure_rate_threshold(0.7)
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.6)
        .minimum_number_of_calls(10)
        .wait_duration_in_open(Duration::from_millis(100))
        .permitted_calls_in_half_open(3)
        .failure_classifier(|result: &Result<(), &str>| result.is_err())
        .name("kitchen-sink")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // 6 failures (60%) < 70%, 5 slow (50%) < 60%
    // Should stay closed
    assert_eq!(cb.state().await, CircuitState::Closed);
}
