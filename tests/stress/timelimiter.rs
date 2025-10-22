//! Time limiter stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_timelimiter::TimeLimiterLayer;

use super::{ConcurrencyTracker, get_memory_usage_mb};

/// Test: 1 million calls through time limiter (all complete before timeout)
#[tokio::test]
#[ignore]
async fn stress_one_million_calls_no_timeouts() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(10))
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..1_000_000 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("1M calls (no timeouts) completed in {:?}", elapsed);
    println!(
        "Throughput: {:.0} calls/sec",
        1_000_000.0 / elapsed.as_secs_f64()
    );
    println!("Actual service calls: {}", actual_calls);

    assert_eq!(actual_calls, 1_000_000);
}

/// Test: Timeout enforcement
#[tokio::test]
#[ignore]
async fn stress_timeout_enforcement() {
    let completed_count = Arc::new(AtomicUsize::new(0));
    let timeout_count = Arc::new(AtomicUsize::new(0));
    let completed = Arc::clone(&completed_count);

    let svc = tower::service_fn(move |req: u32| {
        let completed = Arc::clone(&completed);
        async move {
            // 50% of requests take too long
            if req.is_multiple_of(2) {
                sleep(Duration::from_millis(100)).await;
            } else {
                sleep(Duration::from_millis(1)).await;
            }
            completed.fetch_add(1, Ordering::Relaxed);
            Ok::<_, std::io::Error>(())
        }
    });

    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(50))
        .cancel_running_future(true)
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..1000 {
        match service.ready().await.unwrap().call(i).await {
            Ok(_) => {}
            Err(_) => {
                timeout_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    let elapsed = start.elapsed();
    let completed = completed_count.load(Ordering::Relaxed);
    let timeouts = timeout_count.load(Ordering::Relaxed);

    println!("1000 calls with timeouts in {:?}", elapsed);
    println!("Completed: {}", completed);
    println!("Timed out: {}", timeouts);
    println!("Timeout rate: {:.1}%", timeouts as f64 / 1000.0 * 100.0);

    // Should have ~500 timeouts (50%)
    assert!((400..=600).contains(&timeouts));
    assert!((400..=600).contains(&completed));
}

/// Test: High concurrency with timeouts
#[tokio::test]
#[ignore]
async fn stress_high_concurrency_with_timeouts() {
    let tracker = ConcurrencyTracker::new();
    let completed_count = Arc::new(AtomicUsize::new(0));
    let tracker_clone = Arc::clone(&tracker);
    let completed = Arc::clone(&completed_count);

    let svc = tower::service_fn(move |req: u32| {
        let tracker = Arc::clone(&tracker_clone);
        let completed = Arc::clone(&completed);
        async move {
            tracker.enter();
            // Varying durations
            let delay = if req.is_multiple_of(3) {
                Duration::from_millis(100) // Will timeout
            } else {
                Duration::from_millis(10) // Will complete
            };
            sleep(delay).await;
            tracker.exit();
            completed.fetch_add(1, Ordering::Relaxed);
            Ok::<_, std::io::Error>(())
        }
    });

    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(50))
        .cancel_running_future(true)
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

    let mut success = 0;
    let mut failed = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => success += 1,
            Err(_) => failed += 1,
        }
    }

    let elapsed = start.elapsed();
    let peak = tracker.peak();
    let completed = completed_count.load(Ordering::Relaxed);

    println!("1000 concurrent requests with timeouts in {:?}", elapsed);
    println!("Peak concurrency: {}", peak);
    println!("Success: {}", success);
    println!("Failed (timeout): {}", failed);
    println!("Completed service calls: {}", completed);

    assert!(peak > 100, "Expected high concurrency");
    // ~33% should timeout (every 3rd request)
    assert!((250..=450).contains(&failed));
    assert_eq!(success + failed, 1000);
}

/// Test: Future cancellation behavior
#[tokio::test]
#[ignore]
async fn stress_future_cancellation() {
    let started_count = Arc::new(AtomicUsize::new(0));
    let completed_count = Arc::new(AtomicUsize::new(0));
    let started = Arc::clone(&started_count);
    let completed = Arc::clone(&completed_count);

    let svc = tower::service_fn(move |_req: u32| {
        let started = Arc::clone(&started);
        let completed = Arc::clone(&completed);
        async move {
            started.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(100)).await;
            completed.fetch_add(1, Ordering::Relaxed);
            Ok::<_, std::io::Error>(())
        }
    });

    // With cancellation enabled
    let layer_cancel = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(10))
        .cancel_running_future(true)
        .build();

    let mut service = layer_cancel.layer(svc);

    let start = Instant::now();

    for i in 0..1000 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let started = started_count.load(Ordering::Relaxed);
    let completed = completed_count.load(Ordering::Relaxed);

    println!("Future cancellation test in {:?}", elapsed);
    println!("Started: {}", started);
    println!("Completed: {}", completed);
    println!("Cancelled: {}", started - completed);

    // All should start, most should be cancelled
    assert_eq!(started, 1000);
    assert!(completed < 50, "Most futures should be cancelled");
}

/// Test: No cancellation behavior
#[tokio::test]
#[ignore]
async fn stress_no_cancellation() {
    let started_count = Arc::new(AtomicUsize::new(0));
    let completed_count = Arc::new(AtomicUsize::new(0));
    let started = Arc::clone(&started_count);
    let completed = Arc::clone(&completed_count);

    let svc = tower::service_fn(move |_req: u32| {
        let started = Arc::clone(&started);
        let completed = Arc::clone(&completed);
        async move {
            started.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(50)).await;
            completed.fetch_add(1, Ordering::Relaxed);
            Ok::<_, std::io::Error>(())
        }
    });

    // Without cancellation
    let layer_no_cancel = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(10))
        .cancel_running_future(false)
        .build();

    let mut service = layer_no_cancel.layer(svc);

    let start = Instant::now();

    for i in 0..100 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    // Wait a bit for background futures to complete
    sleep(Duration::from_millis(200)).await;

    let elapsed = start.elapsed();
    let started = started_count.load(Ordering::Relaxed);
    let completed = completed_count.load(Ordering::Relaxed);

    println!("No cancellation test in {:?}", elapsed);
    println!("Started: {}", started);
    println!("Completed: {}", completed);

    // All should start and complete despite timeouts
    assert_eq!(started, 100);
    assert_eq!(completed, 100, "All futures should complete");
}

/// Test: Varying timeout durations under load
#[tokio::test]
#[ignore]
async fn stress_varying_timeouts() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let timeout_count = Arc::new(AtomicUsize::new(0));

    // Create 3 services with different timeout durations
    let create_service = |timeout_ms: u64, counter: Arc<AtomicUsize>| {
        let svc = tower::service_fn(move |_req: u32| {
            counter.fetch_add(1, Ordering::Relaxed);
            async move {
                sleep(Duration::from_millis(50)).await;
                Ok::<_, std::io::Error>(())
            }
        });

        let layer = TimeLimiterLayer::builder()
            .timeout_duration(Duration::from_millis(timeout_ms))
            .build();

        layer.layer(svc)
    };

    let service1 = create_service(25, Arc::clone(&timeout_count)); // Will timeout
    let service2 = create_service(75, Arc::clone(&success_count)); // Will succeed
    let service3 = create_service(100, Arc::clone(&success_count)); // Will succeed

    let start = Instant::now();

    let h1 = {
        let mut svc = service1;
        tokio::spawn(async move {
            for i in 0..300 {
                let _ = svc.ready().await.unwrap().call(i).await;
            }
        })
    };

    let h2 = {
        let mut svc = service2;
        tokio::spawn(async move {
            for i in 0..300 {
                let _ = svc.ready().await.unwrap().call(i).await;
            }
        })
    };

    let h3 = {
        let mut svc = service3;
        tokio::spawn(async move {
            for i in 0..300 {
                let _ = svc.ready().await.unwrap().call(i).await;
            }
        })
    };

    let _ = tokio::join!(h1, h2, h3);

    let elapsed = start.elapsed();
    let successes = success_count.load(Ordering::Relaxed);
    let timeouts = timeout_count.load(Ordering::Relaxed);

    println!("Varying timeouts test in {:?}", elapsed);
    println!("Service calls that succeeded: {}", successes);
    println!("Service calls that timed out: {}", timeouts);

    // service2 and service3 should succeed (600 total)
    assert_eq!(successes, 600);
    // service1 should timeout (300 total)
    assert_eq!(timeouts, 300);
}

/// Test: Memory stability over extended period
#[tokio::test]
#[ignore]
async fn stress_memory_stability() {
    let mem_start = get_memory_usage_mb();

    let svc = tower::service_fn(|_req: u32| async {
        sleep(Duration::from_micros(100)).await;
        Ok::<_, std::io::Error>(())
    });

    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(1))
        .cancel_running_future(true)
        .build();

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

        // Memory shouldn't grow unbounded (allow 100 MB growth)
        if mem_end > mem_start {
            assert!(mem_end - mem_start < 100.0, "Memory leak suspected");
        }
    }
}

/// Test: Mixed success and timeout under high load
#[tokio::test]
#[ignore]
async fn stress_mixed_results_high_volume() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let timeout_count = Arc::new(AtomicUsize::new(0));
    let service_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&service_calls);

    let svc = tower::service_fn(move |req: u32| {
        let calls = Arc::clone(&calls);
        async move {
            calls.fetch_add(1, Ordering::Relaxed);
            // 30% take longer than timeout
            if (req % 10) < 3 {
                sleep(Duration::from_millis(100)).await;
            } else {
                sleep(Duration::from_millis(10)).await;
            }
            Ok::<_, std::io::Error>(())
        }
    });

    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(50))
        .cancel_running_future(true)
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..10_000 {
        match service.ready().await.unwrap().call(i).await {
            Ok(_) => {
                success_count.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                timeout_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    let elapsed = start.elapsed();
    let successes = success_count.load(Ordering::Relaxed);
    let timeouts = timeout_count.load(Ordering::Relaxed);
    let calls = service_calls.load(Ordering::Relaxed);

    println!("10k mixed calls in {:?}", elapsed);
    println!("Successes: {}", successes);
    println!("Timeouts: {}", timeouts);
    println!("Service calls: {}", calls);
    println!("Timeout rate: {:.1}%", timeouts as f64 / 10_000.0 * 100.0);

    // Should have ~30% timeouts
    assert!((2_500..=3_500).contains(&timeouts));
    assert_eq!(successes + timeouts, 10_000);
}
