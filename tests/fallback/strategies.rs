//! Tests for different fallback strategies.

use super::TestError;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_fallback::{FallbackError, FallbackLayer};

#[tokio::test]
async fn test_value_strategy() {
    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

    let layer = FallbackLayer::<String, String, TestError>::value("static fallback".to_string());
    let mut service = layer.layer(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response, "static fallback");
}

#[tokio::test]
async fn test_from_error_strategy() {
    let service = service_fn(|_req: String| async move {
        Err::<String, _>(TestError::with_code("something broke", 503))
    });

    let layer = FallbackLayer::<String, String, TestError>::from_error(|e: &TestError| {
        format!("Error {}: {}", e.code, e.message)
    });
    let mut service = layer.layer(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response, "Error 503: something broke");
}

#[tokio::test]
async fn test_from_request_error_strategy() {
    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

    let layer = FallbackLayer::<String, String, TestError>::from_request_error(
        |req: &String, e: &TestError| format!("Request '{}' failed: {}", req, e.message),
    );
    let mut service = layer.layer(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("my-request".to_string())
        .await
        .unwrap();

    assert_eq!(response, "Request 'my-request' failed: failed");
}

#[tokio::test]
async fn test_from_request_error_with_cache() {
    let cache: Arc<HashMap<String, String>> = Arc::new({
        let mut m = HashMap::new();
        m.insert("key1".to_string(), "cached-value-1".to_string());
        m.insert("key2".to_string(), "cached-value-2".to_string());
        m
    });
    let cache_clone = Arc::clone(&cache);

    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

    let layer = FallbackLayer::<String, String, TestError>::from_request_error(
        move |req: &String, _e: &TestError| {
            cache_clone
                .get(req)
                .cloned()
                .unwrap_or_else(|| "not in cache".to_string())
        },
    );
    let mut service = layer.layer(service);

    // Request for cached key
    let r1 = service
        .ready()
        .await
        .unwrap()
        .call("key1".to_string())
        .await
        .unwrap();
    assert_eq!(r1, "cached-value-1");

    // Request for non-cached key
    let r2 = service
        .ready()
        .await
        .unwrap()
        .call("key3".to_string())
        .await
        .unwrap();
    assert_eq!(r2, "not in cache");
}

#[tokio::test]
async fn test_service_strategy_success() {
    let primary_calls = Arc::new(AtomicUsize::new(0));
    let backup_calls = Arc::new(AtomicUsize::new(0));

    let pc = Arc::clone(&primary_calls);
    let primary = service_fn(move |_req: String| {
        let pc = Arc::clone(&pc);
        async move {
            pc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError::new("primary failed"))
        }
    });

    let bc = Arc::clone(&backup_calls);
    let layer = FallbackLayer::<String, String, TestError>::service(move |req: String| {
        let bc = Arc::clone(&bc);
        async move {
            bc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>(format!("backup handled: {}", req))
        }
    });
    let mut service = layer.layer(primary);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response, "backup handled: test");
    assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
    assert_eq!(backup_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_service_strategy_also_fails() {
    let primary =
        service_fn(
            |_req: String| async move { Err::<String, _>(TestError::new("primary failed")) },
        );

    let layer = FallbackLayer::<String, String, TestError>::service(|_req: String| async move {
        Err::<String, _>(TestError::new("backup also failed"))
    });
    let mut service = layer.layer(primary);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    match result {
        Err(FallbackError::FallbackFailed(e)) => {
            assert_eq!(e.message, "backup also failed");
        }
        _ => panic!("expected FallbackFailed error"),
    }
}

#[tokio::test]
async fn test_exception_strategy() {
    let service = service_fn(|_req: String| async move {
        Err::<String, _>(TestError::with_code("internal error", 500))
    });

    let layer = FallbackLayer::<String, String, TestError>::exception(|_e: TestError| TestError {
        message: "Service temporarily unavailable".to_string(),
        retryable: false,
        code: 503,
    });
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    match result {
        Err(FallbackError::Inner(e)) => {
            assert_eq!(e.message, "Service temporarily unavailable");
            assert_eq!(e.code, 503);
            assert!(!e.retryable);
        }
        _ => panic!("expected transformed error"),
    }
}

#[tokio::test]
async fn test_exception_preserves_original_info() {
    let service = service_fn(|_req: String| async move {
        Err::<String, _>(TestError::with_code("database connection failed", 500))
    });

    let layer = FallbackLayer::<String, String, TestError>::exception(|e: TestError| TestError {
        message: format!("Wrapped: {}", e.message),
        retryable: e.retryable,
        code: e.code,
    });
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    match result {
        Err(FallbackError::Inner(e)) => {
            assert_eq!(e.message, "Wrapped: database connection failed");
            assert_eq!(e.code, 500);
        }
        _ => panic!("expected transformed error"),
    }
}
