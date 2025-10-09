//! Rate limiter example
//!
//! This example demonstrates request rate limiting.
//! Run with: cargo run --example ratelimiter

use std::time::Duration;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_ratelimiter::RateLimiterConfig;

#[tokio::main]
async fn main() {
    println!("=== Rate Limiter Example ===\n");

    // Configure rate limiter: 3 requests per second
    let config = RateLimiterConfig::builder()
        .limit_for_period(3)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_millis(500))
        .on_permit_acquired(|_wait_duration| {
            println!("  Request permitted");
        })
        .on_permit_rejected(|_wait_duration| {
            println!("  Request rejected (rate limit exceeded)");
        })
        .build();

    let layer = config;

    // Simple echo service
    let svc = service_fn(|req: String| async move { Ok::<_, ()>(format!("Echo: {}", req)) });

    let mut service = layer.layer(svc);

    // Make 5 rapid requests (only 3 should succeed immediately)
    println!("Making 5 rapid requests (limit: 3/sec)...\n");

    for i in 1..=5 {
        print!("Request {}: ", i);
        match service
            .ready()
            .await
            .unwrap()
            .call(format!("msg-{}", i))
            .await
        {
            Ok(response) => println!("{}", response),
            Err(_) => println!("Rate limited"),
        }
    }

    println!("\nWaiting 1 second for rate limit to refresh...\n");
    tokio::time::sleep(Duration::from_secs(1)).await;

    // After refresh, we can make more requests
    println!("After refresh:");
    for i in 6..=8 {
        print!("Request {}: ", i);
        match service
            .ready()
            .await
            .unwrap()
            .call(format!("msg-{}", i))
            .await
        {
            Ok(response) => println!("{}", response),
            Err(_) => println!("Rate limited"),
        }
    }

    println!("\nExample complete!");
}
