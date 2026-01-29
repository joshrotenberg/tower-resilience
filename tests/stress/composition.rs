//! Composition stress tests - multiple patterns stacked together

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_bulkhead::BulkheadLayer;
use tower_resilience_circuitbreaker::CircuitBreakerLayer;

use super::{ConcurrencyTracker, get_memory_usage_mb};

/// Inner service error type - used as the base error for composition
#[derive(Debug, Clone)]
struct InnerError(String);

impl std::fmt::Display for InnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for InnerError {}

/// Error type for composition tests
#[derive(Debug)]
struct TestError;

/// Test: Circuit breaker + bulkhead composition
#[tokio::test]
#[ignore]
async fn stress_circuit_breaker_plus_bulkhead() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            if req.is_multiple_of(50) {
                Err(InnerError("simulated failure".to_string()))
            } else {
                Ok(format!("response-{}", req))
            }
        }
    });

    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(20).build();

    // CircuitBreaker now wraps BulkheadServiceError<InnerError>
    let circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.7)
        .sliding_window_size(100)
        .build();

    // Compose: circuit breaker -> bulkhead -> service
    let service = circuit_breaker.layer(bulkhead.layer(svc));

    let start = Instant::now();
    let mut handles = vec![];

    // 1000 concurrent requests
    for i in 0..1000 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(i % 100).await
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

    println!("Circuit breaker + bulkhead: 1000 requests");
    println!("Completed in: {:?}", elapsed);
    println!("Success: {}, Failure: {}", success, failure);
    println!("Actual service calls: {}", service_calls);

    assert!(success > 0);
}

/// Test: Bulkhead + bulkhead (nested resource limits)
#[tokio::test]
#[ignore]
async fn stress_nested_bulkheads() {
    let tracker = ConcurrencyTracker::new();
    let tracker_clone = Arc::clone(&tracker);

    let svc = tower::service_fn(move |_req: u32| {
        let tracker = Arc::clone(&tracker_clone);
        async move {
            tracker.enter();
            sleep(Duration::from_millis(10)).await;
            tracker.exit();
            Ok::<_, TestError>(())
        }
    });

    let bulkhead1 = BulkheadLayer::builder()
        .name("outer")
        .max_concurrent_calls(50)
        .build();

    let bulkhead2 = BulkheadLayer::builder()
        .name("inner")
        .max_concurrent_calls(20)
        .build();

    let service = bulkhead1.layer(bulkhead2.layer(svc));

    let start = Instant::now();
    let mut handles = vec![];

    for i in 0..200 {
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

    println!("Nested bulkheads: 200 requests");
    println!("Completed in: {:?}", elapsed);
    println!("Peak concurrency: {}", peak);

    // Inner bulkhead should limit to 20
    assert!(peak <= 20, "Inner bulkhead should limit concurrency");
}

/// Test: High concurrency through two layers
#[tokio::test]
#[ignore]
async fn stress_two_layer_high_concurrency() {
    let tracker = ConcurrencyTracker::new();
    let tracker_clone = Arc::clone(&tracker);

    let svc = tower::service_fn(move |_req: u32| {
        let tracker = Arc::clone(&tracker_clone);
        async move {
            tracker.enter();
            sleep(Duration::from_millis(5)).await;
            tracker.exit();
            Ok::<_, TestError>(())
        }
    });

    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(100).build();

    let circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.9)
        .sliding_window_size(1000)
        .build();

    let service = circuit_breaker.layer(bulkhead.layer(svc));

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

    println!("5000 concurrent through 2 layers");
    println!("Completed in: {:?}", elapsed);
    println!("Peak concurrency: {}", peak);

    assert!(peak <= 100, "Bulkhead should limit concurrency");
}

/// Test: Memory usage of composed stack
#[tokio::test]
#[ignore]
async fn stress_composed_memory() {
    let mem_start = get_memory_usage_mb();

    let svc = tower::service_fn(|_req: u32| async move { Ok::<_, TestError>(()) });

    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(100).build();

    let circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(1000)
        .build();

    let service = circuit_breaker.layer(bulkhead.layer(svc));

    // Make 10k requests
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
        assert!(mem_delta < 100.0, "Memory usage should be reasonable");
    }
}

/// Test: Burst traffic through composed layers
#[tokio::test]
#[ignore]
async fn stress_composed_burst_traffic() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(5)).await;
            Ok::<_, TestError>(())
        }
    });

    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(50).build();

    let circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.9)
        .sliding_window_size(100)
        .build();

    let service = circuit_breaker.layer(bulkhead.layer(svc));

    let start = Instant::now();

    // 20 bursts of 100 requests
    for burst in 0..20 {
        let mut handles = vec![];

        for i in 0..100 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.ready().await.unwrap().call(burst * 100 + i).await
            }));
        }

        for handle in handles {
            let _ = handle.await.unwrap();
        }
    }

    let elapsed = start.elapsed();
    let service_calls = call_count.load(Ordering::Relaxed);

    println!("20 bursts of 100 requests through composed stack");
    println!("Completed in: {:?}", elapsed);
    println!("Service calls: {}", service_calls);

    assert_eq!(service_calls, 2000);
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
            if req.is_multiple_of(10) {
                Err(InnerError("simulated failure".to_string()))
            } else {
                Ok(())
            }
        }
    });

    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(20).build();

    let circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(100)
        .build();

    let service = circuit_breaker.layer(bulkhead.layer(svc));

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
