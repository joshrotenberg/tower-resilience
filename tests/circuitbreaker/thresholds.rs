use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service};
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_circuitbreaker::CircuitState;

/// Test failure rate exactly at threshold (0.5 with 0.5 threshold)
#[tokio::test]
async fn failure_rate_exactly_at_threshold() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // 5 failures, 5 successes = exactly 50%
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

    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(10)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("exact-threshold")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // At exactly 50%, should trip (>= threshold)
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test failure rate just below threshold
#[tokio::test]
async fn failure_rate_just_below_threshold() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // 4 failures, 6 successes = 40% (below 50%)
    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count < 4 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(10)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("below-threshold")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // 40% < 50%, should stay closed
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test failure rate just above threshold
#[tokio::test]
async fn failure_rate_just_above_threshold() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // 6 failures, 4 successes = 60% (above 50%)
    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count < 6 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(10)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("above-threshold")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // 60% > 50%, should trip
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test slow call rate exactly at threshold
#[tokio::test]
async fn slow_call_rate_exactly_at_threshold() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // 5 slow, 5 fast = exactly 50%
    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count < 5 {
                sleep(Duration::from_millis(150)).await;
            } else {
                sleep(Duration::from_millis(50)).await;
            }
            Ok::<_, String>("success")
        }
    });

    let layer = CircuitBreakerLayer::builder()
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(10)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("slow-exact-threshold")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // Exactly 50% slow, should trip
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test call duration exactly at slow call threshold
#[tokio::test]
async fn duration_exactly_at_slow_threshold() {
    let service = tower::service_fn(|_req: ()| async {
        // Exactly 100ms (at threshold)
        sleep(Duration::from_millis(100)).await;
        Ok::<_, String>("success")
    });

    let layer = CircuitBreakerLayer::builder()
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("duration-exact")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..5 {
        let _ = cb.call(()).await;
    }

    // All calls at exactly threshold, should count as slow
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test call duration below slow call threshold
#[tokio::test]
async fn duration_just_below_slow_threshold() {
    let service = tower::service_fn(|_req: ()| async {
        // 50ms (well below 100ms threshold to account for timing variance)
        sleep(Duration::from_millis(50)).await;
        Ok::<_, String>("success")
    });

    let layer = CircuitBreakerLayer::builder()
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("duration-below")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..5 {
        let _ = cb.call(()).await;
    }

    // All calls well below threshold, 0% slow rate < 50%
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test call duration 1ms above slow call threshold
#[tokio::test]
async fn duration_just_above_slow_threshold() {
    let service = tower::service_fn(|_req: ()| async {
        // 101ms (just above 100ms threshold)
        sleep(Duration::from_millis(101)).await;
        Ok::<_, String>("success")
    });

    let layer = CircuitBreakerLayer::builder()
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.5)
        .sliding_window_size(5)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("duration-above")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..5 {
        let _ = cb.call(()).await;
    }

    // All calls above threshold, 100% slow rate > 50%
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test combined thresholds - both failure rate and slow call rate
#[tokio::test]
async fn combined_failure_and_slow_thresholds() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // Mix: some fast failures, some slow successes, some fast successes
    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            match count {
                0..=2 => {
                    // Fast failures (30%)
                    sleep(Duration::from_millis(50)).await;
                    Err::<(), _>("error")
                }
                3..=5 => {
                    // Slow successes (30%)
                    sleep(Duration::from_millis(150)).await;
                    Ok(())
                }
                _ => {
                    // Fast successes (40%)
                    sleep(Duration::from_millis(50)).await;
                    Ok(())
                }
            }
        }
    });

    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5) // 30% failure rate < 50%
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.4) // 30% slow rate < 40%
        .sliding_window_size(10)
        .minimum_number_of_calls(10)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("combined-thresholds")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..10 {
        let _ = cb.call(()).await;
    }

    // Both rates below thresholds, should stay closed
    assert_eq!(cb.state().await, CircuitState::Closed);
}

/// Test that either threshold can trip the circuit
#[tokio::test]
async fn either_threshold_can_trip() {
    let service = tower::service_fn(|_req: ()| async {
        // All slow successes (0% failure, 100% slow)
        sleep(Duration::from_millis(150)).await;
        Ok::<_, String>("success")
    });

    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5) // 0% < 50%, would not trip
        .slow_call_duration_threshold(Duration::from_millis(100))
        .slow_call_rate_threshold(0.5) // 100% >= 50%, will trip
        .sliding_window_size(5)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("either-trips")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..5 {
        let _ = cb.call(()).await;
    }

    // Slow call rate trips it even though failure rate is 0%
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test precision with floating point thresholds
#[tokio::test]
async fn floating_point_precision() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&call_count);

    // 1 failure, 2 successes = 33.333...%
    let service = tower::service_fn(move |_req: ()| {
        let count = c.fetch_add(1, Ordering::Relaxed);
        async move {
            if count == 0 {
                Err::<(), _>("error")
            } else {
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.33) // 33.333...% > 33%
        .sliding_window_size(3)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("float-precision")
        .build();

    let mut cb = layer.layer(service);

    for _ in 0..3 {
        let _ = cb.call(()).await;
    }

    // 1/3 = 0.333... which is >= 0.33
    assert_eq!(cb.state().await, CircuitState::Open);
}

/// Test threshold with very small window
#[tokio::test]
async fn threshold_small_window() {
    let service = tower::service_fn(|_req: ()| async { Err::<(), _>("error") });

    let layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.99) // Very high threshold
        .sliding_window_size(2)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_millis(100))
        .name("small-window")
        .build();

    let mut cb = layer.layer(service);

    // 2 failures = 100% >= 99%
    for _ in 0..2 {
        let _ = cb.call(()).await;
    }

    assert_eq!(cb.state().await, CircuitState::Open);
}
