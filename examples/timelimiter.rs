//! Time limiter example with timeout handling
//!
//! This example demonstrates timeout handling with configurable behavior.
//! Run with: cargo run --example timelimiter

use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_timelimiter::TimeLimiterConfig;

#[derive(Debug)]
struct TimeoutError;

#[tokio::main]
async fn main() {
    println!("=== Time Limiter Example ===\n");

    // Configure time limiter with 1 second timeout
    let config = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_secs(1))
        .cancel_running_future(true)
        .on_timeout(|| {
            println!("  Timeout occurred!");
        })
        .build();

    let layer = config;

    // Test 1: Fast operation (completes in time)
    println!("Test 1: Fast operation (500ms)");
    let fast_svc = service_fn(|req: String| async move {
        sleep(Duration::from_millis(500)).await;
        Ok::<_, TimeoutError>(format!("Fast response: {}", req))
    });

    let mut service = layer.clone().layer(fast_svc);
    match service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
    {
        Ok(response) => println!("  Success: {}\n", response),
        Err(_) => println!("  Operation timed out\n"),
    }

    // Test 2: Slow operation (times out)
    println!("Test 2: Slow operation (2s, will timeout)");
    let slow_svc = service_fn(|req: String| async move {
        println!("  Starting slow operation...");
        sleep(Duration::from_secs(2)).await;
        Ok::<_, TimeoutError>(format!("Slow response: {}", req))
    });

    let mut service = layer.layer(slow_svc);
    match service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
    {
        Ok(response) => println!("  Success: {}\n", response),
        Err(e) => println!("  Error: {:?}\n", e),
    }

    println!("Example complete!");
}
