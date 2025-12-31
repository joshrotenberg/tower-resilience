//! Tests for selective error handling with predicates.

use super::TestError;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_fallback::{FallbackError, FallbackLayer};

#[tokio::test]
async fn test_predicate_matches_triggers_fallback() {
    let service =
        service_fn(
            |_req: String| async move { Err::<String, _>(TestError::new("retryable error")) },
        );

    let layer = FallbackLayer::builder()
        .value("fallback".to_string())
        .handle(|e: &TestError| e.retryable)
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
async fn test_predicate_no_match_propagates_error() {
    let service = service_fn(|_req: String| async move {
        Err::<String, _>(TestError::non_retryable("permanent error"))
    });

    let layer = FallbackLayer::builder()
        .value("fallback".to_string())
        .handle(|e: &TestError| e.retryable) // Only handle retryable errors
        .build();
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    match result {
        Err(FallbackError::Inner(e)) => {
            assert_eq!(e.message, "permanent error");
            assert!(!e.retryable);
        }
        _ => panic!("expected inner error to be propagated"),
    }
}

#[tokio::test]
async fn test_predicate_by_error_code() {
    let service = service_fn(|req: String| async move {
        let code: u32 = req.parse().unwrap_or(500);
        Err::<String, _>(TestError::with_code("error", code))
    });

    // Only handle 5xx errors
    let layer = FallbackLayer::builder()
        .value("server error fallback".to_string())
        .handle(|e: &TestError| e.code >= 500)
        .build();
    let mut service = layer.layer(service);

    // 503 should trigger fallback
    let r1 = service
        .ready()
        .await
        .unwrap()
        .call("503".to_string())
        .await
        .unwrap();
    assert_eq!(r1, "server error fallback");

    // 400 should propagate
    let r2 = service.ready().await.unwrap().call("400".to_string()).await;
    assert!(matches!(r2, Err(FallbackError::Inner(e)) if e.code == 400));
}

#[tokio::test]
async fn test_predicate_by_message_content() {
    let service = service_fn(|req: String| async move { Err::<String, _>(TestError::new(&req)) });

    // Only handle timeout errors
    let layer = FallbackLayer::builder()
        .value("timeout fallback".to_string())
        .handle(|e: &TestError| e.message.contains("timeout"))
        .build();
    let mut service = layer.layer(service);

    // Timeout error triggers fallback
    let r1 = service
        .ready()
        .await
        .unwrap()
        .call("connection timeout".to_string())
        .await
        .unwrap();
    assert_eq!(r1, "timeout fallback");

    // Other errors propagate
    let r2 = service
        .ready()
        .await
        .unwrap()
        .call("connection refused".to_string())
        .await;
    assert!(matches!(r2, Err(FallbackError::Inner(_))));
}

#[tokio::test]
async fn test_no_predicate_handles_all_errors() {
    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("any error")) });

    // No handle() call means handle all errors
    let layer = FallbackLayer::builder()
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
async fn test_predicate_always_false_never_triggers() {
    let service =
        service_fn(|_req: String| async move { Err::<String, _>(TestError::new("error")) });

    let layer = FallbackLayer::builder()
        .value("fallback".to_string())
        .handle(|_e: &TestError| false) // Never handle
        .build();
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(matches!(result, Err(FallbackError::Inner(_))));
}

#[tokio::test]
async fn test_predicate_always_true_always_triggers() {
    let service =
        service_fn(
            |_req: String| async move { Err::<String, _>(TestError::non_retryable("error")) },
        );

    let layer = FallbackLayer::builder()
        .value("fallback".to_string())
        .handle(|_e: &TestError| true) // Always handle
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
async fn test_predicate_with_from_error_strategy() {
    let service = service_fn(|req: String| async move {
        let code: u32 = req.parse().unwrap_or(500);
        Err::<String, _>(TestError::with_code("error", code))
    });

    let layer = FallbackLayer::builder()
        .from_error(|e: &TestError| format!("Handled error code: {}", e.code))
        .handle(|e: &TestError| e.code >= 500)
        .build();
    let mut service = layer.layer(service);

    // 500 triggers fallback with error info
    let r1 = service
        .ready()
        .await
        .unwrap()
        .call("500".to_string())
        .await
        .unwrap();
    assert_eq!(r1, "Handled error code: 500");

    // 404 propagates
    let r2 = service.ready().await.unwrap().call("404".to_string()).await;
    assert!(matches!(r2, Err(FallbackError::Inner(e)) if e.code == 404));
}
