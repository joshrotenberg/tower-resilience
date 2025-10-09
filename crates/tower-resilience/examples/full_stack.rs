//! Example demonstrating multiple resilience patterns.
//!
//! This example shows each resilience patternspattern withworking separate servicesindependently:
//! - Circuit breaker + Bulkhead (from existing example)
//! - Retry with exponential backoff
//! - Timeout for slow calls
//! - Response caching
//!
//! Note: Composing all patterns in a single stack requires unified error handling.
//! See the individual pattern examples for detailed usage.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service};
use tower_resilience::{
    bulkhead::BulkheadLayer,
    cache::CacheLayer,
    circuitbreaker::CircuitBreakerLayer,
    retry::{ExponentialBackoff, RetryLayer},
    timelimiter::TimeLimiterLayer,
};

#[derive(Debug, Clone)]
struct ServiceError;

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Service error")
    }
}

impl std::error::Error for ServiceError {}

impl From<tower_resilience::bulkhead::BulkheadError> for ServiceError {
    fn from(_: tower_resilience::bulkhead::BulkheadError) -> Self {
        ServiceError
    }
}

#[tokio::main]
async fn main() {
    println!("Tower Resilience - Pattern Showcase");
    println!("====================================\n");

    // Demo 1: Circuit Breaker + Bulkhead
    demo_circuit_breaker_and_bulkhead().await;

    // Demo 2: Retry with Exponential Backoff
    demo_retry().await;

    // Demo 3: Timeout
    demo_timeout().await;

    // Demo 4: Cache
    demo_cache().await;

    println!("\n=== All Patterns Demonstrated ===");
}

async fn demo_circuit_breaker_and_bulkhead() {
    println!("--- Demo 1: Circuit Breaker + Bulkhead ---");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst) + 1;
            if count % 3 == 0 {
                Ok(())
            } else {
                Err(ServiceError)
            }
        }
    });

    let bulkhead_layer = BulkheadLayer::builder().max_concurrent_calls(5).build();

    let service = bulkhead_layer.layer(service);

    let cb_layer = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .build();

    let mut service = cb_layer.layer(service);

    for i in 1..=15 {
        match tower::ServiceExt::ready(&mut service)
            .await
            .unwrap()
            .call(())
            .await
        {
            Ok(()) => println!("  Request {}: Success", i),
            Err(_) => println!("  Request {}: Failed", i),
        }
    }

    println!(
        "  Total service calls: {}\n",
        call_count.load(Ordering::SeqCst)
    );
}

async fn demo_retry() {
    println!("--- Demo 2: Retry with Exponential Backoff ---");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst) + 1;
            println!("  [Service] Attempt {}", count);
            if count < 3 {
                Err(ServiceError)
            } else {
                Ok(format!("Success after {} attempts: {}", count, req))
            }
        }
    });

    let retry_layer = RetryLayer::builder()
        .max_attempts(5)
        .backoff(ExponentialBackoff::new(Duration::from_millis(50)))
        .on_retry(|attempt, delay| {
            println!("  [Retry] Attempt {} after {:?}", attempt, delay);
        })
        .build();

    let mut service = retry_layer.layer(service);

    match tower::ServiceExt::ready(&mut service)
        .await
        .unwrap()
        .call("test".to_string())
        .await
    {
        Ok(resp) => println!("  Result: {}\n", resp),
        Err(_) => println!("  Result: Failed after retries\n"),
    }
}

async fn demo_timeout() {
    println!("--- Demo 3: Timeout ---");

    let service = tower::service_fn(|duration: Duration| async move {
        println!("  [Service] Sleeping for {:?}", duration);
        sleep(duration).await;
        Ok::<_, ServiceError>("Completed")
    });

    let timeout_layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(100))
        .on_timeout(|| println!("  [Timeout] Request timed out!"))
        .on_success(|duration| println!("  [Success] Completed in {:?}", duration))
        .build();

    let mut service = timeout_layer.layer(service);

    // Fast request
    println!("  Fast request (50ms):");
    let _ = tower::ServiceExt::ready(&mut service)
        .await
        .unwrap()
        .call(Duration::from_millis(50))
        .await;

    // Slow request
    println!("  Slow request (200ms):");
    let _ = tower::ServiceExt::ready(&mut service)
        .await
        .unwrap()
        .call(Duration::from_millis(200))
        .await;

    println!();
}

async fn demo_cache() {
    println!("--- Demo 4: Cache ---");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst) + 1;
            println!("  [Service] Processing '{}' (call #{})", req, count);
            sleep(Duration::from_millis(100)).await;
            Ok::<_, ServiceError>(format!("Response to {}", req))
        }
    });

    let cache_layer = CacheLayer::builder()
        .max_size(10)
        .ttl(Duration::from_secs(5))
        .key_extractor(|req: &String| req.clone())
        .on_hit(|| println!("  [Cache] Hit!"))
        .on_miss(|| println!("  [Cache] Miss"))
        .build();

    let mut service = cache_layer.layer(service);

    // First request - cache miss
    println!("  Request 'test1' (first time):");
    let _ = tower::ServiceExt::ready(&mut service)
        .await
        .unwrap()
        .call("test1".to_string())
        .await;

    // Second request - cache hit
    println!("  Request 'test1' (cached):");
    let _ = tower::ServiceExt::ready(&mut service)
        .await
        .unwrap()
        .call("test1".to_string())
        .await;

    // Different request
    println!("  Request 'test2' (first time):");
    let _ = tower::ServiceExt::ready(&mut service)
        .await
        .unwrap()
        .call("test2".to_string())
        .await;

    println!(
        "  Total service calls: {}\n",
        call_count.load(Ordering::SeqCst)
    );
}
