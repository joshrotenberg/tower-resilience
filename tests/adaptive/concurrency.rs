//! Concurrency tests for adaptive limiter.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd, Vegas};

#[tokio::test]
async fn test_concurrent_requests_within_limit() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<_, &str>(())
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(20)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(service);

    // Spawn concurrent requests within the limit
    let mut handles = vec![];
    for _ in 0..10 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(()).await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_concurrent_requests_exceeding_limit() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, &str>(())
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(5)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(service);

    // Spawn more concurrent requests than the limit
    let mut handles = vec![];
    for _ in 0..20 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(()).await
        }));
    }

    // All should eventually complete (either immediately or after waiting)
    let mut successes = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            successes += 1;
        }
    }

    // All requests should succeed eventually
    assert_eq!(successes, 20);
}

#[tokio::test]
async fn test_vegas_concurrent_requests() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<_, &str>(())
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Vegas::builder().initial_limit(10).build(),
        ))
        .service(service);

    let mut handles = vec![];
    for _ in 0..15 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(()).await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 15);
}

#[tokio::test]
async fn test_concurrent_mixed_success_and_failure() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |req: u32| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
            if req.is_multiple_of(3) {
                Err("divisible by 3")
            } else {
                Ok(req)
            }
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(10)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(service);

    let mut handles = vec![];
    for i in 0..15 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(i).await
        }));
    }

    let mut successes = 0;
    let mut failures = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => successes += 1,
            Err(_) => failures += 1,
        }
    }

    // 0, 3, 6, 9, 12 are divisible by 3 = 5 failures
    assert_eq!(failures, 5);
    assert_eq!(successes, 10);
    assert_eq!(call_count.load(Ordering::SeqCst), 15);
}

#[tokio::test]
async fn test_high_concurrency_stress() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |_req: ()| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(1)).await;
            Ok::<_, &str>(())
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(50)
                .max_limit(100)
                .latency_threshold(Duration::from_secs(1))
                .build(),
        ))
        .service(service);

    let mut handles = vec![];
    for _ in 0..200 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(()).await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 200);
}

#[tokio::test]
async fn test_limit_adapts_under_varying_load() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let cc = Arc::clone(&call_count);

    let service = tower::service_fn(move |slow: bool| {
        let count = cc.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            if slow {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Ok::<_, &str>(())
        }
    });

    let service = ServiceBuilder::new()
        .layer(AdaptiveLimiterLayer::new(
            Aimd::builder()
                .initial_limit(10)
                .latency_threshold(Duration::from_millis(50))
                .build(),
        ))
        .service(service);

    // Phase 1: Fast requests
    let mut handles = vec![];
    for _ in 0..10 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(false).await
        }));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // Phase 2: Slow requests (should trigger limit decrease)
    let mut handles = vec![];
    for _ in 0..5 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(true).await
        }));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // Phase 3: Fast again
    let mut handles = vec![];
    for _ in 0..10 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(false).await
        }));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 25);
}
