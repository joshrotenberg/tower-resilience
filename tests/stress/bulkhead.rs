//! Bulkhead stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_bulkhead::{BulkheadError, BulkheadLayer};

use super::{ConcurrencyTracker, get_memory_usage_mb};

#[derive(Debug)]
#[allow(dead_code)]
enum TestError {
    Bulkhead(BulkheadError),
}

impl From<BulkheadError> for TestError {
    fn from(e: BulkheadError) -> Self {
        TestError::Bulkhead(e)
    }
}

/// Test: Thousands of queued requests
#[tokio::test]
#[ignore]
async fn stress_large_queue() {
    let tracker = ConcurrencyTracker::new();
    let tracker_clone = Arc::clone(&tracker);
    let processed = Arc::new(AtomicUsize::new(0));
    let processed_clone = Arc::clone(&processed);

    let svc = tower::service_fn(move |_req: u32| {
        let tracker = Arc::clone(&tracker_clone);
        let processed = Arc::clone(&processed_clone);
        async move {
            tracker.enter();
            sleep(Duration::from_millis(10)).await;
            processed.fetch_add(1, Ordering::Relaxed);
            tracker.exit();
            Ok::<_, TestError>(())
        }
    });

    let layer = BulkheadLayer::builder()
        .max_concurrent_calls(10)
        .max_wait_duration(Duration::from_secs(30))
        .build();

    let service = layer.layer(svc);

    let start = Instant::now();
    let mut handles = vec![];

    // Queue 1000 requests with max concurrency of 10
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
    let total = processed.load(Ordering::Relaxed);

    println!("1000 queued requests with max concurrency 10");
    println!("Completed in: {:?}", elapsed);
    println!("Peak concurrency: {}", peak);
    println!("Total processed: {}", total);

    assert_eq!(peak, 10, "Should not exceed max concurrency");
    assert_eq!(total, 1000, "All requests should complete");
}

/// Test: Rapid permit acquisition/release churn
#[tokio::test]
#[ignore]
async fn stress_permit_churn() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            // Very fast operation = high churn
            Ok::<_, TestError>(())
        }
    });

    let layer = BulkheadLayer::builder()
        .max_concurrent_calls(50)
        // Default: wait indefinitely (no timeout)
        .build();

    let service = layer.layer(svc);

    let start = Instant::now();
    let mut handles = vec![];

    // 10,000 very fast operations
    for i in 0..10_000 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(i).await
        }));
    }

    for handle in handles {
        let _ = handle.await.unwrap();
    }

    let elapsed = start.elapsed();
    let total = call_count.load(Ordering::Relaxed);

    println!("10,000 fast operations in {:?}", elapsed);
    println!("Total processed: {}", total);
    println!(
        "Throughput: {:.0} ops/sec",
        total as f64 / elapsed.as_secs_f64()
    );

    assert_eq!(total, 10_000);
}

/// Test: Long-running operations blocking permits
#[tokio::test]
#[ignore]
async fn stress_long_running_operations() {
    let tracker = ConcurrencyTracker::new();
    let tracker_clone = Arc::clone(&tracker);
    let completed = Arc::new(AtomicUsize::new(0));
    let completed_clone = Arc::clone(&completed);

    let svc = tower::service_fn(move |_req: u32| {
        let tracker = Arc::clone(&tracker_clone);
        let completed = Arc::clone(&completed_clone);
        async move {
            tracker.enter();
            // Simulate long operation
            sleep(Duration::from_secs(1)).await;
            tracker.exit();
            completed.fetch_add(1, Ordering::Relaxed);
            Ok::<_, TestError>(())
        }
    });

    let layer = BulkheadLayer::builder()
        .max_concurrent_calls(5)
        .max_wait_duration(Duration::from_secs(10))
        .build();

    let service = layer.layer(svc);

    let start = Instant::now();
    let mut handles = vec![];

    // 20 long operations with max concurrency 5
    for i in 0..20 {
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
    let total = completed.load(Ordering::Relaxed);

    println!("20 long operations (1s each) with max concurrency 5");
    println!("Completed in: {:?}", elapsed);
    println!("Peak concurrency: {}", peak);
    println!("Total completed: {}", total);

    assert_eq!(peak, 5);
    assert_eq!(total, 20);
    // Should take ~4 seconds (20 ops / 5 concurrent)
    assert!(elapsed.as_secs() >= 3 && elapsed.as_secs() <= 6);
}

/// Test: Timeout behavior under high load
#[tokio::test]
#[ignore]
async fn stress_timeout_under_load() {
    let accepted = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));

    let accepted_clone = Arc::clone(&accepted);

    let svc = tower::service_fn(move |_req: u32| {
        let accepted = Arc::clone(&accepted_clone);
        async move {
            accepted.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(100)).await;
            Ok::<_, TestError>(())
        }
    });

    let layer = BulkheadLayer::builder()
        .max_concurrent_calls(10)
        .max_wait_duration(Duration::from_millis(100))
        .build();

    let service = layer.layer(svc);

    let start = Instant::now();
    let mut handles = vec![];

    // 500 requests with short timeout
    for i in 0..500 {
        let mut svc = service.clone();
        let rejected = Arc::clone(&rejected);
        handles.push(tokio::spawn(async move {
            match svc.ready().await.unwrap().call(i).await {
                Ok(_) => {}
                Err(_) => {
                    rejected.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    let elapsed = start.elapsed();
    let accepted_count = accepted.load(Ordering::Relaxed);
    let rejected_count = rejected.load(Ordering::Relaxed);

    println!("500 requests with aggressive timeout");
    println!("Completed in: {:?}", elapsed);
    println!("Accepted: {}", accepted_count);
    println!("Rejected (timeout): {}", rejected_count);

    assert!(rejected_count > 0, "Expected some timeouts");
    assert_eq!(accepted_count + rejected_count, 500);
}

/// Test: Memory usage with large queue
#[tokio::test]
#[ignore]
async fn stress_memory_with_queue() {
    let mem_start = get_memory_usage_mb();

    let svc = tower::service_fn(|_req: u32| async move {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>(())
    });

    let layer = BulkheadLayer::builder()
        .max_concurrent_calls(10)
        .max_wait_duration(Duration::from_secs(60))
        .build();

    let service = layer.layer(svc);

    let mut handles = vec![];

    // Queue 5000 requests
    for i in 0..5000 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(i).await
        }));
    }

    for handle in handles {
        let _ = handle.await.unwrap();
    }

    let mem_end = get_memory_usage_mb();
    let mem_delta = mem_end - mem_start;

    println!("5000 queued requests");
    println!("Memory delta: {:.2} MB", mem_delta);

    if mem_delta > 0.0 {
        // Should use reasonable memory (< 100 MB)
        assert!(
            mem_delta < 100.0,
            "Memory usage too high: {:.2} MB",
            mem_delta
        );
    }
}

/// Test: Burst traffic pattern
#[tokio::test]
#[ignore]
async fn stress_burst_pattern() {
    let tracker = ConcurrencyTracker::new();
    let tracker_clone = Arc::clone(&tracker);
    let total_processed = Arc::new(AtomicUsize::new(0));
    let total_clone = Arc::clone(&total_processed);

    let svc = tower::service_fn(move |_req: u32| {
        let tracker = Arc::clone(&tracker_clone);
        let total = Arc::clone(&total_clone);
        async move {
            tracker.enter();
            sleep(Duration::from_millis(50)).await;
            total.fetch_add(1, Ordering::Relaxed);
            tracker.exit();
            Ok::<_, TestError>(())
        }
    });

    let layer = BulkheadLayer::builder()
        .max_concurrent_calls(20)
        .max_wait_duration(Duration::from_secs(5))
        .build();

    let service = layer.layer(svc);

    let start = Instant::now();

    // 10 bursts of 100 requests each
    for burst in 0..10 {
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

        // Small gap between bursts
        sleep(Duration::from_millis(100)).await;
    }

    let elapsed = start.elapsed();
    let peak = tracker.peak();
    let total = total_processed.load(Ordering::Relaxed);

    println!("10 bursts of 100 requests");
    println!("Completed in: {:?}", elapsed);
    println!("Peak concurrency: {}", peak);
    println!("Total processed: {}", total);

    assert_eq!(peak, 20, "Should hit max concurrency");
    assert_eq!(total, 1000, "All requests should complete");
}

/// Test: Mixed fast and slow operations
#[tokio::test]
#[ignore]
async fn stress_mixed_operation_speeds() {
    let fast_count = Arc::new(AtomicUsize::new(0));
    let slow_count = Arc::new(AtomicUsize::new(0));

    let fast_clone = Arc::clone(&fast_count);
    let slow_clone = Arc::clone(&slow_count);

    let svc = tower::service_fn(move |req: u32| {
        let fast = Arc::clone(&fast_clone);
        let slow = Arc::clone(&slow_clone);
        async move {
            if req.is_multiple_of(10) {
                // 10% slow operations
                sleep(Duration::from_millis(100)).await;
                slow.fetch_add(1, Ordering::Relaxed);
            } else {
                // 90% fast operations
                fast.fetch_add(1, Ordering::Relaxed);
            }
            Ok::<_, TestError>(())
        }
    });

    let layer = BulkheadLayer::builder()
        .max_concurrent_calls(50)
        .max_wait_duration(Duration::from_secs(10))
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
    let fast = fast_count.load(Ordering::Relaxed);
    let slow = slow_count.load(Ordering::Relaxed);

    println!("1000 mixed operations in {:?}", elapsed);
    println!("Fast operations: {}", fast);
    println!("Slow operations: {}", slow);

    assert_eq!(fast + slow, 1000);
    assert!(slow > 80 && slow < 120, "~10% should be slow");
}
