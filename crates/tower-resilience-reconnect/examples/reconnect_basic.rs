//! Basic reconnection example with exponential backoff.
//!
//! Run with: cargo run --example reconnect_basic -p tower-resilience-reconnect
//!
//! This example shows how to wrap a service with automatic reconnection
//! capabilities using the ReconnectLayer.

use std::time::Duration;
use tower::{Service, ServiceBuilder};
use tower_resilience_reconnect::{ReconnectConfig, ReconnectLayer, ReconnectPolicy};

/// Example demonstrating basic reconnection with exponential backoff.
///
/// This example shows how to wrap a service with automatic reconnection
/// capabilities using the ReconnectLayer.

#[derive(Clone)]
struct ExampleService;

impl Service<String> for ExampleService {
    type Response = String;
    type Error = std::io::Error;
    type Future = futures::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: String) -> Self::Future {
        println!("Processing request: {}", req);
        futures::future::ok(format!("Response: {}", req))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Reconnect Layer - Basic Example\n");

    // Create reconnection configuration with exponential backoff
    let config = ReconnectConfig::builder()
        .policy(ReconnectPolicy::exponential(
            Duration::from_millis(100), // Start at 100ms
            Duration::from_secs(5),     // Max 5 seconds
        ))
        .max_attempts(3) // Try up to 3 times
        .retry_on_reconnect(true) // Retry the command after reconnecting
        .build();

    println!("Configuration:");
    println!("  Policy: Exponential backoff (100ms -> 5s)");
    println!("  Max attempts: 3");
    println!("  Retry on reconnect: true\n");

    // Create the service with reconnection layer
    let service = ExampleService;
    let layer = ReconnectLayer::new(config);
    let mut reconnect_service = ServiceBuilder::new().layer(layer).service(service);

    println!("Service created with reconnection capabilities!");

    // Make a test call
    let response = reconnect_service.call("test".to_string()).await?;
    println!("Received: {}", response);

    println!("\nIn a real application, connection failures would trigger automatic");
    println!("reconnection attempts with exponential backoff delays.");

    Ok(())
}
