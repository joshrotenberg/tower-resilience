//! Circuit breaker stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Service, ServiceExt};
use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitState};

use super::{ConcurrencyTracker, get_memory_usage_mb};

/// Test: 1 million calls through circuit breaker
#[tokio::test]
#[ignore]
async fn stress_one_million_calls() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, ()>(()) }
    });

    let layer = CircuitBreakerLayer::<(), ()>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(100)
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..1_000_000 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("1M calls completed in {:?}", elapsed);
    println!(
        "Throughput: {:.0} calls/sec",
        1_000_000.0 / elapsed.as_secs_f64()
    );
    println!("Actual service calls: {}", actual_calls);

    assert_eq!(actual_calls, 1_000_000);
}

/// Test: Rapid state transitions (thrashing)
#[tokio::test]
#[ignore]
async fn stress_rapid_state_transitions() {
    let svc =
        tower::service_fn(|req: bool| async move { if req { Ok::<_, ()>(()) } else { Err(()) } });

    let layer = CircuitBreakerLayer::<(), ()>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .wait_duration_in_open(Duration::from_millis(10))
        .permitted_calls_in_half_open(1)
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();
    let mut transitions = 0;
    let mut last_state = service.state_sync();

    // Alternate between success and failure to cause state transitions
    for i in 0..10_000 {
        let req = i % 20 < 10; // 10 failures, then 10 successes
        let _ = service.ready().await.unwrap().call(req).await;

        let current_state = service.state_sync();
        if current_state != last_state {
            transitions += 1;
            last_state = current_state;
        }

        // Small delay to allow state transitions
        if i % 100 == 0 {
            sleep(Duration::from_millis(1)).await;
        }
    }

    let elapsed = start.elapsed();
    println!("10k calls with state transitions in {:?}", elapsed);
    println!("State transitions observed: {}", transitions);
    println!("Final state: {:?}", service.state_sync());

    // Should have seen multiple transitions
    assert!(transitions > 10, "Expected multiple state transitions");
}

/// Test: High concurrency (1000 concurrent requests)
#[tokio::test]
#[ignore]
async fn stress_high_concurrency() {
    let tracker = ConcurrencyTracker::new();
    let tracker_clone = Arc::clone(&tracker);

    let svc = tower::service_fn(move |_req: u32| {
        let tracker = Arc::clone(&tracker_clone);
        async move {
            tracker.enter();
            sleep(Duration::from_millis(10)).await;
            tracker.exit();
            Ok::<_, ()>(())
        }
    });

    let layer = CircuitBreakerLayer::<(), ()>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(1000)
        .build();

    let service = layer.layer(svc);

    let start = Instant::now();
    let mut handles = vec![];

    for i in 0..1000 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(i).await
        }));
    }

    for handle in handles {
        let _ = handle.await.unwrap();
    }

    let elapsed = start.elapsed();
    let peak = tracker.peak();

    println!("1000 concurrent requests in {:?}", elapsed);
    println!("Peak concurrency: {}", peak);

    // All requests should complete
    assert!(peak > 100, "Expected high concurrency");
}

/// Test: Large sliding window (10k calls)
#[tokio::test]
#[ignore]
async fn stress_large_sliding_window() {
    let mem_start = get_memory_usage_mb();

    let svc = tower::service_fn(|_req: u32| async { Ok::<_, ()>(()) });

    let layer = CircuitBreakerLayer::<(), ()>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10_000)
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    // Fill the window
    for i in 0..20_000 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let mem_end = get_memory_usage_mb();
    let mem_delta = mem_end - mem_start;

    println!("20k calls with 10k window in {:?}", elapsed);
    println!("Memory delta: {:.2} MB", mem_delta);

    // Memory usage should be reasonable (< 50 MB)
    if mem_delta > 0.0 {
        assert!(
            mem_delta < 50.0,
            "Memory usage too high: {:.2} MB",
            mem_delta
        );
    }
}

/// Test: Time-based window under load
#[tokio::test]
#[ignore]
async fn stress_time_based_window_high_load() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, ()>(()) }
    });

    let layer = CircuitBreakerLayer::<(), ()>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_type(tower_resilience_circuitbreaker::SlidingWindowType::TimeBased)
        .sliding_window_duration(Duration::from_secs(1))
        .minimum_number_of_calls(100)
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    // High load for 3 seconds
    let mut i = 0u32;
    while start.elapsed() < Duration::from_secs(3) {
        let _ = service.ready().await.unwrap().call(i).await;
        i += 1;
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("Time-based window: {} calls in {:?}", actual_calls, elapsed);
    println!(
        "Throughput: {:.0} calls/sec",
        actual_calls as f64 / elapsed.as_secs_f64()
    );

    assert!(actual_calls > 1000, "Expected high throughput");
}

/// Test: Mixed success/failure under load
#[tokio::test]
#[ignore]
async fn stress_mixed_results_high_volume() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let failure_count = Arc::new(AtomicUsize::new(0));
    let rejected_count = Arc::new(AtomicUsize::new(0));

    let success = Arc::clone(&success_count);
    let failure = Arc::clone(&failure_count);

    let svc = tower::service_fn(move |req: u32| {
        let success = Arc::clone(&success);
        let failure = Arc::clone(&failure);
        async move {
            // 30% failure rate
            if req % 10 < 3 {
                failure.fetch_add(1, Ordering::Relaxed);
                Err(())
            } else {
                success.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
        }
    });

    let layer = CircuitBreakerLayer::<(), ()>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(100)
        .wait_duration_in_open(Duration::from_millis(100))
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..100_000 {
        match service.ready().await.unwrap().call(i).await {
            Ok(_) => {}
            Err(_) => {
                if service.state_sync() == CircuitState::Open {
                    rejected_count.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    let elapsed = start.elapsed();
    let successes = success_count.load(Ordering::Relaxed);
    let failures = failure_count.load(Ordering::Relaxed);
    let rejected = rejected_count.load(Ordering::Relaxed);

    println!("100k mixed calls in {:?}", elapsed);
    println!("Successes: {}", successes);
    println!("Failures: {}", failures);
    println!("Rejected (circuit open): {}", rejected);
    println!("Final state: {:?}", service.state_sync());

    assert!(successes > 0);
    assert!(failures > 0);
}

/// Test: Memory stability over extended period
#[tokio::test]
#[ignore]
async fn stress_memory_stability() {
    let mem_start = get_memory_usage_mb();

    let svc =
        tower::service_fn(|req: u32| async move { if req % 10 < 3 { Err(()) } else { Ok(()) } });

    let layer = CircuitBreakerLayer::<(), ()>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(1000)
        .wait_duration_in_open(Duration::from_millis(50))
        .build();

    let mut service = layer.layer(svc);

    let mut mem_samples = vec![];

    // Run for 10 seconds
    let start = Instant::now();
    let mut i = 0u32;

    while start.elapsed() < Duration::from_secs(10) {
        let _ = service.ready().await.unwrap().call(i).await;
        i += 1;

        // Sample memory every 1000 calls
        if i % 1000 == 0 {
            let mem = get_memory_usage_mb();
            if mem > 0.0 {
                mem_samples.push(mem);
            }
        }
    }

    let mem_end = get_memory_usage_mb();

    println!("Ran {} calls over 10 seconds", i);
    println!("Memory start: {:.2} MB", mem_start);
    println!("Memory end: {:.2} MB", mem_end);

    if !mem_samples.is_empty() {
        let mem_max = mem_samples.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let mem_min = mem_samples.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        println!("Memory range: {:.2} - {:.2} MB", mem_min, mem_max);

        // Memory shouldn't grow unbounded (allow 100 MB growth)
        if mem_end > mem_start {
            assert!(mem_end - mem_start < 100.0, "Memory leak suspected");
        }
    }
}
