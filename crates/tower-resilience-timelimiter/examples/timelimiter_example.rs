//! Simple example for tower-timelimiter
//! Run with: cargo run --example simple -p tower-timelimiter

use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_timelimiter::TimeLimiterConfig;

#[tokio::main]
async fn main() {
    // Create a service that sometimes takes too long
    let fast_service = tower::service_fn(|req: &'static str| async move {
        println!("Processing request: {}", req);
        sleep(Duration::from_millis(50)).await;
        Ok::<_, std::io::Error>(format!("Completed: {}", req))
    });

    let slow_service = tower::service_fn(|req: &'static str| async move {
        println!("Processing slow request: {}", req);
        sleep(Duration::from_secs(2)).await;
        Ok::<_, std::io::Error>(format!("Completed: {}", req))
    });

    // Wrap with time limiter - build the layer once
    let timelimiter_layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(100))
        .cancel_running_future(true)
        .name("example-timelimiter")
        .on_success(|duration| {
            println!("✓ Request succeeded in {:?}", duration);
        })
        .on_timeout(|| {
            println!("✗ Request timed out!");
        })
        .on_error(|duration| {
            println!("✗ Request failed after {:?}", duration);
        })
        .build();

    println!("=== Testing fast service (should succeed) ===");
    let mut service = timelimiter_layer.layer(fast_service);
    match service.ready().await.unwrap().call("fast").await {
        Ok(response) => println!("Response: {}", response),
        Err(e) => println!("Error: {}", e),
    }

    println!("\n=== Testing slow service (should timeout) ===");
    let mut service = timelimiter_layer.layer(slow_service);
    match service.ready().await.unwrap().call("slow").await {
        Ok(response) => println!("Response: {}", response),
        Err(e) => println!("Error: {}", e),
    }
}
