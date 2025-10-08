//! Edge case tests for tower-cache.
//!
//! Tests cache behavior in unusual or boundary conditions, including:
//! - Empty cache operations
//! - Single-item cache (LRU edge case)
//! - Large cache stress testing
//! - Zero and very long TTL values
//! - Multiple items with same value but different keys
//! - Rapid insert/evict cycles
//! - Cache full behavior

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_cache::CacheConfig;

#[tokio::test]
async fn empty_cache_behavior() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.clone())
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // First call to empty cache should miss and call inner service
    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response, "Response: test");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Second call should hit cache
    let response2 = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response2, "Response: test");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn single_item_cache_lru_works() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let eviction_count = Arc::new(AtomicUsize::new(0));
    let ec = Arc::clone(&eviction_count);

    let config = CacheConfig::builder()
        .max_size(1) // Single item cache
        .key_extractor(|req: &String| req.clone())
        .on_eviction(move || {
            ec.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // Insert first item
    service
        .ready()
        .await
        .unwrap()
        .call("key1".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
    assert_eq!(eviction_count.load(Ordering::SeqCst), 0);

    // Insert second item (should evict first)
    service
        .ready()
        .await
        .unwrap()
        .call("key2".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
    assert_eq!(eviction_count.load(Ordering::SeqCst), 1);

    // Try to access first item (should miss, as it was evicted)
    service
        .ready()
        .await
        .unwrap()
        .call("key1".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 3);

    // Second item should have been evicted by now
    assert_eq!(eviction_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn large_cache_stress_test() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: u32| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(req * 2)
        }
    });

    let config = CacheConfig::builder()
        .max_size(2000) // Large cache
        .key_extractor(|req: &u32| *req)
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // Insert 1500 unique items
    for i in 0..1500 {
        let result = service.ready().await.unwrap().call(i).await.unwrap();
        assert_eq!(result, i * 2);
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 1500);

    // Access first 100 items again (should all be cached)
    let initial_count = call_count.load(Ordering::SeqCst);
    for i in 0..100 {
        let result = service.ready().await.unwrap().call(i).await.unwrap();
        assert_eq!(result, i * 2);
    }

    // Should still be 1500 (no new calls)
    assert_eq!(call_count.load(Ordering::SeqCst), initial_count);
}

#[tokio::test]
async fn zero_ttl_instant_expiration() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let config = CacheConfig::builder()
        .max_size(10)
        .ttl(Duration::from_nanos(1)) // Effectively instant expiration
        .key_extractor(|req: &String| req.clone())
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // First call
    service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Tiny delay to ensure expiration
    tokio::time::sleep(Duration::from_micros(10)).await;

    // Second call should miss (expired)
    service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn very_long_ttl() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let config = CacheConfig::builder()
        .max_size(10)
        .ttl(Duration::from_secs(365 * 24 * 60 * 60)) // 1 year
        .key_extractor(|req: &String| req.clone())
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // First call
    service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Multiple subsequent calls should all hit cache
    for _ in 0..10 {
        service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
    }

    // Should still be 1 (all cached)
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn multiple_items_same_value_different_keys() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    // Service that returns same value for all requests
    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>("same response".to_string())
        }
    });

    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.clone())
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // Insert 5 different keys, all with same value
    for i in 0..5 {
        let response = service
            .ready()
            .await
            .unwrap()
            .call(format!("key{}", i))
            .await
            .unwrap();
        assert_eq!(response, "same response");
    }

    // Each key should have called the inner service once
    assert_eq!(call_count.load(Ordering::SeqCst), 5);

    // Access each key again (should all hit cache)
    for i in 0..5 {
        let response = service
            .ready()
            .await
            .unwrap()
            .call(format!("key{}", i))
            .await
            .unwrap();
        assert_eq!(response, "same response");
    }

    // Should still be 5 (all cached)
    assert_eq!(call_count.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn rapid_insert_evict_cycles() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: u32| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(req)
        }
    });

    let eviction_count = Arc::new(AtomicUsize::new(0));
    let ec = Arc::clone(&eviction_count);

    let config = CacheConfig::builder()
        .max_size(5) // Small cache
        .key_extractor(|req: &u32| *req)
        .on_eviction(move || {
            ec.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // Rapidly insert 100 items into a size-5 cache
    for i in 0..100 {
        service.ready().await.unwrap().call(i).await.unwrap();
    }

    // All 100 should have been computed
    assert_eq!(call_count.load(Ordering::SeqCst), 100);

    // We should have had many evictions (100 inserts - 5 capacity = 95 evictions)
    assert_eq!(eviction_count.load(Ordering::SeqCst), 95);

    // Only the last 5 items should be cached
    let cached_start = call_count.load(Ordering::SeqCst);
    for i in 95..100 {
        service.ready().await.unwrap().call(i).await.unwrap();
    }
    // No new calls (all cached)
    assert_eq!(call_count.load(Ordering::SeqCst), cached_start);

    // Earlier items should not be cached
    service.ready().await.unwrap().call(0).await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), cached_start + 1);
}

#[tokio::test]
async fn cache_full_behavior() {
    let service = tower::service_fn(|req: u32| async move { Ok::<_, std::io::Error>(req * 2) });

    let eviction_count = Arc::new(AtomicUsize::new(0));
    let ec = Arc::clone(&eviction_count);

    let config = CacheConfig::builder()
        .max_size(3)
        .key_extractor(|req: &u32| *req)
        .on_eviction(move || {
            ec.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // Fill cache to capacity
    service.ready().await.unwrap().call(1).await.unwrap();
    service.ready().await.unwrap().call(2).await.unwrap();
    service.ready().await.unwrap().call(3).await.unwrap();

    assert_eq!(eviction_count.load(Ordering::SeqCst), 0);

    // Insert one more (should evict)
    service.ready().await.unwrap().call(4).await.unwrap();
    assert_eq!(eviction_count.load(Ordering::SeqCst), 1);

    // Insert another (should evict again)
    service.ready().await.unwrap().call(5).await.unwrap();
    assert_eq!(eviction_count.load(Ordering::SeqCst), 2);

    // Cache should still work correctly
    let result = service.ready().await.unwrap().call(5).await.unwrap();
    assert_eq!(result, 10);
}

#[tokio::test]
async fn ttl_expiration_during_service_call() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            // Slow service to allow TTL to expire
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let config = CacheConfig::builder()
        .max_size(10)
        .ttl(Duration::from_millis(80))
        .key_extractor(|req: &String| req.clone())
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // First call - cache miss, takes 100ms
    let start = std::time::Instant::now();
    service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(70)); // Allow some tolerance
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Item is cached, but wait for TTL to expire
    tokio::time::sleep(Duration::from_millis(90)).await;

    // Second call - should miss due to expiration
    service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn clone_overhead_with_large_responses() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: u32| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            // Large response (1000 element vector)
            let large_vec: Vec<u32> = (0..1000).map(|i| req + i).collect();
            Ok::<_, std::io::Error>(large_vec)
        }
    });

    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &u32| *req)
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // First call - computes large response
    let response1 = service.ready().await.unwrap().call(42).await.unwrap();
    assert_eq!(response1.len(), 1000);
    assert_eq!(response1[0], 42);
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Multiple subsequent calls should return clones
    for _ in 0..10 {
        let response = service.ready().await.unwrap().call(42).await.unwrap();
        assert_eq!(response.len(), 1000);
        assert_eq!(response[0], 42);
    }

    // Should still be 1 (all cached, clones returned)
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}
