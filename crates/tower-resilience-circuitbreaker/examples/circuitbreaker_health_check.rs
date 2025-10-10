//! Health check example demonstrating sync state inspection API.
//!
//! This example shows how to use the circuit breaker's sync state inspection
//! methods (`state_sync()`, `is_open()`, `metrics()`) for building health check
//! endpoints, monitoring dashboards, and observability systems.
//!
//! Run with:
//! ```bash
//! cargo run --example circuitbreaker_health_check --features tracing
//! ```

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tower::Service;
use tower_resilience_circuitbreaker::{CircuitBreakerLayer, CircuitMetrics, CircuitState};

// Simulate a backend service that can fail
async fn backend_service(req: String) -> Result<String, String> {
    // Simulate some failures
    if req.contains("fail") {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Err("Backend error".to_string())
    } else {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(format!("Processed: {}", req))
    }
}

// Health check response
#[derive(Debug)]
struct HealthStatus {
    status: &'static str,
    circuit_state: CircuitState,
    details: String,
}

impl HealthStatus {
    fn to_http_status(&self) -> u16 {
        match self.status {
            "healthy" => 200,
            "degraded" => 200,
            _ => 503,
        }
    }
}

// Synchronous health check function (no async!)
fn check_health<S, Req, Res, Err>(
    breaker: &tower_resilience_circuitbreaker::CircuitBreaker<S, Req, Res, Err>,
) -> HealthStatus
where
    S: Service<Req, Response = Res, Error = Err>,
{
    // Use sync state inspection - no await needed!
    let state = breaker.state_sync();
    let is_open = breaker.is_open();

    match state {
        CircuitState::Closed => HealthStatus {
            status: "healthy",
            circuit_state: state,
            details: "All systems operational".to_string(),
        },
        CircuitState::HalfOpen => HealthStatus {
            status: "degraded",
            circuit_state: state,
            details: "Service recovering, limited requests allowed".to_string(),
        },
        CircuitState::Open => HealthStatus {
            status: "unavailable",
            circuit_state: state,
            details: if is_open {
                "Circuit breaker is open, requests are being rejected".to_string()
            } else {
                "Unexpected state".to_string()
            },
        },
    }
}

// Async detailed metrics function
async fn get_detailed_metrics<S, Req, Res, Err>(
    breaker: &tower_resilience_circuitbreaker::CircuitBreaker<S, Req, Res, Err>,
) -> CircuitMetrics
where
    S: Service<Req, Response = Res, Error = Err>,
{
    breaker.metrics().await
}

#[tokio::main]
async fn main() {
    println!("=== Circuit Breaker Health Check Example ===\n");

    // Create circuit breaker with tight thresholds for demo
    let layer = CircuitBreakerLayer::<String, String>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(5)
        .wait_duration_in_open(Duration::from_secs(2))
        .name("backend-service")
        .build();

    let service = tower::service_fn(backend_service);
    let breaker = Arc::new(Mutex::new(layer.layer(service)));

    // Scenario 1: Healthy service
    println!("Scenario 1: Healthy Service");
    println!("----------------------------");
    for i in 1..=3 {
        let mut svc = breaker.lock().await;
        let result = svc.call(format!("request-{}", i)).await;
        println!("Request {}: {:?}", i, result);
    }

    // Health check (sync!)
    {
        let svc = breaker.lock().await;
        let health = check_health(&*svc);
        println!("\nHealth Check:");
        println!("  HTTP Status: {}", health.to_http_status());
        println!("  Status: {}", health.status);
        println!("  Circuit State: {:?}", health.circuit_state);
        println!("  Details: {}", health.details);

        // Detailed metrics (async)
        let metrics = get_detailed_metrics(&*svc).await;
        println!("\nDetailed Metrics:");
        println!("  Total Calls: {}", metrics.total_calls);
        println!(
            "  Success Rate: {:.1}%",
            (1.0 - metrics.failure_rate) * 100.0
        );
        println!("  Failure Rate: {:.1}%", metrics.failure_rate * 100.0);
        println!(
            "  Time Since State Change: {:?}",
            metrics.time_since_state_change
        );
    }

    // Scenario 2: Failing service (trip the circuit)
    println!("\n\nScenario 2: Service Failures (Circuit Opens)");
    println!("---------------------------------------------");
    for i in 1..=6 {
        let mut svc = breaker.lock().await;
        let result = svc.call(format!("fail-{}", i)).await;
        println!("Request {}: {:?}", i, result);
    }

    // Health check after failures
    {
        let svc = breaker.lock().await;
        let health = check_health(&*svc);
        println!("\nHealth Check After Failures:");
        println!("  HTTP Status: {}", health.to_http_status());
        println!("  Status: {}", health.status);
        println!("  Circuit State: {:?}", health.circuit_state);
        println!("  Details: {}", health.details);

        let metrics = get_detailed_metrics(&*svc).await;
        println!("\nDetailed Metrics:");
        println!("  Total Calls: {}", metrics.total_calls);
        println!("  Failures: {}", metrics.failure_count);
        println!("  Failure Rate: {:.1}%", metrics.failure_rate * 100.0);
    }

    // Scenario 3: Requests rejected while open
    println!("\n\nScenario 3: Requests Rejected (Circuit Open)");
    println!("---------------------------------------------");
    for i in 1..=3 {
        let mut svc = breaker.lock().await;
        let result = svc.call(format!("request-{}", i)).await;
        println!("Request {}: {:?}", i, result);
    }

    // Scenario 4: Wait for half-open transition
    println!("\n\nScenario 4: Recovery (Half-Open State)");
    println!("---------------------------------------");
    println!("Waiting 2 seconds for circuit to transition to half-open...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    {
        let svc = breaker.lock().await;
        let health = check_health(&*svc);
        println!("\nHealth Check During Recovery:");
        println!("  HTTP Status: {}", health.to_http_status());
        println!("  Status: {}", health.status);
        println!("  Circuit State: {:?}", health.circuit_state);
        println!("  Details: {}", health.details);
    }

    // Make a successful call to close the circuit
    println!("\nSending successful request to recover...");
    {
        let mut svc = breaker.lock().await;
        let result = svc.call("success".to_string()).await;
        println!("Recovery request: {:?}", result);
    }

    // Final health check
    {
        let svc = breaker.lock().await;
        let health = check_health(&*svc);
        println!("\nFinal Health Check:");
        println!("  HTTP Status: {}", health.to_http_status());
        println!("  Status: {}", health.status);
        println!("  Circuit State: {:?}", health.circuit_state);
        println!("  Details: {}", health.details);
    }

    // Example: Monitoring dashboard data
    println!("\n\nExample: Dashboard Metrics JSON");
    println!("--------------------------------");
    {
        let svc = breaker.lock().await;
        let metrics = get_detailed_metrics(&*svc).await;

        // Simulate JSON response for a monitoring dashboard
        println!(
            r#"{{
  "service": "backend-service",
  "circuit_breaker": {{
    "state": "{:?}",
    "is_open": {},
    "metrics": {{
      "total_calls": {},
      "success_count": {},
      "failure_count": {},
      "failure_rate": {:.2},
      "slow_call_count": {},
      "slow_call_rate": {:.2},
      "uptime_seconds": {}
    }}
  }}
}}"#,
            metrics.state,
            svc.is_open(),
            metrics.total_calls,
            metrics.success_count,
            metrics.failure_count,
            metrics.failure_rate,
            metrics.slow_call_count,
            metrics.slow_call_rate,
            metrics.time_since_state_change.as_secs()
        );
    }

    println!("\n=== Example Complete ===");
}
