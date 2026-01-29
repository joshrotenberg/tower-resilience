//! Circuit breaker with fallback strategies example.
//!
//! This example demonstrates different fallback patterns for graceful degradation
//! when the circuit breaker opens.
//!
//! Run with:
//! ```sh
//! cargo run --example circuitbreaker_fallback --features circuitbreaker
//! ```

use futures::future::BoxFuture;
use std::sync::Arc;
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitState};

#[derive(Debug, Clone)]
struct ServiceError(String);

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ServiceError {}

/// Simulates a flaky service that fails predictably
struct FlakyService {
    call_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl FlakyService {
    fn new() -> Self {
        Self {
            call_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }
}

impl Service<String> for FlakyService {
    type Response = String;
    type Error = ServiceError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: String) -> Self::Future {
        let count = self
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;

            // Fail most requests to trigger circuit breaker
            if count % 10 < 8 {
                Err(ServiceError("Service unavailable".into()))
            } else {
                Ok(format!("Success: {}", req))
            }
        })
    }
}

impl Clone for FlakyService {
    fn clone(&self) -> Self {
        Self {
            call_count: Arc::clone(&self.call_count),
        }
    }
}

#[tokio::main]
async fn main() {
    println!("🔄 Circuit Breaker Fallback Examples\n");

    demo_static_fallback().await;
    demo_cached_fallback().await;
    demo_degraded_fallback().await;
}

/// Example 1: Static fallback response
async fn demo_static_fallback() {
    println!("📋 Example 1: Static Fallback Response");
    println!("   When circuit opens, return a static error message\n");

    let service = FlakyService::new();

    let circuit_breaker = CircuitBreakerLayer::builder()
        .name("static-fallback-demo")
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_secs(2))
        .build();

    let mut service = circuit_breaker.layer(service).with_fallback(
        |_req: String| -> BoxFuture<'static, Result<String, ServiceError>> {
            Box::pin(async {
                Ok("⚠️ Service temporarily unavailable. Please try again later.".to_string())
            })
        },
    );

    // Make requests to trigger circuit breaker
    for i in 0..15 {
        match service
            .ready()
            .await
            .unwrap()
            .call(format!("req-{}", i))
            .await
        {
            Ok(resp) => println!("   ✓ Request {}: {}", i, resp),
            Err(e) => println!("   ✗ Request {}: {}", i, e),
        }
    }

    println!("   Circuit state: {:?}\n", service.state().await);
}

/// Example 2: Cached fallback response
async fn demo_cached_fallback() {
    println!("📦 Example 2: Cached Fallback");
    println!("   When circuit opens, return cached data\n");

    // Simulate a cache
    let cache = Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));
    cache.write().unwrap().insert(
        "user-123".to_string(),
        "Cached User Data: John Doe".to_string(),
    );

    let service = FlakyService::new();

    let circuit_breaker = CircuitBreakerLayer::builder()
        .name("cache-fallback-demo")
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_secs(2))
        .build();

    let cache_clone = Arc::clone(&cache);
    let mut service = circuit_breaker.layer(service).with_fallback(
        move |req: String| -> BoxFuture<'static, Result<String, ServiceError>> {
            let cache = Arc::clone(&cache_clone);
            Box::pin(async move {
                // Try to get from cache
                if let Some(cached) = cache.read().unwrap().get(&req) {
                    Ok(format!("🗂️ From cache: {}", cached))
                } else {
                    Ok("⚠️ Service unavailable and no cached data".to_string())
                }
            })
        },
    );

    // Make requests
    for i in 0..15 {
        let req = if i % 3 == 0 {
            "user-123".to_string() // Cached
        } else {
            format!("user-{}", i) // Not cached
        };

        match service.ready().await.unwrap().call(req.clone()).await {
            Ok(resp) => println!("   ✓ Request {}: {}", req, resp),
            Err(e) => println!("   ✗ Request {}: {}", req, e),
        }
    }

    println!("   Circuit state: {:?}\n", service.state().await);
}

/// Example 3: Degraded functionality fallback
async fn demo_degraded_fallback() {
    println!("🔧 Example 3: Degraded Functionality");
    println!("   When circuit opens, provide limited functionality\n");

    let service = FlakyService::new();

    let circuit_breaker = CircuitBreakerLayer::builder()
        .name("degraded-fallback-demo")
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_secs(2))
        .on_state_transition(|from, to| {
            println!("   🔀 Circuit transitioned: {:?} -> {:?}", from, to);
        })
        .build();

    let mut service = circuit_breaker.layer(service).with_fallback(
        |req: String| -> BoxFuture<'static, Result<String, ServiceError>> {
            Box::pin(async move {
                // Provide basic response with degraded features
                Ok(format!(
                    "⚠️ Degraded mode: Acknowledged '{}' but processing may be delayed",
                    req
                ))
            })
        },
    );

    // Make requests
    for i in 0..15 {
        match service
            .ready()
            .await
            .unwrap()
            .call(format!("order-{}", i))
            .await
        {
            Ok(resp) => println!("   ✓ {}", resp),
            Err(e) => println!("   ✗ Error: {}", e),
        }

        // Check if circuit recovered
        if i == 10 && service.state().await == CircuitState::Open {
            println!("   ⏸️  Circuit is open, waiting for recovery...");
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    println!("   Final circuit state: {:?}\n", service.state().await);
}
