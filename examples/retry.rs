//! Retry example with exponential backoff
//!
//! This example demonstrates retry logic with exponential backoff.
//! Run with: cargo run --example retry

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_retry::RetryLayer;

#[derive(Debug, Clone)]
struct MyError;

#[tokio::main]
async fn main() {
    // Counter to track attempts
    let attempt_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&attempt_count);

    // Service that fails first 2 times, then succeeds
    let svc = service_fn(move |req: String| {
        let counter = Arc::clone(&counter);
        async move {
            let attempt = counter.fetch_add(1, Ordering::SeqCst) + 1;
            println!("Attempt {}: Processing request '{}'", attempt, req);

            if attempt < 3 {
                println!("  -> Failed (simulated transient error)");
                Err(MyError)
            } else {
                println!("  -> Success!");
                Ok(format!("Response: {}", req))
            }
        }
    });

    // Configure retry with exponential backoff
    let layer = RetryLayer::builder()
        .max_attempts(5)
        .exponential_backoff(Duration::from_millis(100))
        .on_retry(|attempt, delay| {
            println!(
                "  Retrying... (attempt {}, waiting {:?})",
                attempt + 1,
                delay
            );
        })
        .on_success(|attempts| {
            println!("Success after {} total attempts!", attempts);
        })
        .build();

    let mut service = layer.layer(svc);

    // Make a request - will retry automatically
    println!("\nMaking request...");
    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    match result {
        Ok(response) => println!("\nFinal result: {}", response),
        Err(_) => println!("\nFailed after all retries"),
    }

    println!(
        "\nTotal attempts made: {}",
        attempt_count.load(Ordering::SeqCst)
    );
}
