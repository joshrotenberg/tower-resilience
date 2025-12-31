//! Sliding log rate limiter integration tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_ratelimiter::{RateLimiterLayer, WindowType};

#[tokio::test]
async fn sliding_log_allows_requests_within_limit() {
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
        .window_type(WindowType::SlidingLog)
        .build();

    let mut service = layer.layer(svc);

    for i in 0..10 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok(), "Request {} should succeed", i);
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn sliding_log_rejects_over_limit() {
    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(5)
        .refresh_period(Duration::from_secs(10))
        .timeout_duration(Duration::from_millis(10))
        .window_type(WindowType::SlidingLog)
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
async fn sliding_log_expires_old_requests() {
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
        .window_type(WindowType::SlidingLog)
        .build();

    let mut service = layer.layer(svc);

    // Use all 3 permits
    for i in 0..3 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    // Wait for requests to expire from the sliding window
    tokio::time::sleep(Duration::from_millis(120)).await;

    // Should be able to make 3 more as old ones expired
    for i in 3..6 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok(), "Request {} after expiry should succeed", i);
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn sliding_log_prevents_boundary_burst() {
    // This test demonstrates that sliding log prevents the boundary burst
    // that fixed window allows
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::SeqCst);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(5)
        .refresh_period(Duration::from_millis(100))
        .timeout_duration(Duration::from_millis(10))
        .window_type(WindowType::SlidingLog)
        .build();

    let mut service = layer.layer(svc);

    // Use all 5 permits
    for i in 0..5 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    // Wait only 50ms (half the window) - requests are still in the sliding window
    tokio::time::sleep(Duration::from_millis(50)).await;

    // With sliding log, we should NOT be able to make more requests yet
    // because the 5 requests from 50ms ago are still within the 100ms window
    let result = service.ready().await.unwrap().call(5).await;
    assert!(
        result.is_err(),
        "Sliding log should prevent burst at boundary"
    );

    // Wait for the rest of the window to expire
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Now requests should work
    let result = service.ready().await.unwrap().call(6).await;
    assert!(result.is_ok(), "Should succeed after window expires");
}

#[tokio::test]
async fn sliding_log_rolling_window_behavior() {
    // Test that the window truly slides - requests expire individually
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::SeqCst);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(2)
        .refresh_period(Duration::from_millis(100))
        .timeout_duration(Duration::from_millis(10))
        .window_type(WindowType::SlidingLog)
        .build();

    let mut service = layer.layer(svc);

    // Request 1 at t=0
    assert!(service.ready().await.unwrap().call(0).await.is_ok());

    // Wait 60ms
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Request 2 at t=60ms
    assert!(service.ready().await.unwrap().call(1).await.is_ok());

    // Request 3 should fail (2 requests in last 100ms)
    assert!(service.ready().await.unwrap().call(2).await.is_err());

    // Wait 50ms (t=110ms) - first request should expire
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Request 3 should now succeed (only 1 request in last 100ms)
    assert!(service.ready().await.unwrap().call(3).await.is_ok());

    assert_eq!(call_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn sliding_log_event_listeners() {
    let acquired = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let acq = Arc::clone(&acquired);
    let rej = Arc::clone(&rejected);

    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(2)
        .refresh_period(Duration::from_secs(10))
        .timeout_duration(Duration::from_millis(10))
        .window_type(WindowType::SlidingLog)
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

#[tokio::test]
async fn sliding_log_concurrent_requests() {
    let call_count = Arc::new(AtomicUsize::new(0));

    let counter = Arc::clone(&call_count);
    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::SeqCst);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(10)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_millis(100))
        .window_type(WindowType::SlidingLog)
        .build();

    let service = layer.layer(svc);

    // Spawn 20 concurrent requests
    let mut handles = vec![];
    for i in 0..20 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(i).await
        }));
    }

    let mut success = 0;
    let mut failed = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => success += 1,
            Err(_) => failed += 1,
        }
    }

    // Exactly 10 should succeed
    assert_eq!(success, 10);
    assert_eq!(failed, 10);
    assert_eq!(call_count.load(Ordering::SeqCst), 10);
}
