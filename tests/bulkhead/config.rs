use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_bulkhead::{BulkheadConfig, BulkheadError};

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
async fn test_max_concurrent_calls_one() {
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .name("single-call-bulkhead")
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(100)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // First call should succeed
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

    // Second call should be blocked (no timeout, so it waits)
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
    // Second call had to wait for first to complete
    assert!(elapsed >= Duration::from_millis(90));
}

#[tokio::test]
async fn test_max_concurrent_calls_large_value() {
    let large_limit = 1000;
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(large_limit)
        .name("large-bulkhead")
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(10)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // Launch 100 concurrent calls (well under the limit)
    let mut handles = vec![];
    for i in 0..100 {
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

    // All should succeed without blocking
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn test_max_concurrent_calls_zero() {
    // Zero concurrent calls means all calls should be rejected immediately
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(0)
        .max_wait_duration(Some(Duration::from_millis(10)))
        .name("zero-capacity-bulkhead")
        .build();

    let mut service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move { Ok::<_, TestError>("ok".to_string()) });

    // Any call should timeout immediately
    let result = service
        .ready()
        .await
        .unwrap()
        .call("request".to_string())
        .await;
    assert!(matches!(
        result,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));
}

#[tokio::test]
async fn test_default_config() {
    // Test that default config works
    let layer = BulkheadConfig::builder().build();

    let mut service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move { Ok::<_, TestError>("ok".to_string()) });

    let result = service
        .ready()
        .await
        .unwrap()
        .call("request".to_string())
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_config_with_event_listeners() {
    let permitted_count = Arc::new(AtomicUsize::new(0));
    let finished_count = Arc::new(AtomicUsize::new(0));

    let p = permitted_count.clone();
    let f = finished_count.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(5)
        .name("event-listener-bulkhead")
        .on_call_permitted(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .on_call_finished(move |_| {
            f.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(10)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // Make 3 calls
    for i in 0..3 {
        let mut svc = service.clone();
        let result = svc
            .ready()
            .await
            .unwrap()
            .call(format!("request-{}", i))
            .await;
        assert!(result.is_ok());
    }

    assert_eq!(permitted_count.load(Ordering::SeqCst), 3);
    assert_eq!(finished_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_config_with_multiple_event_listeners() {
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));

    let c1 = counter1.clone();
    let c2 = counter2.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(5)
        .name("multi-listener-bulkhead")
        .on_call_permitted(move |_| {
            c1.fetch_add(1, Ordering::SeqCst);
        })
        .on_call_permitted(move |_| {
            c2.fetch_add(10, Ordering::SeqCst);
        })
        .build();

    let mut service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move { Ok::<_, TestError>("ok".to_string()) });

    let result = service
        .ready()
        .await
        .unwrap()
        .call("request".to_string())
        .await;
    assert!(result.is_ok());

    assert_eq!(counter1.load(Ordering::SeqCst), 1);
    assert_eq!(counter2.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_config_timeout_some_vs_none() {
    // Config with Some timeout
    let layer_some = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(50)))
        .name("timeout-some")
        .build();

    // Config with None timeout
    let layer_none = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(None)
        .name("timeout-none")
        .build();

    let service_some =
        ServiceBuilder::new()
            .layer(layer_some)
            .service_fn(|_req: String| async move {
                sleep(Duration::from_millis(100)).await;
                Ok::<_, TestError>("ok".to_string())
            });

    let service_none =
        ServiceBuilder::new()
            .layer(layer_none)
            .service_fn(|_req: String| async move {
                sleep(Duration::from_millis(100)).await;
                Ok::<_, TestError>("ok".to_string())
            });

    // Occupy both bulkheads
    let mut s1 = service_some.clone();
    let _h1 =
        tokio::spawn(async move { s1.ready().await.unwrap().call("first".to_string()).await });

    let mut s2 = service_none.clone();
    let _h2 =
        tokio::spawn(async move { s2.ready().await.unwrap().call("first".to_string()).await });

    sleep(Duration::from_millis(10)).await;

    // Some timeout should reject
    let mut s3 = service_some.clone();
    let result_some = s3.ready().await.unwrap().call("second".to_string()).await;
    assert!(matches!(
        result_some,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));

    // None timeout should wait and succeed
    let mut s4 = service_none.clone();
    let result_none = s4.ready().await.unwrap().call("second".to_string()).await;
    assert!(result_none.is_ok());
}

#[tokio::test]
async fn test_builder_pattern_chaining() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();

    // Test that all builder methods can be chained
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(10)
        .max_wait_duration(Some(Duration::from_secs(1)))
        .name("chained-bulkhead")
        .on_call_permitted(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        })
        .on_call_rejected(|_| {})
        .on_call_finished(|_| {})
        .on_call_failed(|_| {})
        .build();

    let mut service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move { Ok::<_, TestError>("ok".to_string()) });

    let result = service
        .ready()
        .await
        .unwrap()
        .call("request".to_string())
        .await;
    assert!(result.is_ok());
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_config_independent_instances() {
    // Create two independent bulkhead configs
    let layer1 = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .name("bulkhead-1")
        .build();

    let layer2 = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .name("bulkhead-2")
        .build();

    let service1 = ServiceBuilder::new()
        .layer(layer1)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(100)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    let service2 = ServiceBuilder::new()
        .layer(layer2)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(100)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // Both services can handle calls independently
    let mut s1 = service1.clone();
    let handle1 = tokio::spawn(async move {
        s1.ready()
            .await
            .unwrap()
            .call("request-1".to_string())
            .await
    });

    let mut s2 = service2.clone();
    let handle2 = tokio::spawn(async move {
        s2.ready()
            .await
            .unwrap()
            .call("request-2".to_string())
            .await
    });

    let result1 = handle1.await.unwrap();
    let result2 = handle2.await.unwrap();

    assert!(result1.is_ok());
    assert!(result2.is_ok());
}

#[tokio::test]
async fn test_config_name_normal() {
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(5)
        .name("my-service-bulkhead")
        .build();

    let mut service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move { Ok::<_, TestError>("ok".to_string()) });

    let result = service
        .ready()
        .await
        .unwrap()
        .call("request".to_string())
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_config_name_empty() {
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(5)
        .name("")
        .build();

    let mut service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move { Ok::<_, TestError>("ok".to_string()) });

    let result = service
        .ready()
        .await
        .unwrap()
        .call("request".to_string())
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_config_name_very_long() {
    let long_name = "a".repeat(1000);
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(5)
        .name(long_name)
        .build();

    let mut service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move { Ok::<_, TestError>("ok".to_string()) });

    let result = service
        .ready()
        .await
        .unwrap()
        .call("request".to_string())
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_config_with_all_options() {
    let permitted = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    let p = permitted.clone();
    let r = rejected.clone();
    let fin = finished.clone();
    let fail = failed.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(2)
        .max_wait_duration(Some(Duration::from_millis(50)))
        .name("comprehensive-bulkhead")
        .on_call_permitted(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .on_call_rejected(move |_| {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .on_call_finished(move |_| {
            fin.fetch_add(1, Ordering::SeqCst);
        })
        .on_call_failed(move |_| {
            fail.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(100)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // Fill the bulkhead
    let mut s1 = service.clone();
    let _h1 =
        tokio::spawn(async move { s1.ready().await.unwrap().call("first".to_string()).await });

    let mut s2 = service.clone();
    let _h2 =
        tokio::spawn(async move { s2.ready().await.unwrap().call("second".to_string()).await });

    sleep(Duration::from_millis(10)).await;

    // This should be rejected due to timeout
    let mut s3 = service.clone();
    let result = s3.ready().await.unwrap().call("third".to_string()).await;
    assert!(matches!(
        result,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));

    // Wait for initial calls to complete
    sleep(Duration::from_millis(150)).await;

    assert_eq!(permitted.load(Ordering::SeqCst), 2);
    assert_eq!(rejected.load(Ordering::SeqCst), 1);
    assert_eq!(finished.load(Ordering::SeqCst), 2);
    assert_eq!(failed.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_config_different_timeouts() {
    // Test multiple configs with different timeout values
    let timeouts = [
        Duration::from_millis(10),
        Duration::from_millis(50),
        Duration::from_millis(100),
        Duration::from_millis(500),
    ];

    for (idx, timeout) in timeouts.iter().enumerate() {
        let layer = BulkheadConfig::builder()
            .max_concurrent_calls(5)
            .max_wait_duration(Some(*timeout))
            .name(format!("bulkhead-{}", idx))
            .build();

        let mut service = ServiceBuilder::new()
            .layer(layer)
            .service_fn(|_req: String| async move { Ok::<_, TestError>("ok".to_string()) });

        let result = service
            .ready()
            .await
            .unwrap()
            .call("request".to_string())
            .await;
        assert!(result.is_ok());
    }
}
