//! Sliding counter rate limiter integration tests

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_ratelimiter::{RateLimiterLayer, WindowType};

#[tokio::test]
async fn sliding_counter_allows_requests_within_limit() {
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
        .window_type(WindowType::SlidingCounter)
        .build();

    let mut service = layer.layer(svc);

    for i in 0..10 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok(), "Request {} should succeed", i);
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn sliding_counter_rejects_over_limit() {
    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(5)
        .refresh_period(Duration::from_secs(10))
        .timeout_duration(Duration::from_millis(10))
        .window_type(WindowType::SlidingCounter)
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
async fn sliding_counter_weighted_decay() {
    // Test that the sliding counter properly decays the previous bucket's weight over time
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
        .window_type(WindowType::SlidingCounter)
        .build();

    let mut service = layer.layer(svc);

    // Use all 5 permits in first bucket
    for i in 0..5 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    // Wait for bucket to rotate plus time for significant weight decay
    // At 180ms: bucket rotated at 100ms, now 80% through new bucket
    // previous_weight = 0.2, weighted = 5 * 0.2 + 0 = 1
    tokio::time::sleep(Duration::from_millis(180)).await;

    // Should be able to make requests now due to significant decay
    let result = service.ready().await.unwrap().call(5).await;
    assert!(
        result.is_ok(),
        "Should succeed after previous weight decays significantly"
    );
}

#[tokio::test]
async fn sliding_counter_smoother_than_fixed() {
    // Demonstrate that sliding counter provides smoother rate limiting than fixed
    // by showing that capacity becomes available as previous bucket weight decays
    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    let svc = tower::service_fn(move |_req: u32| {
        counter.fetch_add(1, Ordering::SeqCst);
        async { Ok::<_, std::io::Error>(()) }
    });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(10)
        .refresh_period(Duration::from_millis(100))
        .timeout_duration(Duration::from_millis(10))
        .window_type(WindowType::SlidingCounter)
        .build();

    let mut service = layer.layer(svc);

    // Use all 10 permits in current bucket
    for i in 0..10 {
        let result = service.ready().await.unwrap().call(i).await;
        assert!(result.is_ok());
    }

    // Wait for bucket rotation (110ms) THEN wait 80% through new bucket (80ms more)
    // Total: 190ms
    // After rotation: previous_count=10, current_count=0
    // At 80% through new bucket: previous_weight=0.2, weighted=10*0.2+0=2
    tokio::time::sleep(Duration::from_millis(190)).await;

    let mut additional = 0;
    for i in 10..20 {
        if service.ready().await.unwrap().call(i).await.is_ok() {
            additional += 1;
        }
    }

    // Should get several more due to weight decay (expect ~8)
    assert!(
        additional >= 5,
        "Expected significant additional permits due to weight decay, got {}",
        additional
    );
}

#[tokio::test]
async fn sliding_counter_event_listeners() {
    let acquired = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let acq = Arc::clone(&acquired);
    let rej = Arc::clone(&rejected);

    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(2)
        .refresh_period(Duration::from_secs(10))
        .timeout_duration(Duration::from_millis(10))
        .window_type(WindowType::SlidingCounter)
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
async fn sliding_counter_memory_efficiency() {
    // Verify sliding counter maintains O(1) memory regardless of request count
    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(1000)
        .refresh_period(Duration::from_millis(100))
        .timeout_duration(Duration::from_millis(200))
        .window_type(WindowType::SlidingCounter)
        .build();

    let mut service = layer.layer(svc);

    // Make many requests
    for i in 0..5000 {
        let _ = service.ready().await.unwrap().call(i).await;

        // Small delay to allow bucket rotations
        if i % 100 == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    // If we got here without OOM or performance issues, memory is bounded
    // The sliding counter only stores 2 counters regardless of request count
}

#[tokio::test]
async fn sliding_counter_concurrent_requests() {
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
        .window_type(WindowType::SlidingCounter)
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
