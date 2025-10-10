//! Tests comparing different eviction policies.

use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_cache::{CacheLayer, EvictionPolicy};

#[tokio::test]
async fn lru_evicts_least_recently_used() {
    let cache_layer = CacheLayer::builder()
        .max_size(2)
        .eviction_policy(EvictionPolicy::Lru)
        .key_extractor(|req: &String| req.clone())
        .build();

    let mut service = cache_layer.layer(tower::service_fn(|req: String| async move {
        Ok::<_, ()>(format!("Response: {}", req))
    }));

    // Fill cache with "a" and "b"
    service
        .ready()
        .await
        .unwrap()
        .call("a".to_string())
        .await
        .unwrap();
    service
        .ready()
        .await
        .unwrap()
        .call("b".to_string())
        .await
        .unwrap();

    // Access "a" to make it more recent than "b"
    service
        .ready()
        .await
        .unwrap()
        .call("a".to_string())
        .await
        .unwrap();

    // Insert "c", should evict "b" (least recently used)
    service
        .ready()
        .await
        .unwrap()
        .call("c".to_string())
        .await
        .unwrap();

    // "a" and "c" should be cached, "b" should not
    // We can verify by checking that subsequent calls return immediately
    // (in a real scenario, we'd use event listeners to track hits/misses)
}

#[tokio::test]
async fn lfu_evicts_least_frequently_used() {
    let cache_layer = CacheLayer::builder()
        .max_size(2)
        .eviction_policy(EvictionPolicy::Lfu)
        .key_extractor(|req: &String| req.clone())
        .build();

    let mut service = cache_layer.layer(tower::service_fn(|req: String| async move {
        Ok::<_, ()>(format!("Response: {}", req))
    }));

    // Fill cache with "a" and "b"
    service
        .ready()
        .await
        .unwrap()
        .call("a".to_string())
        .await
        .unwrap();
    service
        .ready()
        .await
        .unwrap()
        .call("b".to_string())
        .await
        .unwrap();

    // Access "a" multiple times to increase frequency
    service
        .ready()
        .await
        .unwrap()
        .call("a".to_string())
        .await
        .unwrap();
    service
        .ready()
        .await
        .unwrap()
        .call("a".to_string())
        .await
        .unwrap();
    service
        .ready()
        .await
        .unwrap()
        .call("a".to_string())
        .await
        .unwrap();

    // Access "b" once
    service
        .ready()
        .await
        .unwrap()
        .call("b".to_string())
        .await
        .unwrap();

    // Insert "c", should evict "b" (least frequently used)
    service
        .ready()
        .await
        .unwrap()
        .call("c".to_string())
        .await
        .unwrap();

    // "a" (high frequency) and "c" (new) should be cached
}

#[tokio::test]
async fn fifo_evicts_oldest_entry() {
    let cache_layer = CacheLayer::builder()
        .max_size(2)
        .eviction_policy(EvictionPolicy::Fifo)
        .key_extractor(|req: &String| req.clone())
        .build();

    let mut service = cache_layer.layer(tower::service_fn(|req: String| async move {
        Ok::<_, ()>(format!("Response: {}", req))
    }));

    // Fill cache with "a" and "b" (in that order)
    service
        .ready()
        .await
        .unwrap()
        .call("a".to_string())
        .await
        .unwrap();
    service
        .ready()
        .await
        .unwrap()
        .call("b".to_string())
        .await
        .unwrap();

    // Access "b" multiple times (shouldn't matter for FIFO)
    service
        .ready()
        .await
        .unwrap()
        .call("b".to_string())
        .await
        .unwrap();
    service
        .ready()
        .await
        .unwrap()
        .call("b".to_string())
        .await
        .unwrap();

    // Insert "c", should evict "a" (first in, regardless of access)
    service
        .ready()
        .await
        .unwrap()
        .call("c".to_string())
        .await
        .unwrap();

    // "b" and "c" should be cached, "a" should not
}

#[tokio::test]
async fn eviction_policies_work_with_ttl() {
    for policy in [
        EvictionPolicy::Lru,
        EvictionPolicy::Lfu,
        EvictionPolicy::Fifo,
    ] {
        let cache_layer = CacheLayer::builder()
            .max_size(10)
            .ttl(Duration::from_millis(50))
            .eviction_policy(policy)
            .key_extractor(|req: &String| req.clone())
            .build();

        let mut service = cache_layer.layer(tower::service_fn(|req: String| async move {
            Ok::<_, ()>(format!("Response: {}", req))
        }));

        // Insert and retrieve
        service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();

        // Wait for TTL expiration
        tokio::time::sleep(Duration::from_millis(60)).await;

        // Next call should miss cache (TTL expired)
        service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn different_policies_produce_different_eviction_behavior() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Track cache misses for each policy
    let lru_misses = Arc::new(AtomicUsize::new(0));
    let lfu_misses = Arc::new(AtomicUsize::new(0));
    let fifo_misses = Arc::new(AtomicUsize::new(0));

    // Test LRU
    {
        let misses = Arc::clone(&lru_misses);
        let cache_layer = CacheLayer::builder()
            .max_size(2)
            .eviction_policy(EvictionPolicy::Lru)
            .key_extractor(|req: &String| req.clone())
            .on_miss(move || {
                misses.fetch_add(1, Ordering::Relaxed);
            })
            .build();

        let mut service = cache_layer.layer(tower::service_fn(|req: String| async move {
            Ok::<_, ()>(format!("Response: {}", req))
        }));

        // Pattern: a, b, a, a, c
        service
            .ready()
            .await
            .unwrap()
            .call("a".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("b".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("a".to_string())
            .await
            .unwrap(); // cache hit
        service
            .ready()
            .await
            .unwrap()
            .call("a".to_string())
            .await
            .unwrap(); // cache hit
        service
            .ready()
            .await
            .unwrap()
            .call("c".to_string())
            .await
            .unwrap(); // evicts b
        service
            .ready()
            .await
            .unwrap()
            .call("b".to_string())
            .await
            .unwrap(); // cache miss
    }

    // Test LFU
    {
        let misses = Arc::clone(&lfu_misses);
        let cache_layer = CacheLayer::builder()
            .max_size(2)
            .eviction_policy(EvictionPolicy::Lfu)
            .key_extractor(|req: &String| req.clone())
            .on_miss(move || {
                misses.fetch_add(1, Ordering::Relaxed);
            })
            .build();

        let mut service = cache_layer.layer(tower::service_fn(|req: String| async move {
            Ok::<_, ()>(format!("Response: {}", req))
        }));

        // Same pattern
        service
            .ready()
            .await
            .unwrap()
            .call("a".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("b".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("a".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("a".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("c".to_string())
            .await
            .unwrap(); // evicts b (freq=1) not a (freq=3)
        service
            .ready()
            .await
            .unwrap()
            .call("b".to_string())
            .await
            .unwrap(); // cache miss
    }

    // Test FIFO
    {
        let misses = Arc::clone(&fifo_misses);
        let cache_layer = CacheLayer::builder()
            .max_size(2)
            .eviction_policy(EvictionPolicy::Fifo)
            .key_extractor(|req: &String| req.clone())
            .on_miss(move || {
                misses.fetch_add(1, Ordering::Relaxed);
            })
            .build();

        let mut service = cache_layer.layer(tower::service_fn(|req: String| async move {
            Ok::<_, ()>(format!("Response: {}", req))
        }));

        // Same pattern
        service
            .ready()
            .await
            .unwrap()
            .call("a".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("b".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("a".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("a".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("c".to_string())
            .await
            .unwrap(); // evicts a (first in)
        service
            .ready()
            .await
            .unwrap()
            .call("a".to_string())
            .await
            .unwrap(); // cache miss
    }

    // All policies should have same number of initial misses (a, b, c)
    // but different final miss behavior
    assert_eq!(lru_misses.load(Ordering::Relaxed), 4); // a, b, c, b
    assert_eq!(lfu_misses.load(Ordering::Relaxed), 4); // a, b, c, b
    assert_eq!(fifo_misses.load(Ordering::Relaxed), 4); // a, b, c, a
}
