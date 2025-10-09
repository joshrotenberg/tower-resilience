//! Comprehensive metrics example showing all resilience patterns.
//!
//! This example demonstrates:
//! - Enabling metrics for all patterns
//! - Instance naming for multi-instance tracking
//! - How metrics are recorded during operations
//!
//! Run with:
//! ```sh
//! cargo run --example observability_metrics --features metrics,circuitbreaker,bulkhead,retry,ratelimiter,timelimiter,cache
//! ```
//!
//! To collect and visualize these metrics in production:
//! 1. Add `metrics-exporter-prometheus` to your dependencies
//! 2. Install the Prometheus recorder at startup
//! 3. Expose a /metrics HTTP endpoint for Prometheus to scrape

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_bulkhead::BulkheadLayer;
use tower_resilience_cache::CacheLayer;
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_ratelimiter::RateLimiterLayer;
use tower_resilience_retry::RetryLayer;
use tower_resilience_timelimiter::TimeLimiterLayer;

#[derive(Debug, Clone)]
struct AppError(String);

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AppError {}

impl From<tower_resilience_bulkhead::BulkheadError> for AppError {
    fn from(e: tower_resilience_bulkhead::BulkheadError) -> Self {
        AppError(format!("bulkhead: {}", e))
    }
}

impl From<tower_resilience_ratelimiter::RateLimiterError> for AppError {
    fn from(e: tower_resilience_ratelimiter::RateLimiterError) -> Self {
        AppError(format!("rate limiter: {}", e))
    }
}

#[tokio::main]
async fn main() {
    println!("📊 Tower Resilience - Metrics Demo\n");
    println!("This example demonstrates metrics collection with named instances.");
    println!("In production, add metrics-exporter-prometheus and expose a /metrics endpoint.\n");

    // Demonstrate different patterns individually
    demo_circuit_breaker().await;
    demo_retry().await;
    demo_time_limiter().await;
    demo_bulkhead().await;
    demo_rate_limiter().await;
    demo_cache().await;

    println!("\n✅ Demo complete!");
    println!("\n📈 Metrics that would be available in Prometheus:");
    println!("   Circuit Breaker:");
    println!(
        "     - circuitbreaker_calls_total{{circuitbreaker=\"user-service\",outcome=\"success|failure|rejected\"}}"
    );
    println!(
        "     - circuitbreaker_transitions_total{{circuitbreaker=\"user-service\",from=\"Closed\",to=\"Open\"}}"
    );
    println!(
        "     - circuitbreaker_state{{circuitbreaker=\"user-service\",state=\"Open|Closed|HalfOpen\"}}"
    );
    println!("   Retry:");
    println!("     - retry_calls_total{{retry=\"api-retry\",result=\"success|exhausted\"}}");
    println!("     - retry_attempts_total{{retry=\"api-retry\"}}");
    println!("   Bulkhead:");
    println!("     - bulkhead_calls_permitted_total{{bulkhead=\"payment-bulkhead\"}}");
    println!("     - bulkhead_concurrent_calls{{bulkhead=\"payment-bulkhead\"}}");
    println!("   Rate Limiter:");
    println!(
        "     - ratelimiter_calls_total{{ratelimiter=\"payment-ratelimit\",result=\"permitted|rejected\"}}"
    );
    println!("   Time Limiter:");
    println!(
        "     - timelimiter_calls_total{{timelimiter=\"api-timeout\",result=\"success|timeout\"}}"
    );
    println!("   Cache:");
    println!("     - cache_requests_total{{cache=\"data-cache\",result=\"hit|miss\"}}");
    println!("     - cache_size{{cache=\"data-cache\"}}");
    println!("\n📊 Example Prometheus Queries:");
    println!("   Failure Rate:");
    println!(
        "     rate(circuitbreaker_calls_total{{outcome=\"failure\"}}[5m]) / rate(circuitbreaker_calls_total[5m])"
    );
    println!("   Retry Attempts per Call:");
    println!("     rate(retry_attempts_total[5m]) / rate(retry_calls_total[5m])");
    println!("   Cache Hit Rate:");
    println!(
        "     rate(cache_requests_total{{result=\"hit\"}}[5m]) / rate(cache_requests_total[5m])"
    );
    println!("   P95 Latency:");
    println!(
        "     histogram_quantile(0.95, rate(circuitbreaker_call_duration_seconds_bucket[5m]))"
    );
}

/// Demo: Circuit breaker with named instance
async fn demo_circuit_breaker() {
    println!("🔌 Circuit Breaker Demo");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    // Simulate flaky service that fails first 3 calls
    let base_service = tower::service_fn(move |_req: ()| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;

            if count < 3 {
                Err(AppError("simulated failure".into()))
            } else {
                Ok::<_, AppError>(format!("Success {}", count))
            }
        }
    });

    let service = CircuitBreakerLayer::builder()
        .name("user-service") // ← Named instance for metrics
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .build()
        .layer(base_service);

    let mut service = service;

    // Make calls to generate metrics
    for i in 0..5 {
        match service.ready().await.unwrap().call(()).await {
            Ok(resp) => println!("   ✓ Call {}: {}", i + 1, resp),
            Err(e) => println!("   ✗ Call {}: {}", i + 1, e),
        }
    }
}

/// Demo: Retry with named instance
async fn demo_retry() {
    println!("\n🔄 Retry Demo");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    // Simulate service that succeeds on 3rd attempt
    let base_service = tower::service_fn(move |_req: ()| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;

            if count < 2 {
                Err(AppError("retry me".into()))
            } else {
                Ok::<_, AppError>(format!("Success after {} attempts", count + 1))
            }
        }
    });

    let service = RetryLayer::<AppError>::builder()
        .name("api-retry") // ← Named instance for metrics
        .max_attempts(5)
        .build()
        .layer(base_service);

    let mut service = service;

    match service.ready().await.unwrap().call(()).await {
        Ok(resp) => println!("   ✓ {}", resp),
        Err(e) => println!("   ✗ Failed: {}", e),
    }
}

/// Demo: Time limiter with named instance
async fn demo_time_limiter() {
    println!("\n⏱️  Time Limiter Demo");

    let base_service = tower::service_fn(move |req: u64| async move {
        tokio::time::sleep(Duration::from_millis(req)).await;
        Ok::<_, AppError>(format!("Completed after {}ms", req))
    });

    let service = TimeLimiterLayer::builder()
        .name("api-timeout") // ← Named instance for metrics
        .timeout_duration(Duration::from_millis(50))
        .build()
        .layer(base_service);

    let mut service = service;

    // Fast call - should succeed
    match service.ready().await.unwrap().call(20).await {
        Ok(resp) => println!("   ✓ Fast call: {}", resp),
        Err(e) => println!("   ✗ Fast call: {}", e),
    }

    // Slow call - should timeout
    match service.ready().await.unwrap().call(100).await {
        Ok(resp) => println!("   ✓ Slow call: {}", resp),
        Err(e) => println!("   ✗ Slow call: {}", e),
    }
}

/// Demo: Bulkhead with named instance
async fn demo_bulkhead() {
    println!("\n🚧 Bulkhead Demo");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let base_service = tower::service_fn(move |_req: ()| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, AppError>(format!("Processed {}", count))
        }
    });

    let service = BulkheadLayer::builder()
        .name("payment-bulkhead") // ← Named instance for metrics
        .max_concurrent_calls(2)
        .max_wait_duration(Some(Duration::from_millis(100)))
        .build()
        .layer(base_service);

    // Make concurrent calls
    let mut handles = vec![];
    for i in 0..4 {
        let mut svc = service.clone();
        let handle = tokio::spawn(async move {
            match svc.ready().await.unwrap().call(()).await {
                Ok(resp) => println!("   ✓ Call {}: {}", i + 1, resp),
                Err(e) => println!("   ✗ Call {}: {}", i + 1, e),
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

/// Demo: Rate limiter with named instance
async fn demo_rate_limiter() {
    println!("\n⚡ Rate Limiter Demo");

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let base_service = tower::service_fn(move |_req: ()| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, AppError>(format!("Request {}", count))
        }
    });

    let service = RateLimiterLayer::builder()
        .name("payment-ratelimit") // ← Named instance for metrics
        .limit_for_period(3)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_millis(50))
        .build()
        .layer(base_service);

    let mut service = service;

    // Make calls - some will be rate limited
    for i in 0..5 {
        match service.ready().await.unwrap().call(()).await {
            Ok(resp) => println!("   ✓ Call {}: {}", i + 1, resp),
            Err(e) => println!("   ✗ Call {}: {}", i + 1, e),
        }
    }
}

/// Demo: Cache with named instance
async fn demo_cache() {
    println!("\n📦 Cache Demo");

    #[derive(Clone)]
    struct Request {
        id: u64,
    }

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let base_service = tower::service_fn(move |req: Request| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<_, AppError>(format!("Data for id {}", req.id))
        }
    });

    let service = CacheLayer::builder()
        .name("data-cache") // ← Named instance for metrics
        .max_size(10)
        .ttl(Duration::from_secs(60))
        .key_extractor(|req: &Request| req.id)
        .build()
        .layer(base_service);

    let mut service = service;

    // Make calls - some should hit cache
    println!("   Making requests (repeated IDs will hit cache):");
    for id in &[1, 2, 1, 3, 2, 1] {
        match service
            .ready()
            .await
            .unwrap()
            .call(Request { id: *id })
            .await
        {
            Ok(resp) => println!("   ✓ Request id={}: {}", id, resp),
            Err(e) => println!("   ✗ Request id={}: {}", id, e),
        }
    }

    println!(
        "   📊 Backend called {} times (expected 3: ids 1, 2, 3)",
        call_count.load(Ordering::SeqCst)
    );
}
