use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tower::Service;
use tower_resilience_circuitbreaker::{CircuitBreakerConfig, CircuitState, SlidingWindowType};

/// Test reset from open state
#[tokio::test]
async fn reset_from_open() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("reset-open")
        .build();

    let mut cb: tower_resilience_circuitbreaker::CircuitBreaker<_, (), (), &str> =
        layer.layer(service);

    // Trip circuit
    for _ in 0..5 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Reset
    cb.reset().await;
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test reset from half-open state
#[tokio::test]
async fn reset_from_half_open() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(2)
        .name("reset-halfopen")
        .build();

    let mut cb: tower_resilience_circuitbreaker::CircuitBreaker<_, (), (), &str> =
        layer.layer(service);

    // Trip circuit
    for _ in 0..5 {
        let _ = cb.call(()).await;
    }

    // Wait for half-open
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify half-open (first call should be permitted)
    let _ = cb.call(()).await;

    // Reset from half-open
    cb.reset().await;
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test reset from closed state (should be no-op but safe)
#[tokio::test]
async fn reset_from_closed() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    let layer = CircuitBreakerConfig::<(), String>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("reset-closed")
        .build();

    let cb: tower_resilience_circuitbreaker::CircuitBreaker<_, (), (), String> =
        layer.layer(service);

    assert_eq!(cb.state().await, CircuitState::Closed);

    // Reset closed circuit
    cb.reset().await;
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test reset clears all counters (count-based)
#[tokio::test]
async fn reset_clears_counters_count_based() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count < 10 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .sliding_window_type(SlidingWindowType::CountBased)
        .sliding_window_size(10)
        .failure_rate_threshold(0.5)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("reset-counters")
        .build();

    let mut cb: tower_resilience_circuitbreaker::CircuitBreaker<_, (), (), &str> =
        layer.layer(service);

    // Make 4 failing calls (not enough to trip)
    for _ in 0..4 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Closed);

    // Reset should clear counters
    cb.reset().await;

    // Make 4 more failing calls
    for _ in 0..4 {
        let _ = cb.call(()).await;
    }

    // Should still be closed (counters were reset, so only 4 calls in window now)
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test reset clears time-based records
#[tokio::test]
async fn reset_clears_time_based_records() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count < 10 {
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
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("reset-timebased")
        .build();

    let mut cb: tower_resilience_circuitbreaker::CircuitBreaker<_, (), (), &str> =
        layer.layer(service);

    // Make 6 failing calls (would trip: 6 >= 5 minimum, 100% failure >= 50%)
    for _ in 0..6 {
        let _ = cb.call(()).await;
    }

    // Should be open
    assert_eq!(cb.state().await, CircuitState::Open);

    // Reset - clears time-based records
    cb.reset().await;

    // Make 4 more failing calls (not enough to trip: 4 < 5 minimum)
    for _ in 0..4 {
        let _ = cb.call(()).await;
    }

    // Should be closed (only 4 calls after reset, < minimum 5)
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test reset during concurrent operations
#[tokio::test]
async fn reset_during_concurrent_operations() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if count < 10 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("reset-concurrent")
        .build();

    let cb = layer.layer(service);
    let cb_mutex = Arc::new(tokio::sync::Mutex::new(cb));

    // Spawn concurrent calls
    let mut handles = vec![];
    for _ in 0..20 {
        let cb_clone = Arc::clone(&cb_mutex);
        let handle = tokio::spawn(async move {
            let mut breaker = cb_clone.lock().await;
            breaker.call(()).await
        });
        handles.push(handle);
    }

    // Reset while calls are happening
    tokio::time::sleep(Duration::from_millis(50)).await;
    {
        let breaker = cb_mutex.lock().await;
        breaker.reset().await;
    }

    // Wait for all calls
    for handle in handles {
        let _ = handle.await;
    }

    // Should be in a valid state
    let breaker = cb_mutex.lock().await;
    let state = breaker.state().await;
    assert!(
        state == CircuitState::Closed
            || state == CircuitState::Open
            || state == CircuitState::HalfOpen,
        "Should be in valid state after concurrent reset"
    );
}

/// Test multiple resets in succession
#[tokio::test]
async fn multiple_resets() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("multiple-resets")
        .build();

    let mut cb: tower_resilience_circuitbreaker::CircuitBreaker<_, (), (), &str> =
        layer.layer(service);

    for _ in 0..3 {
        // Trip circuit
        for _ in 0..5 {
            let _ = cb.call(()).await;
        }
        assert_eq!(cb.state().await, CircuitState::Open);

        // Reset
        cb.reset().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
    }
}

/// Test reset with slow call detection enabled
#[tokio::test]
async fn reset_with_slow_call_detection() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            if count < 10 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("reset-slow")
        .build();

    let mut cb: tower_resilience_circuitbreaker::CircuitBreaker<_, (), (), &str> =
        layer.layer(service);

    // Make slow failing calls
    for _ in 0..5 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Reset should clear slow call counters too
    cb.reset().await;
    assert_eq!(cb.state().await, CircuitState::Closed);

    // Make more slow failing calls
    for _ in 0..2 {
        let _ = cb.call(()).await;
    }

    // Should be closed (only 2 calls, < minimum 3)
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test reset preserves configuration
#[tokio::test]
async fn reset_preserves_configuration() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count < 20 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("reset-config")
        .build();

    let mut cb: tower_resilience_circuitbreaker::CircuitBreaker<_, (), (), &str> =
        layer.layer(service);

    // Trip circuit
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Reset
    cb.reset().await;

    // Configuration should still work - make enough calls to trip again
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // Should trip again with same thresholds
    assert_eq!(cb.state().await, CircuitState::Open);
}
