//! Hedge stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_hedge::{HedgeError, HedgeLayer};

use super::{ConcurrencyTracker, get_memory_usage_mb};

#[derive(Clone, Debug)]
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

/// Test: High volume with fast primary (no hedges triggered)
#[tokio::test]
#[ignore]
async fn stress_high_volume_fast_primary() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok::<_, TestError>("fast response".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(100))
        .max_hedged_attempts(2)
        .build();
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..100_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("100k calls (fast primary) completed in {:?}", elapsed);
    println!(
        "Throughput: {:.0} calls/sec",
        100_000.0 / elapsed.as_secs_f64()
    );
    println!("Actual service calls: {}", actual_calls);

    // Should be exactly 100k since primary is fast
    assert_eq!(actual_calls, 100_000);
}

/// Test: Parallel mode - all hedges fire immediately
#[tokio::test]
#[ignore]
async fn stress_parallel_mode_high_volume() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            // Small delay to ensure all parallel calls start
            sleep(Duration::from_micros(100)).await;
            Ok::<_, TestError>("response".to_string())
        }
    });

    let layer = HedgeLayer::<u32, String, TestError>::builder()
        .no_delay()
        .max_hedged_attempts(3)
        .build();
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..10_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    // Wait for all spawned tasks to complete
    sleep(Duration::from_millis(50)).await;

    let elapsed = start.elapsed();
    let actual_calls = call_count.load(Ordering::Relaxed);

    println!("10k calls (parallel mode x3) completed in {:?}", elapsed);
    println!("Actual service calls: {}", actual_calls);

    // Should be 3x since we're in parallel mode with 3 attempts
    assert!(
        actual_calls >= 25_000,
        "Expected ~30k calls, got {}",
        actual_calls
    );
}

/// Test: Slow primary triggers hedge, hedge wins
#[tokio::test]
#[ignore]
async fn stress_hedge_wins_over_slow_primary() {
    let primary_count = Arc::new(AtomicUsize::new(0));
    let hedge_count = Arc::new(AtomicUsize::new(0));
    let pc = Arc::clone(&primary_count);
    let hc = Arc::clone(&hedge_count);

    let svc = tower::service_fn(move |_req: u32| {
        let pc = Arc::clone(&pc);
        let hc = Arc::clone(&hc);
        async move {
            let call_num = pc.fetch_add(1, Ordering::Relaxed);
            // Every other call is slow (simulating primary)
            // Note: Due to parallel execution, we can't guarantee order
            if call_num.is_multiple_of(2) {
                sleep(Duration::from_millis(50)).await;
            } else {
                hc.fetch_add(1, Ordering::Relaxed);
            }
            Ok::<_, TestError>("response".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(10))
        .max_hedged_attempts(2)
        .build();
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..1_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let total_calls = primary_count.load(Ordering::Relaxed);

    println!("1000 calls (hedge wins) completed in {:?}", elapsed);
    println!("Total service calls: {}", total_calls);
    println!(
        "Average latency: {:.2}ms",
        elapsed.as_millis() as f64 / 1000.0
    );

    // Should have more than 1000 calls due to hedging
    assert!(total_calls > 1_000);
}

/// Test: High concurrency with hedging
#[tokio::test]
#[ignore]
async fn stress_high_concurrency_with_hedging() {
    let tracker = ConcurrencyTracker::new();
    let call_count = Arc::new(AtomicUsize::new(0));
    let tracker_clone = Arc::clone(&tracker);
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        let tracker = Arc::clone(&tracker_clone);
        let counter = Arc::clone(&counter);
        async move {
            tracker.enter();
            counter.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(10)).await;
            tracker.exit();
            Ok::<_, TestError>("response".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(5))
        .max_hedged_attempts(2)
        .build();
    let service = layer.layer(svc);

    let start = Instant::now();
    let mut handles = vec![];

    for i in 0..500 {
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
    let total_calls = call_count.load(Ordering::Relaxed);

    println!("500 concurrent hedged requests in {:?}", elapsed);
    println!("Peak concurrency: {}", peak);
    println!("Total service calls: {}", total_calls);

    assert!(peak > 50, "Expected high concurrency, got {}", peak);
}

/// Test: All attempts fail
#[tokio::test]
#[ignore]
async fn stress_all_attempts_fail() {
    let failure_count = Arc::new(AtomicUsize::new(0));
    let fc = Arc::clone(&failure_count);

    let svc = tower::service_fn(move |_req: u32| {
        let fc = Arc::clone(&fc);
        async move {
            fc.fetch_add(1, Ordering::Relaxed);
            Err::<String, _>(TestError::new("always fail"))
        }
    });

    let layer = HedgeLayer::<u32, String, TestError>::builder()
        .no_delay()
        .max_hedged_attempts(3)
        .build();
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..10_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(matches!(result, Err(HedgeError::AllAttemptsFailed(_))));
    }

    // Wait for spawned tasks
    sleep(Duration::from_millis(50)).await;

    let elapsed = start.elapsed();
    let failures = failure_count.load(Ordering::Relaxed);

    println!("10k calls (all fail) completed in {:?}", elapsed);
    println!("Total failure attempts: {}", failures);

    // Should be 3x since all 3 attempts fail
    assert!(
        failures >= 25_000,
        "Expected ~30k attempts, got {}",
        failures
    );
}

/// Test: Dynamic delay function under load
#[tokio::test]
#[ignore]
async fn stress_dynamic_delay() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            // Slow enough to trigger hedge
            sleep(Duration::from_millis(30)).await;
            Ok::<_, TestError>("response".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        // Increasing delays: 5ms, 10ms, 15ms
        .delay_fn(|attempt| Duration::from_millis(5 * attempt as u64))
        .max_hedged_attempts(4)
        .build();
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..1_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let total_calls = call_count.load(Ordering::Relaxed);

    println!("1000 calls with dynamic delay in {:?}", elapsed);
    println!("Total service calls: {}", total_calls);

    // Should have triggered multiple hedges due to slow service
    assert!(total_calls > 1_000);
}

/// Test: Memory stability over extended period
#[tokio::test]
#[ignore]
async fn stress_memory_stability() {
    let mem_start = get_memory_usage_mb();

    let svc = tower::service_fn(|_req: u32| async move {
        sleep(Duration::from_micros(10)).await;
        Ok::<_, TestError>("response".to_string())
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(5))
        .max_hedged_attempts(2)
        .build();
    let mut service = layer.layer(svc);

    let mut mem_samples = vec![];

    // Run for 10 seconds
    let start = Instant::now();
    let mut i = 0u32;

    while start.elapsed() < Duration::from_secs(10) {
        let _ = service.ready().await.unwrap().call(i).await;
        i += 1;

        // Sample memory every 5000 calls
        if i.is_multiple_of(5000) {
            let mem = get_memory_usage_mb();
            if mem > 0.0 {
                mem_samples.push(mem);
            }
        }
    }

    let mem_end = get_memory_usage_mb();

    println!("Ran {} hedged calls over 10 seconds", i);
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

/// Test: Latency improvement measurement
#[tokio::test]
#[ignore]
async fn stress_latency_improvement() {
    // Track call count to create variable latency per service call, not per request
    let call_counter = Arc::new(AtomicUsize::new(0));

    // Baseline without hedging - simulate variable latency
    let mut baseline_latencies = vec![];
    for i in 0..100 {
        let start = Instant::now();
        // 10% of requests are slow (100ms vs 5ms)
        if i % 10 == 0 {
            sleep(Duration::from_millis(100)).await;
        } else {
            sleep(Duration::from_millis(5)).await;
        }
        baseline_latencies.push(start.elapsed());
    }

    // With hedging - the key insight is that each service CALL has independent
    // latency, so if the primary is slow, the hedge will likely be fast
    let cc = Arc::clone(&call_counter);
    let hedged_svc = tower::service_fn(move |_req: u32| {
        let cc = Arc::clone(&cc);
        async move {
            let call_num = cc.fetch_add(1, Ordering::Relaxed);
            // Only first call of each pair is slow (simulates primary being slow,
            // hedge being fast due to load balancing, different server, etc.)
            if call_num.is_multiple_of(2) {
                sleep(Duration::from_millis(100)).await;
            } else {
                sleep(Duration::from_millis(5)).await;
            }
            Ok::<_, TestError>("response".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(10))
        .max_hedged_attempts(2)
        .build();
    let mut hedged_svc = layer.layer(hedged_svc);

    let mut hedged_latencies = vec![];
    for i in 0..100 {
        let start = Instant::now();
        let _ = hedged_svc.ready().await.unwrap().call(i).await;
        hedged_latencies.push(start.elapsed());
    }

    // Calculate P99
    baseline_latencies.sort();
    hedged_latencies.sort();

    let baseline_p99 = baseline_latencies[98];
    let hedged_p99 = hedged_latencies[98];

    println!("Baseline P99: {:?}", baseline_p99);
    println!("Hedged P99: {:?}", hedged_p99);
    println!(
        "Latency improvement: {:.1}%",
        (1.0 - hedged_p99.as_secs_f64() / baseline_p99.as_secs_f64()) * 100.0
    );

    // Hedged P99 should be significantly better (hedge at 10ms + fast response at 5ms = ~15ms vs 100ms)
    assert!(
        hedged_p99 < baseline_p99,
        "Expected hedging to improve P99 latency"
    );
}

/// Test: Burst load with hedging
#[tokio::test]
#[ignore]
async fn stress_burst_load() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            sleep(Duration::from_millis(5)).await;
            Ok::<_, TestError>("response".to_string())
        }
    });

    let layer = HedgeLayer::builder()
        .delay(Duration::from_millis(10))
        .max_hedged_attempts(2)
        .build();
    let service = layer.layer(svc);

    let start = Instant::now();

    // 10 bursts of 100 concurrent requests
    for burst in 0..10 {
        let mut handles = vec![];
        for i in 0..100 {
            let mut svc = service.clone();
            let req = burst * 100 + i;
            handles.push(tokio::spawn(async move {
                svc.ready().await.unwrap().call(req).await
            }));
        }

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }

        // Small gap between bursts
        sleep(Duration::from_millis(10)).await;
    }

    let elapsed = start.elapsed();
    let total_calls = call_count.load(Ordering::Relaxed);

    println!("10 bursts of 100 requests in {:?}", elapsed);
    println!("Total service calls: {}", total_calls);

    // Should have at least 1000 calls (1 per request)
    assert!(total_calls >= 1_000);
}

/// Test: Named hedge instances under load
#[tokio::test]
#[ignore]
async fn stress_named_instances() {
    let svc =
        tower::service_fn(|_req: u32| async move { Ok::<_, TestError>("response".to_string()) });

    let layer = HedgeLayer::builder()
        .name("stress-test-hedge")
        .delay(Duration::from_millis(100))
        .max_hedged_attempts(2)
        .build();
    let mut service = layer.layer(svc);

    let start = Instant::now();

    for i in 0..50_000 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();

    println!("50k calls with named hedge in {:?}", elapsed);
    println!(
        "Throughput: {:.0} calls/sec",
        50_000.0 / elapsed.as_secs_f64()
    );
}
