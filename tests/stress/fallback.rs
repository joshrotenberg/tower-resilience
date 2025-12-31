//! Fallback stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_fallback::FallbackLayer;

use super::{ConcurrencyTracker, get_memory_usage_mb};

#[derive(Debug, Clone)]
struct TestError {
    #[allow(dead_code)]
    message: String,
}

impl TestError {
    fn new(msg: &str) -> Self {
        Self {
            message: msg.to_string(),
        }
    }
}

/// Test: 1 million calls through fallback layer (all succeed, no fallback triggered)
#[tokio::test]
#[ignore]
async fn stress_one_million_calls_no_fallback() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, TestError>("success".to_string()) }
    });

    let layer = FallbackLayer::<u32, String, TestError>::value("fallback".to_string());
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..1_000_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("1M calls (no fallback) completed in {:?}", elapsed);
    println!(
        "Throughput: {:.0} calls/sec",
        1_000_000.0 / elapsed.as_secs_f64()
    );
    println!("Actual service calls: {}", actual_calls);

    assert_eq!(actual_calls, 1_000_000);
}

/// Test: High volume with all failures triggering fallback
#[tokio::test]
#[ignore]
async fn stress_high_volume_all_fallback() {
    let fallback_count = Arc::new(AtomicUsize::new(0));
    let fc = Arc::clone(&fallback_count);

    let svc =
        tower::service_fn(|_req: u32| async { Err::<String, _>(TestError::new("always fail")) });

    let layer = FallbackLayer::<u32, String, TestError>::from_error(move |_e: &TestError| {
        fc.fetch_add(1, Ordering::Relaxed);
        "fallback response".to_string()
    });
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..100_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "fallback response");
    }

    let elapsed = start.elapsed();
    let fallbacks = fallback_count.load(Ordering::Relaxed);

    println!("100k calls (all fallback) completed in {:?}", elapsed);
    println!(
        "Throughput: {:.0} calls/sec",
        100_000.0 / elapsed.as_secs_f64()
    );
    println!("Fallback invocations: {}", fallbacks);

    assert_eq!(fallbacks, 100_000);
}

/// Test: Mixed success and fallback under load
#[tokio::test]
#[ignore]
async fn stress_mixed_success_and_fallback() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let fallback_count = Arc::new(AtomicUsize::new(0));
    let sc = Arc::clone(&success_count);
    let fc = Arc::clone(&fallback_count);

    let svc = tower::service_fn(move |req: u32| {
        let sc = Arc::clone(&sc);
        async move {
            // 30% failure rate
            if req % 10 < 3 {
                Err(TestError::new("transient failure"))
            } else {
                sc.fetch_add(1, Ordering::Relaxed);
                Ok("success".to_string())
            }
        }
    });

    let layer = FallbackLayer::<u32, String, TestError>::from_error(move |_e: &TestError| {
        fc.fetch_add(1, Ordering::Relaxed);
        "fallback".to_string()
    });
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..100_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let successes = success_count.load(Ordering::Relaxed);
    let fallbacks = fallback_count.load(Ordering::Relaxed);

    println!("100k calls (mixed) completed in {:?}", elapsed);
    println!("Successes: {}, Fallbacks: {}", successes, fallbacks);
    println!("Fallback rate: {:.1}%", fallbacks as f64 / 1000.0);

    assert_eq!(successes + fallbacks, 100_000);
    // Should be ~30% fallbacks
    assert!((25_000..=35_000).contains(&fallbacks));
}

/// Test: High concurrency with fallback
#[tokio::test]
#[ignore]
async fn stress_high_concurrency_with_fallback() {
    let tracker = ConcurrencyTracker::new();
    let success_count = Arc::new(AtomicUsize::new(0));
    let fallback_count = Arc::new(AtomicUsize::new(0));
    let tracker_clone = Arc::clone(&tracker);
    let sc = Arc::clone(&success_count);
    let fc = Arc::clone(&fallback_count);

    let svc = tower::service_fn(move |req: u32| {
        let tracker = Arc::clone(&tracker_clone);
        let sc = Arc::clone(&sc);
        async move {
            tracker.enter();
            sleep(Duration::from_millis(5)).await;
            tracker.exit();

            // 20% failure rate
            if req.is_multiple_of(5) {
                Err(TestError::new("failure"))
            } else {
                sc.fetch_add(1, Ordering::Relaxed);
                Ok("success".to_string())
            }
        }
    });

    let layer = FallbackLayer::<u32, String, TestError>::from_error(move |_e: &TestError| {
        fc.fetch_add(1, Ordering::Relaxed);
        "fallback".to_string()
    });
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
    let successes = success_count.load(Ordering::Relaxed);
    let fallbacks = fallback_count.load(Ordering::Relaxed);

    println!("1000 concurrent requests with fallback in {:?}", elapsed);
    println!("Peak concurrency: {}", peak);
    println!("Successes: {}, Fallbacks: {}", successes, fallbacks);

    assert!(peak > 100, "Expected high concurrency");
    assert_eq!(successes + fallbacks, 1000);
}

/// Test: Backup service fallback under load
#[tokio::test]
#[ignore]
async fn stress_backup_service_fallback() {
    let primary_calls = Arc::new(AtomicUsize::new(0));
    let backup_calls = Arc::new(AtomicUsize::new(0));
    let pc = Arc::clone(&primary_calls);
    let bc = Arc::clone(&backup_calls);

    let svc = tower::service_fn(move |_req: u32| {
        pc.fetch_add(1, Ordering::Relaxed);
        async { Err::<String, _>(TestError::new("primary failed")) }
    });

    let layer = FallbackLayer::<u32, String, TestError>::service(move |req: u32| {
        let bc = Arc::clone(&bc);
        async move {
            bc.fetch_add(1, Ordering::Relaxed);
            Ok::<_, TestError>(format!("backup: {}", req))
        }
    });
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..50_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let primary = primary_calls.load(Ordering::Relaxed);
    let backup = backup_calls.load(Ordering::Relaxed);

    println!("50k calls with backup service in {:?}", elapsed);
    println!("Primary calls: {}, Backup calls: {}", primary, backup);
    println!(
        "Throughput: {:.0} calls/sec",
        50_000.0 / elapsed.as_secs_f64()
    );

    assert_eq!(primary, 50_000);
    assert_eq!(backup, 50_000);
}

/// Test: Fallback predicate filtering under load
#[tokio::test]
#[ignore]
async fn stress_fallback_predicate() {
    let handled_count = Arc::new(AtomicUsize::new(0));
    let skipped_count = Arc::new(AtomicUsize::new(0));
    let hc = Arc::clone(&handled_count);

    #[derive(Debug, Clone)]
    struct CategorizedError {
        recoverable: bool,
    }

    let svc = tower::service_fn(move |req: u32| async move {
        // 50% recoverable, 50% non-recoverable errors
        Err::<String, _>(CategorizedError {
            recoverable: req.is_multiple_of(2),
        })
    });

    let layer = FallbackLayer::<u32, String, CategorizedError>::builder()
        .from_error(move |_e: &CategorizedError| {
            hc.fetch_add(1, Ordering::Relaxed);
            "handled".to_string()
        })
        .handle(|e: &CategorizedError| e.recoverable)
        .build();
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..10_000 {
        let result = service.ready().await.unwrap().call(i).await;
        match result {
            Ok(_) => {} // Fallback applied
            Err(_) => {
                skipped_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    let elapsed = start.elapsed();
    let handled = handled_count.load(Ordering::Relaxed);
    let skipped = skipped_count.load(Ordering::Relaxed);

    println!("10k calls with predicate in {:?}", elapsed);
    println!("Handled: {}, Skipped: {}", handled, skipped);

    // Should be ~50% each
    assert!((4_500..=5_500).contains(&handled));
    assert!((4_500..=5_500).contains(&skipped));
}

/// Test: Memory stability over extended period
#[tokio::test]
#[ignore]
async fn stress_memory_stability() {
    let mem_start = get_memory_usage_mb();

    let svc = tower::service_fn(|req: u32| async move {
        // 50% failure rate
        if req.is_multiple_of(2) {
            Err(TestError::new("fail"))
        } else {
            Ok("success".to_string())
        }
    });

    let layer = FallbackLayer::<u32, String, TestError>::value("fallback".to_string());
    let mut service = layer.layer(svc);

    let mut mem_samples = vec![];

    // Run for 10 seconds
    let start = Instant::now();
    let mut i = 0u32;

    while start.elapsed() < Duration::from_secs(10) {
        let _ = service.ready().await.unwrap().call(i).await;
        i += 1;

        // Sample memory every 10000 calls
        if i.is_multiple_of(10000) {
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

        // Memory shouldn't grow unbounded
        if mem_end > mem_start {
            assert!(mem_end - mem_start < 100.0, "Memory leak suspected");
        }
    }
}

/// Test: Fallback with request context under load
#[tokio::test]
#[ignore]
async fn stress_request_context_fallback() {
    let svc =
        tower::service_fn(|_req: String| async { Err::<String, _>(TestError::new("failure")) });

    let layer = FallbackLayer::<String, String, TestError>::from_request_error(
        |req: &String, _e: &TestError| format!("fallback for: {}", req),
    );
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..50_000 {
        let req = format!("request-{}", i);
        let result = service.ready().await.unwrap().call(req.clone()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), format!("fallback for: {}", req));
    }

    let elapsed = start.elapsed();

    println!("50k calls with request context in {:?}", elapsed);
    println!(
        "Throughput: {:.0} calls/sec",
        50_000.0 / elapsed.as_secs_f64()
    );
}
