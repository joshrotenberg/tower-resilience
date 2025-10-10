//! Composition stress tests - multiple patterns stacked together

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_bulkhead::BulkheadLayer;
use tower_resilience_cache::CacheLayer;
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_ratelimiter::RateLimiterLayer;
use tower_resilience_retry::RetryLayer;
use tower_resilience_timelimiter::TimeLimiterLayer;

use super::{ConcurrencyTracker, get_memory_usage_mb};

/// Test: All patterns composed (full stack)
#[tokio::test]
#[ignore]
async fn stress_full_stack_composition() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            if req % 50 == 0 {
                Err(()) // Occasional failure
            } else {
                Ok(format!("response-{}", req))
            }
        }
    });

    // Stack all patterns
    let cache = CacheLayer::builder()
        .max_size(100)
        .key_extractor(|req: &u32| *req)
        .build();

    let rate_limiter = RateLimiterLayer::builder()
        .limit_for_period(1000)
        .refresh_period(Duration::from_secs(1))
        .build();

    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(50).build();

    let circuit_breaker = CircuitBreakerLayer::<String, ()>::builder()
        .failure_rate_threshold(0.7)
        .sliding_window_size(100)
        .build();

    let timelimiter = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(5))
        .build();

    let retry = RetryLayer::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::from_millis(10))
        .build();

    // Compose: cache -> rate limit -> bulkhead -> circuit breaker -> timeout -> retry -> service
    let service = cache.layer(
        rate_limiter
            .layer(bulkhead.layer(circuit_breaker.layer(timelimiter.layer(retry.layer(svc))))),
    );

    let start = Instant::now();
    let mut handles = vec![];

    // 1000 concurrent requests through full stack
    for i in 0..1000 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            match svc.ready().await {
                Ok(ready_svc) => ready_svc.call(i % 100).await,
                Err(e) => Err(e),
            }
        }));
    }

    let mut success = 0;
    let mut failure = 0;

    for handle in handles {
        match handle.await {
            Ok(Ok(_)) => success += 1,
            _ => failure += 1,
        }
    }

    let elapsed = start.elapsed();
    let service_calls = call_count.load(Ordering::Relaxed);

    println!("Full stack: 1000 requests through 6 layers");
    println!("Completed in: {:?}", elapsed);
    println!("Success: {}, Failure: {}", success, failure);
    println!("Actual service calls: {}", service_calls);

    // Some should succeed
    assert!(success > 0);
    // Cache should reduce service calls
    assert!(service_calls < 1000);
}

/// Test: Deep layer stack (10 layers)
#[tokio::test]
#[ignore]
async fn stress_deep_layer_stack() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>(req)
        }
    });

    // Stack 10 bulkheads (extreme case)
    let mut service = svc;
    for i in 0..10 {
        let bulkhead = BulkheadLayer::builder()
            .name(&format!("bulkhead-{}", i))
            .max_concurrent_calls(100)
            .build();
        service = bulkhead.layer(service);
    }

    let start = Instant::now();

    // 1000 requests through 10 layers
    for i in 0..1000 {
        let mut svc = service.clone();
        let _ = svc.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let calls = call_count.load(Ordering::Relaxed);

    println!("10-layer stack: 1000 requests");
    println!("Completed in: {:?}", elapsed);
    println!("Service calls: {}", calls);
    println!("Overhead per layer: {:?}", elapsed / 10);

    assert_eq!(calls, 1000);
}

/// Test: Error propagation through layers
#[tokio::test]
#[ignore]
async fn stress_error_propagation() {
    let attempt_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&attempt_count);

    let svc = tower::service_fn(move |_req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            let attempt = counter.fetch_add(1, Ordering::Relaxed) + 1;
            // Always fail
            Err::<(), _>(format!("error-{}", attempt))
        }
    });

    let retry = RetryLayer::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::from_millis(1))
        .build();

    let circuit_breaker = CircuitBreakerLayer::<(), String>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .build();

    let service = circuit_breaker.layer(retry.layer(svc));

    let start = Instant::now();

    // 100 failing requests
    for i in 0..100 {
        let mut svc = service.clone();
        let _ = svc.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let attempts = attempt_count.load(Ordering::Relaxed);

    println!("Error propagation: 100 requests");
    println!("Completed in: {:?}", elapsed);
    println!("Total attempts (with retries): {}", attempts);

    // Should see retries initially, then circuit should open
    assert!(attempts > 100, "Should retry some requests");
    assert!(attempts < 300, "Circuit should open and stop retries");
}

/// Test: High concurrency through composed layers
#[tokio::test]
#[ignore]
async fn stress_composed_high_concurrency() {
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

    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(100).build();

    let rate_limiter = RateLimiterLayer::builder()
        .limit_for_period(10000)
        .refresh_period(Duration::from_secs(1))
        .build();

    let service = bulkhead.layer(rate_limiter.layer(svc));

    let start = Instant::now();
    let mut handles = vec![];

    // 5000 concurrent requests
    for i in 0..5000 {
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

    println!("5000 concurrent through bulkhead + rate limiter");
    println!("Completed in: {:?}", elapsed);
    println!("Peak concurrency: {}", peak);

    // Bulkhead should limit concurrency
    assert!(peak <= 100, "Bulkhead should limit to 100");
}

/// Test: Memory usage of composed stack
#[tokio::test]
#[ignore]
async fn stress_composed_memory() {
    let mem_start = get_memory_usage_mb();

    let svc = tower::service_fn(|req: u32| async move { Ok::<_, ()>(format!("response-{}", req)) });

    let cache = CacheLayer::builder()
        .max_size(10_000)
        .key_extractor(|req: &u32| *req)
        .build();

    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(100).build();

    let circuit_breaker = CircuitBreakerLayer::<String, ()>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(1000)
        .build();

    let service = cache.layer(bulkhead.layer(circuit_breaker.layer(svc)));

    // Fill cache
    for i in 0..10_000 {
        let mut svc = service.clone();
        let _ = svc.ready().await.unwrap().call(i).await;
    }

    let mem_end = get_memory_usage_mb();
    let mem_delta = mem_end - mem_start;

    println!("Composed stack memory usage");
    println!("Start: {:.2} MB", mem_start);
    println!("End: {:.2} MB", mem_end);
    println!("Delta: {:.2} MB", mem_delta);

    if mem_delta > 0.0 {
        assert!(mem_delta < 150.0, "Memory usage should be reasonable");
    }
}

/// Test: Burst traffic through composed layers
#[tokio::test]
#[ignore]
async fn stress_composed_burst_traffic() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(5)).await;
            Ok::<_, ()>(format!("response-{}", req))
        }
    });

    let cache = CacheLayer::builder()
        .max_size(100)
        .key_extractor(|req: &u32| *req)
        .build();

    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(50).build();

    let service = cache.layer(bulkhead.layer(svc));

    let start = Instant::now();

    // 20 bursts of 100 requests
    for burst in 0..20 {
        let mut handles = vec![];

        for i in 0..100 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.ready().await.unwrap().call(i % 50).await
            }));
        }

        for handle in handles {
            let _ = handle.await.unwrap();
        }
    }

    let elapsed = start.elapsed();
    let service_calls = call_count.load(Ordering::Relaxed);
    let hit_rate = (1.0 - service_calls as f64 / 2000.0) * 100.0;

    println!("20 bursts of 100 requests through composed stack");
    println!("Completed in: {:?}", elapsed);
    println!(
        "Service calls: {} (hit rate: {:.1}%)",
        service_calls, hit_rate
    );

    // Cache should help
    assert!(service_calls < 2000);
}

/// Test: Stability of composed stack over time
#[tokio::test]
#[ignore]
async fn stress_composed_stability() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            // 10% failure rate
            if req % 10 == 0 {
                Err(())
            } else {
                Ok(format!("response-{}", req))
            }
        }
    });

    let cache = CacheLayer::builder()
        .max_size(100)
        .ttl(Duration::from_millis(500))
        .key_extractor(|req: &u32| *req)
        .build();

    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(20).build();

    let circuit_breaker = CircuitBreakerLayer::<String, ()>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(100)
        .build();

    let retry = RetryLayer::builder()
        .max_attempts(2)
        .fixed_backoff(Duration::from_millis(5))
        .build();

    let service = cache.layer(bulkhead.layer(circuit_breaker.layer(retry.layer(svc))));

    let start = Instant::now();
    let mut total_requests = 0;
    let mut mem_samples = vec![];

    // Run for 10 seconds
    while start.elapsed() < Duration::from_secs(10) {
        for i in 0..50 {
            let mut svc = service.clone();
            let _ = svc.ready().await.unwrap().call(i).await;
            total_requests += 1;
        }

        let mem = get_memory_usage_mb();
        if mem > 0.0 {
            mem_samples.push(mem);
        }

        sleep(Duration::from_millis(100)).await;
    }

    let elapsed = start.elapsed();
    let service_calls = call_count.load(Ordering::Relaxed);

    println!(
        "Composed stability: {} requests over {:?}",
        total_requests, elapsed
    );
    println!("Service calls: {}", service_calls);

    if !mem_samples.is_empty() {
        let mem_max = mem_samples.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let mem_min = mem_samples.iter().fold(f64::INFINITY, |a, &b| a.min(b));

        println!("Memory: min={:.2} MB, max={:.2} MB", mem_min, mem_max);

        // Memory should be stable
        assert!(mem_max - mem_min < 100.0, "Memory should be stable");
    }
}
