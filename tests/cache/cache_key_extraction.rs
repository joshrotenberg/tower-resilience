//! Key extraction tests for tower-cache.
//!
//! Tests various key extraction strategies, including:
//! - Complex key extraction from struct fields
//! - Key collision handling
//! - Different request types with same key
//! - Hash-based key extraction
//! - Simple type key extraction
//! - Key extraction consistency

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::{Layer, Service, ServiceExt};
use tower_resilience_cache::CacheConfig;

#[derive(Clone, Debug)]
struct ComplexRequest {
    user_id: u32,
    resource_id: u32,
    action: String,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
struct ComplexKey {
    user_id: u32,
    resource_id: u32,
}

#[tokio::test]
async fn complex_key_extraction_from_struct() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: ComplexRequest| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!(
                "{} by user {} on resource {}",
                req.action, req.user_id, req.resource_id
            ))
        }
    });

    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &ComplexRequest| ComplexKey {
            user_id: req.user_id,
            resource_id: req.resource_id,
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    // First request
    let req1 = ComplexRequest {
        user_id: 100,
        resource_id: 200,
        action: "read".to_string(),
    };
    service.ready().await.unwrap().call(req1).await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Same user and resource, different action - should hit cache
    let req2 = ComplexRequest {
        user_id: 100,
        resource_id: 200,
        action: "write".to_string(), // Different action
    };
    service.ready().await.unwrap().call(req2).await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1); // Cache hit

    // Different user - should miss cache
    let req3 = ComplexRequest {
        user_id: 101,
        resource_id: 200,
        action: "read".to_string(),
    };
    service.ready().await.unwrap().call(req3).await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 2); // Cache miss

    // Different resource - should miss cache
    let req4 = ComplexRequest {
        user_id: 100,
        resource_id: 201,
        action: "read".to_string(),
    };
    service.ready().await.unwrap().call(req4).await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 3); // Cache miss
}

#[tokio::test]
async fn key_collision_handling() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        }
    });

    // Key extractor that intentionally creates collisions
    // (extracts first character only)
    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.chars().next().unwrap_or('?').to_string())
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    // First request with key "a"
    let response1 = service
        .ready()
        .await
        .unwrap()
        .call("apple".to_string())
        .await
        .unwrap();
    assert_eq!(response1, "Response: apple");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Different request but same key "a" - should hit cache
    let response2 = service
        .ready()
        .await
        .unwrap()
        .call("apricot".to_string())
        .await
        .unwrap();
    // Returns cached response from "apple"
    assert_eq!(response2, "Response: apple");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Request with different key "b" - should miss cache
    let response3 = service
        .ready()
        .await
        .unwrap()
        .call("banana".to_string())
        .await
        .unwrap();
    assert_eq!(response3, "Response: banana");
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn different_request_types_same_key_extracted() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    #[derive(Clone)]
    struct RequestV1 {
        id: u64,
        name: String,
    }

    let service = tower::service_fn(move |req: RequestV1| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("Response: {} - {}", req.id, req.name))
        }
    });

    // Key extraction only uses the id field
    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &RequestV1| req.id)
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    // First request with id=1
    let req1 = RequestV1 {
        id: 1,
        name: "Alice".to_string(),
    };
    let response1 = service.ready().await.unwrap().call(req1).await.unwrap();
    assert_eq!(response1, "Response: 1 - Alice");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Same id, different name - should hit cache
    let req2 = RequestV1 {
        id: 1,
        name: "Bob".to_string(),
    };
    let response2 = service.ready().await.unwrap().call(req2).await.unwrap();
    assert_eq!(response2, "Response: 1 - Alice"); // Cached response
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Different id - should miss cache
    let req3 = RequestV1 {
        id: 2,
        name: "Alice".to_string(),
    };
    let response3 = service.ready().await.unwrap().call(req3).await.unwrap();
    assert_eq!(response3, "Response: 2 - Alice");
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn key_extractor_with_hash_of_struct_fields() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    #[derive(Clone, Hash)]
    struct SearchQuery {
        terms: Vec<String>,
        filters: Vec<String>,
    }

    let service = tower::service_fn(move |req: SearchQuery| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!(
                "Results for: {:?} with filters: {:?}",
                req.terms, req.filters
            ))
        }
    });

    // Key extractor uses hash of entire struct
    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &SearchQuery| {
            let mut hasher = DefaultHasher::new();
            req.hash(&mut hasher);
            hasher.finish()
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    // First query
    let query1 = SearchQuery {
        terms: vec!["rust".to_string(), "tower".to_string()],
        filters: vec!["recent".to_string()],
    };
    service
        .ready()
        .await
        .unwrap()
        .call(query1.clone())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Same query - should hit cache
    service
        .ready()
        .await
        .unwrap()
        .call(query1.clone())
        .await
        .unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Different query - should miss cache
    let query2 = SearchQuery {
        terms: vec!["rust".to_string()], // Different terms
        filters: vec!["recent".to_string()],
    };
    service.ready().await.unwrap().call(query2).await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 2);

    // Order matters in vectors - different order = different hash
    let query3 = SearchQuery {
        terms: vec!["tower".to_string(), "rust".to_string()], // Reversed
        filters: vec!["recent".to_string()],
    };
    service.ready().await.unwrap().call(query3).await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn key_extractor_with_simple_types() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    // Test with various simple types
    let service = tower::service_fn(move |req: u64| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(req * 2)
        }
    });

    // Key extractor just returns the request itself
    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &u64| *req)
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    // Test multiple calls with same key
    for _ in 0..5 {
        let result = service.ready().await.unwrap().call(42).await.unwrap();
        assert_eq!(result, 84);
    }
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Test with different keys
    for i in 0..10 {
        service.ready().await.unwrap().call(i).await.unwrap();
    }
    assert_eq!(call_count.load(Ordering::SeqCst), 11); // 1 + 10 new keys

    // Test String keys
    let string_service =
        tower::service_fn(
            move |req: String| async move { Ok::<_, std::io::Error>(req.to_uppercase()) },
        );

    let string_config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &String| req.clone())
        .build();

    let string_layer = string_config;
    let mut string_service = string_layer.layer(string_service);

    let result = string_service
        .ready()
        .await
        .unwrap()
        .call("hello".to_string())
        .await
        .unwrap();
    assert_eq!(result, "HELLO");
}

#[tokio::test]
async fn key_extraction_consistency() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    #[derive(Clone)]
    struct Request {
        a: u32,
        b: u32,
    }

    let service = tower::service_fn(move |req: Request| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(req.a + req.b)
        }
    });

    // Key extractor that combines fields
    let config = CacheConfig::builder()
        .max_size(10)
        .key_extractor(|req: &Request| (req.a, req.b))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    // Multiple calls with same (a, b) values
    let req = Request { a: 10, b: 20 };
    for _ in 0..10 {
        let result = service
            .ready()
            .await
            .unwrap()
            .call(req.clone())
            .await
            .unwrap();
        assert_eq!(result, 30);
    }

    // Should only call inner service once (all subsequent calls cached)
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Different values should produce different keys
    let req2 = Request { a: 10, b: 21 }; // Different b
    service.ready().await.unwrap().call(req2).await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 2);

    let req3 = Request { a: 11, b: 20 }; // Different a
    service.ready().await.unwrap().call(req3).await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 3);

    // Original request should still hit cache
    let req4 = Request { a: 10, b: 20 };
    service.ready().await.unwrap().call(req4).await.unwrap();
    assert_eq!(call_count.load(Ordering::SeqCst), 3); // No new call
}
