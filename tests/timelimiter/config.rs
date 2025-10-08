//! Configuration validation tests for tower-timelimiter.
//!
//! Tests that verify configuration options work correctly.

use super::TestError;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_timelimiter::TimeLimiterConfig;

#[tokio::test]
async fn duration_zero_config() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::ZERO)
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        // tokio::time::timeout with Duration::ZERO allows one poll
        Ok::<_, TestError>("instant")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    // Duration::ZERO with tokio allows instant responses - this documents the behavior
    assert!(result.is_ok());
}

#[tokio::test]
async fn duration_max_config() {
    // Use a reasonable substitute for Duration::MAX (1 hour)
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_secs(3600))
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>("completes quickly")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "completes quickly");
}

#[tokio::test]
async fn default_builder_values() {
    let config = TimeLimiterConfig::builder().build();

    // Verify defaults from the builder
    let layer = config.layer();
    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>("ok")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    // Should succeed with default 5 second timeout
    assert!(result.is_ok());
}

#[tokio::test]
async fn all_config_options_combined() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));
    let timeout_count = Arc::new(AtomicUsize::new(0));

    let sc = Arc::clone(&success_count);
    let ec = Arc::clone(&error_count);
    let tc = Arc::clone(&timeout_count);

    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .cancel_running_future(true)
        .name("test-limiter")
        .on_success(move |_| {
            sc.fetch_add(1, Ordering::SeqCst);
        })
        .on_error(move |_| {
            ec.fetch_add(1, Ordering::SeqCst);
        })
        .on_timeout(move || {
            tc.fetch_add(1, Ordering::SeqCst);
        })
        .build()
        .layer();

    // Test success
    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>("success")
    });
    let mut service = layer.clone().layer(svc);
    let result = service.ready().await.unwrap().call(()).await;
    assert!(result.is_ok());
    assert_eq!(success_count.load(Ordering::SeqCst), 1);

    // Test error
    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Err::<(), _>(TestError("error".to_string()))
    });
    let mut service = layer.clone().layer(svc);
    let result = service.ready().await.unwrap().call(()).await;
    assert!(result.is_err());
    assert_eq!(error_count.load(Ordering::SeqCst), 1);

    // Test timeout
    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<_, TestError>("timeout")
    });
    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;
    assert!(result.is_err());
    assert_eq!(timeout_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn event_listeners_work() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));
    let timeout_count = Arc::new(AtomicUsize::new(0));

    let sc = Arc::clone(&success_count);
    let ec = Arc::clone(&error_count);
    let tc = Arc::clone(&timeout_count);

    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .on_success(move |duration| {
            sc.fetch_add(1, Ordering::SeqCst);
            assert!(duration.as_millis() < 50);
        })
        .on_error(move |duration| {
            ec.fetch_add(1, Ordering::SeqCst);
            assert!(duration.as_millis() < 50);
        })
        .on_timeout(move || {
            tc.fetch_add(1, Ordering::SeqCst);
        })
        .build()
        .layer();

    // Test success event
    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>("ok")
    });
    let mut service = layer.clone().layer(svc);
    let _ = service.ready().await.unwrap().call(()).await;
    assert_eq!(success_count.load(Ordering::SeqCst), 1);

    // Test error event
    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Err::<(), _>(TestError("error".to_string()))
    });
    let mut service = layer.clone().layer(svc);
    let _ = service.ready().await.unwrap().call(()).await;
    assert_eq!(error_count.load(Ordering::SeqCst), 1);

    // Test timeout event
    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<_, TestError>("timeout")
    });
    let mut service = layer.layer(svc);
    let _ = service.ready().await.unwrap().call(()).await;
    assert_eq!(timeout_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn name_configuration() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .name("my-custom-limiter")
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>("ok")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn multiple_event_listeners() {
    let count1 = Arc::new(AtomicUsize::new(0));
    let count2 = Arc::new(AtomicUsize::new(0));
    let count3 = Arc::new(AtomicUsize::new(0));

    let c1 = Arc::clone(&count1);
    let c2 = Arc::clone(&count2);
    let c3 = Arc::clone(&count3);

    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .on_success(move |_| {
            c1.fetch_add(1, Ordering::SeqCst);
        })
        .on_success(move |_| {
            c2.fetch_add(1, Ordering::SeqCst);
        })
        .on_success(move |_| {
            c3.fetch_add(1, Ordering::SeqCst);
        })
        .build()
        .layer();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(10)).await;
        Ok::<_, TestError>("ok")
    });

    let mut service = layer.layer(svc);
    let _ = service.ready().await.unwrap().call(()).await;

    // All three listeners should have been called
    assert_eq!(count1.load(Ordering::SeqCst), 1);
    assert_eq!(count2.load(Ordering::SeqCst), 1);
    assert_eq!(count3.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancel_running_future_true_config() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .cancel_running_future(true)
        .build()
        .layer();
    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<_, TestError>("should timeout")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());
}

#[tokio::test]
async fn cancel_running_future_false_config() {
    let layer = TimeLimiterConfig::builder()
        .timeout_duration(Duration::from_millis(50))
        .cancel_running_future(false)
        .build()
        .layer();
    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(100)).await;
        Ok::<_, TestError>("should timeout")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());
}
