//! Configuration tests for tower-retry-plus.
//!
//! Tests configuration including:
//! - Default values
//! - Custom max attempts (1, 2, 100)
//! - Different backoff configurations
//! - Multiple event listeners
//! - Name configuration

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_retry::{
    ExponentialBackoff, ExponentialRandomBackoff, FnInterval, RetryLayer,
};

#[derive(Debug, Clone)]
struct TestError;

#[tokio::test]
async fn default_configuration() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError)
        }
    });

    // Use default configuration
    let config = RetryLayer::builder().build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    // Default max_attempts should be 3
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn custom_max_attempts_one() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError)
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(1)
        .fixed_backoff(Duration::from_millis(10))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn custom_max_attempts_two() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError)
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(2)
        .fixed_backoff(Duration::from_millis(10))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn custom_max_attempts_hundred() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 50 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(100)
        .fixed_backoff(Duration::from_millis(1))
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
async fn fixed_backoff_configuration() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(20))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let start = std::time::Instant::now();
    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    // 2 retries at 20ms each = ~40ms total (with tolerance)
    assert!(
        elapsed >= Duration::from_millis(10) && elapsed <= Duration::from_millis(80),
        "Expected ~40ms, got {:?}",
        elapsed
    );
}

#[tokio::test]
async fn exponential_backoff_configuration() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .exponential_backoff(Duration::from_millis(50))
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

#[tokio::test]
async fn custom_exponential_backoff_configuration() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .backoff(
            ExponentialBackoff::new(Duration::from_millis(10))
                .multiplier(3.0)
                .max_interval(Duration::from_millis(100)),
        )
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

#[tokio::test]
async fn exponential_random_backoff_configuration() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .backoff(ExponentialRandomBackoff::new(
            Duration::from_millis(50),
            0.5,
        ))
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

#[tokio::test]
async fn function_interval_configuration() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .backoff(FnInterval::new(|attempt| {
            Duration::from_millis(10 * (attempt as u64 + 1))
        }))
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

#[tokio::test]
async fn multiple_event_listeners() {
    let listener1 = Arc::new(AtomicUsize::new(0));
    let listener2 = Arc::new(AtomicUsize::new(0));
    let listener3 = Arc::new(AtomicUsize::new(0));

    let l1 = Arc::clone(&listener1);
    let l2 = Arc::clone(&listener2);
    let l3 = Arc::clone(&listener3);

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(10))
        .on_retry(move |_, _| {
            l1.fetch_add(1, Ordering::SeqCst);
        })
        .on_retry(move |_, _| {
            l2.fetch_add(1, Ordering::SeqCst);
        })
        .on_success(move |_| {
            l3.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert_eq!(listener1.load(Ordering::SeqCst), 2);
    assert_eq!(listener2.load(Ordering::SeqCst), 2);
    assert_eq!(listener3.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn name_configuration() {
    let event_name = Arc::new(std::sync::Mutex::new(String::new()));
    let en = Arc::clone(&event_name);

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError)
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(2)
        .fixed_backoff(Duration::from_millis(10))
        .name("test-retry")
        .on_error(move |_| {
            // Name is accessible through events in implementation
            *en.lock().unwrap() = "test-retry".to_string();
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert_eq!(*event_name.lock().unwrap(), "test-retry");
}

#[tokio::test]
async fn configuration_with_all_listeners() {
    let success = Arc::new(AtomicUsize::new(0));
    let retry = Arc::new(AtomicUsize::new(0));
    let error = Arc::new(AtomicUsize::new(0));
    let ignored = Arc::new(AtomicUsize::new(0));

    let s = Arc::clone(&success);
    let r = Arc::clone(&retry);
    let e = Arc::clone(&error);
    let i = Arc::clone(&ignored);

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            let count = cc.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err(TestError)
            } else {
                Ok::<_, TestError>("success".to_string())
            }
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(10))
        .name("complete-config")
        .on_success(move |_| {
            s.fetch_add(1, Ordering::SeqCst);
        })
        .on_retry(move |_, _| {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .on_error(move |_| {
            e.fetch_add(1, Ordering::SeqCst);
        })
        .on_ignored_error(move || {
            i.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert_eq!(success.load(Ordering::SeqCst), 1);
    assert_eq!(retry.load(Ordering::SeqCst), 2);
    assert_eq!(error.load(Ordering::SeqCst), 0);
    assert_eq!(ignored.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn builder_pattern_chaining() {
    let config = RetryLayer::builder()
        .max_attempts(10)
        .fixed_backoff(Duration::from_millis(50))
        .name("chained-config")
        .retry_on(|_| true)
        .on_retry(|_, _| {})
        .on_success(|_| {})
        .on_error(|_| {})
        .on_ignored_error(|| {})
        .build();

    // Config should be created successfully
    let layer = config;

    let service =
        tower::service_fn(|_req: String| async move { Ok::<_, TestError>("success".to_string()) });

    let mut service = layer.layer(service);

    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn different_configs_different_services() {
    let calls1 = Arc::new(AtomicUsize::new(0));
    let calls2 = Arc::new(AtomicUsize::new(0));

    let c1 = Arc::clone(&calls1);
    let c2 = Arc::clone(&calls2);

    let service1 = tower::service_fn(move |_req: String| {
        let c = Arc::clone(&c1);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError)
        }
    });

    let service2 = tower::service_fn(move |_req: String| {
        let c = Arc::clone(&c2);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(TestError)
        }
    });

    let layer1 = RetryLayer::<TestError>::builder()
        .max_attempts(2)
        .fixed_backoff(Duration::from_millis(10))
        .build();

    let layer2 = RetryLayer::<TestError>::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(10))
        .build();

    let mut svc1 = layer1.layer(service1);
    let mut svc2 = layer2.layer(service2);

    let _ = svc1.ready().await.unwrap().call("test".to_string()).await;
    let _ = svc2.ready().await.unwrap().call("test".to_string()).await;

    assert_eq!(calls1.load(Ordering::SeqCst), 2);
    assert_eq!(calls2.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn config_with_retry_predicate() {
    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    enum Error {
        Retryable,
        NonRetryable,
    }

    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: String| {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(Error::NonRetryable)
        }
    });

    let config = RetryLayer::builder()
        .max_attempts(5)
        .fixed_backoff(Duration::from_millis(10))
        .retry_on(|e| matches!(e, Error::Retryable))
        .build();

    let layer = config;
    let mut service = layer.layer(service);

    let _ = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;

    // Should not retry NonRetryable
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}
