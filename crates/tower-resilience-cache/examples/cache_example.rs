use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_cache::CacheConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Tower Cache Example");
    println!("===================\n");

    // Track how many times the inner service is called
    let call_count = Arc::new(AtomicUsize::new(0));

    // Create a simple service that counts calls
    let cc = Arc::clone(&call_count);
    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst) + 1;
            println!("  Inner service called (call #{})", count);
            tokio::time::sleep(Duration::from_millis(100)).await; // Simulate work
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    // Configure cache with event listeners
    let cache_config = CacheConfig::builder()
        .max_size(3)
        .ttl(Duration::from_secs(2))
        .name("example-cache")
        .key_extractor(|req: &String| req.clone())
        .on_hit(|| println!("  [EVENT] Cache HIT"))
        .on_miss(|| println!("  [EVENT] Cache MISS"))
        .on_eviction(|| println!("  [EVENT] Cache EVICTION"))
        .build();

    let cache_layer = cache_config.layer();
    let mut service = cache_layer.layer(service);

    // Test 1: Cache miss
    println!("Test 1: First call (cache miss)");
    let response = service.ready().await?.call("request1".to_string()).await?;
    println!("  Got: {}\n", response);

    // Test 2: Cache hit
    println!("Test 2: Same request (cache hit)");
    let response = service.ready().await?.call("request1".to_string()).await?;
    println!("  Got: {}\n", response);

    // Test 3: Different request
    println!("Test 3: Different request (cache miss)");
    let response = service.ready().await?.call("request2".to_string()).await?;
    println!("  Got: {}\n", response);

    // Test 4: Fill cache
    println!("Test 4: Fill cache to capacity");
    service.ready().await?.call("request3".to_string()).await?;
    service.ready().await?.call("request4".to_string()).await?; // Should evict request1
    println!();

    // Test 5: Verify eviction
    println!("Test 5: Request1 should be evicted (cache miss)");
    let response = service.ready().await?.call("request1".to_string()).await?;
    println!("  Got: {}\n", response);

    // Test 6: TTL expiration
    println!("Test 6: Wait for TTL expiration");
    println!("  Waiting 3 seconds...");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let response = service.ready().await?.call("request2".to_string()).await?;
    println!("  Got: {} (should be cache miss due to TTL)\n", response);

    println!(
        "Summary: Inner service called {} times total",
        call_count.load(Ordering::SeqCst)
    );

    Ok(())
}
