//! Integration tests for fallback basic functionality.

use super::TestError;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_fallback::{FallbackError, FallbackLayer};

#[tokio::test]
async fn test_success_no_fallback_triggered() {
    let service =
        service_fn(|req: String| async move { Ok::<_, TestError>(format!("response: {}", req)) });

    let layer = FallbackLayer::<String, String, TestError>::value("fallback".to_string());
    let mut service = layer.layer(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("hello".to_string())
        .await
        .unwrap();

    assert_eq!(response, "response: hello");
}

#[tokio::test]
async fn test_failure_triggers_fallback() {
    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

    let layer = FallbackLayer::<String, String, TestError>::value("fallback response".to_string());
    let mut service = layer.layer(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("hello".to_string())
        .await
        .unwrap();

    assert_eq!(response, "fallback response");
}

#[tokio::test]
async fn test_multiple_sequential_calls() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count.is_multiple_of(2) {
                Ok::<_, TestError>("success".to_string())
            } else {
                Err(TestError::new("failed"))
            }
        }
    });

    let layer = FallbackLayer::<String, String, TestError>::value("fallback".to_string());
    let mut service = layer.layer(service);

    // First call succeeds
    let r1 = service
        .ready()
        .await
        .unwrap()
        .call("1".to_string())
        .await
        .unwrap();
    assert_eq!(r1, "success");

    // Second call fails, triggers fallback
    let r2 = service
        .ready()
        .await
        .unwrap()
        .call("2".to_string())
        .await
        .unwrap();
    assert_eq!(r2, "fallback");

    // Third call succeeds
    let r3 = service
        .ready()
        .await
        .unwrap()
        .call("3".to_string())
        .await
        .unwrap();
    assert_eq!(r3, "success");

    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_service_cloning_preserves_config() {
    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

    let layer = FallbackLayer::<String, String, TestError>::value("fallback".to_string());
    let service = layer.layer(service);

    // Clone the service
    let mut clone1 = service.clone();
    let mut clone2 = service.clone();

    // Both clones should have the same fallback behavior
    let r1 = clone1
        .ready()
        .await
        .unwrap()
        .call("a".to_string())
        .await
        .unwrap();
    let r2 = clone2
        .ready()
        .await
        .unwrap()
        .call("b".to_string())
        .await
        .unwrap();

    assert_eq!(r1, "fallback");
    assert_eq!(r2, "fallback");
}

#[tokio::test]
async fn test_named_fallback() {
    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

    let layer = FallbackLayer::builder()
        .name("my-fallback")
        .value("fallback".to_string())
        .build();
    let mut service = layer.layer(service);

    let response = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
        .unwrap();

    assert_eq!(response, "fallback");
}

#[tokio::test]
async fn test_error_types_preserved() {
    let service = service_fn(|_req: String| async move {
        Err::<String, _>(TestError::with_code("original error", 503))
    });

    // Use exception strategy to verify error is passed through
    let layer = FallbackLayer::<String, String, TestError>::exception(|e: TestError| TestError {
        message: format!("transformed: {}", e.message),
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
            assert_eq!(e.message, "transformed: original error");
            assert_eq!(e.code, 503);
        }
        _ => panic!("expected transformed error"),
    }
}

#[tokio::test]
async fn test_concurrent_calls() {
    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("failed")) });

    let layer = FallbackLayer::<String, String, TestError>::value("fallback".to_string());
    let service = layer.layer(service);

    let handles: Vec<_> = (0..10)
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
}
