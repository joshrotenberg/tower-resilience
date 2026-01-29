//! Simple chaos engineering example
//!
//! This example demonstrates using chaos engineering to test resilience.
//! Run with: cargo run --example chaos

use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_chaos::ChaosLayer;

#[tokio::main]
async fn main() {
    println!("Chaos Engineering Example");
    println!("=========================\n");

    // Create a chaos layer that injects failures and latency
    // Types are inferred from the error_fn closure signature
    let chaos = ChaosLayer::builder()
        .name("test-chaos")
        .error_rate(0.3) // 30% of requests fail
        .error_fn(|_req: &String| std::io::Error::other("chaos error!"))
        .latency_rate(0.2) // 20% of remaining requests delayed
        .min_latency(Duration::from_millis(50))
        .max_latency(Duration::from_millis(100))
        .on_error_injected(|| {
            println!("  [CHAOS] Error injected!");
        })
        .on_latency_injected(|delay| {
            println!("  [CHAOS] Latency injected: {:?}", delay);
        })
        .build();

    // Simple echo service
    let svc = tower::service_fn(|req: String| async move {
        Ok::<String, std::io::Error>(format!("Echo: {}", req))
    });

    let mut service = chaos.layer(svc);

    // Make 10 requests and observe chaos
    println!("Making 10 requests with chaos injection:\n");
    let mut successes = 0;
    let mut failures = 0;

    for i in 1..=10 {
        let start = std::time::Instant::now();
        match service
            .ready()
            .await
            .unwrap()
            .call(format!("request-{}", i))
            .await
        {
            Ok(response) => {
                successes += 1;
                let elapsed = start.elapsed();
                println!("Request {}: {} ({:?})", i, response, elapsed);
            }
            Err(e) => {
                failures += 1;
                println!("Request {}: {}", i, e);
            }
        }
    }

    println!("\nResults:");
    println!("  Successes: {}", successes);
    println!("  Failures: {}", failures);
    println!("\nNote: Use deterministic seeding with .seed(42) for reproducible tests!");
}
