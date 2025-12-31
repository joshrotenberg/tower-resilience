//! Fixed window rate limiter integration tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_ratelimiter::{RateLimiterLayer, WindowType};

#[tokio::test]
async fn fixed_window_allows_requests_within_limit() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::SeqCst);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(10)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_millis(50))
        .window_type(WindowType::Fixed)
        .build();

    let mut service = layer.layer(svc);

    for i in 0..10 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok(), "Request {} should succeed", i);
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn fixed_window_rejects_over_limit() {
    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(5)
        .refresh_period(Duration::from_secs(10))
        .timeout_duration(Duration::from_millis(10))
        .window_type(WindowType::Fixed)
        .build();

    let mut service = layer.layer(svc);

    // First 5 should succeed
    for i in 0..5 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok(), "Request {} should succeed", i);
    }

    // 6th should fail
    let result = service.ready().await.unwrap().call(5).await;
    assert!(result.is_err(), "Request 6 should be rejected");
}

#[tokio::test]
async fn fixed_window_refreshes_permits() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::SeqCst);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(3)
        .refresh_period(Duration::from_millis(100))
        .timeout_duration(Duration::from_millis(50))
        .window_type(WindowType::Fixed)
        .build();

    let mut service = layer.layer(svc);

    // Use all 3 permits
    for i in 0..3 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    // Wait for refresh
    tokio::time::sleep(Duration::from_millis(120)).await;

    // Should be able to make 3 more
    for i in 3..6 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok(), "Request {} after refresh should succeed", i);
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn fixed_window_allows_burst_at_boundary() {
    // This test demonstrates the fixed window boundary burst behavior
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::SeqCst);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(5)
        .refresh_period(Duration::from_millis(100))
        .timeout_duration(Duration::from_millis(50))
        .window_type(WindowType::Fixed)
        .build();

    let mut service = layer.layer(svc);

    // Use all 5 permits near end of window
    for i in 0..5 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    // Wait for window boundary
    tokio::time::sleep(Duration::from_millis(110)).await;

    // Use all 5 permits at start of new window
    for i in 5..10 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    // 10 requests in ~110ms - this is the "burst" behavior
    assert_eq!(call_count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn fixed_window_event_listeners() {
    let acquired = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let acq = Arc::clone(&acquired);
    let rej = Arc::clone(&rejected);

    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(2)
        .refresh_period(Duration::from_secs(10))
        .timeout_duration(Duration::from_millis(10))
        .window_type(WindowType::Fixed)
        .on_permit_acquired(move |_| {
            acq.fetch_add(1, Ordering::SeqCst);
        })
        .on_permit_rejected(move |_| {
            rej.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let mut service = layer.layer(svc);

    // 2 should succeed, 1 should fail
    for i in 0..3 {
        let _ = service.ready().await.unwrap().call(i).await;
    }

    assert_eq!(acquired.load(Ordering::SeqCst), 2);
    assert_eq!(rejected.load(Ordering::SeqCst), 1);
}
