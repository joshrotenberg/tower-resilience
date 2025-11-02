//! Health check example with proactive monitoring
//!
//! Run with: cargo run --example healthcheck

use std::time::Duration;
use tower_resilience_healthcheck::{
    HealthCheckWrapper, HealthChecker, HealthStatus, SelectionStrategy,
};

#[derive(Clone)]
struct Database {
    name: String,
    responsive: bool,
}

impl Database {
    fn new(name: impl Into<String>, responsive: bool) -> Self {
        Self {
            name: name.into(),
            responsive,
        }
    }

    async fn ping(&self) -> Result<(), String> {
        if self.responsive {
            Ok(())
        } else {
            Err(format!("Database {} not responding", self.name))
        }
    }
}

/// Health checker for database connections
struct DbHealthChecker;

impl HealthChecker<Database> for DbHealthChecker {
    async fn check(&self, db: &Database) -> HealthStatus {
        match db.ping().await {
            Ok(_) => HealthStatus::Healthy,
            Err(_) => HealthStatus::Unhealthy,
        }
    }
}

#[tokio::main]
async fn main() {
    println!("Health Check Example");
    println!("====================\n");

    // Create wrapper with multiple database instances
    let wrapper = HealthCheckWrapper::builder()
        .with_context(Database::new("primary-db", true), "primary")
        .with_context(Database::new("secondary-db", true), "secondary")
        .with_context(Database::new("failing-db", false), "failing")
        .with_checker(DbHealthChecker)
        .with_interval(Duration::from_secs(2))
        .with_selection_strategy(SelectionStrategy::RoundRobin)
        .build();

    println!("Starting background health checks...\n");
    wrapper.start().await;

    // Wait for initial health checks to complete
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Demonstrate getting healthy resources
    println!("Making 5 requests with automatic failover:\n");
    for i in 1..=5 {
        if let Some(db) = wrapper.get_healthy().await {
            println!("  Request {}: Using {}", i, db.name);
        } else {
            println!("  Request {}: No healthy database available!", i);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    println!("\n--- Health Status ---");
    let details = wrapper.get_health_details().await;
    for detail in &details {
        println!(
            "  {}: {:?} (failures: {}, successes: {})",
            detail.name, detail.status, detail.consecutive_failures, detail.consecutive_successes
        );
    }

    println!("\nKey Benefits:");
    println!("  - Proactive health monitoring (vs reactive circuit breaker)");
    println!("  - Automatic failover to healthy resources");
    println!("  - Intelligent selection strategies (RoundRobin, Random, LeastLoaded)");
    println!("  - Complements circuit breakers perfectly!");

    wrapper.stop().await;
}
