//! Basic integration tests for tower-timelimiter.
//!
//! These tests verify core functionality by moving existing tests from lib.rs
//! and adding additional integration scenarios.

use super::TestError;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_timelimiter::TimeLimiterConfig;

#[tokio::test]
async fn success_within_timeout() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(100))
        .build();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>("success")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "success");
}

#[tokio::test]
async fn timeout_occurs() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(10))
        .build();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<_, TestError>("success")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());
}

#[tokio::test]
async fn inner_error_propagates() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(100))
        .build();

    let svc = service_fn(|_req: ()| async { Err::<(), _>(TestError("inner error".to_string())) });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(!err.is_timeout());
    assert_eq!(err.into_inner(), Some(TestError("inner error".to_string())));
}

#[tokio::test]
async fn event_listeners_called() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let timeout_count = Arc::new(AtomicUsize::new(0));

    let sc = Arc::clone(&success_count);
    let tc = Arc::clone(&timeout_count);

    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .on_success(move |_| {
            sc.fetch_add(1, Ordering::SeqCst);
        })
        .on_timeout(move || {
            tc.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    // Test success
    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>("ok")
    });
    let mut service = layer.clone().layer(svc);
    let _ = service.ready().await.unwrap().call(()).await;
    assert_eq!(success_count.load(Ordering::SeqCst), 1);

    // Test timeout
    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<_, TestError>("ok")
    });
    let mut service = layer.layer(svc);
    let _ = service.ready().await.unwrap().call(()).await;
    assert_eq!(timeout_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn error_event_listener_called() {
    let error_count = Arc::new(AtomicUsize::new(0));
    let ec = Arc::clone(&error_count);

    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(100))
        .on_error(move |_| {
            ec.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Err::<(), _>(TestError("service error".to_string()))
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    assert_eq!(error_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn multiple_sequential_calls() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .build();

    let svc = service_fn(|req: u32| async move {
        if req.is_multiple_of(2) {
            sleep(Duration::from_millis(10)).await;
            Ok::<_, TestError>(req * 2)
        } else {
            sleep(Duration::from_millis(100)).await;
            Ok(req * 2)
        }
    });

    let mut service = layer.layer(svc);

    // Even request should succeed
    let result = service.ready().await.unwrap().call(2).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 4);

    // Odd request should timeout
    let result = service.ready().await.unwrap().call(3).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());

    // Another even request should succeed
    let result = service.ready().await.unwrap().call(4).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 8);
}

#[tokio::test]
async fn service_cloning_preserves_config() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .build();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>("ok")
    });

    let mut service1 = layer.layer(svc);
    let mut service2 = service1.clone();

    // Both services should work independently
    let result1 = service1.ready().await.unwrap().call(()).await;
    let result2 = service2.ready().await.unwrap().call(()).await;

    assert!(result1.is_ok());
    assert!(result2.is_ok());
}

#[tokio::test]
async fn named_timelimiter() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .name("test-limiter")
        .build();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>("ok")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn instant_success() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(100))
        .build();

    let svc = service_fn(|_req: ()| async { Ok::<_, TestError>("instant") });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "instant");
}

#[tokio::test]
async fn instant_error() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(100))
        .build();

    let svc = service_fn(|_req: ()| async { Err::<(), _>(TestError("instant error".to_string())) });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    assert!(!result.unwrap_err().is_timeout());
}
