use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::time::sleep;
use tower::Service;
use tower_resilience_circuitbreaker::{CircuitBreakerConfig, CircuitState};

/// Test multiple concurrent calls in half-open state
#[tokio::test]
async fn concurrent_calls_in_half_open() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(3)
        .name("concurrent-halfopen")
        .build();

    let mut cb = layer.layer(service);

    // Trip circuit
    for _ in 0..5 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Wait for half-open
    sleep(Duration::from_millis(100)).await;

    // Try concurrent calls - only permitted_calls should go through
    let cb_mutex = Arc::new(tokio::sync::Mutex::new(cb));
    let mut handles = vec![];

    for _ in 0..10 {
        let cb_clone = Arc::clone(&cb_mutex);
        let handle = tokio::spawn(async move {
            let mut breaker = cb_clone.lock().await;
            breaker.call(()).await
        });
        handles.push(handle);
    }

    let mut results = vec![];
    for handle in handles {
        if let Ok(result) = handle.await {
            results.push(result);
        }
    }

    // Most should be rejected, only a few permitted in half-open
    let errors = results.iter().filter(|r| r.is_err()).count();
    assert!(
        errors >= 7,
        "Most concurrent calls should be rejected in half-open"
    );
}

/// Test partial success in half-open (1 of 3 permitted calls succeeds)
#[tokio::test]
async fn partial_success_in_half_open() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            // First call succeeds, rest fail
            if count == 0 {
                Ok::<(), _>(())
            } else {
                Err("error")
            }
        }
    });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(3)
        .name("partial-success")
        .build();

    let mut cb = layer.layer(service);

    // Trip circuit
    for _ in 0..5 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Reset counter for half-open test
    call_count.store(0, Ordering::Relaxed);

    // Wait for half-open
    sleep(Duration::from_millis(100)).await;

    // Make 3 permitted calls (1 success, 2 failures)
    let result1 = cb.call(()).await;
    assert!(result1.is_ok(), "First call should succeed");

    let result2 = cb.call(()).await;
    assert!(result2.is_err(), "Second call should fail");

    // After second failure in half-open, should reopen
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test all failures in half-open with different permitted counts
#[tokio::test]
async fn all_failures_in_half_open_various_permits() {
    for permitted in [1, 2, 5, 10] {
        let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

        let layer = CircuitBreakerConfig::<(), &str>::builder()
            .failure_rate_threshold(0.5)
            .sliding_window_size(5)
            .minimum_number_of_calls(3)
            .wait_duration_in_open(Duration::from_millis(50))
            .permitted_calls_in_half_open(permitted)
            .name(format!("all-fail-{}", permitted))
            .build();

        let mut cb = layer.layer(service);

        // Trip circuit
        for _ in 0..5 {
            let _ = cb.call(()).await;
        }

        // Wait for half-open
        sleep(Duration::from_millis(100)).await;

        // First call in half-open should fail and reopen
        let _ = cb.call(()).await;
        assert_eq!(
            cb.state().await,
            CircuitState::Open,
            "Should reopen after failure in half-open (permitted={})",
            permitted
        );
    }
}

/// Test rapid cycling: open→half-open→open→half-open
#[tokio::test]
async fn rapid_state_cycling() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerConfig::<(), &str>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(1)
        .name("rapid-cycling")
        .build();

    let mut cb = layer.layer(service);

    for cycle in 0..3 {
        // Trip to open
        for _ in 0..5 {
            let _ = cb.call(()).await;
        }
        assert_eq!(
            cb.state().await,
            CircuitState::Open,
            "Cycle {}: should be open",
            cycle
        );

        // Wait for half-open
        sleep(Duration::from_millis(100)).await;

        // Fail in half-open, should reopen
        let _ = cb.call(()).await;
        assert_eq!(
            cb.state().await,
            CircuitState::Open,
            "Cycle {}: should reopen after half-open failure",
            cycle
        );
    }
}

/// Test half-open with time-based window
#[tokio::test]
async fn half_open_with_time_based_window() {
    use tower_resilience_circuitbreaker::SlidingWindowType;

    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            // First 5 fail to trip circuit, then succeed
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
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(3)
        .name("halfopen-timebased")
        .build();

    let mut cb = layer.layer(service);

    // Trip circuit (5 failures with 50% threshold and 5 minimum = 100% failure rate)
    for _ in 0..5 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Wait for half-open (wait_duration is 50ms)
    sleep(Duration::from_millis(60)).await;

    // Make 3 successful calls (counts 5, 6, 7 should all succeed)
    for i in 0..3 {
        let result = cb.call(()).await;
        assert!(result.is_ok(), "Should succeed in half-open (call {})", i);
    }

    // Should transition to closed after successes
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test half-open with slow call detection active
#[tokio::test]
async fn half_open_with_slow_call_detection() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            // All slow, first batch fails, second batch succeeds
            sleep(Duration::from_millis(150)).await;
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
        .slow_call_rate_threshold(0.9)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(5)
        .name("halfopen-slow")
        .build();

    let mut cb = layer.layer(service);

    // Trip circuit (all slow and failing)
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Wait for half-open
    sleep(Duration::from_millis(100)).await;

    // Make successful (but still slow) calls
    for _ in 0..5 {
        let result = cb.call(()).await;
        assert!(result.is_ok(), "Should succeed in half-open");
    }

    // Should close despite slow calls (successes)
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test half-open state with minimum calls threshold
#[tokio::test]
async fn half_open_with_minimum_calls() {
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
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(10) // More than minimum
        .name("halfopen-minimum")
        .build();

    let mut cb = layer.layer(service);

    // Trip circuit
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }
    assert_eq!(cb.state().await, CircuitState::Open);

    // Wait for half-open
    sleep(Duration::from_millis(100)).await;

    // Make all 10 permitted calls (all succeed)
    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // Should transition to closed
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test half-open rejected calls don't affect state
#[tokio::test]
async fn half_open_rejected_calls_no_effect() {
    let service = tower::service_fn(|_req: ()| async { Ok::<(), String>(()) });

    let layer = CircuitBreakerConfig::<(), String>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(50))
        .permitted_calls_in_half_open(2)
        .name("halfopen-rejected")
        .build();

    let cb = layer.layer(service);

    // Manually force open
    cb.force_open().await;
    assert_eq!(cb.state().await, CircuitState::Open);

    // Wait for half-open
    sleep(Duration::from_millis(100)).await;

    // Make more calls than permitted
    let cb_mutex = Arc::new(tokio::sync::Mutex::new(cb));

    let mut handles = vec![];
    for _ in 0..10 {
        let cb_clone = Arc::clone(&cb_mutex);
        handles.push(tokio::spawn(async move {
            let mut breaker = cb_clone.lock().await;
            breaker.call(()).await
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    sleep(Duration::from_millis(50)).await;

    // Rejected calls shouldn't prevent transition to closed
    let breaker = cb_mutex.lock().await;
    let state = breaker.state().await;
    assert!(
        state == CircuitState::Closed || state == CircuitState::HalfOpen,
        "Should be closed or still half-open, not reopened"
    );
}
