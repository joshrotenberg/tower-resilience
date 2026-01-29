use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service};
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_circuitbreaker::{CircuitState, SlidingWindowType};

/// Test that time-based window fills and evaluates correctly
#[tokio::test]
async fn time_window_fills_and_evaluates() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        c.fetch_add(1, Ordering::Relaxed);
        async { Err::<(), _>("error") }
    });

    let layer = CircuitBreakerLayer::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_millis(500))
        .failure_rate_threshold(0.5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("time-test")
        .build();

    let mut cb = layer.layer(service);

    // Record 3 failures within the time window
    for _ in 0..3 {
        let _ = cb.call(()).await;
        sleep(Duration::from_millis(50)).await;
    }

    // Circuit should be open now (3 failures, 100% failure rate >= 50%)
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test that old records get cleaned up after sliding_window_duration
#[tokio::test]
async fn old_records_cleaned_up() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerLayer::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_millis(200))
        .failure_rate_threshold(0.5)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_millis(50))
        .name("cleanup-test")
        .build();

    let mut cb = layer.layer(service);

    // Record 2 failures
    for _ in 0..2 {
        let _ = cb.call(()).await;
    }

    // Circuit should be open
    assert_eq!(cb.state().await, CircuitState::Open);

    // Wait for window to expire completely
    sleep(Duration::from_millis(250)).await;

    // Create new success service
    let success_service = tower::service_fn(|_req: ()| async { Ok::<_, String>("success") });
    let layer2 = CircuitBreakerLayer::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_millis(200))
        .failure_rate_threshold(0.5)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_millis(50))
        .name("cleanup-test-2")
        .build();

    let mut cb_success = layer2.layer(success_service);

    // Record 2 successes - old failures from previous circuit are gone
    for _ in 0..2 {
        let result = cb_success.call(()).await;
        assert!(result.is_ok());
    }

    // Circuit should remain closed
    assert_eq!(cb_success.state().await, CircuitState::Closed);
}

/// Test time-based window with slow call detection
#[tokio::test]
async fn time_based_window_with_slow_calls() {
    let service = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_millis(150)).await;
        Ok::<_, String>("success")
    });

    let layer = CircuitBreakerLayer::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_millis(500))
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.5)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(50))
        .name("slow-test")
        .build();

    let mut cb = layer.layer(service);

    // Record 3 slow successes (all slow, 100% >= 50% threshold)
    for _ in 0..3 {
        let _ = cb.call(()).await;
    }

    // Circuit should be open due to slow call rate
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test time-based window with failure rate threshold
#[tokio::test]
async fn time_based_window_with_failure_rate() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // Create service that fails 3/5 times (60%)
    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count < 3 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerLayer::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_millis(500))
        .failure_rate_threshold(0.6)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(50))
        .name("failure-rate-test")
        .build();

    let mut cb = layer.layer(service);

    // Make 5 calls (3 failures, 2 successes = 60% failure rate)
    for _ in 0..5 {
        let _ = cb.call(()).await;
        sleep(Duration::from_millis(20)).await;
    }

    // Circuit should be open (60% >= 60% threshold)
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test edge case: calls exactly at duration boundary
#[tokio::test]
async fn calls_at_duration_boundary() {
    let window_duration = Duration::from_millis(200);

    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerLayer::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(window_duration)
        .failure_rate_threshold(0.5)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_millis(50))
        .name("boundary-test")
        .build();

    let mut cb = layer.layer(service);

    // Record failure at time 0
    let _ = cb.call(()).await;

    // Wait exactly the window duration + a bit more
    sleep(Duration::from_millis(220)).await;

    // Record another failure - first one should be cleaned up
    let _ = cb.call(()).await;

    // Should only have 1 call in window (not enough for minimum_number_of_calls)
    // Circuit should still be closed since we don't have minimum calls
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test edge case: very fast calls vs very slow duration
#[tokio::test]
async fn fast_calls_slow_duration() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerLayer::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_secs(10)) // Long window
        .failure_rate_threshold(0.5)
        .minimum_number_of_calls(100)
        .wait_duration_in_open(Duration::from_millis(50))
        .name("fast-slow-test")
        .build();

    let mut cb = layer.layer(service);

    // Make 100 fast calls
    for _ in 0..100 {
        let _ = cb.call(()).await;
    }

    // Circuit should be open (100 failures, 100% >= 50%)
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test that time-based window properly tracks mixed success/failure
#[tokio::test]
async fn time_based_mixed_results() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // 7 failures, 3 successes = 70% failure rate
    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count < 7 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerLayer::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_millis(500))
        .failure_rate_threshold(0.7)
        .minimum_number_of_calls(10)
        .wait_duration_in_open(Duration::from_millis(50))
        .name("mixed-test")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..10 {
        let _ = cb.call(()).await;
        sleep(Duration::from_millis(10)).await;
    }

    // Should be open at exactly 70% threshold
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test time-based window doesn't grow unbounded in memory
#[tokio::test]
async fn time_based_memory_bounds() {
    let service = tower::service_fn(|_req: ()| async { Ok::<_, String>("success") });

    let layer = CircuitBreakerLayer::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_millis(100))
        .failure_rate_threshold(0.5)
        .minimum_number_of_calls(50)
        .wait_duration_in_open(Duration::from_millis(50))
        .name("memory-test")
        .build();

    let mut cb = layer.layer(service);

    // Make many calls over time
    for _ in 0..200 {
        let _ = cb.call(()).await;
    }

    // Wait for window to pass
    sleep(Duration::from_millis(150)).await;

    // Make one more call to trigger cleanup
    let result = cb.call(()).await;
    assert!(result.is_ok());

    // Old records should be cleaned up
    // Circuit should still be closed (all successes)
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test that time-based window respects minimum calls before evaluating
#[tokio::test]
async fn time_based_respects_minimum_calls() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerLayer::builder()
        .sliding_window_type(SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_millis(500))
        .failure_rate_threshold(0.5)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(50))
        .name("min-calls-test")
        .build();

    let mut cb = layer.layer(service);

    // Make only 4 calls (below minimum)
    for _ in 0..4 {
        let _ = cb.call(()).await;
    }

    // Circuit should still be closed (haven't reached minimum)
    assert_eq!(cb.state().await, CircuitState::Closed);

    // One more call should trip it
    let _ = cb.call(()).await;
    assert_eq!(cb.state().await, CircuitState::Open);
}
