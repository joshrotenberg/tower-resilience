use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_bulkhead::{BulkheadConfig, BulkheadError};

#[derive(Debug)]
enum TestError {
    Bulkhead(BulkheadError),
    Other(()),
}

impl From<BulkheadError> for TestError {
    fn from(e: BulkheadError) -> Self {
        TestError::Bulkhead(e)
    }
}

#[tokio::test]
async fn test_permits_released_after_success() {
    let permitted = Arc::new(AtomicUsize::new(0));
    let p = permitted.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(2)
        .name("permit-success-bulkhead")
        .on_call_permitted(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(50)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // First two calls occupy the bulkhead
    let mut s1 = service.clone();
    let h1 = tokio::spawn(async move { s1.ready().await.unwrap().call("first".to_string()).await });

    let mut s2 = service.clone();
    let h2 =
        tokio::spawn(async move { s2.ready().await.unwrap().call("second".to_string()).await });

    // Wait for them to complete
    let r1 = h1.await.unwrap();
    let r2 = h2.await.unwrap();
    assert!(r1.is_ok());
    assert!(r2.is_ok());

    // Now permits should be released, third call should succeed immediately
    let mut s3 = service.clone();
    let r3 = s3.ready().await.unwrap().call("third".to_string()).await;
    assert!(r3.is_ok());

    assert_eq!(permitted.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_permits_released_after_error() {
    let permitted = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    let p = permitted.clone();
    let f = failed.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .name("permit-error-bulkhead")
        .on_call_permitted(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .on_call_failed(move |_| {
            f.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let call_count = Arc::new(AtomicUsize::new(0));
    let c = call_count.clone();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(move |_req: String| {
            let count = c.fetch_add(1, Ordering::SeqCst);
            async move {
                sleep(Duration::from_millis(50)).await;
                if count == 0 {
                    Err(TestError::Other(()))
                } else {
                    Ok("ok".to_string())
                }
            }
        });

    // First call fails
    let mut s1 = service.clone();
    let r1 = s1.ready().await.unwrap().call("first".to_string()).await;
    assert!(matches!(r1, Err(TestError::Other(_))));

    // Permit should be released, second call should succeed
    let mut s2 = service.clone();
    let r2 = s2.ready().await.unwrap().call("second".to_string()).await;
    assert!(r2.is_ok());

    assert_eq!(permitted.load(Ordering::SeqCst), 2);
    assert_eq!(failed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_permits_released_after_panic() {
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .name("permit-panic-bulkhead")
        .build();

    let panic_count = Arc::new(AtomicUsize::new(0));
    let c = panic_count.clone();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(move |_req: String| {
            let count = c.fetch_add(1, Ordering::SeqCst);
            async move {
                if count == 0 {
                    panic!("intentional panic");
                }
                Ok::<_, TestError>("ok".to_string())
            }
        });

    // First call panics
    let mut s1 = service.clone();
    let handle =
        tokio::spawn(async move { s1.ready().await.unwrap().call("first".to_string()).await });

    let result = handle.await;
    assert!(result.is_err()); // Task panicked

    // Wait a bit for cleanup
    sleep(Duration::from_millis(50)).await;

    // Permit should still be released, second call should succeed
    // Note: In Tokio, panics in spawned tasks don't release resources immediately,
    // but the semaphore permit is RAII so it gets released when dropped
    let mut s2 = service.clone();
    let r2 = s2.ready().await.unwrap().call("second".to_string()).await;
    assert!(r2.is_ok());
}

#[tokio::test]
async fn test_permit_reuse() {
    let permitted = Arc::new(AtomicUsize::new(0));
    let p = permitted.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .name("permit-reuse-bulkhead")
        .on_call_permitted(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(10)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // Make 10 sequential calls - each should reuse the same permit
    for i in 0..10 {
        let mut s = service.clone();
        let result = s
            .ready()
            .await
            .unwrap()
            .call(format!("request-{}", i))
            .await;
        assert!(result.is_ok());
    }

    assert_eq!(permitted.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_permits_not_leaked_on_timeout() {
    let permitted = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));

    let p = permitted.clone();
    let r = rejected.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .max_wait_duration(Some(Duration::from_millis(50)))
        .name("no-leak-timeout-bulkhead")
        .on_call_permitted(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
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
    let mut s1 = service.clone();
    let h1 = tokio::spawn(async move { s1.ready().await.unwrap().call("first".to_string()).await });

    sleep(Duration::from_millis(10)).await;

    // Second call times out
    let mut s2 = service.clone();
    let r2 = s2.ready().await.unwrap().call("second".to_string()).await;
    assert!(matches!(
        r2,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));

    // Wait for first call to complete
    let r1 = h1.await.unwrap();
    assert!(r1.is_ok());

    // Third call should succeed (permits not leaked)
    let mut s3 = service.clone();
    let r3 = s3.ready().await.unwrap().call("third".to_string()).await;
    assert!(r3.is_ok());

    assert_eq!(permitted.load(Ordering::SeqCst), 2); // First and third
    assert_eq!(rejected.load(Ordering::SeqCst), 1); // Second
}

#[tokio::test]
async fn test_rapid_permit_acquisition_release() {
    let permitted = Arc::new(AtomicUsize::new(0));
    let p = permitted.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(5)
        .name("rapid-permits-bulkhead")
        .on_call_permitted(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            // Very fast call
            Ok::<_, TestError>("ok".to_string())
        });

    // Make 100 rapid calls
    for i in 0..100 {
        let mut s = service.clone();
        let result = s
            .ready()
            .await
            .unwrap()
            .call(format!("request-{}", i))
            .await;
        assert!(result.is_ok());
    }

    assert_eq!(permitted.load(Ordering::SeqCst), 100);
}

#[tokio::test]
async fn test_permit_fifo_fairness() {
    let order = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let o = order.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(1)
        .name("fifo-bulkhead")
        .on_call_permitted(move |_| {
            // This doesn't help us track order, so we'll track in the service
        })
        .build();

    let order_for_service = order.clone();
    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(move |req: String| {
            let o = order_for_service.clone();
            async move {
                let mut guard = o.lock().await;
                guard.push(req.clone());
                drop(guard);
                sleep(Duration::from_millis(50)).await;
                Ok::<_, TestError>("ok".to_string())
            }
        });

    // Launch 5 calls quickly
    let mut handles = vec![];
    for i in 0..5 {
        let mut s = service.clone();
        let handle = tokio::spawn(async move {
            s.ready()
                .await
                .unwrap()
                .call(format!("request-{}", i))
                .await
        });
        handles.push(handle);
        // Small delay to ensure ordering
        sleep(Duration::from_millis(5)).await;
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    let final_order = o.lock().await;
    // Should be in order (FIFO)
    assert_eq!(final_order.len(), 5);
    for i in 0..5 {
        assert_eq!(final_order[i], format!("request-{}", i));
    }
}

#[tokio::test]
async fn test_concurrent_permit_requests() {
    let permitted = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));

    let p = permitted.clone();
    let f = finished.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(5)
        .name("concurrent-permits-bulkhead")
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
            sleep(Duration::from_millis(50)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // Launch 10 concurrent requests (max 5 concurrent)
    let mut handles = vec![];
    for i in 0..10 {
        let mut s = service.clone();
        let handle = tokio::spawn(async move {
            s.ready()
                .await
                .unwrap()
                .call(format!("request-{}", i))
                .await
        });
        handles.push(handle);
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    assert_eq!(permitted.load(Ordering::SeqCst), 10);
    assert_eq!(finished.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_permits_with_mixed_outcomes() {
    let permitted = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    let p = permitted.clone();
    let fin = finished.clone();
    let fail = failed.clone();

    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(3)
        .name("mixed-outcomes-bulkhead")
        .on_call_permitted(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .on_call_finished(move |_| {
            fin.fetch_add(1, Ordering::SeqCst);
        })
        .on_call_failed(move |_| {
            fail.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let call_count = Arc::new(AtomicUsize::new(0));
    let c = call_count.clone();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(move |_req: String| {
            let count = c.fetch_add(1, Ordering::SeqCst);
            async move {
                sleep(Duration::from_millis(10)).await;
                if count.is_multiple_of(2) {
                    Ok("ok".to_string())
                } else {
                    Err(TestError::Other(()))
                }
            }
        });

    // Make 10 calls with mixed outcomes
    for i in 0..10 {
        let mut s = service.clone();
        let _result = s
            .ready()
            .await
            .unwrap()
            .call(format!("request-{}", i))
            .await;
        // Don't assert on result, we expect mix of success and failure
    }

    assert_eq!(permitted.load(Ordering::SeqCst), 10);
    assert_eq!(finished.load(Ordering::SeqCst), 5); // Even indices
    assert_eq!(failed.load(Ordering::SeqCst), 5); // Odd indices
}

#[tokio::test]
async fn test_starvation_prevention() {
    let layer = BulkheadConfig::builder()
        .max_concurrent_calls(2)
        .name("starvation-prevention-bulkhead")
        .build();

    let service = ServiceBuilder::new()
        .layer(layer)
        .service_fn(|_req: String| async move {
            sleep(Duration::from_millis(100)).await;
            Ok::<_, TestError>("ok".to_string())
        });

    // Launch many concurrent requests
    let mut handles = vec![];
    for i in 0..20 {
        let mut s = service.clone();
        let handle = tokio::spawn(async move {
            s.ready()
                .await
                .unwrap()
                .call(format!("request-{}", i))
                .await
        });
        handles.push(handle);
    }

    // All requests should eventually complete (no starvation)
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "Request should not be starved");
    }
}
