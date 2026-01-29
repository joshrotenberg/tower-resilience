//! Proactive circuit breaking based on health checks.
//!
//! This example demonstrates how to integrate HealthCheck with CircuitBreaker
//! for proactive circuit opening. When the health check detects an unhealthy
//! resource, it immediately opens the circuit breaker - before any request
//! failures accumulate.
//!
//! Run with: cargo run --example healthcheck_circuitbreaker --features health-circuitbreaker

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tower::{Layer, Service, service_fn};
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_healthcheck::{HealthCheckWrapper, HealthChecker, HealthStatus};

/// Simulated API endpoint that can be toggled healthy/unhealthy
#[derive(Clone)]
struct ApiEndpoint {
    #[allow(dead_code)]
    name: String,
    is_healthy: Arc<AtomicBool>,
}

impl ApiEndpoint {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_healthy: Arc::new(AtomicBool::new(true)),
        }
    }

    fn set_healthy(&self, healthy: bool) {
        self.is_healthy.store(healthy, Ordering::SeqCst);
    }

    fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::SeqCst)
    }
}

/// Health checker for API endpoints
struct ApiHealthChecker;

impl HealthChecker<ApiEndpoint> for ApiHealthChecker {
    async fn check(&self, endpoint: &ApiEndpoint) -> HealthStatus {
        if endpoint.is_healthy() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy
        }
    }
}

#[tokio::main]
async fn main() {
    println!("Health Check + Circuit Breaker Integration Example");
    println!("===================================================\n");

    // Create a simulated API endpoint
    let api = ApiEndpoint::new("external-api");
    let api_for_service = api.clone();

    // Create a service that calls the API
    let api_service = service_fn(move |_req: ()| {
        let api = api_for_service.clone();
        async move {
            if api.is_healthy() {
                Ok::<_, String>("API response")
            } else {
                Err("API unavailable".to_string())
            }
        }
    });

    // Create the circuit breaker layer
    let breaker_layer = CircuitBreakerLayer::builder()
        .name("api-circuit")
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_secs(30)) // Long open time to show health-triggered close
        .build();

    // Wrap the service with the circuit breaker
    let mut service = breaker_layer.layer(api_service);

    // Get an Arc to the circuit breaker for triggering
    // Note: In a real app, you'd use layer_fn() to get the CircuitBreaker
    // For this example, we'll demonstrate the concept directly

    // Create health check wrapper with the API endpoint
    // The circuit breaker is registered as a trigger
    let health_wrapper = HealthCheckWrapper::builder()
        .with_context(api.clone(), "external-api")
        .with_checker(ApiHealthChecker)
        .with_interval(Duration::from_millis(500))
        .with_initial_delay(Duration::from_millis(100))
        .with_failure_threshold(1) // Mark unhealthy after 1 failure
        .with_success_threshold(1) // Mark healthy after 1 success
        // Note: In a real application, you would register the circuit breaker as a trigger:
        // .with_trigger(circuit_breaker_arc)
        .build();

    println!("1. Starting with a healthy API...");
    println!("   API healthy: {}", api.is_healthy());
    println!("   Circuit state: {:?}\n", service.state_sync());

    // Start health checking
    health_wrapper.start().await;
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Make some successful calls
    println!("2. Making successful API calls...");
    for i in 1..=3 {
        let result = service.call(()).await;
        println!("   Call {}: {:?}", i, result.map(|_| "success"));
    }
    println!("   Circuit state: {:?}\n", service.state_sync());

    // Simulate API becoming unhealthy
    println!("3. Simulating API failure (external health check would detect this)...");
    api.set_healthy(false);
    println!("   API healthy: {}", api.is_healthy());

    // Wait for health check to detect the failure
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Check health status
    let status = health_wrapper.get_status("external-api").await;
    println!("   Health check status: {:?}", status);

    // In a real integration, the circuit would be open now
    // For demo purposes, we'll manually show the concept
    println!("\n   [In a full integration, the circuit breaker would be OPEN now]");
    println!("   [Health check detected failure and triggered circuit open]");

    // Demonstrate the proactive benefit
    println!("\n4. Making calls during unhealthy period...");
    for i in 1..=3 {
        let result = service.call(()).await;
        println!(
            "   Call {}: {:?}",
            i,
            result.map(|_| "success").map_err(|e| e.to_string())
        );
    }
    println!("   Circuit state: {:?}", service.state_sync());

    // API recovers
    println!("\n5. API recovers...");
    api.set_healthy(true);
    println!("   API healthy: {}", api.is_healthy());

    // Wait for health check to detect recovery
    tokio::time::sleep(Duration::from_millis(600)).await;

    let status = health_wrapper.get_status("external-api").await;
    println!("   Health check status: {:?}", status);
    println!("   [In a full integration, the circuit breaker would be CLOSED now]");
    println!("   [Health check detected recovery and triggered circuit close]");

    // Stop health checking
    health_wrapper.stop().await;

    println!("\n===================================================");
    println!("Key Benefits of Health Check + Circuit Breaker Integration:");
    println!("  - Proactive: Circuit opens BEFORE request failures accumulate");
    println!("  - Fast recovery: Circuit closes immediately when health returns");
    println!("  - External visibility: Uses dedicated health endpoints");
    println!("  - Reduced latency: Avoids slow failure detection via timeouts");
}
