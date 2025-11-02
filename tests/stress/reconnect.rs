//! Reconnect stress tests

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tower::{Layer, Service};
use tower_resilience_reconnect::{ReconnectConfig, ReconnectLayer, ReconnectPolicy};

use super::{ConcurrencyTracker, get_memory_usage_mb};

/// Service that fails for a configured number of attempts
#[derive(Clone)]
struct FailingService {
    fail_count: Arc<AtomicUsize>,
    max_fails: usize,
    call_count: Arc<AtomicUsize>,
}

impl FailingService {
    fn new(max_fails: usize) -> Self {
        Self {
            fail_count: Arc::new(AtomicUsize::new(0)),
            max_fails,
            call_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Service<String> for FailingService {
    type Response = String;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: String) -> Self::Future {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
        let max_fails = self.max_fails;

        Box::pin(async move {
            if count < max_fails {
                Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "connection failed",
                ))
            } else {
                Ok(format!("Response: {}", req))
            }
        })
    }
}

/// Test: High volume of reconnection attempts
#[tokio::test]
#[ignore]
async fn stress_one_million_successful_calls() {
    println!("\n=== Reconnect: 1M successful calls (no failures) ===");

    let inner = FailingService::new(0); // Never fails
    let call_count = Arc::clone(&inner.call_count);

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
        .max_attempts(3)
        .build();

    let layer = ReconnectLayer::new(config);
    let mut service = layer.layer(inner);

    let start = Instant::now();
    let count = 1_000_000;

    for i in 0..count {
        let result = service.call(format!("req-{}", i)).await;
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let calls_per_sec = count as f64 / elapsed.as_secs_f64();

    println!("  Completed: {} calls", count);
    println!("  Time: {:?}", elapsed);
    println!("  Throughput: {:.0} calls/sec", calls_per_sec);
    println!(
        "  Total service calls: {}",
        call_count.load(Ordering::SeqCst)
    );

    assert_eq!(call_count.load(Ordering::SeqCst), count);
}

/// Test: High volume with reconnections
#[tokio::test]
#[ignore]
async fn stress_high_volume_with_reconnections() {
    println!("\n=== Reconnect: High volume with reconnections ===");

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
        .max_attempts(10)
        .build();

    let layer = ReconnectLayer::new(config);

    let start = Instant::now();
    let count = 10_000;
    let mut successes = 0;
    let mut total_attempts = 0;

    for _ in 0..count {
        // Each iteration creates a new failing service that fails 3 times
        let inner = FailingService::new(3);
        let attempts_tracker = Arc::clone(&inner.call_count);
        let mut service = layer.layer(inner);

        let result = service.call("test".to_string()).await;
        if result.is_ok() {
            successes += 1;
        }
        total_attempts += attempts_tracker.load(Ordering::SeqCst);
    }

    let elapsed = start.elapsed();

    println!("  Requests: {}", count);
    println!("  Successes: {}", successes);
    println!("  Total attempts: {}", total_attempts);
    println!("  Time: {:?}", elapsed);
    println!(
        "  Avg attempts per request: {:.2}",
        total_attempts as f64 / count as f64
    );

    assert_eq!(successes, count);
    assert!(total_attempts >= count * 4); // Should be 4 attempts each (3 fails + 1 success)
}

/// Test: High concurrency with reconnections
#[tokio::test]
#[ignore]
async fn stress_high_concurrency_with_reconnections() {
    println!("\n=== Reconnect: High concurrency ===");

    let tracker = ConcurrencyTracker::new();
    let successes = Arc::new(AtomicUsize::new(0));

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(1),
            Duration::from_millis(100),
        ))
        .max_attempts(5)
        .build();

    let layer = ReconnectLayer::new(config);

    let start = Instant::now();
    let concurrency = 1000;
    let mut handles = Vec::new();

    for _ in 0..concurrency {
        let layer = layer.clone();
        let tracker = Arc::clone(&tracker);
        let successes = Arc::clone(&successes);

        let handle = tokio::spawn(async move {
            tracker.enter();

            let inner = FailingService::new(2); // Fail 2 times
            let mut service = layer.layer(inner);

            let result = service.call("test".to_string()).await;
            if result.is_ok() {
                successes.fetch_add(1, Ordering::SeqCst);
            }

            tracker.exit();
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let elapsed = start.elapsed();
    let success_count = successes.load(Ordering::SeqCst);

    println!("  Concurrent requests: {}", concurrency);
    println!("  Successes: {}", success_count);
    println!("  Peak concurrency: {}", tracker.peak());
    println!("  Time: {:?}", elapsed);

    assert_eq!(success_count, concurrency);
    assert!(tracker.peak() > 100, "Should have high concurrency");
}

/// Test: Exponential backoff timing accuracy
#[tokio::test]
#[ignore]
async fn stress_exponential_backoff_timing() {
    println!("\n=== Reconnect: Exponential backoff timing ===");

    let inner = FailingService::new(5);
    let attempts_tracker = Arc::clone(&inner.fail_count);

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(10),
            Duration::from_millis(1000),
        ))
        .max_attempts(6)
        .build();

    let layer = ReconnectLayer::new(config);
    let mut service = layer.layer(inner);

    let start = Instant::now();
    let result = service.call("test".to_string()).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    assert_eq!(attempts_tracker.load(Ordering::SeqCst), 6);

    // Expected delays: 10ms, 20ms, 40ms, 80ms, 160ms ≈ 310ms minimum
    println!("  Attempts: {}", attempts_tracker.load(Ordering::SeqCst));
    println!("  Elapsed: {:?}", elapsed);
    println!("  Expected minimum: ~300ms with exponential backoff");

    assert!(
        elapsed.as_millis() >= 200,
        "Expected at least 200ms with backoff, got {:?}",
        elapsed
    );
}

/// Test: Memory stability over many reconnect cycles
#[tokio::test]
#[ignore]
async fn stress_memory_stability() {
    println!("\n=== Reconnect: Memory stability ===");

    let mem_start = get_memory_usage_mb();

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
        .max_attempts(10)
        .build();

    let layer = ReconnectLayer::new(config);

    // Perform many reconnect cycles
    for _ in 0..10_000 {
        let inner = FailingService::new(3);
        let mut service = layer.layer(inner);
        let _ = service.call("test".to_string()).await;
    }

    let mem_end = get_memory_usage_mb();
    let mem_growth = mem_end - mem_start;

    println!("  Memory start: {:.2} MB", mem_start);
    println!("  Memory end: {:.2} MB", mem_end);
    println!("  Growth: {:.2} MB", mem_growth);

    // Memory growth should be minimal (< 10MB for 10k cycles)
    assert!(
        mem_growth < 10.0,
        "Excessive memory growth: {:.2} MB",
        mem_growth
    );
}

/// Test: Unlimited attempts with eventual success
#[tokio::test]
#[ignore]
async fn stress_unlimited_attempts() {
    println!("\n=== Reconnect: Unlimited attempts ===");

    let inner = FailingService::new(100); // Fail many times
    let attempts_tracker = Arc::clone(&inner.fail_count);

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
        .unlimited_attempts()
        .build();

    let layer = ReconnectLayer::new(config);
    let mut service = layer.layer(inner);

    let start = Instant::now();
    let result = service.call("test".to_string()).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());

    let attempts = attempts_tracker.load(Ordering::SeqCst);
    println!("  Attempts until success: {}", attempts);
    println!("  Time: {:?}", elapsed);

    assert!(attempts >= 101, "Should keep trying until success");
}

/// Test: Mixed success and failure patterns
#[tokio::test]
#[ignore]
async fn stress_mixed_patterns() {
    println!("\n=== Reconnect: Mixed success/failure patterns ===");

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(1),
            Duration::from_millis(50),
        ))
        .max_attempts(5)
        .build();

    let layer = ReconnectLayer::new(config);

    let start = Instant::now();
    let iterations = 1000;
    let mut successes = 0;
    let mut failures = 0;

    for i in 0..iterations {
        // Vary failure count: 0-4 failures
        let fail_count = i % 5;
        let inner = FailingService::new(fail_count);
        let mut service = layer.layer(inner);

        match service.call(format!("req-{}", i)).await {
            Ok(_) => successes += 1,
            Err(_) => failures += 1,
        }
    }

    let elapsed = start.elapsed();

    println!("  Iterations: {}", iterations);
    println!("  Successes: {}", successes);
    println!("  Failures: {}", failures);
    println!("  Time: {:?}", elapsed);

    // Most should succeed (only those with 4 failures should fail with max_attempts=5)
    assert!(successes >= iterations * 4 / 5);
}

/// Test: Reconnect predicate filtering
#[tokio::test]
#[ignore]
async fn stress_reconnect_predicate() {
    println!("\n=== Reconnect: Predicate filtering ===");

    let should_reconnect = Arc::new(AtomicBool::new(true));
    let should_reconnect_clone = Arc::clone(&should_reconnect);

    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::fixed(Duration::from_millis(1)))
        .max_attempts(10)
        .reconnect_predicate(move |error: &dyn std::error::Error| {
            // Only reconnect on connection errors
            error.to_string().contains("connection")
                && should_reconnect_clone.load(Ordering::SeqCst)
        })
        .build();

    let layer = ReconnectLayer::new(config);

    let start = Instant::now();
    let mut reconnected = 0;
    let mut failed_immediately = 0;

    for i in 0..1000 {
        // Toggle predicate every 100 iterations
        if i % 100 == 0 {
            should_reconnect.fetch_xor(true, Ordering::SeqCst);
        }

        let inner = FailingService::new(2);
        let mut service = layer.layer(inner);

        match service.call("test".to_string()).await {
            Ok(_) => reconnected += 1,
            Err(_) => failed_immediately += 1,
        }
    }

    let elapsed = start.elapsed();

    println!("  Total requests: 1000");
    println!("  Reconnected and succeeded: {}", reconnected);
    println!("  Failed immediately: {}", failed_immediately);
    println!("  Time: {:?}", elapsed);

    assert!(reconnected > 0);
    assert!(failed_immediately > 0);
}
