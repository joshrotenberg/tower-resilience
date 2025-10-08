//! Timeout precision tests for tower-timelimiter.
//!
//! Tests that verify timeout behavior is accurate and handles edge cases correctly.

use super::TestError;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_timelimiter::TimeLimiterConfig;

// Windows has less precise timers, so use larger tolerance
const TOLERANCE_MS: u64 = 30;

#[tokio::test]
async fn timeout_fires_at_correct_time() {
    let timeout_duration = Duration::from_millis(50);
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(timeout_duration)
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(200)).await;
        Ok::<_, TestError>("should not complete")
    });

    let mut service = layer.layer(svc);
    let start = Instant::now();
    let result = service.ready().await.unwrap().call(()).await;
    let elapsed = start.elapsed();

    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());

    // Verify timeout occurred within tolerance
    let diff = elapsed.abs_diff(timeout_duration);
    assert!(
        diff.as_millis() <= TOLERANCE_MS as u128,
        "Timeout accuracy outside tolerance: expected ~{}ms, got {}ms (diff: {}ms)",
        timeout_duration.as_millis(),
        elapsed.as_millis(),
        diff.as_millis()
    );
}

#[tokio::test]
async fn duration_zero_immediate_timeout() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::ZERO)
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        // tokio::time::timeout with Duration::ZERO allows one poll,
        // so an instant response actually succeeds
        Ok::<_, TestError>("instant")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    // With tokio::time::timeout, Duration::ZERO allows instant responses to succeed
    // This documents the actual behavior of tokio's timeout implementation
    assert!(result.is_ok());
}

#[tokio::test]
async fn very_short_timeout_1ms() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(1))
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(50)).await;
        Ok::<_, TestError>("should timeout")
    });

    let mut service = layer.layer(svc);
    let start = Instant::now();
    let result = service.ready().await.unwrap().call(()).await;
    let elapsed = start.elapsed();

    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());
    assert!(elapsed.as_millis() < 50);
}

#[tokio::test]
async fn very_short_timeout_10ms() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(10))
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<_, TestError>("should timeout")
    });

    let mut service = layer.layer(svc);
    let start = Instant::now();
    let result = service.ready().await.unwrap().call(()).await;
    let elapsed = start.elapsed();

    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());
    assert!(elapsed.as_millis() < 50);
}

#[tokio::test]
async fn very_long_timeout() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_secs(60))
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>("completes quickly")
    });

    let mut service = layer.layer(svc);
    let start = Instant::now();
    let result = service.ready().await.unwrap().call(()).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    assert!(elapsed.as_millis() < 100);
}

#[tokio::test]
async fn timeout_exactly_at_service_completion() {
    // This test verifies behavior when timeout and service complete at approximately the same time
    let timeout_duration = Duration::from_millis(50);
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(timeout_duration)
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(50)).await;
        Ok::<_, TestError>("completes at timeout boundary")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    // Either success or timeout is acceptable at the boundary
    // This test documents the behavior without being flaky
    match result {
        Ok(_) => {
            // Service completed just before timeout
        }
        Err(e) => {
            // Timeout occurred at the same time or just before completion
            assert!(e.is_timeout());
        }
    }
}

#[tokio::test]
async fn timeout_just_before_completion() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(30))
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(50)).await;
        Ok::<_, TestError>("should timeout before completion")
    });

    let mut service = layer.layer(svc);
    let start = Instant::now();
    let result = service.ready().await.unwrap().call(()).await;
    let elapsed = start.elapsed();

    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());
    // Windows has less precise timers, allow more margin
    assert!(
        elapsed.as_millis() < 60,
        "Expected timeout ~30ms, got {}ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn timeout_just_after_completion() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(70))
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(50)).await;
        Ok::<_, TestError>("should complete before timeout")
    });

    let mut service = layer.layer(svc);
    let start = Instant::now();
    let result = service.ready().await.unwrap().call(()).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    assert!(elapsed.as_millis() < 70);
}

#[tokio::test]
async fn multiple_different_timeout_durations() {
    let timeouts = vec![
        Duration::from_millis(10),
        Duration::from_millis(50),
        Duration::from_millis(100),
        Duration::from_millis(200),
    ];

    for timeout in timeouts {
        let layer = TimeLimiterConfig::builder()
            .timeout_duration(timeout)
            .build()
            .layer();

        // Service that takes 75ms
        let svc = service_fn(|_req: ()| async {
            sleep(Duration::from_millis(75)).await;
            Ok::<_, TestError>("response")
        });

        let mut service = layer.layer(svc);
        let result = service.ready().await.unwrap().call(()).await;

        if timeout.as_millis() < 75 {
            assert!(
                result.is_err(),
                "Should timeout with {}ms timeout",
                timeout.as_millis()
            );
            assert!(result.unwrap_err().is_timeout());
        } else {
            assert!(
                result.is_ok(),
                "Should succeed with {}ms timeout",
                timeout.as_millis()
            );
        }
    }
}
