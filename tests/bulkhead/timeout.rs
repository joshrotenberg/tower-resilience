use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_bulkhead::{BulkheadConfig, BulkheadError};

#[derive(Debug)]
enum TestError {
    Bulkhead(BulkheadError),
}

impl From<BulkheadError> for TestError {
    fn from(e: BulkheadError) -> Self {
        TestError::Bulkhead(e)
    }
}

#[tokio::test]
async fn test_zero_timeout() {
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::ZERO))
        .name("zero-timeout-bulkhead")
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(100)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // First call occupies the bulkhead
    let mut service1 = service.clone();
    let handle1 = tokio::spawn(async move {
        service1
            .ready()
            .await
            .unwrap()
            .call("first".to_string())
            .await
    });

    // Wait a bit to ensure first call starts
    sleep(Duration::from_millis(10)).await;

    // Second call should timeout immediately since timeout is zero and bulkhead is full
    let mut service2 = service.clone();
    let handle2 = tokio::spawn(async move {
        service2
            .ready()
            .await
            .unwrap()
            .call("second".to_string())
            .await
    });

    let result1 = handle1.await.unwrap();
    let result2 = handle2.await.unwrap();

    assert!(result1.is_ok());
    assert!(matches!(
        result2,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));
}

#[tokio::test]
async fn test_very_short_timeout() {
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(1)))
        .name("short-timeout-bulkhead")
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(100)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // First call occupies the bulkhead
    let mut service1 = service.clone();
    let handle1 = tokio::spawn(async move {
        service1
            .ready()
            .await
            .unwrap()
            .call("first".to_string())
            .await
    });

    // Wait a bit to ensure first call starts
    sleep(Duration::from_millis(10)).await;

    // Second call should timeout very quickly
    let mut service2 = service.clone();
    let handle2 = tokio::spawn(async move {
        let start = std::time::Instant::now();
        let result = service2
            .ready()
            .await
            .unwrap()
            .call("second".to_string())
            .await;
        let elapsed = start.elapsed();
        (result, elapsed)
    });

    let result1 = handle1.await.unwrap();
    let (result2, elapsed) = handle2.await.unwrap();

    assert!(result1.is_ok());
    assert!(matches!(
        result2,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));
    assert!(elapsed < Duration::from_millis(50)); // Should timeout quickly
}

#[tokio::test]
async fn test_long_timeout() {
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_secs(5)))
        .name("long-timeout-bulkhead")
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(50)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // First call occupies the bulkhead briefly
    let mut service1 = service.clone();
    let handle1 = tokio::spawn(async move {
        service1
            .ready()
            .await
            .unwrap()
            .call("first".to_string())
            .await
    });

    // Wait a bit to ensure first call starts
    sleep(Duration::from_millis(10)).await;

    // Second call should wait and succeed when first completes
    let mut service2 = service.clone();
    let start = std::time::Instant::now();
    let handle2 = tokio::spawn(async move {
        service2
            .ready()
            .await
            .unwrap()
            .call("second".to_string())
            .await
    });

    let result1 = handle1.await.unwrap();
    let result2 = handle2.await.unwrap();
    let elapsed = start.elapsed();

    assert!(result1.is_ok());
    assert!(result2.is_ok());
    assert!(elapsed < Duration::from_secs(5)); // Should not timeout
}

#[tokio::test]
async fn test_no_timeout_none() {
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(None) // No timeout
        .name("no-timeout-bulkhead")
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(50)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // First call occupies the bulkhead
    let mut service1 = service.clone();
    let handle1 = tokio::spawn(async move {
        service1
            .ready()
            .await
            .unwrap()
            .call("first".to_string())
            .await
    });

    sleep(Duration::from_millis(10)).await;

    // Second call waits indefinitely until first completes
    let mut service2 = service.clone();
    let handle2 = tokio::spawn(async move {
        service2
            .ready()
            .await
            .unwrap()
            .call("second".to_string())
            .await
    });

    let result1 = handle1.await.unwrap();
    let result2 = handle2.await.unwrap();

    assert!(result1.is_ok());
    assert!(result2.is_ok());
}

#[tokio::test]
async fn test_timeout_precision() {
    let timeout_duration = Duration::from_millis(100);
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(timeout_duration))
        .name("precision-timeout-bulkhead")
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(200)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // First call occupies the bulkhead
    let mut service1 = service.clone();
    let _handle1 = tokio::spawn(async move {
        service1
            .ready()
            .await
            .unwrap()
            .call("first".to_string())
            .await
    });

    sleep(Duration::from_millis(10)).await;

    // Second call should timeout at approximately 100ms
    let mut service2 = service.clone();
    let start = std::time::Instant::now();
    let handle2 = tokio::spawn(async move {
        service2
            .ready()
            .await
            .unwrap()
            .call("second".to_string())
            .await
    });

    let result = handle2.await.unwrap();
    let elapsed = start.elapsed();

    assert!(matches!(
        result,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));
    // Allow some margin for timing precision
    assert!(elapsed >= Duration::from_millis(90));
    assert!(elapsed <= Duration::from_millis(150));
}

#[tokio::test]
async fn test_multiple_timeouts() {
    let rejections = Arc::new(AtomicUsize::new(0));
    let r = rejections.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(50)))
        .name("multiple-timeout-bulkhead")
        .on_call_rejected(move |_| {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(200)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // First call occupies the bulkhead
    let mut service1 = service.clone();
    let _handle1 = tokio::spawn(async move {
        service1
            .ready()
            .await
            .unwrap()
            .call("first".to_string())
            .await
    });

    sleep(Duration::from_millis(10)).await;

    // Launch multiple calls that should all timeout
    let mut handles = vec![];
    for i in 0..5 {
        let mut svc = service.clone();
        let handle = tokio::spawn(async move {
            svc.ready()
                .await
                .unwrap()
                .call(format!("request-{}", i))
                .await
        });
        handles.push(handle);
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(matches!(
            result,
            Err(TestError::Bulkhead(BulkheadError::Timeout))
        ));
    }

    assert_eq!(rejections.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn test_timeout_then_success() {
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(50)))
        .name("timeout-success-bulkhead")
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(100)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // First call occupies the bulkhead
    let mut service1 = service.clone();
    let handle1 = tokio::spawn(async move {
        service1
            .ready()
            .await
            .unwrap()
            .call("first".to_string())
            .await
    });

    sleep(Duration::from_millis(10)).await;

    // Second call should timeout
    let mut service2 = service.clone();
    let handle2 = tokio::spawn(async move {
        service2
            .ready()
            .await
            .unwrap()
            .call("second".to_string())
            .await
    });

    let result1 = handle1.await.unwrap();
    let result2 = handle2.await.unwrap();

    assert!(result1.is_ok());
    assert!(matches!(
        result2,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));

    // Now the bulkhead should be available again
    sleep(Duration::from_millis(50)).await;

    let mut service3 = service.clone();
    let result3 = service3
        .ready()
        .await
        .unwrap()
        .call("third".to_string())
        .await;
    assert!(result3.is_ok());
}

#[tokio::test]
async fn test_concurrent_timeouts() {
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(2)
        .max_wait_duration(Some(Duration::from_millis(50)))
        .name("concurrent-timeout-bulkhead")
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(200)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // Fill bulkhead with 2 concurrent calls
    let mut service1 = service.clone();
    let _handle1 = tokio::spawn(async move {
        service1
            .ready()
            .await
            .unwrap()
            .call("first".to_string())
            .await
    });

    let mut service2 = service.clone();
    let _handle2 = tokio::spawn(async move {
        service2
            .ready()
            .await
            .unwrap()
            .call("second".to_string())
            .await
    });

    sleep(Duration::from_millis(10)).await;

    // Third call should timeout
    let mut service3 = service.clone();
    let result3 = service3
        .ready()
        .await
        .unwrap()
        .call("third".to_string())
        .await;
    assert!(matches!(
        result3,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));
}

#[tokio::test]
async fn test_timeout_boundary_conditions() {
    // Test with max duration (approximately 2 minutes)
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_secs(120)))
        .name("boundary-timeout-bulkhead")
        .build();

    let mut service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(10)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // Should succeed quickly
    let result = service
        .ready()
        .await
        .unwrap()
        .call("request".to_string())
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_changing_timeout_behavior() {
    // Create two bulkheads with different timeouts
    let short_timeout = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(10)))
        .name("short-timeout")
        .build();

    let long_timeout = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(200)))
        .name("long-timeout")
        .build();

    let service_short =
        ServiceBuilder::new()
            .layer(short_timeout)
            .service_fn(|_req: String| async move {
                sleep(Duration::from_millis(100)).await;
                Ok::<_, TestError>("ok".to_string())
            });

    let service_long =
        ServiceBuilder::new()
            .layer(long_timeout)
            .service_fn(|_req: String| async move {
                sleep(Duration::from_millis(100)).await;
                Ok::<_, TestError>("ok".to_string())
            });

    // Occupy both bulkheads
    let mut s1 = service_short.clone();
    let _h1 =
        tokio::spawn(async move { s1.ready().await.unwrap().call("first".to_string()).await });

    let mut s2 = service_long.clone();
    let _h2 =
        tokio::spawn(async move { s2.ready().await.unwrap().call("first".to_string()).await });

    sleep(Duration::from_millis(5)).await;

    // Short timeout should fail
    let mut s3 = service_short.clone();
    let result_short = s3.ready().await.unwrap().call("second".to_string()).await;
    assert!(matches!(
        result_short,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));

    // Long timeout should succeed (waits for first call to complete)
    let mut s4 = service_long.clone();
    let result_long = s4.ready().await.unwrap().call("second".to_string()).await;
    assert!(result_long.is_ok());
}
