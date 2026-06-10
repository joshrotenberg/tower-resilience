//! Backoff strategy tests for tower-retry-plus.
//!
//! Tests different backoff behaviors including:
//! - Fixed interval consistency
//! - Exponential growth with various multipliers
//! - Exponential random variance bounds
//! - Custom function intervals
//! - Max backoff duration enforcement

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tower::{Layer, Service, ServiceExt};
use tower_resilience_retry::{
    ExponentialBackoff, ExponentialRandomBackoff, FixedInterval, FnInterval, IntervalFunction,
    RetryLayer,
};

#[derive(Debug, Clone)]
struct TestError;

#[tokio::test]
async fn fixed_interval_consistent_delays() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);
    let timestamps = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ts = Arc::clone(&timestamps);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        let ts = Arc::clone(&ts);
        async move {
            ts.lock().unwrap().push(Instant::now());
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 3 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .backoff(FixedInterval::new(Duration::from_millis(50)))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    let times = timestamps.lock().unwrap();
    assert_eq!(times.len(), 4); // 1 initial + 3 retries

    // Check delays are at least the configured minimum (50ms)
    // Upper bound is generous for CI environments with variable load
    for i in 1..times.len() {
        let delay = times[i].duration_since(times[i - 1]);
        assert!(
            delay >= Duration::from_millis(40) && delay <= Duration::from_millis(200),
            "Expected delay around 50ms (got {:?} at attempt {})",
            delay,
            i
        );
    }
}

#[tokio::test]
async fn exponential_backoff_doubles_delay() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);
    let timestamps = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ts = Arc::clone(&timestamps);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        let ts = Arc::clone(&ts);
        async move {
            ts.lock().unwrap().push(Instant::now());
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 3 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .backoff(ExponentialBackoff::new(Duration::from_millis(50)))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    let times = timestamps.lock().unwrap();
    assert_eq!(times.len(), 4);

    // Upper bounds are generous (~3-4x expected) -- wall-clock scheduling on
    // busy CI runners regularly overshoots tight windows. Lower bounds verify
    // the backoff actually fires. See #301.

    // First retry: ~50ms
    let delay1 = times[1].duration_since(times[0]);
    assert!(
        delay1 >= Duration::from_millis(20) && delay1 <= Duration::from_millis(200),
        "Expected first delay ~50ms, got {:?}",
        delay1
    );

    // Second retry: ~100ms (50 * 2^1)
    let delay2 = times[2].duration_since(times[1]);
    assert!(
        delay2 >= Duration::from_millis(70) && delay2 <= Duration::from_millis(300),
        "Expected second delay ~100ms, got {:?}",
        delay2
    );

    // Third retry: ~200ms (50 * 2^2)
    let delay3 = times[3].duration_since(times[2]);
    assert!(
        delay3 >= Duration::from_millis(170) && delay3 <= Duration::from_millis(500),
        "Expected third delay ~200ms, got {:?}",
        delay3
    );
}

#[tokio::test]
async fn exponential_backoff_custom_multiplier() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);
    let timestamps = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ts = Arc::clone(&timestamps);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        let ts = Arc::clone(&ts);
        async move {
            ts.lock().unwrap().push(Instant::now());
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(4)
        .backoff(ExponentialBackoff::new(Duration::from_millis(50)).multiplier(3.0))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    let times = timestamps.lock().unwrap();
    assert_eq!(times.len(), 3);

    // First retry: ~50ms
    let delay1 = times[1].duration_since(times[0]);
    assert!(
        delay1 >= Duration::from_millis(20) && delay1 <= Duration::from_millis(200),
        "Expected first delay ~50ms, got {:?}",
        delay1
    );

    // Second retry: ~150ms (50 * 3^1)
    let delay2 = times[2].duration_since(times[1]);
    assert!(
        delay2 >= Duration::from_millis(120) && delay2 <= Duration::from_millis(400),
        "Expected second delay ~150ms, got {:?}",
        delay2
    );
}

#[tokio::test]
async fn exponential_backoff_respects_max_interval() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);
    let timestamps = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ts = Arc::clone(&timestamps);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        let ts = Arc::clone(&ts);
        async move {
            ts.lock().unwrap().push(Instant::now());
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 4 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(6)
        .backoff(
            ExponentialBackoff::new(Duration::from_millis(50))
                .max_interval(Duration::from_millis(150)),
        )
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    let times = timestamps.lock().unwrap();
    assert_eq!(times.len(), 5);

    // First retry: ~50ms
    let delay1 = times[1].duration_since(times[0]);
    assert!(
        delay1 >= Duration::from_millis(20) && delay1 <= Duration::from_millis(200),
        "First delay should be ~50ms, got {:?}",
        delay1
    );

    // Second retry: ~100ms
    let delay2 = times[2].duration_since(times[1]);
    assert!(
        delay2 >= Duration::from_millis(70) && delay2 <= Duration::from_millis(300),
        "Second delay should be ~100ms, got {:?}",
        delay2
    );

    // Third retry: capped at ~150ms (would be 200 without cap)
    // Use generous tolerance for CI environments with variable timing
    let delay3 = times[3].duration_since(times[2]);
    assert!(
        delay3 >= Duration::from_millis(100) && delay3 <= Duration::from_millis(400),
        "Third delay should be capped at ~150ms, got {:?}",
        delay3
    );

    // Fourth retry: still capped at ~150ms
    let delay4 = times[4].duration_since(times[3]);
    assert!(
        delay4 >= Duration::from_millis(100) && delay4 <= Duration::from_millis(400),
        "Fourth delay should be capped at ~150ms, got {:?}",
        delay4
    );
}

#[tokio::test]
async fn exponential_random_backoff_has_variance() {
    // This test verifies that the randomized backoff actually randomizes.
    //
    // It used to drive a real retrying service and measure wall-clock gaps
    // between attempts, then assert variance and range on those measured
    // delays. That approach was inherently flaky on loaded CI runners
    // (scheduler jitter / timer granularity), and the upper wall-clock bound
    // had already been widened once (#301) yet still flaked (#337).
    //
    // Instead we now exercise the backoff's *computed* delays directly via the
    // public IntervalFunction::next_interval API -- the same way the crate's
    // own unit tests do. This removes scheduler/timer dependence entirely
    // while preserving the test's intent (randomized backoff varies).
    let backoff = ExponentialRandomBackoff::new(Duration::from_millis(100), 0.5);

    // Compute the delay for the same attempt many times. attempt 0 has a base
    // of 100ms (100 * 2^0); with a 0.5 randomization factor the result is
    // nominally in 50ms..=150ms. This mirrors the first retry delay the old
    // timing-based version measured.
    let delays: Vec<Duration> = (0..50).map(|_| backoff.next_interval(0)).collect();

    // Variance: a correct randomized implementation must not return the same
    // value every time. Assert "at least 2 unique values" rather than a
    // numeric variance threshold -- effectively impossible to fail for a
    // correct impl, and no longer probabilistic in any timing sense.
    let mut unique_delays = delays.clone();
    unique_delays.sort();
    unique_delays.dedup();
    assert!(
        unique_delays.len() >= 2,
        "Randomized backoff should produce at least 2 different delays, got {} unique values from {} samples",
        unique_delays.len(),
        delays.len()
    );

    // Range: every computed delay must fall within the nominal randomized
    // band (50ms..=150ms for attempt 0 with a 0.5 factor). Because these are
    // computed (not wall-clock measured) values, the bounds can be exact --
    // no CI scheduling slop to absorb.
    for delay in &delays {
        assert!(
            *delay >= Duration::from_millis(50) && *delay <= Duration::from_millis(150),
            "Delay {:?} outside expected randomized range (50ms..=150ms)",
            delay
        );
    }
}

#[tokio::test]
async fn exponential_random_backoff_respects_max() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);
    let timestamps = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ts = Arc::clone(&timestamps);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        let ts = Arc::clone(&ts);
        async move {
            ts.lock().unwrap().push(Instant::now());
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 3 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .backoff(
            ExponentialRandomBackoff::new(Duration::from_millis(50), 0.3)
                .max_interval(Duration::from_millis(100)),
        )
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    let times = timestamps.lock().unwrap();
    assert_eq!(times.len(), 4);

    // Third retry should be capped at ~100ms (cap=100ms ±30% jitter, so
    // ~70-130ms ideal). Uncapped would be ~200ms ±30% (~140-260ms). Upper
    // bound 200ms cleanly distinguishes capped from uncapped while leaving
    // room for CI scheduling slop on top of the capped value. See #301.
    let delay3 = times[3].duration_since(times[2]);
    assert!(
        delay3 <= Duration::from_millis(200),
        "Delay should be capped with randomization, got {:?}",
        delay3
    );
}

#[tokio::test]
async fn custom_function_interval_linear_growth() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);
    let timestamps = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ts = Arc::clone(&timestamps);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        let ts = Arc::clone(&ts);
        async move {
            ts.lock().unwrap().push(Instant::now());
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 3 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    // Linear backoff: 50ms, 100ms, 150ms, ...
    let config = RetryLayer::builder()
        .max_attempts(5)
        .backoff(FnInterval::new(|attempt| {
            Duration::from_millis(50 * (attempt as u64 + 1))
        }))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    let times = timestamps.lock().unwrap();
    assert_eq!(times.len(), 4);

    // Upper bounds are generous (~2x expected) -- wall-clock scheduling on
    // busy CI runners regularly overshoots tight windows. Lower bounds verify
    // the backoff actually fires; upper bounds just guard against runaway
    // delays.

    // First retry: ~50ms
    let delay1 = times[1].duration_since(times[0]);
    assert!(
        delay1 >= Duration::from_millis(20) && delay1 <= Duration::from_millis(150),
        "Expected ~50ms, got {:?}",
        delay1
    );

    // Second retry: ~100ms
    let delay2 = times[2].duration_since(times[1]);
    assert!(
        delay2 >= Duration::from_millis(70) && delay2 <= Duration::from_millis(250),
        "Expected ~100ms, got {:?}",
        delay2
    );

    // Third retry: ~150ms
    let delay3 = times[3].duration_since(times[2]);
    assert!(
        delay3 >= Duration::from_millis(120) && delay3 <= Duration::from_millis(350),
        "Expected ~150ms, got {:?}",
        delay3
    );
}

#[tokio::test]
async fn custom_function_interval_fibonacci() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);
    let timestamps = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ts = Arc::clone(&timestamps);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        let ts = Arc::clone(&ts);
        async move {
            ts.lock().unwrap().push(Instant::now());
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 4 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    // Fibonacci backoff: 10, 10, 20, 30, 50, ...
    let config = RetryLayer::builder()
        .max_attempts(6)
        .backoff(FnInterval::new(|attempt| {
            let fib = match attempt {
                0 => 1,
                1 => 1,
                n => {
                    let mut a = 1u64;
                    let mut b = 1u64;
                    for _ in 2..=n {
                        let temp = a + b;
                        a = b;
                        b = temp;
                    }
                    b
                }
            };
            Duration::from_millis(10 * fib)
        }))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    let times = timestamps.lock().unwrap();
    assert_eq!(times.len(), 5);

    // Delays should follow fibonacci pattern (with tolerance)
    // attempt 0: 10ms, attempt 1: 10ms, attempt 2: 20ms, attempt 3: 30ms
}

#[tokio::test]
async fn custom_function_interval_constant() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    // Constant 100ms regardless of attempt
    let config = RetryLayer::builder()
        .max_attempts(4)
        .backoff(FnInterval::new(|_| Duration::from_millis(100)))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_ok());
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn zero_backoff_retries_immediately() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);
    let timestamps = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ts = Arc::clone(&timestamps);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        let ts = Arc::clone(&ts);
        async move {
            ts.lock().unwrap().push(Instant::now());
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(4)
        .backoff(FixedInterval::new(Duration::from_millis(0)))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let start = Instant::now();
    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;
    let elapsed = start.elapsed();

    // With zero backoff, three calls should complete very quickly. Upper
    // bound generous for CI scheduling slop; see #301.
    assert!(
        elapsed < Duration::from_millis(200),
        "Zero backoff should complete quickly, took {:?}",
        elapsed
    );
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}
