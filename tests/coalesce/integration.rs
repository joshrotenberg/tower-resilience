//! Integration tests for basic coalesce functionality.

use super::TestError;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_coalesce::CoalesceLayer;

#[tokio::test]
async fn test_single_request_passes_through() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let mut service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response, "response: test");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_sequential_requests_execute_separately() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            let n = count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>(format!("response-{}: {}", n, req))
        }
    });

    let mut service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    // First request
    let r1 = service
        .ready()
        .await
        .unwrap()
        .call("key".to_string())
        .await
        .unwrap();
    assert_eq!(r1, "response-0: key");

    // Second request after first completes - should execute again
    let r2 = service
        .ready()
        .await
        .unwrap()
        .call("key".to_string())
        .await
        .unwrap();
    assert_eq!(r2, "response-1: key");

    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_error_returned_correctly() {
    let service = tower::service_fn(|_req: String| async move {
        Err::<String, _>(TestError::new("expected error"))
    });

    let mut service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("expected error"));
}

#[tokio::test]
async fn test_custom_key_extractor() {
    #[derive(Clone)]
    struct Request {
        id: u64,
        #[allow(dead_code)]
        data: String,
    }

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: Request| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>(format!("response for id {}", req.id))
        }
    });

    let mut service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &Request| req.id))
        .service(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request {
            id: 42,
            data: "test".to_string(),
        })
        .await
        .unwrap();

    assert_eq!(response, "response for id 42");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_service_clone() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>(format!("response: {}", req))
        }
    });

    let service = ServiceBuilder::new()
        .layer(CoalesceLayer::new(|req: &String| req.clone()))
        .service(service);

    // Clone should work
    let mut svc1 = service.clone();
    let mut svc2 = service.clone();

    let r1 = svc1
        .ready()
        .await
        .unwrap()
        .call("a".to_string())
        .await
        .unwrap();
    let r2 = svc2
        .ready()
        .await
        .unwrap()
        .call("b".to_string())
        .await
        .unwrap();

    assert_eq!(r1, "response: a");
    assert_eq!(r2, "response: b");
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}
