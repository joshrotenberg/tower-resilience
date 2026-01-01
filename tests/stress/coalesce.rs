//! Coalesce stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_coalesce::CoalesceLayer;

use super::{ConcurrencyTracker, get_memory_usage_mb};

#[derive(Debug, Clone)]
struct TestError(String);

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TestError {}

/// Test: High volume sequential calls (no coalescing opportunity)
#[tokio::test]
#[ignore]
async fn stress_sequential_no_coalesce() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u64| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::Relaxed);
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let mut service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &u64| *req))
        .service(svc);

    let start = Instant::now();

    for i in 0..100_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("100k sequential calls in {:?}", elapsed);
    println!(
        "Throughput: {:.0} calls/sec",
        100_000.0 / elapsed.as_secs_f64()
    );
    println!("Backend calls: {}", actual_calls);

    // All calls should execute (no coalescing for sequential)
    assert_eq!(actual_calls, 100_000);
}

/// Test: High concurrency same key - maximum coalescing
#[tokio::test]
#[ignore]
async fn stress_high_concurrency_same_key() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(10)).await;
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(svc);

    let start = Instant::now();
    let mut handles = vec![];

    // 1000 concurrent requests for same key
    for _ in 0..1000 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call("hot-key".to_string()).await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "response: hot-key");
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("1000 concurrent same-key requests in {:?}", elapsed);
    println!(
        "Backend calls: {} (coalescing ratio: {}x)",
        actual_calls,
        1000 / actual_calls.max(1)
    );

    // Should be heavily coalesced - ideally 1 call
    assert!(
        actual_calls <= 5,
        "Expected heavy coalescing, got {} calls",
        actual_calls
    );
}

/// Test: Mixed keys with varying popularity
#[tokio::test]
#[ignore]
async fn stress_mixed_key_popularity() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(5)).await;
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(svc);

    let start = Instant::now();
    let mut handles = vec![];

    // 500 requests for "hot" key, 500 for various "cold" keys
    for i in 0..1000 {
        let mut svc = service.clone();
        let key = if i < 500 {
            "hot-key".to_string()
        } else {
            format!("cold-key-{}", i)
        };
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(key).await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("1000 mixed-key requests in {:?}", elapsed);
    println!("Backend calls: {}", actual_calls);
    // ~1 for hot key + ~500 for cold keys
    println!("Expected ~501 calls, got {}", actual_calls);

    // Should be around 501 (1 coalesced hot + 500 unique cold)
    assert!(actual_calls < 600, "Expected coalescing on hot key");
    assert!(actual_calls > 400, "Cold keys should execute separately");
}

/// Test: Error propagation under high concurrency
#[tokio::test]
#[ignore]
async fn stress_error_propagation() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let error_received = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);
    let er = Arc::clone(&error_received);

    let svc = tower::service_fn(move |_req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(10)).await;
            Err::<String, _>(TestError("shared error".to_string()))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(svc);

    let start = Instant::now();
    let mut handles = vec![];

    for _ in 0..500 {
        let mut svc = service.clone();
        let er = Arc::clone(&er);
        handles.push(tokio::spawn(async move {
            let result = svc
                .ready()
                .await
                .unwrap()
                .call("error-key".to_string())
                .await;
            if result.is_err() {
                er.fetch_add(1, Ordering::Relaxed);
            }
            result
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_err());
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);
    let errors = error_received.load(Ordering::Relaxed);

    println!("500 concurrent error requests in {:?}", elapsed);
    println!(
        "Backend calls: {}, Errors received: {}",
        actual_calls, errors
    );

    // All 500 should receive error, but only ~1 call made
    assert_eq!(errors, 500);
    assert!(actual_calls <= 5, "Expected heavy coalescing");
}

/// Test: Sustained load over time
#[tokio::test]
#[ignore]
async fn stress_sustained_load() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let request_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u64| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_micros(100)).await;
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &u64| req % 10)) // 10 unique keys
        .service(svc);

    let start = Instant::now();
    let rc = Arc::clone(&request_count);

    // Run for 5 seconds with continuous load
    let mut handles = vec![];
    for _ in 0..100 {
        let mut svc = service.clone();
        let rc = Arc::clone(&rc);
        handles.push(tokio::spawn(async move {
            let mut i = 0u64;
            while Instant::now().duration_since(start) < Duration::from_secs(5) {
                let _ = svc.ready().await.unwrap().call(i).await;
                rc.fetch_add(1, Ordering::Relaxed);
                i += 1;
            }
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    let elapsed = start.elapsed();
    let requests = request_count.load(Ordering::Relaxed);
    let actual_calls = call_count.load(Ordering::Relaxed);
    let coalesce_ratio = requests as f64 / actual_calls.max(1) as f64;

    println!("Sustained load over {:?}", elapsed);
    println!("Total requests: {}", requests);
    println!("Backend calls: {}", actual_calls);
    println!("Coalesce ratio: {:.2}x", coalesce_ratio);
    println!(
        "Throughput: {:.0} req/sec",
        requests as f64 / elapsed.as_secs_f64()
    );

    // Should have significant coalescing
    assert!(coalesce_ratio > 1.5, "Expected coalescing benefit");
}

/// Test: Memory stability
#[tokio::test]
#[ignore]
async fn stress_memory_stability() {
    let mem_start = get_memory_usage_mb();

    let svc = tower::service_fn(|req: u64| async move {
        sleep(Duration::from_micros(10)).await;
        Ok::<_, TestError>(format!("response: {}", req))
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &u64| req % 100))
        .service(svc);

    let mut mem_samples = vec![];
    let start = Instant::now();
    let mut total_requests = 0u64;

    // Run for 10 seconds
    while start.elapsed() < Duration::from_secs(10) {
        let mut handles = vec![];
        for i in 0..100 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.ready().await.unwrap().call(i).await
            }));
        }
        for handle in handles {
            let _ = handle.await;
        }
        total_requests += 100;

        if total_requests.is_multiple_of(1000) {
            let mem = get_memory_usage_mb();
            if mem > 0.0 {
                mem_samples.push(mem);
            }
        }
    }

    let mem_end = get_memory_usage_mb();

    println!("Ran {} requests over 10 seconds", total_requests);
    println!("Memory start: {:.2} MB", mem_start);
    println!("Memory end: {:.2} MB", mem_end);

    if !mem_samples.is_empty() {
        let mem_max = mem_samples.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let mem_min = mem_samples.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        println!("Memory range: {:.2} - {:.2} MB", mem_min, mem_max);

        if mem_end > mem_start {
            assert!(mem_end - mem_start < 100.0, "Memory leak suspected");
        }
    }
}

/// Test: Peak concurrency tracking
#[tokio::test]
#[ignore]
async fn stress_peak_concurrency() {
    let tracker = ConcurrencyTracker::new();
    let call_count = Arc::new(AtomicUsize::new(0));
    let tracker_clone = Arc::clone(&tracker);
    let cc = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: String| {
        let tracker = Arc::clone(&tracker_clone);
        let count = cc.clone();
        async move {
            tracker.enter();
            count.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(20)).await;
            tracker.exit();
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(svc);

    let mut handles = vec![];

    // Launch 500 requests for same key
    for _ in 0..500 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready()
                .await
                .unwrap()
                .call("shared-key".to_string())
                .await
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    let peak = tracker.peak();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("500 requests for same key");
    println!("Peak backend concurrency: {}", peak);
    println!("Backend calls: {}", actual_calls);

    // Peak concurrency should be very low due to coalescing
    assert!(peak <= 5, "Coalescing should limit backend concurrency");
    assert!(actual_calls <= 5, "Expected heavy coalescing");
}
