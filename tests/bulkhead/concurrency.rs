//! P0 tests for concurrent request handling with bulkhead.

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
async fn high_concurrency_stress_test() {
    let concurrent_counter = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let max_allowed = 10;

    let counter_clone = Arc::clone(&concurrent_counter);
    let max_clone = Arc::clone(&max_concurrent);

    let service = ServiceBuilder::new()
        .layer(
            BulkheadConfig::builder()
                .max_concurrent_calls(max_allowed)
                .build(),
        )
        .service_fn(move |_req: ()| {
            let counter = Arc::clone(&counter_clone);
            let max = Arc::clone(&max_clone);
            async move {
                let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(current, Ordering::SeqCst);
                sleep(Duration::from_millis(10)).await;
                counter.fetch_sub(1, Ordering::SeqCst);
                Ok::<_, TestError>(())
            }
        });

    let mut handles = vec![];
    for _ in 0..100 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(
            async move { svc.ready().await?.call(()).await },
        ));
    }

    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    let actual_max = max_concurrent.load(Ordering::SeqCst);
    assert!(
        actual_max <= max_allowed,
        "Max concurrent {} exceeded limit {}",
        actual_max,
        max_allowed
    );
    assert_eq!(concurrent_counter.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn concurrent_requests_respect_limit() {
    let concurrent_counter = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let max_allowed = 3;

    let counter_clone = Arc::clone(&concurrent_counter);
    let max_clone = Arc::clone(&max_concurrent);

    let service = ServiceBuilder::new()
        .layer(
            BulkheadConfig::builder()
                .max_concurrent_calls(max_allowed)
                .build(),
        )
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

    let actual_max = max_concurrent.load(Ordering::SeqCst);
    assert!(
        actual_max <= max_allowed,
        "Max concurrent {} exceeded limit {}",
        actual_max,
        max_allowed
    );
}

#[tokio::test]
async fn rejection_under_load_with_timeout() {
    let service = ServiceBuilder::new()
        .layer(
            BulkheadConfig::builder()
                .max_concurrent_calls(2)
                .max_wait_duration(Some(Duration::from_millis(10)))
                .build(),
        )
        .service_fn(|_req: ()| async {
            sleep(Duration::from_millis(200)).await;
            Ok::<_, TestError>(())
        });

    let mut svc1 = service.clone();
    let mut svc2 = service.clone();
    let handle1 = tokio::spawn(async move { svc1.ready().await?.call(()).await });
    let handle2 = tokio::spawn(async move { svc2.ready().await?.call(()).await });

    sleep(Duration::from_millis(20)).await;

    let mut rejected = 0;
    for _ in 0..10 {
        let mut svc = service.clone();
        let result = svc.ready().await.unwrap().call(()).await;
        if matches!(result, Err(TestError::Bulkhead(BulkheadError::Timeout))) {
            rejected += 1;
        }
    }

    assert!(rejected > 0, "Expected some requests to be rejected");

    handle1.await.unwrap().unwrap();
    handle2.await.unwrap().unwrap();
}

#[tokio::test]
async fn single_permit_bulkhead_serializes_requests() {
    let concurrent_counter = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));

    let counter_clone = Arc::clone(&concurrent_counter);
    let max_clone = Arc::clone(&max_concurrent);

    let service = ServiceBuilder::new()
        .layer(BulkheadConfig::builder().max_concurrent_calls(1).build())
        .service_fn(move |_req: ()| {
            let counter = Arc::clone(&counter_clone);
            let max = Arc::clone(&max_clone);
            async move {
                let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(current, Ordering::SeqCst);
                sleep(Duration::from_millis(10)).await;
                counter.fetch_sub(1, Ordering::SeqCst);
                Ok::<_, TestError>(())
            }
        });

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

    assert_eq!(max_concurrent.load(Ordering::SeqCst), 1);
    assert_eq!(concurrent_counter.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn mixed_fast_and_slow_requests() {
    let fast_count = Arc::new(AtomicUsize::new(0));
    let slow_count = Arc::new(AtomicUsize::new(0));

    let fast_clone = Arc::clone(&fast_count);
    let slow_clone = Arc::clone(&slow_count);

    let service = ServiceBuilder::new()
        .layer(BulkheadConfig::builder().max_concurrent_calls(5).build())
        .service_fn(move |req: bool| {
            let fast = Arc::clone(&fast_clone);
            let slow = Arc::clone(&slow_clone);
            async move {
                if req {
                    sleep(Duration::from_millis(5)).await;
                    fast.fetch_add(1, Ordering::SeqCst);
                } else {
                    sleep(Duration::from_millis(50)).await;
                    slow.fetch_add(1, Ordering::SeqCst);
                }
                Ok::<_, TestError>(())
            }
        });

    let mut handles = vec![];

    for _ in 0..5 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(
            async move { svc.ready().await?.call(false).await },
        ));
    }

    for _ in 0..20 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(
            async move { svc.ready().await?.call(true).await },
        ));
    }

    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    assert_eq!(fast_count.load(Ordering::SeqCst), 20);
    assert_eq!(slow_count.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn burst_traffic_pattern() {
    let processed = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let concurrent = Arc::new(AtomicUsize::new(0));

    let proc_clone = Arc::clone(&processed);
    let max_clone = Arc::clone(&max_concurrent);
    let conc_clone = Arc::clone(&concurrent);

    let service = ServiceBuilder::new()
        .layer(BulkheadConfig::builder().max_concurrent_calls(5).build())
        .service_fn(move |_req: ()| {
            let proc = Arc::clone(&proc_clone);
            let max = Arc::clone(&max_clone);
            let conc = Arc::clone(&conc_clone);
            async move {
                let current = conc.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(current, Ordering::SeqCst);
                sleep(Duration::from_millis(10)).await;
                conc.fetch_sub(1, Ordering::SeqCst);
                proc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, TestError>(())
            }
        });

    for _ in 0..3 {
        let mut handles = vec![];
        for _ in 0..15 {
            let mut svc = service.clone();
            handles.push(tokio::spawn(
                async move { svc.ready().await?.call(()).await },
            ));
        }

        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        sleep(Duration::from_millis(20)).await;
    }

    assert_eq!(processed.load(Ordering::SeqCst), 45);
    assert!(max_concurrent.load(Ordering::SeqCst) <= 5);
    assert_eq!(concurrent.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn concurrent_with_varied_delays() {
    let service = ServiceBuilder::new()
        .layer(BulkheadConfig::builder().max_concurrent_calls(10).build())
        .service_fn(|delay_ms: u64| async move {
            sleep(Duration::from_millis(delay_ms)).await;
            Ok::<_, TestError>(delay_ms)
        });

    let mut handles = vec![];
    let delays = vec![5, 10, 15, 20, 25, 30, 35, 40, 45, 50];

    for &delay in &delays {
        let mut svc = service.clone();
        handles.push(tokio::spawn(
            async move { svc.ready().await?.call(delay).await },
        ));
    }

    let mut results = vec![];
    for handle in handles {
        results.push(handle.await.unwrap().unwrap());
    }

    assert_eq!(results.len(), delays.len());
    for (i, &result) in results.iter().enumerate() {
        assert_eq!(result, delays[i]);
    }
}

#[tokio::test]
async fn zero_concurrent_immediately_rejects() {
    let service = ServiceBuilder::new()
        .layer(
            BulkheadConfig::builder()
                .max_concurrent_calls(0)
                .max_wait_duration(Some(Duration::from_millis(10)))
                .build(),
        )
        .service_fn(|_req: ()| async { Ok::<_, TestError>(()) });

    let mut svc = service.clone();
    let result = svc.ready().await.unwrap().call(()).await;

    assert!(matches!(
        result,
        Err(TestError::Bulkhead(BulkheadError::Timeout))
    ));
}
