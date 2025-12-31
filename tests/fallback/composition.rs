//! Tests for composing fallback with other layers.

use super::TestError;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::{Layer, Service, ServiceBuilder, ServiceExt, service_fn};
use tower_resilience_fallback::FallbackLayer;

#[tokio::test]
async fn test_fallback_preserves_successful_responses() {
    let service =
        service_fn(|req: String| async move { Ok::<_, TestError>(format!("processed: {}", req)) });

    let mut service = ServiceBuilder::new()
        .layer(FallbackLayer::<String, String, TestError>::value(
            "fallback".to_string(),
        ))
        .service(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("hello".to_string())
        .await
        .unwrap();

    // Should get the actual response, not the fallback
    assert_eq!(response, "processed: hello");
}

#[tokio::test]
async fn test_fallback_layer_is_clone() {
    let layer = FallbackLayer::<String, String, TestError>::value("fallback".to_string());
    let _cloned = layer.clone();
}

#[tokio::test]
async fn test_fallback_service_is_clone() {
    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

    let layer = FallbackLayer::<String, String, TestError>::value("fallback".to_string());
    let service = layer.layer(service);
    let _cloned = service.clone();
}

#[tokio::test]
async fn test_fallback_with_map_response() {
    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

    let layer = FallbackLayer::<String, String, TestError>::value("fallback".to_string());
    let service = layer.layer(service);

    // Map the response to uppercase
    let mut service = tower::ServiceBuilder::new()
        .map_response(|r: String| r.to_uppercase())
        .service(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response, "FALLBACK");
}

#[tokio::test]
async fn test_fallback_concurrent_clones() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError::new("failed"))
        }
    });

    let layer = FallbackLayer::<String, String, TestError>::value("fallback".to_string());
    let service = layer.layer(service);

    // Spawn multiple concurrent tasks using clones
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let mut svc = service.clone();
            tokio::spawn(async move {
                svc.ready()
                    .await
                    .unwrap()
                    .call(format!("req-{}", i))
                    .await
                    .unwrap()
            })
        })
        .collect();

    for handle in handles {
        let result = handle.await.unwrap();
        assert_eq!(result, "fallback");
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn test_fallback_with_service_builder() {
    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

    let mut service = ServiceBuilder::new()
        .layer(FallbackLayer::<String, String, TestError>::from_error(
            |e: &TestError| format!("Handled: {}", e.message),
        ))
        .service(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response, "Handled: failed");
}
