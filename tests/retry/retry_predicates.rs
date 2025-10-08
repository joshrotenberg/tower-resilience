//! Retry predicate tests for tower-retry-plus.
//!
//! Tests error filtering including:
//! - Retry all errors (default)
//! - Retry specific error types
//! - Custom predicate logic
//! - Predicate with stateful logic
//! - Combining predicates

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tower::{Layer, Service, ServiceExt};
use tower_retry_plus::RetryConfig;

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum TestError {
    Retryable(String),
    NonRetryable(String),
    Transient,
    Permanent,
}

#[tokio::test]
async fn retry_all_errors_by_default() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            match count {
                0 => Err(TestError::Transient),
                1 => Err(TestError::Permanent),
                _ => Ok::<_, TestError>("success".to_string()),
            }
        }
    });

    let config: RetryConfig<TestError> = RetryConfig::builder()
        .max_attempts(5)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_ok());
    // Both Transient and Permanent were retried
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn retry_only_retryable_errors() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count == 0 {
                Err(TestError::Retryable("temporary".to_string()))
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config: RetryConfig<TestError> = RetryConfig::builder()
        .max_attempts(3)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .retry_on(|e| matches!(e, TestError::Retryable(_)))
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_ok());
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn dont_retry_non_retryable_errors() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError::NonRetryable("permanent".to_string()))
        }
    });

    let config: RetryConfig<TestError> = RetryConfig::builder()
        .max_attempts(5)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .retry_on(|e| matches!(e, TestError::Retryable(_)))
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_err());
    // Only called once, no retries
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn predicate_based_on_error_content() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            match count {
                0 => Err(TestError::Retryable("timeout".to_string())),
                1 => Err(TestError::Retryable("rate-limit".to_string())),
                _ => Ok::<_, TestError>("success".to_string()),
            }
        }
    });

    // Only retry timeout errors
    let config: RetryConfig<TestError> = RetryConfig::builder()
        .max_attempts(3)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .retry_on(|e| match e {
            TestError::Retryable(msg) => msg.contains("timeout"),
            _ => false,
        })
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    // First error (timeout) is retried, second (rate-limit) is not
    assert!(result.is_err());
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn predicate_never_retries() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError::Transient)
        }
    });

    let config: RetryConfig<TestError> = RetryConfig::builder()
        .max_attempts(5)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .retry_on(|_| false) // Never retry
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_err());
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn predicate_always_retries() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError::NonRetryable("error".to_string()))
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config: RetryConfig<TestError> = RetryConfig::builder()
        .max_attempts(4)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .retry_on(|_| true) // Always retry
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_ok());
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn predicate_with_multiple_error_types() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            match count {
                0 => Err(TestError::Transient),
                1 => Err(TestError::Retryable("retry".to_string())),
                _ => Ok::<_, TestError>("success".to_string()),
            }
        }
    });

    // Retry both Transient and Retryable variants
    let config: RetryConfig<TestError> = RetryConfig::builder()
        .max_attempts(5)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .retry_on(|e| matches!(e, TestError::Transient | TestError::Retryable(_)))
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_ok());
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn predicate_with_external_state() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    // External state: only allow retries if flag is true
    let allow_retries = Arc::new(AtomicBool::new(true));
    let ar = Arc::clone(&allow_retries);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 3 {
                Err(TestError::Transient)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config: RetryConfig<TestError> = RetryConfig::builder()
        .max_attempts(5)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .retry_on(move |_| ar.load(Ordering::SeqCst))
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    // First request with retries enabled
    let result1 = service
        .ready()
        .await
        .unwrap()
        .call("test1".to_string())
        .await;
    assert!(result1.is_ok());
    assert_eq!(call_count.load(Ordering::SeqCst), 4);

    // Disable retries
    allow_retries.store(false, Ordering::SeqCst);
    call_count.store(0, Ordering::SeqCst);

    // Second request with retries disabled
    let result2 = service
        .ready()
        .await
        .unwrap()
        .call("test2".to_string())
        .await;
    assert!(result2.is_err());
    assert_eq!(call_count.load(Ordering::SeqCst), 1); // No retries
}

#[tokio::test]
async fn predicate_complex_logic() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            match count {
                0 => Err(TestError::Retryable("503 Service Unavailable".to_string())),
                1 => Err(TestError::Retryable("429 Too Many Requests".to_string())),
                2 => Err(TestError::Retryable("404 Not Found".to_string())),
                _ => Ok::<_, TestError>("success".to_string()),
            }
        }
    });

    // Retry only 5xx and 429 errors, not 404
    let config: RetryConfig<TestError> = RetryConfig::builder()
        .max_attempts(5)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .retry_on(|e| match e {
            TestError::Retryable(msg) => {
                msg.contains("503") || msg.contains("429") || msg.contains("500")
            }
            _ => false,
        })
        .build();

    let layer = config.layer();
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    // 503 and 429 are retried, 404 is not
    assert!(result.is_err());
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn different_predicates_different_services() {
    let call_count1 = Arc::new(AtomicUsize::new(0));
    let cc1 = Arc::clone(&call_count1);

    let call_count2 = Arc::new(AtomicUsize::new(0));
    let cc2 = Arc::clone(&call_count2);

    let service1 = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc1);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError::Transient)
        }
    });

    let service2 = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc2);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError::Transient)
        }
    });

    // Service 1: retry transient errors
    let config1: RetryConfig<TestError> = RetryConfig::builder()
        .max_attempts(3)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .retry_on(|e| matches!(e, TestError::Transient))
        .build();

    // Service 2: don't retry transient errors
    let config2: RetryConfig<TestError> = RetryConfig::builder()
        .max_attempts(3)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .retry_on(|e| matches!(e, TestError::Permanent))
        .build();

    let mut svc1 = config1.layer().layer(service1);
    let mut svc2 = config2.layer().layer(service2);

    let _ = svc1.ready().await.unwrap().call("test".to_string()).await;
    let _ = svc2.ready().await.unwrap().call("test".to_string()).await;

    // Service 1 retries
    assert_eq!(call_count1.load(Ordering::SeqCst), 3);
    // Service 2 doesn't retry
    assert_eq!(call_count2.load(Ordering::SeqCst), 1);
}
