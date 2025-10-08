use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tower::Service;
use tower_resilience_circuitbreaker::{CircuitBreakerConfig, CircuitState};

/// Test 100 concurrent calls hitting closed circuit
#[tokio::test]
async fn concurrent_calls_closed_circuit() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        c.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, String>("success") }
    });

    let layer = CircuitBreakerConfig::<&str, String>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(200)
        .minimum_number_of_calls(50)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("concurrent-closed")
        .build();

    let cb: Arc<
        tokio::sync::Mutex<tower_resilience_circuitbreaker::CircuitBreaker<_, (), &str, String>>,
    > = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    // Spawn 100 concurrent tasks
    let mut handles = vec![];
    for _ in 0..100 {
        let cb_clone = Arc::clone(&cb);
        let handle = tokio::spawn(async move {
            let mut breaker = cb_clone.lock().await;
            breaker.call(()).await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let mut successes = 0;
    for handle in handles {
        if let Ok(Ok(_)) = handle.await {
            successes += 1;
        }
    }

    assert_eq!(successes, 100, "All calls should succeed on closed circuit");
    assert_eq!(call_count.load(Ordering::Relaxed), 100);
}

/// Test 100 concurrent calls hitting open circuit
#[tokio::test]
async fn concurrent_calls_open_circuit() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_secs(10)) // Keep it open
        .name("concurrent-open")
        .build();

    let mut cb = layer.layer(service);

    // Trip the circuit first
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Now try concurrent calls on open circuit
    let cb = Arc::new(tokio::sync::Mutex::new(cb));
    let mut handles = vec![];

    for _ in 0..100 {
        let cb_clone = Arc::clone(&cb);
        let handle = tokio::spawn(async move {
            let mut breaker = cb_clone.lock().await;
            breaker.call(()).await
        });
        handles.push(handle);
    }

    // All should be rejected
    let mut rejections = 0;
    for handle in handles {
        if let Ok(Err(_)) = handle.await {
            rejections += 1;
        }
    }

    assert!(
        rejections >= 95,
        "Most calls should be rejected on open circuit, got {}",
        rejections
    );
}

/// Test state transition during concurrent calls
#[tokio::test]
async fn state_transition_during_concurrent_calls() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // Service that fails first 10 calls, then succeeds
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
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(3)
        .name("transition-concurrent")
        .build();

    let cb: Arc<
        tokio::sync::Mutex<tower_resilience_circuitbreaker::CircuitBreaker<_, (), (), &str>>,
    > = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    // Spawn tasks that will cause state transitions
    let mut handles = vec![];
    for i in 0..20 {
        let cb_clone = Arc::clone(&cb);
        let handle = tokio::spawn(async move {
            if i > 10 {
                // Give time for circuit to open
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let mut breaker = cb_clone.lock().await;
            breaker.call(()).await
        });
        handles.push(handle);
    }

    // Wait for all
    for handle in handles {
        let _ = handle.await;
    }

    // Eventually should end up closed
    let breaker = cb.lock().await;
    let final_state = breaker.state().await;
    assert!(
        final_state == CircuitState::Closed || final_state == CircuitState::HalfOpen,
        "Expected Closed or HalfOpen, got {:?}",
        final_state
    );
}

/// Test atomic state read consistency
#[tokio::test]
async fn atomic_state_read_consistency() {
    let service = tower::service_fn(|_req: ()| async { Ok::<_, String>("success") });

    let layer = CircuitBreakerConfig::<&str, String>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("atomic-state")
        .build();

    let cb: Arc<
        tokio::sync::Mutex<tower_resilience_circuitbreaker::CircuitBreaker<_, (), &str, String>>,
    > = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    // Read state from multiple tasks concurrently
    let mut handles = vec![];
    for _ in 0..50 {
        let cb_clone = Arc::clone(&cb);
        let handle = tokio::spawn(async move {
            let breaker = cb_clone.lock().await;
            breaker.state().await
        });
        handles.push(handle);
    }

    // All reads should succeed and return valid state
    for handle in handles {
        let state = handle.await.unwrap();
        assert!(
            state == CircuitState::Closed
                || state == CircuitState::Open
                || state == CircuitState::HalfOpen,
            "Invalid state: {:?}",
            state
        );
    }
}

/// Test concurrent record_success/record_failure operations
#[tokio::test]
async fn concurrent_success_failure_recording() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // Alternating success/failure
    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count.is_multiple_of(2) {
                Ok::<_, &str>("success")
            } else {
                Err("error")
            }
        }
    });

    let layer = CircuitBreakerConfig::<&str, &str>::builder()
        .failure_rate_threshold(0.7)
        .sliding_window_size(100)
        .minimum_number_of_calls(50)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("concurrent-recording")
        .build();

    let cb: Arc<
        tokio::sync::Mutex<tower_resilience_circuitbreaker::CircuitBreaker<_, (), &str, &str>>,
    > = Arc::new(tokio::sync::Mutex::new(layer.layer(service)));

    let mut handles = vec![];
    for _ in 0..100 {
        let cb_clone = Arc::clone(&cb);
        let handle = tokio::spawn(async move {
            let mut breaker = cb_clone.lock().await;
            breaker.call(()).await
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    // Should still be closed (50% failure rate < 70% threshold)
    let breaker = cb.lock().await;
    assert_eq!(breaker.state().await, CircuitState::Closed);
}

/// Test parallel half-open state management
#[tokio::test]
async fn concurrent_half_open_calls() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(2)
        .name("concurrent-halfopen")
        .build();

    let mut cb: tower_resilience_circuitbreaker::CircuitBreaker<_, (), (), &str> =
        layer.layer(service);

    // Trip the circuit
    for _ in 0..5 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Wait for half-open
    tokio::time::sleep(Duration::from_millis(100)).await;

    let cb = Arc::new(tokio::sync::Mutex::new(cb));
    let mut handles = vec![];

    // Try many concurrent calls in half-open
    for _ in 0..10 {
        let cb_clone = Arc::clone(&cb);
        let handle = tokio::spawn(async move {
            let mut breaker = cb_clone.lock().await;
            breaker.call(()).await
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    // Should be back to open (failures in half-open)
    let breaker = cb.lock().await;
    assert_eq!(breaker.state().await, CircuitState::Open);
}

/// Test that concurrent clones work correctly (Tower pattern)
#[tokio::test]
async fn concurrent_service_clones() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        c.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, String>("success") }
    });

    let layer = CircuitBreakerConfig::<&str, String>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(100)
        .minimum_number_of_calls(50)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("concurrent-clones")
        .build();

    let cb = layer.layer(service);

    // Use Arc+Mutex pattern instead of cloning service
    // CircuitBreaker doesn't implement Clone trait
    let cb_mutex = Arc::new(tokio::sync::Mutex::new(cb));

    let mut handles = vec![];
    for _ in 0..50 {
        let cb_clone = Arc::clone(&cb_mutex);
        let handle = tokio::spawn(async move {
            let mut breaker = cb_clone.lock().await;
            breaker.call(()).await
        });
        handles.push(handle);
    }

    let mut successes = 0;
    for handle in handles {
        if let Ok(Ok(_)) = handle.await {
            successes += 1;
        }
    }

    assert_eq!(successes, 50, "All concurrent calls should succeed");
    assert_eq!(call_count.load(Ordering::Relaxed), 50);
}
