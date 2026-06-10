//! Executor stress tests - task-spawning delegation under load
//!
//! `ExecutorLayer` spawns each request onto a captured tokio runtime handle and
//! ferries the result back over a oneshot channel. Because it depends on a live
//! runtime, every test runs under `#[tokio::test]`. These tests validate
//! throughput, genuine parallel execution, error propagation across the spawn
//! boundary, and memory stability under sustained load.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_executor::{ExecutorError, ExecutorLayer};

use super::{ConcurrencyTracker, get_memory_usage_mb};

/// Test: High-volume sequential throughput through the spawn boundary.
///
/// Every request crosses a spawn + oneshot round trip. This drives a large
/// sequential volume and validates that results round-trip correctly at scale.
#[tokio::test]
#[ignore]
async fn stress_high_volume_throughput() {
    let service = tower::service_fn(|req: usize| async move { Ok::<usize, Infallible>(req * 2) });

    let mut service = ServiceBuilder::new()
        .layer(ExecutorLayer::current())
        .service(service);

    let total = 100_000;
    let start = Instant::now();
    let mut mismatches = 0;

    for i in 0..total {
        let resp = service.ready().await.unwrap().call(i).await.unwrap();
        if resp != i * 2 {
            mismatches += 1;
        }
    }

    let elapsed = start.elapsed();
    let throughput = total as f64 / elapsed.as_secs_f64();

    println!("Executor throughput: {} requests", total);
    println!("Completed in: {:?}", elapsed);
    println!("Throughput: {:.0} req/sec", throughput);

    assert_eq!(mismatches, 0, "every response must round-trip correctly");
    assert!(throughput > 1_000.0, "should sustain reasonable throughput");
}

/// Test: High concurrency yields genuine parallel execution.
///
/// On a multi-threaded runtime the executor should run many requests at once.
/// A concurrency tracker confirms the peak in-flight count exceeds one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn stress_high_concurrency_parallel() {
    let tracker = ConcurrencyTracker::new();
    let tracker_clone = Arc::clone(&tracker);

    let service = tower::service_fn(move |_req: usize| {
        let tracker = Arc::clone(&tracker_clone);
        async move {
            tracker.enter();
            sleep(Duration::from_millis(5)).await;
            tracker.exit();
            Ok::<(), Infallible>(())
        }
    });

    let service = ServiceBuilder::new()
        .layer(ExecutorLayer::current())
        .service(service);

    let total = 5_000;
    let start = Instant::now();
    let mut handles = Vec::with_capacity(total);

    for i in 0..total {
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

    println!("Executor parallel: {} concurrent requests", total);
    println!("Completed in: {:?}", elapsed);
    println!("Peak concurrency: {}", peak);

    assert_eq!(success, total, "every request should complete successfully");
    assert!(
        peak > 1,
        "executor should run requests in parallel, peak was {}",
        peak
    );
}

/// Test: Service errors propagate across the spawn boundary under load.
///
/// A fraction of requests fail in the inner service. Each failure must surface
/// as `ExecutorError::Service` on the caller side, with an exact count match.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn stress_error_propagation_under_load() {
    let service = tower::service_fn(|req: usize| async move {
        if req.is_multiple_of(7) {
            Err::<usize, &'static str>("boom")
        } else {
            Ok(req)
        }
    });

    let service = ServiceBuilder::new()
        .layer(ExecutorLayer::current())
        .service(service);

    let total: usize = 10_000;
    let expected_errors = (0..total).filter(|i| i.is_multiple_of(7)).count();

    let start = Instant::now();
    let mut handles = Vec::with_capacity(total);

    for i in 0..total {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(i).await
        }));
    }

    let mut service_errors = 0;
    let mut successes = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => successes += 1,
            Err(ExecutorError::Service("boom")) => service_errors += 1,
            Err(other) => panic!("unexpected error variant: {:?}", other),
        }
    }

    let elapsed = start.elapsed();
    println!("Executor error propagation: {} requests", total);
    println!("Completed in: {:?}", elapsed);
    println!(
        "Successes: {}, Service errors: {}",
        successes, service_errors
    );

    assert_eq!(
        service_errors, expected_errors,
        "all inner errors must propagate"
    );
    assert_eq!(
        successes + service_errors,
        total,
        "every request accounted for"
    );
}

/// Test: Memory stays stable across sustained spawn/oneshot churn.
///
/// Each request allocates a task and a oneshot channel. Over many requests the
/// resident memory should not grow unbounded.
#[tokio::test]
#[ignore]
async fn stress_memory_stability() {
    let mem_start = get_memory_usage_mb();

    let service = tower::service_fn(|_req: usize| async move { Ok::<(), Infallible>(()) });

    let mut service = ServiceBuilder::new()
        .layer(ExecutorLayer::current())
        .service(service);

    let total = 50_000;
    let mut mem_samples = vec![];

    for i in 0..total {
        let _ = service.ready().await.unwrap().call(i).await;
        if i % 5_000 == 0 {
            let mem = get_memory_usage_mb();
            if mem > 0.0 {
                mem_samples.push(mem);
            }
        }
    }

    let mem_end = get_memory_usage_mb();
    let mem_delta = mem_end - mem_start;

    println!("Executor memory stability: {} requests", total);
    println!(
        "Start: {:.2} MB, End: {:.2} MB, Delta: {:.2} MB",
        mem_start, mem_end, mem_delta
    );

    if !mem_samples.is_empty() {
        let mem_max = mem_samples.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let mem_min = mem_samples.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        println!("Memory: min={:.2} MB, max={:.2} MB", mem_min, mem_max);
        assert!(
            mem_max - mem_min < 100.0,
            "memory should stay stable under load"
        );
    }

    if mem_delta > 0.0 {
        assert!(mem_delta < 100.0, "memory growth should be bounded");
    }
}
