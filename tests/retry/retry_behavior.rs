//! Core retry behavior tests for tower-retry-plus.
//!
//! Tests core retry logic including:
//! - Success on first attempt (no retries)
//! - Success after N retries
//! - Exhaust all attempts
//! - Stop retrying on non-retryable error
//! - Request cloning works correctly

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::{Layer, Service, ServiceExt};
use tower_resilience_retry::RetryConfig;

#[derive(Debug, Clone)]
struct TestError {
    message: String,
}

impl TestError {
    fn new(msg: &str) -> Self {
        Self {
            message: msg.to_string(),
        }
    }
}

#[tokio::test]
async fn success_on_first_attempt_no_retry() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Ok::<_, TestError>(format!("Response: {}", req))
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(5)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Response: test");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn success_after_one_retry() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count == 0 {
                Err(TestError::new("first attempt failed"))
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(3)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "success");
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn success_after_multiple_retries() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 4 {
                Err(TestError::new("temporary failure"))
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(6)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "success");
    assert_eq!(call_count.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn exhaust_all_attempts() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError::new("permanent failure"))
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(4)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().message, "permanent failure");
    assert_eq!(call_count.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn single_attempt_no_retries() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError::new("error"))
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(1)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config;
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
async fn request_cloning_works_correctly() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let received_requests = Arc::new(std::sync::Mutex::new(Vec::new()));

    let cc = Arc::clone(&call_count);
    let rr = Arc::clone(&received_requests);

    let service = tower::service_fn(move |req: String| {
        let cc = Arc::clone(&cc);
        let rr = Arc::clone(&rr);
        async move {
            rr.lock().unwrap().push(req.clone());
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError::new("retry"))
            } else {
                Ok::<_, TestError>(format!("Response: {}", req))
            }
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(4)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test-request".to_string())
        .await;

    assert!(result.is_ok());
    assert_eq!(call_count.load(Ordering::SeqCst), 3);

    // Verify the same request was sent each time
    let requests = received_requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|r| r == "test-request"));
}

#[tokio::test]
async fn different_requests_independently_retried() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count == 0 || count == 2 {
                Err(TestError::new("fail"))
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(3)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    // First request: fails once, then succeeds
    let result1 = service
        .ready()
        .await
        .unwrap()
        .call("request1".to_string())
        .await;
    assert!(result1.is_ok());

    // Second request: fails once, then succeeds
    let result2 = service
        .ready()
        .await
        .unwrap()
        .call("request2".to_string())
        .await;
    assert!(result2.is_ok());

    assert_eq!(call_count.load(Ordering::SeqCst), 4); // 2 + 2
}

#[tokio::test]
async fn stop_on_non_retryable_error() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    #[derive(Debug, Clone)]
    enum Error {
        Retryable,
        NonRetryable,
    }

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count == 0 {
                Err(Error::Retryable)
            } else {
                Err(Error::NonRetryable)
            }
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(5)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .retry_on(|e| matches!(e, Error::Retryable))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let result: Result<String, Error> = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::NonRetryable));
    // First attempt (Retryable) triggers retry, second (NonRetryable) stops
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn max_attempts_two_allows_one_retry() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count == 0 {
                Err(TestError::new("first attempt failed"))
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(2)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config;
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
async fn max_attempts_hundred() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 50 {
                Err(TestError::new("retry"))
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(100)
        .fixed_backoff(std::time::Duration::from_millis(1))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_ok());
    assert_eq!(call_count.load(Ordering::SeqCst), 51);
}

#[tokio::test]
async fn service_cloning_preserves_retry_behavior() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count.is_multiple_of(2) {
                Err(TestError::new("fail"))
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(3)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config;
    let mut service1 = layer.layer(service);
    let mut service2 = service1.clone();

    // Both clones should have retry behavior
    let result1 = service1
        .ready()
        .await
        .unwrap()
        .call("test1".to_string())
        .await;
    assert!(result1.is_ok());

    let result2 = service2
        .ready()
        .await
        .unwrap()
        .call("test2".to_string())
        .await;
    assert!(result2.is_ok());

    // Each should have retried once
    assert_eq!(call_count.load(Ordering::SeqCst), 4); // 2 + 2
}

#[tokio::test]
async fn concurrent_requests_independently_retried() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count.is_multiple_of(3) {
                Err(TestError::new("fail"))
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(3)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config;
    let service = layer.layer(service);

    // Launch multiple concurrent requests
    let mut handles = vec![];
    for i in 0..5 {
        let mut svc = service.clone();
        let handle = tokio::spawn(async move {
            svc.ready()
                .await
                .unwrap()
                .call(format!("request{}", i))
                .await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let mut success_count = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            success_count += 1;
        }
    }

    assert_eq!(success_count, 5);
}

#[tokio::test]
async fn empty_response_type() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError::new("fail"))
            } else {
                Ok::<_, TestError>(())
            }
        }
    });

    let config = RetryConfig::builder()
        .max_attempts(4)
        .fixed_backoff(std::time::Duration::from_millis(10))
        .build();

    let layer = config;
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
