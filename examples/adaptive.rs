//! Adaptive Concurrency Limiter Example
//!
//! This example demonstrates how to use the adaptive concurrency limiter
//! with both AIMD and Vegas algorithms.
//!
//! Run with: cargo run --example adaptive

use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd, Vegas};

#[derive(Clone)]
struct SimulatedBackend {
    /// Simulate variable latency based on load
    base_latency_ms: u64,
}

impl SimulatedBackend {
    fn new(base_latency_ms: u64) -> Self {
        Self { base_latency_ms }
    }
}

impl Service<String> for SimulatedBackend {
    type Response = String;
    type Error = std::io::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: String) -> Self::Future {
        let base_latency = self.base_latency_ms;

        Box::pin(async move {
            // Simulate variable latency
            let latency = base_latency + (rand::random::<u64>() % 20);
            tokio::time::sleep(Duration::from_millis(latency)).await;

            Ok(format!("Processed: {}", req))
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Adaptive Concurrency Limiter Example ===\n");

    // Example 1: AIMD Algorithm
    println!("--- AIMD Algorithm ---");
    println!("AIMD uses additive increase (on success) and multiplicative decrease (on failure)");
    println!("This creates a 'sawtooth' pattern as it probes for optimal concurrency.\n");

    let aimd_layer = AdaptiveLimiterLayer::new(
        Aimd::builder()
            .initial_limit(5)
            .min_limit(1)
            .max_limit(50)
            .increase_by(1)
            .decrease_factor(0.5)
            .latency_threshold(Duration::from_millis(50))
            .build(),
    );

    let mut aimd_service = ServiceBuilder::new()
        .layer(aimd_layer)
        .service(SimulatedBackend::new(10));

    println!("Making requests with AIMD limiter...");

    // Make some requests
    for i in 0..10 {
        match aimd_service
            .ready()
            .await?
            .call(format!("request-{}", i))
            .await
        {
            Ok(response) => {
                println!("  [{}] Success: {}", i, response);
            }
            Err(e) => {
                println!("  [{}] Error: {}", i, e);
            }
        }
    }

    println!();

    // Example 2: Vegas Algorithm
    println!("--- Vegas Algorithm ---");
    println!("Vegas uses RTT measurements to estimate queue depth.");
    println!("It's more stable than AIMD and avoids the sawtooth pattern.\n");

    let vegas_layer = AdaptiveLimiterLayer::new(
        Vegas::builder()
            .initial_limit(5)
            .min_limit(1)
            .max_limit(50)
            .alpha(2) // Increase when queue < 2
            .beta(4) // Decrease when queue > 4
            .build(),
    );

    let mut vegas_service = ServiceBuilder::new()
        .layer(vegas_layer)
        .service(SimulatedBackend::new(10));

    println!("Making requests with Vegas limiter...");

    // Make some requests
    for i in 0..10 {
        match vegas_service
            .ready()
            .await?
            .call(format!("request-{}", i))
            .await
        {
            Ok(response) => {
                println!("  [{}] Success: {}", i, response);
            }
            Err(e) => {
                println!("  [{}] Error: {}", i, e);
            }
        }
    }

    println!();

    // Example 3: Concurrent requests
    println!("--- Concurrent Requests ---");
    println!("The limiter automatically queues requests when at capacity.\n");

    let concurrent_layer = AdaptiveLimiterLayer::new(
        Aimd::builder()
            .initial_limit(3) // Start with low limit
            .min_limit(1)
            .max_limit(20)
            .latency_threshold(Duration::from_millis(100))
            .build(),
    );

    let service = ServiceBuilder::new()
        .layer(concurrent_layer)
        .service(SimulatedBackend::new(20));

    // Spawn concurrent requests
    let mut handles = vec![];
    for i in 0..10 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            let start = std::time::Instant::now();
            let result = svc
                .ready()
                .await
                .unwrap()
                .call(format!("concurrent-{}", i))
                .await;
            (i, result, start.elapsed())
        }));
    }

    // Collect results
    for handle in handles {
        let (i, result, elapsed) = handle.await?;
        match result {
            Ok(response) => {
                println!("  [{}] {:?}: {}", i, elapsed, response);
            }
            Err(e) => {
                println!("  [{}] {:?}: Error - {}", i, elapsed, e);
            }
        }
    }

    println!("\n=== Example Complete ===");
    println!("\nKey takeaways:");
    println!("- AIMD: Simple, aggressive probing, good for most use cases");
    println!("- Vegas: Smoother, RTT-based, better for latency-sensitive apps");
    println!("- Both automatically find optimal concurrency without manual tuning");

    Ok(())
}
