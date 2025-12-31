//! Adaptive concurrency limiter stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd, Vegas};

use super::{ConcurrencyTracker, get_memory_usage_mb};

/// Test: High volume sequential calls
#[tokio::test]
#[ignore]
async fn stress_sequential_high_volume() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let svc = tower::service_fn(move |req: u64| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::Relaxed);
            Ok::<_, &str>(req * 2)
        }
    });

    let mut service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(100)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
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

    assert_eq!(actual_calls, 100_000);
}

/// Test: High concurrency with AIMD
#[tokio::test]
#[ignore]
async fn stress_high_concurrency_aimd() {
    let tracker = ConcurrencyTracker::new();
    let call_count = Arc::new(AtomicUsize::new(0));
    let tracker_clone = Arc::clone(&tracker);
    let cc = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: ()| {
        let tracker = Arc::clone(&tracker_clone);
        let count = cc.clone();
        async move {
            tracker.enter();
            count.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(10)).await;
            tracker.exit();
            Ok::<_, &str>(())
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(50)
                .max_limit(100)
                .latency_threshold(Duration::from_millis(500))
                .build(),
        ))
        .service(svc);

    let start = Instant::now();
    let mut handles = vec![];

    for _ in 0..1000 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(()).await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let peak = tracker.peak();
    let calls = call_count.load(Ordering::Relaxed);

    println!("1000 concurrent requests with AIMD in {:?}", elapsed);
    println!("Peak concurrency: {}", peak);
    println!("Total calls: {}", calls);

    assert_eq!(calls, 1000);
    // Peak should be limited by the adaptive limiter
    assert!(peak <= 100, "Peak {} exceeded max limit", peak);
}

/// Test: High concurrency with Vegas
#[tokio::test]
#[ignore]
async fn stress_high_concurrency_vegas() {
    let tracker = ConcurrencyTracker::new();
    let call_count = Arc::new(AtomicUsize::new(0));
    let tracker_clone = Arc::clone(&tracker);
    let cc = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: ()| {
        let tracker = Arc::clone(&tracker_clone);
        let count = cc.clone();
        async move {
            tracker.enter();
            count.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(10)).await;
            tracker.exit();
            Ok::<_, &str>(())
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Vegas::builder().initial_limit(50).max_limit(100).build(),
        ))
        .service(svc);

    let start = Instant::now();
    let mut handles = vec![];

    for _ in 0..1000 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(()).await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let peak = tracker.peak();
    let calls = call_count.load(Ordering::Relaxed);

    println!("1000 concurrent requests with Vegas in {:?}", elapsed);
    println!("Peak concurrency: {}", peak);
    println!("Total calls: {}", calls);

    assert_eq!(calls, 1000);
}

/// Test: Limit adaptation under varying latency
#[tokio::test]
#[ignore]
async fn stress_latency_adaptation() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let svc = tower::service_fn(move |slow: bool| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::Relaxed);
            if slow {
                sleep(Duration::from_millis(100)).await;
            } else {
                sleep(Duration::from_millis(5)).await;
            }
            Ok::<_, &str>(())
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(20)
                .min_limit(5)
                .max_limit(50)
                .latency_threshold(Duration::from_millis(50))
                .increase_by(2)
                .decrease_factor(0.8)
                .build(),
        ))
        .service(svc);

    let start = Instant::now();

    // Phase 1: Fast requests (should increase limit)
    println!("Phase 1: Fast requests");
    let mut handles = vec![];
    for _ in 0..100 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(false).await
        }));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // Phase 2: Slow requests (should decrease limit)
    println!("Phase 2: Slow requests");
    let mut handles = vec![];
    for _ in 0..50 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(true).await
        }));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // Phase 3: Fast again (should recover)
    println!("Phase 3: Fast requests again");
    let mut handles = vec![];
    for _ in 0..100 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(false).await
        }));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    let elapsed = start.elapsed();
    let calls = call_count.load(Ordering::Relaxed);

    println!("Latency adaptation test completed in {:?}", elapsed);
    println!("Total calls: {}", calls);

    assert_eq!(calls, 250);
}

/// Test: Error rate adaptation
#[tokio::test]
#[ignore]
async fn stress_error_adaptation() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);
    let ec = Arc::clone(&error_count);

    let svc = tower::service_fn(move |fail: bool| {
        let count = cc.clone();
        let errors = ec.clone();
        async move {
            count.fetch_add(1, Ordering::Relaxed);
            if fail {
                errors.fetch_add(1, Ordering::Relaxed);
                Err("intentional failure")
            } else {
                Ok(())
            }
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(20)
                .min_limit(2)
                .decrease_factor(0.5)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(svc);

    let start = Instant::now();

    // Successful requests
    for _ in 0..50 {
        let mut svc = service.clone();
        let _ = svc.ready().await.unwrap().call(false).await;
    }

    // Failing requests (should decrease limit)
    for _ in 0..20 {
        let mut svc = service.clone();
        let _ = svc.ready().await.unwrap().call(true).await;
    }

    // More successful requests
    for _ in 0..50 {
        let mut svc = service.clone();
        let _ = svc.ready().await.unwrap().call(false).await;
    }

    let elapsed = start.elapsed();
    let calls = call_count.load(Ordering::Relaxed);
    let errors = error_count.load(Ordering::Relaxed);

    println!("Error adaptation test completed in {:?}", elapsed);
    println!("Total calls: {}, Errors: {}", calls, errors);

    assert_eq!(calls, 120);
    assert_eq!(errors, 20);
}

/// Test: Sustained load over time
#[tokio::test]
#[ignore]
async fn stress_sustained_load() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: ()| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_micros(100)).await;
            Ok::<_, &str>(())
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(50)
                .latency_threshold(Duration::from_millis(10))
                .build(),
        ))
        .service(svc);

    let start = Instant::now();

    // Run for 5 seconds with continuous load
    let mut handles = vec![];
    for _ in 0..50 {
        let svc = service.clone();
        handles.push(tokio::spawn(async move {
            let mut svc = svc;
            let mut count = 0;
            while Instant::now().duration_since(start) < Duration::from_secs(5) {
                let _ = svc.ready().await.unwrap().call(()).await;
                count += 1;
            }
            count
        }));
    }

    let mut total_requests = 0;
    for handle in handles {
        total_requests += handle.await.unwrap();
    }

    let elapsed = start.elapsed();
    let calls = call_count.load(Ordering::Relaxed);

    println!("Sustained load over {:?}", elapsed);
    println!("Total requests: {}", total_requests);
    println!("Backend calls: {}", calls);
    println!(
        "Throughput: {:.0} req/sec",
        calls as f64 / elapsed.as_secs_f64()
    );
}

/// Test: Memory stability
#[tokio::test]
#[ignore]
async fn stress_memory_stability() {
    let mem_start = get_memory_usage_mb();

    let svc = tower::service_fn(|_req: ()| async {
        sleep(Duration::from_micros(10)).await;
        Ok::<_, &str>(())
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(100)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(svc);

    let mut mem_samples = vec![];
    let start = Instant::now();
    let mut total_requests = 0u64;

    // Run for 10 seconds
    while start.elapsed() < Duration::from_secs(10) {
        let mut handles = vec![];
        for _ in 0..100 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.ready().await.unwrap().call(()).await
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

/// Test: Compare AIMD vs Vegas performance
#[tokio::test]
#[ignore]
async fn stress_algorithm_comparison() {
    println!("Algorithm comparison (500 requests with 10% slow):");

    // Test AIMD
    {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let svc = tower::service_fn(move |_req: ()| {
            let count = cc.clone();
            async move {
                count.fetch_add(1, Ordering::Relaxed);
                let delay = if rand::random::<f32>() < 0.1 {
                    Duration::from_millis(100)
                } else {
                    Duration::from_millis(5)
                };
                sleep(delay).await;
                Ok::<_, &str>(())
            }
        });

        let service = ServiceBuilder::new()
            .layer(AdaptiveLimiterLayer::new(
                Aimd::builder()
                    .initial_limit(20)
                    .latency_threshold(Duration::from_millis(50))
                    .build(),
            ))
            .service(svc);

        let start = Instant::now();
        let mut handles = vec![];

        for _ in 0..500 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.ready().await.unwrap().call(()).await
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }

        let elapsed = start.elapsed();
        let calls = call_count.load(Ordering::Relaxed);

        println!(
            "AIMD: {} calls in {:?} ({:.0} req/sec)",
            calls,
            elapsed,
            calls as f64 / elapsed.as_secs_f64()
        );
    }

    // Test Vegas
    {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let svc = tower::service_fn(move |_req: ()| {
            let count = cc.clone();
            async move {
                count.fetch_add(1, Ordering::Relaxed);
                let delay = if rand::random::<f32>() < 0.1 {
                    Duration::from_millis(100)
                } else {
                    Duration::from_millis(5)
                };
                sleep(delay).await;
                Ok::<_, &str>(())
            }
        });

        let service = ServiceBuilder::new()
            .layer(AdaptiveLimiterLayer::new(
                Vegas::builder().initial_limit(20).build(),
            ))
            .service(svc);

        let start = Instant::now();
        let mut handles = vec![];

        for _ in 0..500 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(async move {
                svc.ready().await.unwrap().call(()).await
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }

        let elapsed = start.elapsed();
        let calls = call_count.load(Ordering::Relaxed);

        println!(
            "Vegas: {} calls in {:?} ({:.0} req/sec)",
            calls,
            elapsed,
            calls as f64 / elapsed.as_secs_f64()
        );
    }
}
