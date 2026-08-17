//! Comparison tests between different window types

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_ratelimiter::{RateLimiterLayer, WindowType};

/// Helper to create a rate limiter with a specific window type
fn create_limiter(window_type: WindowType, limit: usize, period_ms: u64) -> RateLimiterLayer {
    RateLimiterLayer::builder()
        .limit_for_period(limit)
        .refresh_period(Duration::from_millis(period_ms))
        .timeout_duration(Duration::from_millis(10))
        .window_type(window_type)
        .build()
}

#[tokio::test]
async fn all_window_types_enforce_limit() {
    // All window types should enforce the same limit for immediate requests
    for window_type in [
        WindowType::Fixed,
        WindowType::SlidingLog,
        WindowType::SlidingCounter,
    ] {
        let call_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&call_count);

        let svc = tower::service_fn(move |_req: u32| {
            counter.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, std::io::Error>(()) }
        });

        let layer = create_limiter(window_type, 5, 1000);
        let mut service = layer.layer(svc);

        let mut success = 0;
        for i in 0..10 {
            if service.ready().await.unwrap().call(i).await.is_ok() {
                success += 1;
            }
        }

        assert_eq!(
            success, 5,
            "{:?} should allow exactly 5 requests",
            window_type
        );
        assert_eq!(call_count.load(Ordering::SeqCst), 5);
    }
}

#[tokio::test]
async fn fixed_window_refreshes_while_sliding_log_limits_recent_requests() {
    // A fixed window refreshes all permits after its period. A sliding log
    // continues to reject requests while the recorded requests are recent.

    // Test Fixed Window - should allow burst
    {
        let call_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&call_count);

        let svc = tower::service_fn(move |_req: u32| {
            counter.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, std::io::Error>(()) }
        });

        let layer = create_limiter(WindowType::Fixed, 5, 100);
        let mut service = layer.layer(svc);

        // Use all 5 permits
        for i in 0..5 {
            assert!(service.ready().await.unwrap().call(i).await.is_ok());
        }

        // Wait for boundary
        tokio::time::sleep(Duration::from_millis(110)).await;

        // Should get 5 more immediately
        for i in 5..10 {
            assert!(
                service.ready().await.unwrap().call(i).await.is_ok(),
                "Fixed window should allow burst at boundary"
            );
        }

        assert_eq!(call_count.load(Ordering::SeqCst), 10);
    }

    // Test Sliding Log - should NOT allow burst
    {
        let call_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&call_count);

        let svc = tower::service_fn(move |_req: u32| {
            counter.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, std::io::Error>(()) }
        });

        // Use a deliberately long window here. A 100ms window made this test
        // depend on CI scheduler latency: a sufficiently delayed task could
        // cross the whole window before the assertion ran.
        let layer = create_limiter(WindowType::SlidingLog, 5, 10_000);
        let mut service = layer.layer(svc);

        // Use all 5 permits
        for i in 0..5 {
            assert!(service.ready().await.unwrap().call(i).await.is_ok());
        }

        // Requests were just recorded, so the sliding log must reject another
        // request without relying on a wall-clock sleep or a narrow boundary.
        let result = service.ready().await.unwrap().call(5).await;
        assert!(
            result.is_err(),
            "Sliding log should prevent burst at boundary"
        );
    }
}

#[tokio::test]
async fn default_is_fixed_window() {
    // Verify default window type is Fixed
    let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });

    let layer = RateLimiterLayer::builder()
        .limit_for_period(5)
        .refresh_period(Duration::from_millis(100))
        .timeout_duration(Duration::from_millis(10))
        // No .window_type() call - should default to Fixed
        .build();

    let mut service = layer.layer(svc);

    // Use all 5 permits
    for i in 0..5 {
        assert!(service.ready().await.unwrap().call(i).await.is_ok());
    }

    // Wait for boundary
    tokio::time::sleep(Duration::from_millis(110)).await;

    // Fixed window behavior: should get 5 more immediately
    for i in 5..10 {
        assert!(
            service.ready().await.unwrap().call(i).await.is_ok(),
            "Default (Fixed) should allow requests after refresh"
        );
    }
}

#[tokio::test]
async fn window_type_can_be_changed() {
    // Verify each window type can be explicitly set
    for window_type in [
        WindowType::Fixed,
        WindowType::SlidingLog,
        WindowType::SlidingCounter,
    ] {
        let svc = tower::service_fn(|_req: u32| async { Ok::<_, std::io::Error>(()) });

        let layer = RateLimiterLayer::builder()
            .limit_for_period(3)
            .refresh_period(Duration::from_secs(1))
            .timeout_duration(Duration::from_millis(10))
            .window_type(window_type)
            .build();

        let mut service = layer.layer(svc);

        // All should work for basic case
        let mut success = 0;
        for i in 0..5 {
            if service.ready().await.unwrap().call(i).await.is_ok() {
                success += 1;
            }
        }

        assert_eq!(success, 3, "{:?} should allow 3 requests", window_type);
    }
}
