//! Rate limiter stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Barrier;
use tokio::time::{Instant, sleep};
use tower::{Layer, Service, ServiceExt};
use tower_resilience_ratelimiter::RateLimiterLayer;

use super::{ConcurrencyTracker, get_memory_usage_mb};

/// Starts a batch of cloned services at one virtual instant and reports outcomes.
///
/// A barrier makes the asserted admission window explicit: task-spawn
/// latency cannot silently replenish permits before some callers arrive,
/// and a paused virtual clock keeps the outcome independent of scheduler
/// speed.
async fn run_gated_batch<S>(service: &S, requests: std::ops::Range<u32>) -> (usize, usize)
where
    S: Service<u32> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    let barrier = Arc::new(Barrier::new(requests.len() + 1));
    let mut handles = Vec::with_capacity(requests.len());

    for request in requests {
        let barrier = Arc::clone(&barrier);
        let mut service = service.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let ready = service
                .ready()
                .await
                .unwrap_or_else(|_| panic!("rate limiter readiness failed"));
            ready.call(request).await.is_ok()
        }));
    }

    barrier.wait().await;

    let mut admitted = 0;
    let mut rejected = 0;
    for handle in handles {
        if handle.await.expect("rate-limit task panicked") {
            admitted += 1;
        } else {
            rejected += 1;
        }
    }

    (admitted, rejected)
}

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
        .build()
        .unwrap();

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
#[tokio::test(start_paused = true)]
#[ignore]
async fn stress_rate_limit_enforcement() {
    const LIMIT: usize = 100;
    const CALLERS: u32 = 1000;
    const TIMEOUT: Duration = Duration::from_millis(10);

    let permitted_count = Arc::new(AtomicUsize::new(0));
    let permitted = Arc::clone(&permitted_count);

    let svc = tower::service_fn(move |_req: u32| {
        permitted.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(LIMIT)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(TIMEOUT) // Short timeout
        .build()
        .unwrap();

    let service = layer.layer(svc);

    let start = Instant::now();
    let (admitted, rejected) = run_gated_batch(&service, 0..CALLERS).await;
    let elapsed = start.elapsed();

    println!("{CALLERS} simultaneous calls with rate limiting in {elapsed:?}");
    println!("Permitted: {admitted}");
    println!("Rejected (rate limited): {rejected}");
    println!(
        "Rejection rate: {:.1}%",
        rejected as f64 / f64::from(CALLERS) * 100.0
    );

    // All callers arrive in the same fixed window (barrier-gated). Exactly
    // LIMIT consume a permit; the short timeout rejects the rest before the
    // next refresh -- deterministic under the paused clock, independent of
    // scheduler speed.
    assert_eq!(admitted, LIMIT);
    assert_eq!(rejected, CALLERS as usize - LIMIT);
    assert_eq!(permitted_count.load(Ordering::Relaxed), admitted);
    assert_eq!(admitted + rejected, CALLERS as usize);
    assert_eq!(elapsed, TIMEOUT);
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
        .build()
        .unwrap();

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
#[tokio::test(start_paused = true)]
#[ignore]
async fn stress_burst_traffic() {
    const BURSTS: u32 = 10;
    const CALLERS_PER_BURST: u32 = 200;
    const LIMIT: usize = 100;
    const REFRESH_PERIOD: Duration = Duration::from_millis(100);

    let permitted_count = Arc::new(AtomicUsize::new(0));
    let permitted = Arc::clone(&permitted_count);

    let svc = tower::service_fn(move |_req: u32| {
        permitted.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(LIMIT)
        .refresh_period(REFRESH_PERIOD)
        .timeout_duration(Duration::from_millis(5))
        .build()
        .unwrap();

    let service = layer.layer(svc);

    let start = Instant::now();
    let mut admitted = 0;
    let mut rejected = 0;

    // 10 barrier-gated bursts of 200 concurrent requests, one full refresh
    // period apart so each burst starts against a freshly reset window.
    for burst in 0..BURSTS {
        let first_request = burst * CALLERS_PER_BURST;
        let (batch_admitted, batch_rejected) =
            run_gated_batch(&service, first_request..first_request + CALLERS_PER_BURST).await;
        assert_eq!(batch_admitted, LIMIT, "burst {burst} admission count");
        assert_eq!(
            batch_rejected,
            CALLERS_PER_BURST as usize - LIMIT,
            "burst {burst} rejection count"
        );
        admitted += batch_admitted;
        rejected += batch_rejected;

        if burst + 1 < BURSTS {
            sleep(REFRESH_PERIOD).await;
        }
    }

    let elapsed = start.elapsed();

    println!("{BURSTS} bursts of {CALLERS_PER_BURST} requests in {elapsed:?}");
    println!("Permitted: {admitted}");
    println!("Rejected: {rejected}");
    println!("Total: {}", admitted + rejected);

    let total = (BURSTS * CALLERS_PER_BURST) as usize;
    assert_eq!(admitted, BURSTS as usize * LIMIT);
    assert_eq!(permitted_count.load(Ordering::Relaxed), admitted);
    assert_eq!(admitted + rejected, total);
}

/// Test: Permit refresh over time
#[tokio::test(start_paused = true)]
#[ignore]
async fn stress_permit_refresh_timing() {
    const LIMIT: usize = 100;
    const CALLERS: u32 = 1000;
    const REFRESH_PERIOD: Duration = Duration::from_millis(100);

    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(LIMIT)
        .refresh_period(REFRESH_PERIOD)
        // Wide enough that no caller times out while waiting out refreshes.
        .timeout_duration(Duration::from_secs(2))
        .build()
        .unwrap();

    let service = layer.layer(svc);

    let start = Instant::now();
    let (admitted, rejected) = run_gated_batch(&service, 0..CALLERS).await;
    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    // The initial window admits LIMIT immediately; every following group of
    // LIMIT callers needs one more complete refresh. With a timeout longer
    // than the total wait, all callers are eventually admitted after nine
    // refreshes -- deterministic under the paused clock, independent of
    // scheduler or wall-clock speed.
    let refreshes_needed = (CALLERS as usize - 1) / LIMIT;
    let expected_elapsed = REFRESH_PERIOD.saturating_mul(refreshes_needed as u32);

    println!("Permit refresh test over {elapsed:?}");
    println!("Total requests attempted: {CALLERS}");
    println!("Successful requests: {admitted}");
    println!("Actual service calls: {actual_calls}");

    assert_eq!(admitted, CALLERS as usize);
    assert_eq!(rejected, 0);
    assert_eq!(actual_calls, admitted);
    assert_eq!(elapsed, expected_elapsed);
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
        .build()
        .unwrap();

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
#[tokio::test(start_paused = true)]
#[ignore]
async fn stress_timeout_behavior() {
    const LIMIT: usize = 50;
    const CALLERS: u32 = 1000;
    const TIMEOUT: Duration = Duration::from_millis(1);

    let permitted_count = Arc::new(AtomicUsize::new(0));
    let permitted = Arc::clone(&permitted_count);

    let svc = tower::service_fn(move |_req: u32| {
        permitted.fetch_add(1, Ordering::Relaxed);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(LIMIT)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(TIMEOUT) // Very short timeout
        .build()
        .unwrap();

    let service = layer.layer(svc);

    let start = Instant::now();
    let (admitted, timeouts) = run_gated_batch(&service, 0..CALLERS).await;
    let elapsed = start.elapsed();

    println!("Timeout test: {CALLERS} simultaneous requests in {elapsed:?}");
    println!("Permitted: {admitted}");
    println!("Timed out: {timeouts}");
    println!(
        "Timeout rate: {:.1}%",
        timeouts as f64 / f64::from(CALLERS) * 100.0
    );

    // All callers arrive together; exactly LIMIT consume a permit and the
    // 1ms timeout rejects the rest before the next refresh -- deterministic
    // under the paused clock.
    assert_eq!(admitted, LIMIT);
    assert_eq!(timeouts, CALLERS as usize - LIMIT);
    assert_eq!(permitted_count.load(Ordering::Relaxed), admitted);
    assert_eq!(admitted + timeouts, CALLERS as usize);
    assert_eq!(elapsed, TIMEOUT);
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
            .build()
            .unwrap();

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
