//! Health check stress tests

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower_resilience_healthcheck::{
    HealthCheckWrapper, HealthChecker, HealthStatus, SelectionStrategy,
};

use super::{ConcurrencyTracker, get_memory_usage_mb};

/// Mock service for testing
#[derive(Clone)]
struct MockResource {
    #[allow(dead_code)]
    id: usize,
    is_healthy: Arc<AtomicBool>,
    check_count: Arc<AtomicUsize>,
}

impl MockResource {
    fn new(id: usize, is_healthy: bool) -> Self {
        Self {
            id,
            is_healthy: Arc::new(AtomicBool::new(is_healthy)),
            check_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn set_healthy(&self, healthy: bool) {
        self.is_healthy.store(healthy, Ordering::SeqCst);
    }
}

struct TestHealthChecker;

impl HealthChecker<MockResource> for TestHealthChecker {
    async fn check(&self, resource: &MockResource) -> HealthStatus {
        resource.check_count.fetch_add(1, Ordering::SeqCst);
        // Small delay to simulate realistic health check
        sleep(Duration::from_micros(10)).await;

        if resource.is_healthy.load(Ordering::SeqCst) {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy
        }
    }
}

/// Test: Many resources with frequent health checks
#[tokio::test]
#[ignore]
async fn stress_many_resources() {
    println!("\n=== HealthCheck: Many resources ===");

    let mut builder = HealthCheckWrapper::builder();

    // Add 100 resources
    let resources: Vec<_> = (0..100)
        .map(|i| MockResource::new(i, i % 3 != 0)) // Every 3rd is unhealthy
        .collect();

    for (i, resource) in resources.iter().enumerate() {
        builder = builder.with_context(resource.clone(), format!("resource-{}", i));
    }

    let wrapper = builder
        .with_checker(TestHealthChecker)
        .with_interval(Duration::from_millis(100))
        .with_selection_strategy(SelectionStrategy::RoundRobin)
        .build();

    wrapper.start().await;

    // Wait for several health check cycles
    sleep(Duration::from_millis(500)).await;

    let start = Instant::now();
    let requests = 10_000;
    let mut healthy_count = 0;

    for _ in 0..requests {
        if wrapper.get_healthy().await.is_some() {
            healthy_count += 1;
        }
    }

    let elapsed = start.elapsed();

    println!("  Resources: {}", resources.len());
    println!("  Requests: {}", requests);
    println!("  Got healthy: {}", healthy_count);
    println!("  Time: {:?}", elapsed);
    println!(
        "  Throughput: {:.0} req/sec",
        requests as f64 / elapsed.as_secs_f64()
    );

    wrapper.stop().await;

    // Should get healthy resources (not all are unhealthy)
    assert!(healthy_count > requests * 9 / 10);
}

/// Test: High concurrency accessing health check wrapper
#[tokio::test]
#[ignore]
async fn stress_high_concurrency() {
    println!("\n=== HealthCheck: High concurrency ===");

    let resources: Vec<_> = (0..10).map(|i| MockResource::new(i, true)).collect();

    let mut builder = HealthCheckWrapper::builder();
    for (i, resource) in resources.iter().enumerate() {
        builder = builder.with_context(resource.clone(), format!("resource-{}", i));
    }

    let wrapper = Arc::new(
        builder
            .with_checker(TestHealthChecker)
            .with_interval(Duration::from_millis(100))
            .with_selection_strategy(SelectionStrategy::RoundRobin)
            .build(),
    );

    wrapper.start().await;
    sleep(Duration::from_millis(200)).await;

    let tracker = ConcurrencyTracker::new();
    let success_count = Arc::new(AtomicUsize::new(0));

    let start = Instant::now();
    let concurrency = 1000;
    let mut handles = Vec::new();

    for _ in 0..concurrency {
        let wrapper = Arc::clone(&wrapper);
        let tracker = Arc::clone(&tracker);
        let success_count = Arc::clone(&success_count);

        let handle = tokio::spawn(async move {
            tracker.enter();

            // Each task makes 100 requests
            for _ in 0..100 {
                if wrapper.get_healthy().await.is_some() {
                    success_count.fetch_add(1, Ordering::SeqCst);
                }
            }

            tracker.exit();
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let elapsed = start.elapsed();
    let total_requests = concurrency * 100;
    let successes = success_count.load(Ordering::SeqCst);

    println!("  Concurrent tasks: {}", concurrency);
    println!("  Total requests: {}", total_requests);
    println!("  Successes: {}", successes);
    println!("  Peak concurrency: {}", tracker.peak());
    println!("  Time: {:?}", elapsed);
    println!(
        "  Throughput: {:.0} req/sec",
        total_requests as f64 / elapsed.as_secs_f64()
    );

    wrapper.stop().await;

    assert_eq!(successes, total_requests);
    assert!(tracker.peak() > 100);
}

/// Test: Rapid resource status changes
#[tokio::test]
#[ignore]
async fn stress_rapid_status_changes() {
    println!("\n=== HealthCheck: Rapid status changes ===");

    let resources: Vec<_> = (0..5).map(|i| MockResource::new(i, true)).collect();

    let mut builder = HealthCheckWrapper::builder();
    for (i, resource) in resources.iter().enumerate() {
        builder = builder.with_context(resource.clone(), format!("resource-{}", i));
    }

    let wrapper = builder
        .with_checker(TestHealthChecker)
        .with_interval(Duration::from_millis(50))
        .with_selection_strategy(SelectionStrategy::RoundRobin)
        .build();

    wrapper.start().await;
    sleep(Duration::from_millis(100)).await;

    // Spawn task to rapidly toggle resource health
    let resources_clone = resources.clone();
    let toggler = tokio::spawn(async move {
        for _ in 0..100 {
            for resource in &resources_clone {
                let current = resource.is_healthy.load(Ordering::SeqCst);
                resource.set_healthy(!current);
            }
            sleep(Duration::from_millis(10)).await;
        }
    });

    // Meanwhile, continuously request healthy resources
    let start = Instant::now();
    let requests = 10_000;
    let mut healthy_count = 0;

    for _ in 0..requests {
        if wrapper.get_healthy().await.is_some() {
            healthy_count += 1;
        }
    }

    let elapsed = start.elapsed();

    toggler.await.unwrap();
    wrapper.stop().await;

    println!("  Requests: {}", requests);
    println!("  Got healthy: {}", healthy_count);
    println!("  Time: {:?}", elapsed);

    // Should still get some healthy resources despite churn
    assert!(healthy_count > 0);
}

/// Test: Memory stability with continuous health checking
#[tokio::test]
#[ignore]
async fn stress_memory_stability() {
    println!("\n=== HealthCheck: Memory stability ===");

    let mem_start = get_memory_usage_mb();

    let resources: Vec<_> = (0..50).map(|i| MockResource::new(i, i % 2 == 0)).collect();

    let mut builder = HealthCheckWrapper::builder();
    for (i, resource) in resources.iter().enumerate() {
        builder = builder.with_context(resource.clone(), format!("resource-{}", i));
    }

    let wrapper = builder
        .with_checker(TestHealthChecker)
        .with_interval(Duration::from_millis(10))
        .with_selection_strategy(SelectionStrategy::RoundRobin)
        .build();

    wrapper.start().await;

    // Run for a while with continuous health checks
    for _ in 0..10_000 {
        let _ = wrapper.get_healthy().await;
    }

    sleep(Duration::from_secs(1)).await;

    let mem_end = get_memory_usage_mb();
    let mem_growth = mem_end - mem_start;

    wrapper.stop().await;

    println!("  Memory start: {:.2} MB", mem_start);
    println!("  Memory end: {:.2} MB", mem_end);
    println!("  Growth: {:.2} MB", mem_growth);

    // Memory growth should be minimal
    assert!(
        mem_growth < 5.0,
        "Excessive memory growth: {:.2} MB",
        mem_growth
    );
}

/// Test: All selection strategies under load
#[tokio::test]
#[ignore]
async fn stress_selection_strategies() {
    println!("\n=== HealthCheck: Selection strategies ===");

    for strategy in &[SelectionStrategy::RoundRobin] {
        println!("\n  Strategy: RoundRobin");

        let resources: Vec<_> = (0..20)
            .map(|i| MockResource::new(i, i % 4 != 0)) // 75% healthy
            .collect();

        let mut builder = HealthCheckWrapper::builder();
        for (i, resource) in resources.iter().enumerate() {
            builder = builder.with_context(resource.clone(), format!("resource-{}", i));
        }

        let wrapper = builder
            .with_checker(TestHealthChecker)
            .with_interval(Duration::from_millis(100))
            .with_selection_strategy(strategy.clone())
            .build();

        wrapper.start().await;
        sleep(Duration::from_millis(200)).await;

        let start = Instant::now();
        let requests = 10_000;
        let mut healthy_count = 0;

        for _ in 0..requests {
            if wrapper.get_healthy().await.is_some() {
                healthy_count += 1;
            }
        }

        let elapsed = start.elapsed();

        println!("    Requests: {}", requests);
        println!("    Got healthy: {}", healthy_count);
        println!(
            "    Success rate: {:.1}%",
            (healthy_count as f64 / requests as f64) * 100.0
        );
        println!("    Time: {:?}", elapsed);

        wrapper.stop().await;

        // Should get healthy resources most of the time
        assert!(healthy_count > requests * 7 / 10);
    }
}

/// Test: Health check throughput
#[tokio::test]
#[ignore]
async fn stress_health_check_throughput() {
    println!("\n=== HealthCheck: Health check throughput ===");

    let resources: Vec<_> = (0..100).map(|i| MockResource::new(i, true)).collect();

    let check_counts: Vec<_> = resources
        .iter()
        .map(|r| Arc::clone(&r.check_count))
        .collect();

    let mut builder = HealthCheckWrapper::builder();
    for (i, resource) in resources.iter().enumerate() {
        builder = builder.with_context(resource.clone(), format!("resource-{}", i));
    }

    let wrapper = builder
        .with_checker(TestHealthChecker)
        .with_interval(Duration::from_millis(10)) // Very frequent
        .with_selection_strategy(SelectionStrategy::RoundRobin)
        .build();

    wrapper.start().await;

    // Let it run for a bit
    sleep(Duration::from_secs(2)).await;

    wrapper.stop().await;

    let total_checks: usize = check_counts.iter().map(|c| c.load(Ordering::SeqCst)).sum();

    println!("  Resources: {}", resources.len());
    println!("  Total health checks: {}", total_checks);
    println!(
        "  Checks per resource: {:.0}",
        total_checks as f64 / resources.len() as f64
    );
    println!("  Checks per second: {:.0}", total_checks as f64 / 2.0);

    // Should perform many health checks
    assert!(
        total_checks > 1000,
        "Expected many health checks, got {}",
        total_checks
    );
}

/// Test: Recovery after all resources become unhealthy
#[tokio::test]
#[ignore]
async fn stress_recovery_pattern() {
    println!("\n=== HealthCheck: Recovery pattern ===");

    let resources: Vec<_> = (0..10).map(|i| MockResource::new(i, true)).collect();

    let mut builder = HealthCheckWrapper::builder();
    for (i, resource) in resources.iter().enumerate() {
        builder = builder.with_context(resource.clone(), format!("resource-{}", i));
    }

    let wrapper = builder
        .with_checker(TestHealthChecker)
        .with_interval(Duration::from_millis(50))
        .with_selection_strategy(SelectionStrategy::RoundRobin)
        .build();

    wrapper.start().await;
    sleep(Duration::from_millis(100)).await;

    // Phase 1: All healthy
    println!("  Phase 1: All healthy");
    let mut healthy_count = 0;
    for _ in 0..100 {
        if wrapper.get_healthy().await.is_some() {
            healthy_count += 1;
        }
    }
    println!("    Got healthy: {}/100", healthy_count);
    assert_eq!(healthy_count, 100);

    // Phase 2: Make all unhealthy
    println!("  Phase 2: All unhealthy");
    for resource in &resources {
        resource.set_healthy(false);
    }
    sleep(Duration::from_millis(200)).await; // Wait for health checks

    healthy_count = 0;
    for _ in 0..100 {
        if wrapper.get_healthy().await.is_some() {
            healthy_count += 1;
        }
    }
    println!("    Got healthy: {}/100", healthy_count);
    assert_eq!(healthy_count, 0);

    // Phase 3: Recover some resources
    println!("  Phase 3: Partial recovery");
    for (i, resource) in resources.iter().enumerate() {
        if i % 2 == 0 {
            resource.set_healthy(true);
        }
    }
    sleep(Duration::from_millis(200)).await;

    healthy_count = 0;
    for _ in 0..100 {
        if wrapper.get_healthy().await.is_some() {
            healthy_count += 1;
        }
    }
    println!("    Got healthy: {}/100", healthy_count);
    assert!(healthy_count > 90); // Should get healthy most of the time

    // Phase 4: Full recovery
    println!("  Phase 4: Full recovery");
    for resource in &resources {
        resource.set_healthy(true);
    }
    sleep(Duration::from_millis(200)).await;

    healthy_count = 0;
    for _ in 0..100 {
        if wrapper.get_healthy().await.is_some() {
            healthy_count += 1;
        }
    }
    println!("    Got healthy: {}/100", healthy_count);
    assert_eq!(healthy_count, 100);

    wrapper.stop().await;
}
