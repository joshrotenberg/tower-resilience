//! Router stress tests - weighted traffic distribution under load
//!
//! `WeightedRouter` coordinates backend selection through an atomic counter.
//! These tests validate that the weighted distribution stays exact at high
//! volume and under concurrent access, and that many-backend fan-out remains
//! consistent.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tower::util::BoxService;
use tower::{Service, ServiceExt};
use tower_resilience_router::WeightedRouter;

/// All backends must share a single type, so we box them. `BoxService` is not
/// `Clone`, which is why the concurrency test drives a single shared router
/// behind a `Mutex` rather than cloning per task.
type Backend = BoxService<usize, usize, ()>;

/// Builds a backend that counts every request it handles and echoes the value.
fn counting_backend(counter: Arc<AtomicUsize>) -> Backend {
    BoxService::new(tower::service_fn(move |req: usize| {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok::<usize, ()>(req)
        }
    }))
}

/// Test: High-volume deterministic distribution stays exact.
///
/// With weights `[70, 20, 10]` (total 100) and a request count that is a whole
/// number of cycles, the deterministic selector must route exactly according to
/// the weights -- no drift, no rounding error at scale.
#[tokio::test]
#[ignore]
async fn stress_high_volume_deterministic_distribution() {
    let counts: Vec<Arc<AtomicUsize>> = (0..3).map(|_| Arc::new(AtomicUsize::new(0))).collect();

    let mut router = WeightedRouter::builder()
        .route(counting_backend(Arc::clone(&counts[0])), 70)
        .route(counting_backend(Arc::clone(&counts[1])), 20)
        .route(counting_backend(Arc::clone(&counts[2])), 10)
        .build();

    let total = 300_000;
    let start = Instant::now();

    for i in 0..total {
        let _ = router.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let a = counts[0].load(Ordering::Relaxed);
    let b = counts[1].load(Ordering::Relaxed);
    let c = counts[2].load(Ordering::Relaxed);

    println!("Deterministic distribution: {} requests", total);
    println!("Completed in: {:?}", elapsed);
    println!(
        "Throughput: {:.0} req/sec",
        total as f64 / elapsed.as_secs_f64()
    );
    println!("Backend counts: a={}, b={}, c={}", a, b, c);

    // 300k / 100 = 3000 full cycles -> exact weighted split.
    assert_eq!(a, 210_000, "70% backend");
    assert_eq!(b, 60_000, "20% backend");
    assert_eq!(c, 30_000, "10% backend");
    assert_eq!(a + b + c, total, "every request routed exactly once");
}

/// Test: Concurrent routing keeps the distribution exact.
///
/// The selector's atomic counter is the only coordination point. Driving a
/// single shared router from many concurrent tasks must still produce an exact
/// weighted split, because each `call` performs exactly one atomic increment.
#[tokio::test]
#[ignore]
async fn stress_concurrent_distribution_consistency() {
    let count_a = Arc::new(AtomicUsize::new(0));
    let count_b = Arc::new(AtomicUsize::new(0));

    let router = WeightedRouter::builder()
        .route(counting_backend(Arc::clone(&count_a)), 80)
        .route(counting_backend(Arc::clone(&count_b)), 20)
        .build();

    // BoxService backends are not Clone, so share one router instance. The lock
    // is held only long enough to obtain the backend future; the future itself
    // is awaited concurrently outside the lock.
    let router = Arc::new(Mutex::new(router));

    let total = 40_000;
    let start = Instant::now();
    let mut handles = Vec::with_capacity(total);

    for i in 0..total {
        let router = Arc::clone(&router);
        handles.push(tokio::spawn(async move {
            // service_fn backends are always ready, so calling without an
            // explicit poll_ready is safe here.
            let fut = {
                let mut guard = router.lock().unwrap();
                guard.call(i)
            };
            fut.await
        }));
    }

    let mut completed = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            completed += 1;
        }
    }

    let elapsed = start.elapsed();
    let a = count_a.load(Ordering::Relaxed);
    let b = count_b.load(Ordering::Relaxed);

    println!("Concurrent distribution: {} requests", total);
    println!("Completed in: {:?}", elapsed);
    println!("Backend counts: a={}, b={}", a, b);

    assert_eq!(completed, total, "all requests completed successfully");
    // 40k / 100 = 400 full cycles -> exact split regardless of interleaving.
    assert_eq!(a, 32_000, "80% backend under concurrency");
    assert_eq!(b, 8_000, "20% backend under concurrency");
}

/// Test: Many backends with equal weight fan out evenly.
///
/// 64 backends each weighted 1 exercise the cumulative-weight binary search and
/// confirm state consistency across a large backend set.
#[tokio::test]
#[ignore]
async fn stress_many_backends_even_split() {
    let backend_count = 64;
    let counts: Vec<Arc<AtomicUsize>> = (0..backend_count)
        .map(|_| Arc::new(AtomicUsize::new(0)))
        .collect();

    let mut builder = WeightedRouter::builder();
    for c in &counts {
        builder = builder.route(counting_backend(Arc::clone(c)), 1);
    }
    let mut router = builder.build();

    let cycles = 2_000;
    let total = backend_count * cycles;
    let start = Instant::now();

    for i in 0..total {
        let _ = router.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let observed: usize = counts.iter().map(|c| c.load(Ordering::Relaxed)).sum();

    println!(
        "Many backends: {} backends, {} requests",
        backend_count, total
    );
    println!("Completed in: {:?}", elapsed);

    for (idx, c) in counts.iter().enumerate() {
        let n = c.load(Ordering::Relaxed);
        assert_eq!(
            n, cycles,
            "backend {} should get exactly {} requests",
            idx, cycles
        );
    }
    assert_eq!(observed, total, "every request routed exactly once");
}

/// Test: Random strategy converges to the configured weights at volume.
///
/// Unlike the deterministic strategy, random selection only converges
/// statistically. At high volume the observed ratios should land close to the
/// configured weights. Bounds are intentionally generous for CI stability.
#[tokio::test]
#[ignore]
async fn stress_random_strategy_convergence() {
    let counts: Vec<Arc<AtomicUsize>> = (0..3).map(|_| Arc::new(AtomicUsize::new(0))).collect();

    let mut router = WeightedRouter::builder()
        .route(counting_backend(Arc::clone(&counts[0])), 50)
        .route(counting_backend(Arc::clone(&counts[1])), 30)
        .route(counting_backend(Arc::clone(&counts[2])), 20)
        .random()
        .build();

    let total = 300_000;
    let start = Instant::now();

    for i in 0..total {
        let _ = router.ready().await.unwrap().call(i).await;
    }

    let elapsed = start.elapsed();
    let ratios: Vec<f64> = counts
        .iter()
        .map(|c| c.load(Ordering::Relaxed) as f64 / total as f64)
        .collect();

    println!("Random strategy: {} requests", total);
    println!("Completed in: {:?}", elapsed);
    println!(
        "Observed ratios: {:.3}, {:.3}, {:.3}",
        ratios[0], ratios[1], ratios[2]
    );

    let expected = [0.50, 0.30, 0.20];
    for (idx, (&r, &e)) in ratios.iter().zip(expected.iter()).enumerate() {
        assert!(
            (r - e).abs() < 0.05,
            "backend {} ratio {:.3} should be near {:.2}",
            idx,
            r,
            e
        );
    }
}
