//! Integration tests for bulkhead pattern.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_bulkhead::{BulkheadConfig, BulkheadError};

#[derive(Debug)]
enum TestError {
    Bulkhead(BulkheadError),
    #[allow(dead_code)]
    Io(std::io::Error),
}

impl From<BulkheadError> for TestError {
    fn from(e: BulkheadError) -> Self {
        TestError::Bulkhead(e)
    }
}

impl From<std::io::Error> for TestError {
    fn from(e: std::io::Error) -> Self {
        TestError::Io(e)
    }
}

#[tokio::test]
async fn test_bulkhead_limits_concurrency() {
    let concurrent_counter = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));

    let counter_clone = Arc::clone(&concurrent_counter);
    let max_clone = Arc::clone(&max_concurrent);

    let service = ServiceBuilder::new()
        .layer(BulkheadConfig::builder().max_concurrent_calls(5).build())
        .service_fn(move |_req: ()| {
            let counter = Arc::clone(&counter_clone);
            let max = Arc::clone(&max_clone);
            async move {
                let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(current, Ordering::SeqCst);
                sleep(Duration::from_millis(50)).await;
                counter.fetch_sub(1, Ordering::SeqCst);
                Ok::<_, TestError>(())
            }
        });

    // Spawn 20 concurrent requests
    let mut handles = vec![];
    for _ in 0..20 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(
            async move { svc.ready().await?.call(()).await },
        ));
    }

    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // Max concurrent should never exceed 5
    assert!(max_concurrent.load(Ordering::SeqCst) <= 5);
}

#[tokio::test]
async fn test_bulkhead_rejects_when_full_with_timeout() {
    let service = ServiceBuilder::new()
        .layer(
            BulkheadConfig::builder()
                .max_concurrent_calls(2)
                .max_wait_duration(Some(Duration::from_millis(10)))
                .build(),
        )
        .service_fn(|_req: ()| async {
            sleep(Duration::from_millis(100)).await;
            Ok::<_, TestError>(())
        });

    // Start 2 requests that will block
    let mut svc1 = service.clone();
    let mut svc2 = service.clone();
    let handle1 = tokio::spawn(async move { svc1.ready().await?.call(()).await });
    let handle2 = tokio::spawn(async move { svc2.ready().await?.call(()).await });

    // Give them time to acquire permits
    sleep(Duration::from_millis(10)).await;

    // Third request should timeout
    let mut svc3 = service.clone();
    let result = svc3.ready().await.unwrap().call(()).await;
    assert!(matches!(
        result,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));

    // Clean up
    handle1.await.unwrap().unwrap();
    handle2.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_bulkhead_event_listeners() {
    let permitted_count = Arc::new(AtomicUsize::new(0));
    let finished_count = Arc::new(AtomicUsize::new(0));

    let p_clone = Arc::clone(&permitted_count);
    let f_clone = Arc::clone(&finished_count);

    let service = ServiceBuilder::new()
        .layer(
            BulkheadConfig::builder()
                .max_concurrent_calls(5)
                .on_call_permitted(move |_| {
                    p_clone.fetch_add(1, Ordering::SeqCst);
                })
                .on_call_finished(move |_| {
                    f_clone.fetch_add(1, Ordering::SeqCst);
                })
                .build(),
        )
        .service_fn(|_req: ()| async { Ok::<_, TestError>(()) });

    let mut handles = vec![];
    for _ in 0..10 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(
            async move { svc.ready().await?.call(()).await },
        ));
    }

    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    assert_eq!(permitted_count.load(Ordering::SeqCst), 10);
    assert_eq!(finished_count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_bulkhead_releases_on_error() {
    let concurrent_counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&concurrent_counter);

    let service = ServiceBuilder::new()
        .layer(BulkheadConfig::builder().max_concurrent_calls(2).build())
        .service_fn(move |_req: ()| {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                sleep(Duration::from_millis(10)).await;
                let current = counter.fetch_sub(1, Ordering::SeqCst);
                Err::<(), _>(TestError::Io(std::io::Error::other(format!(
                    "error at {}",
                    current
                ))))
            }
        });

    // Spawn multiple requests that will all fail
    let mut handles = vec![];
    for _ in 0..5 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(
            async move { svc.ready().await?.call(()).await },
        ));
    }

    for handle in handles {
        assert!(handle.await.unwrap().is_err());
    }

    // All permits should be released
    sleep(Duration::from_millis(50)).await;
    assert_eq!(concurrent_counter.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_bulkhead_without_timeout_waits() {
    let service = ServiceBuilder::new()
        .layer(
            BulkheadConfig::builder()
                .max_concurrent_calls(1)
                .max_wait_duration(None) // Wait indefinitely
                .build(),
        )
        .service_fn(|_req: ()| async {
            sleep(Duration::from_millis(50)).await;
            Ok::<_, TestError>(())
        });

    // Start first request
    let mut svc1 = service.clone();
    let handle1 = tokio::spawn(async move { svc1.ready().await?.call(()).await });

    // Give it time to acquire permit
    sleep(Duration::from_millis(10)).await;

    // Second request should wait and succeed eventually
    let mut svc2 = service.clone();
    let start = std::time::Instant::now();
    let result = svc2.ready().await.unwrap().call(()).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    // Should have waited at least 40ms (first request takes 50ms, we waited 10ms)
    assert!(elapsed >= Duration::from_millis(40));

    handle1.await.unwrap().unwrap();
}
