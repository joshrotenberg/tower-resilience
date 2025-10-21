//! Rate limiter stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_ratelimiter::RateLimiterLayer;

use super::{ConcurrencyTracker, get_memory_usage_mb};

/// Test: 1 million calls through rate limiter (high limit, no throttling)
#[tokio::test]
#[ignore]
async fn stress_one_million_calls_no_throttling() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(1_000_000) // Very high limit
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_secs(1))
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..1_000_000 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("1M calls (no throttling) completed in {:?}", elapsed);
    println!(
        "Throughput: {:.0} calls/sec",
        1_000_000.0 / elapsed.as_secs_f64()
    );
    println!("Actual service calls: {}", actual_calls);

    assert_eq!(actual_calls, 1_000_000);
}

/// Test: Rate limiting enforcement with permit exhaustion
#[tokio::test]
#[ignore]
async fn stress_rate_limit_enforcement() {
    let permitted_count = Arc::new(AtomicUsize::new(0));
    let rejected_count = Arc::new(AtomicUsize::new(0));
    let permitted = Arc::clone(&permitted_count);

    let svc = tower::service_fn(move |_req: u32| {
        permitted.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(100)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_millis(10)) // Short timeout
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..1000 {
        match service.ready().await.unwrap().call(i).await {
            Ok(_) => {}
            Err(_) => {
                rejected_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    let elapsed = start.elapsed();
    let permitted = permitted_count.load(Ordering::Relaxed);
    let rejected = rejected_count.load(Ordering::Relaxed);

    println!("1000 calls with rate limiting in {:?}", elapsed);
    println!("Permitted: {}", permitted);
    println!("Rejected (rate limited): {}", rejected);
    println!("Rejection rate: {:.1}%", rejected as f64 / 1000.0 * 100.0);

    // Should have significant rejections due to low timeout
    assert!(rejected > 500, "Expected significant rate limiting");
    assert_eq!(permitted + rejected, 1000);
}

/// Test: High concurrency with rate limiting
#[tokio::test]
#[ignore]
async fn stress_high_concurrency_rate_limited() {
    let tracker = ConcurrencyTracker::new();
    let permitted_count = Arc::new(AtomicUsize::new(0));
    let tracker_clone = Arc::clone(&tracker);
    let permitted = Arc::clone(&permitted_count);

    let svc = tower::service_fn(move |_req: u32| {
        let tracker = Arc::clone(&tracker_clone);
        let permitted = Arc::clone(&permitted);
        async move {
            tracker.enter();
            permitted.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(10)).await;
            tracker.exit();
            Ok::<_, std::io::Error>(())
        }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(1000)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_secs(5))
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
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            success += 1;
        }
    }

    let elapsed = start.elapsed();
    let peak = tracker.peak();
    let permitted = permitted_count.load(Ordering::Relaxed);

    println!("1000 concurrent rate-limited requests in {:?}", elapsed);
    println!("Peak concurrency: {}", peak);
    println!("Successful: {}", success);
    println!("Permitted service calls: {}", permitted);

    assert_eq!(success, 1000);
    assert_eq!(permitted, 1000);
}

/// Test: Burst traffic handling
#[tokio::test]
#[ignore]
async fn stress_burst_traffic() {
    let permitted_count = Arc::new(AtomicUsize::new(0));
    let rejected_count = Arc::new(AtomicUsize::new(0));
    let permitted = Arc::clone(&permitted_count);

    let svc = tower::service_fn(move |_req: u32| {
        permitted.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(100)
        .refresh_period(Duration::from_millis(100))
        .timeout_duration(Duration::from_millis(5))
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();

    // 10 bursts of 200 requests each
    for burst in 0..10 {
        for i in 0..200 {
            let req = burst * 200 + i;
            match service.ready().await.unwrap().call(req).await {
                Ok(_) => {}
                Err(_) => {
                    rejected_count.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        // Small delay between bursts
        sleep(Duration::from_millis(150)).await;
    }

    let elapsed = start.elapsed();
    let permitted = permitted_count.load(Ordering::Relaxed);
    let rejected = rejected_count.load(Ordering::Relaxed);

    println!("10 bursts of 200 requests in {:?}", elapsed);
    println!("Permitted: {}", permitted);
    println!("Rejected: {}", rejected);
    println!("Total: {}", permitted + rejected);

    assert_eq!(permitted + rejected, 2000);
    // With 100ms refresh and 100 limit, should permit ~1000-1500 over all bursts
    assert!((800..=1600).contains(&permitted));
}

/// Test: Permit refresh over time
#[tokio::test]
#[ignore]
async fn stress_permit_refresh_timing() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(100)
        .refresh_period(Duration::from_millis(100))
        .timeout_duration(Duration::from_millis(200))
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();
    let mut success = 0;

    // Run for 1 second
    let mut i = 0u32;
    while start.elapsed() < Duration::from_secs(1) {
        if service.ready().await.unwrap().call(i).await.is_ok() {
            success += 1;
        }
        i += 1;
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("Permit refresh test over {:?}", elapsed);
    println!("Total requests attempted: {}", i);
    println!("Successful requests: {}", success);
    println!("Actual service calls: {}", actual_calls);
    println!(
        "Effective rate: {:.0} req/sec",
        actual_calls as f64 / elapsed.as_secs_f64()
    );

    // With 100ms refresh and 100 limit, should get ~1000 req/sec
    assert!((800..=1200).contains(&actual_calls));
    assert_eq!(success, actual_calls);
}

/// Test: Memory stability over extended period
#[tokio::test]
#[ignore]
async fn stress_memory_stability() {
    let mem_start = get_memory_usage_mb();

    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(1000)
        .refresh_period(Duration::from_millis(100))
        .timeout_duration(Duration::from_millis(50))
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

/// Test: Timeout behavior under load
#[tokio::test]
#[ignore]
async fn stress_timeout_behavior() {
    let permitted_count = Arc::new(AtomicUsize::new(0));
    let timeout_count = Arc::new(AtomicUsize::new(0));
    let permitted = Arc::clone(&permitted_count);

    let svc = tower::service_fn(move |_req: u32| {
        permitted.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(50)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_millis(1)) // Very short timeout
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
    let permitted = permitted_count.load(Ordering::Relaxed);
    let timeouts = timeout_count.load(Ordering::Relaxed);

    println!("Timeout test: 1000 requests in {:?}", elapsed);
    println!("Permitted: {}", permitted);
    println!("Timed out: {}", timeouts);
    println!("Timeout rate: {:.1}%", timeouts as f64 / 1000.0 * 100.0);

    // With 50 limit and very short timeout, most should timeout
    assert!(timeouts > 900, "Expected most requests to timeout");
    assert!(permitted < 100, "Expected few requests to be permitted");
    assert_eq!(permitted + timeouts, 1000);
}

/// Test: Multiple concurrent rate limiters
#[tokio::test]
#[ignore]
async fn stress_multiple_rate_limiters() {
    let total_calls = Arc::new(AtomicUsize::new(0));

    let create_service = |counter: Arc<AtomicUsize>, limit: usize| {
        let svc = tower::service_fn(move |_req: u32| {
            counter.fetch_add(1, Ordering::Relaxed);
            async { Ok::<_, std::io::Error>(()) }
        });

        let layer = RateLimiterLayer::builder()
            .limit_for_period(limit)
            .refresh_period(Duration::from_millis(100))
            .timeout_duration(Duration::from_millis(50))
            .build();

        layer.layer(svc)
    };

    let service1 = create_service(Arc::clone(&total_calls), 100);
    let service2 = create_service(Arc::clone(&total_calls), 200);
    let service3 = create_service(Arc::clone(&total_calls), 300);

    let start = Instant::now();

    let h1 = {
        let mut svc = service1;
        tokio::spawn(async move {
            for i in 0..500 {
                let _ = svc.ready().await.unwrap().call(i).await;
            }
        })
    };

    let h2 = {
        let mut svc = service2;
        tokio::spawn(async move {
            for i in 0..500 {
                let _ = svc.ready().await.unwrap().call(i).await;
            }
        })
    };

    let h3 = {
        let mut svc = service3;
        tokio::spawn(async move {
            for i in 0..500 {
                let _ = svc.ready().await.unwrap().call(i).await;
            }
        })
    };

    let _ = tokio::join!(h1, h2, h3);

    let elapsed = start.elapsed();
    let calls = total_calls.load(Ordering::Relaxed);

    println!("3 concurrent rate limiters in {:?}", elapsed);
    println!("Total service calls: {}", calls);
    println!(
        "Effective rate: {:.0} req/sec",
        calls as f64 / elapsed.as_secs_f64()
    );

    // Each limiter should permit some requests
    assert!(calls > 0);
    assert!(calls <= 1500); // Max 1500 total
}
