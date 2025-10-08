use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::time::sleep;
use tower::Service;
use tower_circuitbreaker::{CircuitBreakerConfig, CircuitState};

/// Test multiple event listeners on same event type
#[tokio::test]
async fn multiple_event_listeners() {
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));
    let counter3 = Arc::new(AtomicUsize::new(0));

    let c1 = Arc::clone(&counter1);
    let c2 = Arc::clone(&counter2);
    let c3 = Arc::clone(&counter3);

    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(50))
        .on_state_transition(move |_from, _to| {
            c1.fetch_add(1, Ordering::Relaxed);
        })
        .on_state_transition(move |_from, _to| {
            c2.fetch_add(1, Ordering::Relaxed);
        })
        .on_state_transition(move |_from, _to| {
            c3.fetch_add(1, Ordering::Relaxed);
        })
        .build();

    let mut cb = layer.layer(service);

    // Trip circuit
    for _ in 0..5 {
        let _ = cb.call(()).await;
    }

    // All three listeners should have been called
    assert_eq!(
        counter1.load(Ordering::Relaxed),
        1,
        "Listener 1 should be called once"
    );
    assert_eq!(
        counter2.load(Ordering::Relaxed),
        1,
        "Listener 2 should be called once"
    );
    assert_eq!(
        counter3.load(Ordering::Relaxed),
        1,
        "Listener 3 should be called once"
    );
}

/// Test failure classifier that classifies all results as success
#[tokio::test]
async fn failure_classifier_all_success() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    // Classifier that treats all results as success (even errors)
    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .failure_classifier(|_result: &Result<(), &str>| false) // Nothing is a failure
        .build();

    let mut cb = layer.layer(service);

    // Make 10 error calls
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // Circuit should stay closed (0% failure rate)
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test failure classifier that classifies all results as failure
#[tokio::test]
async fn failure_classifier_all_failure() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), &str>(()) });

    // Classifier that treats all results as failure (even successes)
    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .failure_classifier(|_result: &Result<(), &str>| true) // Everything is a failure
        .build();

    let mut cb = layer.layer(service);

    // Make 10 success calls
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // Circuit should open (100% failure rate)
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test slow call detection with exactly threshold duration
#[tokio::test]
async fn slow_call_exactly_at_threshold() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<(), &str>(())
    });

    let slow_call_count = Arc::new(AtomicUsize::new(0));
    let sc = Arc::clone(&slow_call_count);

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(10)
        .on_slow_call(move |_duration| {
            sc.fetch_add(1, Ordering::Relaxed);
        })
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // Calls at exactly 100ms should be considered slow (>=)
    assert!(
        slow_call_count.load(Ordering::Relaxed) > 0,
        "Calls at exactly threshold should be detected as slow"
    );
}

/// Test slow call detection with very fast calls
#[tokio::test]
async fn slow_call_very_fast_calls() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), &str>(()) });

    let slow_call_count = Arc::new(AtomicUsize::new(0));
    let sc = Arc::clone(&slow_call_count);

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .slow_call_duration_threshold(Duration::from_secs(10))
        .slow_call_rate_threshold(0.5)
        .sliding_window_size(100)
        .minimum_number_of_calls(100)
        .on_slow_call(move |_duration| {
            sc.fetch_add(1, Ordering::Relaxed);
        })
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..100 {
        let _ = cb.call(()).await;
    }

    // No calls should be detected as slow
    assert_eq!(
        slow_call_count.load(Ordering::Relaxed),
        0,
        "Fast calls should not be detected as slow"
    );
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test failure classifier with mixed error types
#[tokio::test]
async fn failure_classifier_mixed_error_types() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            match count % 3 {
                0 => Err::<(), _>("timeout"),
                1 => Err("internal_error"),
                _ => Err("rate_limited"),
            }
        }
    });

    // Only count "internal_error" as failures
    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.4)
        .sliding_window_size(12)
        .minimum_number_of_calls(12)
        .failure_classifier(|result: &Result<(), &str>| {
            result
                .as_ref()
                .err()
                .map(|e| *e == "internal_error")
                .unwrap_or(false)
        })
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..12 {
        let _ = cb.call(()).await;
    }

    // 4 internal_errors out of 12 = 33.3% < 40%
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test event listener with all event types
#[tokio::test]
async fn all_event_types_emitted() {
    let permitted = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let success = Arc::new(AtomicUsize::new(0));
    let failure = Arc::new(AtomicUsize::new(0));
    let transition = Arc::new(AtomicUsize::new(0));

    let p = Arc::clone(&permitted);
    let r = Arc::clone(&rejected);
    let s = Arc::clone(&success);
    let f = Arc::clone(&failure);
    let t = Arc::clone(&transition);

    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count < 5 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(50))
        .on_call_permitted(move |_state| {
            p.fetch_add(1, Ordering::Relaxed);
        })
        .on_call_rejected(move || {
            r.fetch_add(1, Ordering::Relaxed);
        })
        .on_success(move |_state| {
            s.fetch_add(1, Ordering::Relaxed);
        })
        .on_failure(move |_state| {
            f.fetch_add(1, Ordering::Relaxed);
        })
        .on_state_transition(move |_from, _to| {
            t.fetch_add(1, Ordering::Relaxed);
        })
        .build();

    let mut cb = layer.layer(service);

    // Make 5 failing calls
    for _ in 0..5 {
        let _ = cb.call(()).await;
    }

    // Try one more call while open (should be rejected)
    let _ = cb.call(()).await;

    assert!(
        permitted.load(Ordering::Relaxed) >= 5,
        "Should have permitted calls"
    );
    assert_eq!(
        rejected.load(Ordering::Relaxed),
        1,
        "Should have 1 rejected call"
    );
    assert_eq!(
        success.load(Ordering::Relaxed),
        0,
        "Should have 0 successes"
    );
    assert_eq!(failure.load(Ordering::Relaxed), 5, "Should have 5 failures");
    assert_eq!(
        transition.load(Ordering::Relaxed),
        1,
        "Should have 1 transition (Closed -> Open)"
    );
}

/// Test window size of 1
#[tokio::test]
async fn sliding_window_size_one() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(1)
        .minimum_number_of_calls(1)
        .build();

    let mut cb = layer.layer(service);

    // Single failing call should trip it
    let _ = cb.call(()).await;

    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test permitted_calls_in_half_open = 1 (minimum)
#[tokio::test]
async fn half_open_one_permitted_call() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count < 5 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(1)
        .build();

    let mut cb = layer.layer(service);

    // Trip circuit
    for _ in 0..5 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Wait for half-open
    sleep(Duration::from_millis(60)).await;

    // Make 1 successful call
    let result = cb.call(()).await;
    assert!(result.is_ok());

    // Should transition to closed immediately
    assert_eq!(cb.state().await, CircuitState::Closed);
}
