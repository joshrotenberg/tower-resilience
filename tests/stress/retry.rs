//! Retry stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_retry::RetryLayer;

use super::{ConcurrencyTracker, get_memory_usage_mb};

/// Test: 1 million calls through retry layer (all succeed on first attempt)
#[tokio::test]
#[ignore]
async fn stress_one_million_calls_no_retries() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, String>(()) }
    });

    let layer = RetryLayer::<String>::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::from_millis(10))
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..1_000_000 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("1M calls (no retries) completed in {:?}", elapsed);
    println!(
        "Throughput: {:.0} calls/sec",
        1_000_000.0 / elapsed.as_secs_f64()
    );
    println!("Actual service calls: {}", actual_calls);

    assert_eq!(actual_calls, 1_000_000);
}

/// Test: High volume with consistent failures requiring retries
#[tokio::test]
#[ignore]
async fn stress_high_volume_with_retries() {
    let attempt_count = Arc::new(AtomicUsize::new(0));
    let success_count = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&attempt_count);
    let successes = Arc::clone(&success_count);

    let svc = tower::service_fn(move |_req: u32| {
        let attempts = Arc::clone(&attempts);
        let successes = Arc::clone(&successes);
        async move {
            let attempt = attempts.fetch_add(1, Ordering::Relaxed);
            // Fail first 2 attempts, succeed on 3rd
            if attempt % 3 < 2 {
                Err("transient failure".to_string())
            } else {
                successes.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
        }
    });

    let layer = RetryLayer::<String>::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_micros(100))
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..10_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok(), "All retries should eventually succeed");
    }

    let elapsed = start.elapsed();
    let total_attempts = attempt_count.load(Ordering::Relaxed);
    let successes = success_count.load(Ordering::Relaxed);

    println!("10k calls with retries completed in {elapsed:?}");
    println!("Total service attempts: {total_attempts}");
    println!("Successful calls: {successes}");
    let avg_attempts = total_attempts as f64 / 10_000.0;
    println!("Average attempts per call: {avg_attempts:.2}");

    assert_eq!(successes, 10_000);
    // Should be ~30k attempts (3 per call)
    assert!((20_000..=40_000).contains(&total_attempts));
}

/// Test: Exponential backoff under load
#[tokio::test]
#[ignore]
async fn stress_exponential_backoff_timing() {
    let attempt_count = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&attempt_count);

    let svc = tower::service_fn(move |_req: u32| {
        let attempts = Arc::clone(&attempts);
        async move {
            let attempt = attempts.fetch_add(1, Ordering::Relaxed);
            // Fail first 3 attempts, succeed on 4th
            if attempt % 4 < 3 {
                Err("transient failure".to_string())
            } else {
                Ok(())
            }
        }
    });

    let layer = RetryLayer::<String>::builder()
        .max_attempts(5)
        .exponential_backoff(Duration::from_millis(1))
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..1_000 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let total_attempts = attempt_count.load(Ordering::Relaxed);

    println!("1k calls with exponential backoff in {elapsed:?}");
    println!("Total service attempts: {total_attempts}");
    let avg_attempts = total_attempts as f64 / 1_000.0;
    println!("Average attempts per call: {avg_attempts:.2}");

    // Should be ~4k attempts (4 per call)
    assert!((3_500..=4_500).contains(&total_attempts));
}

/// Test: High concurrency with retries
#[tokio::test]
#[ignore]
async fn stress_high_concurrency_with_retries() {
    let tracker = ConcurrencyTracker::new();
    let attempt_count = Arc::new(AtomicUsize::new(0));
    let tracker_clone = Arc::clone(&tracker);
    let attempts = Arc::clone(&attempt_count);

    let svc = tower::service_fn(move |_req: u32| {
        let tracker = Arc::clone(&tracker_clone);
        let attempts = Arc::clone(&attempts);
        async move {
            tracker.enter();
            let attempt = attempts.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(5)).await;
            tracker.exit();

            // Fail first attempt, succeed on second
            if attempt.is_multiple_of(2) {
                Err("transient failure".to_string())
            } else {
                Ok(())
            }
        }
    });

    let layer = RetryLayer::<String>::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::from_millis(1))
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
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let peak = tracker.peak();
    let total_attempts = attempt_count.load(Ordering::Relaxed);

    println!("1000 concurrent requests with retries in {:?}", elapsed);
    println!("Peak concurrency: {}", peak);
    println!("Total attempts: {}", total_attempts);

    assert!(peak > 100, "Expected high concurrency");
    // Should be ~2k attempts (2 per call)
    assert!((1_800..=2_200).contains(&total_attempts));
}

/// Test: Retry exhaustion scenarios
#[tokio::test]
#[ignore]
async fn stress_retry_exhaustion() {
    let attempt_count = Arc::new(AtomicUsize::new(0));
    let exhausted_count = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&attempt_count);

    let svc = tower::service_fn(move |req: u32| {
        let attempts = Arc::clone(&attempts);
        async move {
            attempts.fetch_add(1, Ordering::Relaxed);
            // 20% of requests always fail
            if req.is_multiple_of(5) {
                Err("permanent failure".to_string())
            } else {
                Ok(())
            }
        }
    });

    let layer = RetryLayer::<String>::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::from_micros(100))
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..10_000 {
        let result = service.ready().await.unwrap().call(i).await;
        if result.is_err() {
            exhausted_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    let elapsed = start.elapsed();
    let total_attempts = attempt_count.load(Ordering::Relaxed);
    let exhausted = exhausted_count.load(Ordering::Relaxed);

    println!("10k calls with exhaustion in {:?}", elapsed);
    println!("Total service attempts: {}", total_attempts);
    println!("Exhausted retries: {}", exhausted);
    println!(
        "Success rate: {:.1}%",
        100.0 - (exhausted as f64 / 10_000.0 * 100.0)
    );

    // Should have ~2000 exhausted (20% of 10k)
    assert!((1_800..=2_200).contains(&exhausted));
    // Exhausted calls should have made max_attempts (3), successful made 1
    // Total: 2000 * 3 + 8000 * 1 = 14000
    assert!((13_000..=15_000).contains(&total_attempts));
}

/// Test: Memory stability over extended period with retries
#[tokio::test]
#[ignore]
async fn stress_memory_stability() {
    let mem_start = get_memory_usage_mb();
    let attempt_count = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&attempt_count);

    let svc = tower::service_fn(move |_req: u32| {
        let attempts = Arc::clone(&attempts);
        async move {
            let attempt = attempts.fetch_add(1, Ordering::Relaxed);
            // Fail first 2 attempts, succeed on 3rd
            if attempt % 3 < 2 {
                Err("transient failure".to_string())
            } else {
                Ok(())
            }
        }
    });

    let layer = RetryLayer::<String>::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_micros(100))
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
        if i.is_multiple_of(1000) {
            let mem = get_memory_usage_mb();
            if mem > 0.0 {
                mem_samples.push(mem);
            }
        }
    }

    let mem_end = get_memory_usage_mb();
    let total_attempts = attempt_count.load(Ordering::Relaxed);

    println!("Ran {} calls over 10 seconds", i);
    println!("Total attempts: {}", total_attempts);
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

/// Test: Custom retry predicate under load
#[tokio::test]
#[ignore]
async fn stress_custom_retry_predicate() {
    let attempt_count = Arc::new(AtomicUsize::new(0));
    let retryable_failures = Arc::new(AtomicUsize::new(0));
    let permanent_failures = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&attempt_count);

    let svc = tower::service_fn(move |req: u32| {
        let attempts = Arc::clone(&attempts);
        async move {
            let attempt = attempts.fetch_add(1, Ordering::Relaxed);
            match req % 10 {
                // 10% permanent failures (non-retryable)
                0 => Err("PERMANENT".to_string()),
                // 20% transient failures (retryable, succeed on retry)
                1 | 2 if attempt.is_multiple_of(2) => Err("TRANSIENT".to_string()),
                // 70% success
                _ => Ok(()),
            }
        }
    });

    let retryable = Arc::clone(&retryable_failures);
    let permanent = Arc::clone(&permanent_failures);

    let layer = RetryLayer::<String>::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::from_micros(100))
        .retry_on(move |err: &String| {
            if err.starts_with("PERMANENT") {
                permanent.fetch_add(1, Ordering::Relaxed);
                false
            } else {
                retryable.fetch_add(1, Ordering::Relaxed);
                true
            }
        })
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();
    let mut success = 0;
    let mut failed = 0;

    for i in 0..10_000 {
        match service.ready().await.unwrap().call(i).await {
            Ok(_) => success += 1,
            Err(_) => failed += 1,
        }
    }

    let elapsed = start.elapsed();
    let total_attempts = attempt_count.load(Ordering::Relaxed);
    let retryable = retryable_failures.load(Ordering::Relaxed);
    let permanent = permanent_failures.load(Ordering::Relaxed);

    println!("10k calls with custom predicate in {:?}", elapsed);
    println!("Success: {}, Failed: {}", success, failed);
    println!("Total attempts: {}", total_attempts);
    println!("Retryable failures seen: {}", retryable);
    println!("Permanent failures seen: {}", permanent);

    // Should have ~1000 permanent failures (10%)
    assert!((800..=1_200).contains(&permanent));
    // Rest should succeed
    assert!((8_500..=9_200).contains(&success));
}
