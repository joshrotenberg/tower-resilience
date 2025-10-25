//! Basic example demonstrating health checking with mock services.
//!
//! Run with: cargo run --example basic

use std::time::Duration;
use tower_resilience_healthcheck::{HealthCheckWrapper, HealthStatus, SelectionStrategy};

#[derive(Clone)]
struct MockService {
    name: String,
    is_healthy: bool,
}

impl MockService {
    fn new(name: impl Into<String>, is_healthy: bool) -> Self {
        Self {
            name: name.into(),
            is_healthy,
        }
    }
}

struct ServiceHealthChecker;

impl tower_resilience_healthcheck::HealthChecker<MockService> for ServiceHealthChecker {
    async fn check(&self, service: &MockService) -> HealthStatus {
        println!("Checking health of: {}", service.name);
        if service.is_healthy {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Health Check Basic Example ===\n");

    // Create wrapper with multiple services
    let wrapper = HealthCheckWrapper::builder()
        .with_context(MockService::new("primary", true), "primary")
        .with_context(MockService::new("secondary", true), "secondary")
        .with_context(MockService::new("tertiary", false), "tertiary")
        .with_checker(ServiceHealthChecker)
        .with_interval(Duration::from_secs(2))
        .with_initial_delay(Duration::from_millis(100))
        .with_selection_strategy(SelectionStrategy::RoundRobin)
        .build();

    println!("Starting health checks...\n");
    wrapper.start().await;

    // Wait for initial health checks
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Get health status of all services
    println!("Health status of all services:");
    let statuses = wrapper.get_all_statuses().await;
    for (name, status) in &statuses {
        println!("  {}: {:?}", name, status);
    }
    println!();

    // Get detailed health information
    println!("Detailed health information:");
    let details = wrapper.get_health_details().await;
    for detail in &details {
        println!("  {}:", detail.name);
        println!("    Status: {:?}", detail.status);
        println!("    Consecutive failures: {}", detail.consecutive_failures);
        println!(
            "    Consecutive successes: {}",
            detail.consecutive_successes
        );
    }
    println!();

    // Demonstrate getting healthy services
    println!("Getting healthy services using RoundRobin:");
    for i in 1..=5 {
        if let Some(service) = wrapper.get_healthy().await {
            println!("  Request {}: Using {}", i, service.name);
        } else {
            println!("  Request {}: No healthy service available", i);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!();

    // Get a specific service status
    println!("Checking specific service status:");
    if let Some(status) = wrapper.get_status("tertiary").await {
        println!("  tertiary: {:?}", status);
    }

    // Wait a bit longer to see more health checks
    println!("\nWaiting for more health checks...");
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Stop health checking
    println!("\nStopping health checks...");
    wrapper.stop().await;

    println!("Done!");
    Ok(())
}
