//! Cache example with TTL
//!
//! This example demonstrates response caching with time-to-live.
//! Run with: cargo run --example cache

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_cache::CacheLayer;

#[tokio::main]
async fn main() {
    println!("=== Cache Example ===\n");

    // Counter to track actual service calls
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    // Expensive service that we want to cache
    let svc = service_fn(move |req: String| {
        let counter = Arc::clone(&counter);
        async move {
            let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
            println!("  Cache miss - calling expensive service (call #{})", count);
            tokio::time::sleep(Duration::from_millis(100)).await; // Simulate work
            Ok::<_, ()>(format!("Result for '{}'", req))
        }
    });

    // Configure cache with 2 second TTL
    let config = CacheLayer::builder()
        .max_size(100)
        .ttl(Duration::from_secs(2))
        .key_extractor(|req: &String| req.clone())
        .on_hit(|| {
            println!("  Cache hit - returning cached response");
        })
        .build();

    let layer = config;
    let mut service = layer.layer(svc);

    // First call - cache miss
    println!("Request 1 for 'key-a':");
    let result = service
        .ready()
        .await
        .unwrap()
        .call("key-a".to_string())
        .await;
    println!("  Response: {:?}\n", result);

    // Second call with same key - cache hit
    println!("Request 2 for 'key-a' (should hit cache):");
    let result = service
        .ready()
        .await
        .unwrap()
        .call("key-a".to_string())
        .await;
    println!("  Response: {:?}\n", result);

    // Different key - cache miss
    println!("Request 3 for 'key-b':");
    let result = service
        .ready()
        .await
        .unwrap()
        .call("key-b".to_string())
        .await;
    println!("  Response: {:?}\n", result);

    // Original key again - still cached
    println!("Request 4 for 'key-a' (should still be cached):");
    let result = service
        .ready()
        .await
        .unwrap()
        .call("key-a".to_string())
        .await;
    println!("  Response: {:?}\n", result);

    // Wait for TTL to expire
    println!("Waiting for cache TTL to expire (2 seconds)...\n");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // After TTL expires - cache miss again
    println!("Request 5 for 'key-a' (after TTL expiry):");
    let result = service
        .ready()
        .await
        .unwrap()
        .call("key-a".to_string())
        .await;
    println!("  Response: {:?}\n", result);

    println!(
        "Total expensive service calls: {} (out of 5 requests)",
        call_count.load(Ordering::SeqCst)
    );
    println!("Example complete!");
}
