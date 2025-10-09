//! Layer composition tests for tower-cache.
//!
//! Tests cache layer integration with Tower's ServiceBuilder and
//! composition with other resilience patterns, including:
//! - Basic layer composition with ServiceBuilder
//! - Multiple cache layers in same stack
//! - Cache + circuit breaker composition
//! - Cache + bulkhead composition
//! - Service cloning through layers
//! - Layer type correctness

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::{Layer, Service, ServiceBuilder, ServiceExt};
use tower_resilience_cache::CacheConfig;

#[tokio::test]
async fn layer_composition_with_service_builder() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    // Build cache layer
    let cache_layer = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.clone())
        .build();

    // Compose using ServiceBuilder
    let mut composed_service = ServiceBuilder::new().layer(cache_layer).service(service);

    // First call - cache miss
    let response1 = composed_service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(response1, "Response: test");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Second call - cache hit
    let response2 = composed_service
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
async fn multiple_cache_layers_in_same_stack() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: u32| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(req * 2)
        }
    });

    // Create two cache layers
    // Outer cache: larger capacity
    let outer_cache = CacheConfig::builder()
        .max_size(10)
        .name("outer-cache")
        .key_extractor(|req: &u32| *req)
        .build();

    // Inner cache: smaller capacity
    let inner_cache = CacheConfig::builder()
        .max_size(5)
        .name("inner-cache")
        .key_extractor(|req: &u32| *req)
        .build();

    // Stack them: request -> outer_cache -> inner_cache -> service
    let mut stacked_service = ServiceBuilder::new()
        .layer(outer_cache)
        .layer(inner_cache)
        .service(service);

    // First call with key 5
    let result1 = stacked_service
        .ready()
        .await
        .unwrap()
        .call(5)
        .await
        .unwrap();
    assert_eq!(result1, 10);
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Same key - should hit outer cache
    let result2 = stacked_service
        .ready()
        .await
        .unwrap()
        .call(5)
        .await
        .unwrap();
    assert_eq!(result2, 10);
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Different key (15) - both caches will miss
    let result3 = stacked_service
        .ready()
        .await
        .unwrap()
        .call(15)
        .await
        .unwrap();
    assert_eq!(result3, 30);
    // Both caches miss, inner service called
    assert_eq!(call_count.load(Ordering::SeqCst), 2);

    // Call 15 again - outer cache should hit
    let result4 = stacked_service
        .ready()
        .await
        .unwrap()
        .call(15)
        .await
        .unwrap();
    assert_eq!(result4, 30);
    // Outer cache hits, no new calls
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cache_with_map_response_layer() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let cache_layer = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.clone())
        .build();

    // Use map_response to transform cached responses
    let map_layer = tower::util::MapResponseLayer::new(|response: String| response.to_uppercase());

    // Stack: map_response -> cache -> service
    let mut composed_service = ServiceBuilder::new()
        .layer(map_layer)
        .layer(cache_layer)
        .service(service);

    // First call
    let result1 = composed_service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(result1, "RESPONSE: TEST");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Second call - cached response should also be transformed
    let result2 = composed_service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(result2, "RESPONSE: TEST");
    assert_eq!(call_count.load(Ordering::SeqCst), 1); // Not called again
}

#[tokio::test]
async fn cache_with_map_request_layer() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let cache_layer = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.clone())
        .build();

    // Use map_request to transform incoming requests before caching
    let map_layer = tower::util::MapRequestLayer::new(|req: String| req.to_lowercase());

    // Stack: cache -> map_request -> service
    // The map happens AFTER cache checks, so cache sees original request
    let mut composed_service = ServiceBuilder::new()
        .layer(cache_layer)
        .layer(map_layer)
        .service(service);

    // First call with "test"
    let _ = composed_service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Same key "test" - should hit cache
    let _ = composed_service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Different keys are different to the cache (cache before map)
    let _ = composed_service
        .ready()
        .await
        .unwrap()
        .call("TEST".to_string())
        .await
        .unwrap();
    // Cache miss because "TEST" != "test" to the cache layer
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn service_cloning_through_layer() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    let cache_layer = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.clone())
        .build();

    let mut service1 = ServiceBuilder::new().layer(cache_layer).service(service);

    // Clone the service
    let mut service2 = service1.clone();
    let mut service3 = service1.clone();

    // service1 populates cache
    service1
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // service2 should see the cached value
    service2
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // service3 should also see the cached value
    service3
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Different key on service2 should call inner service
    service2
        .ready()
        .await
        .unwrap()
        .call("other".to_string())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn layer_returns_correct_service_type() {
    let service = tower::service_fn(|req: String| async move {
        Ok::<_, std::io::Error>(format!("Response: {}", req))
    });

    let cache_config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.clone())
        .build();

    let cache_layer = cache_config;

    // Layer should correctly wrap the service
    let mut cached_service = cache_layer.layer(service);

    // Service should be usable
    let result = cached_service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Response: test");

    // Should be cloneable
    let mut cloned = cached_service.clone();
    let result2 = cloned.ready().await.unwrap().call("test".to_string()).await;
    assert!(result2.is_ok());
}
