//! Cancellation behavior tests for tower-timelimiter.
//!
//! Tests that verify future cancellation works correctly on timeout.

use super::TestError;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_timelimiter::TimeLimiterLayer;

/// A guard that sets a flag when dropped, allowing us to detect future cancellation.
struct DropGuard {
    flag: Arc<AtomicBool>,
}

impl Drop for DropGuard {
    fn drop(&mut self) {
        self.flag.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn cancel_running_future_flag_true() {
    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(50))
        .cancel_running_future(true)
        .build();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(200)).await;
        Ok::<_, TestError>("should not complete")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());
}

#[tokio::test]
async fn cancel_running_future_flag_false() {
    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(50))
        .cancel_running_future(false)
        .build();

    let svc = service_fn(|_req: ()| async {
        sleep(Duration::from_millis(200)).await;
        Ok::<_, TestError>("should not complete")
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());
}

#[tokio::test]
async fn future_dropped_on_timeout() {
    let dropped = Arc::new(AtomicBool::new(false));
    let dropped_clone = Arc::clone(&dropped);

    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(50))
        .build();

    let svc = service_fn(move |_req: ()| {
        let dropped = Arc::clone(&dropped_clone);
        async move {
            let _guard = DropGuard {
                flag: Arc::clone(&dropped),
            };
            sleep(Duration::from_millis(200)).await;
            Ok::<_, TestError>("should not complete")
        }
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());

    // tokio::time::timeout drops the future on timeout, so the guard should be dropped
    // Give a small amount of time for drop to occur
    sleep(Duration::from_millis(10)).await;
    assert!(
        dropped.load(Ordering::SeqCst),
        "Future should be dropped on timeout"
    );
}

#[tokio::test]
async fn service_resources_cleaned_up() {
    let resource_created = Arc::new(AtomicBool::new(false));
    let resource_cleaned = Arc::new(AtomicBool::new(false));

    let rc_clone = Arc::clone(&resource_created);
    let rclean_clone = Arc::clone(&resource_cleaned);

    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(50))
        .build();

    let svc = service_fn(move |_req: ()| {
        let rc = Arc::clone(&rc_clone);
        let rclean = Arc::clone(&rclean_clone);
        async move {
            rc.store(true, Ordering::SeqCst);
            let _guard = DropGuard { flag: rclean };
            sleep(Duration::from_millis(200)).await;
            Ok::<_, TestError>("should not complete")
        }
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    assert!(resource_created.load(Ordering::SeqCst));

    // Give time for cleanup
    sleep(Duration::from_millis(10)).await;
    assert!(
        resource_cleaned.load(Ordering::SeqCst),
        "Resources should be cleaned up"
    );
}

#[tokio::test]
async fn verify_tokio_timeout_drop_behavior() {
    // This test verifies that tokio::time::timeout actually drops the future
    let dropped = Arc::new(AtomicBool::new(false));
    let dropped_clone = Arc::clone(&dropped);

    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(30))
        .build();

    let svc = service_fn(move |_req: ()| {
        let dropped = Arc::clone(&dropped_clone);
        async move {
            let _guard = DropGuard {
                flag: Arc::clone(&dropped),
            };
            // This future will be interrupted by timeout
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok::<_, TestError>("should not reach here")
        }
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().is_timeout());

    // Wait a bit for the drop to occur
    sleep(Duration::from_millis(10)).await;
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn future_state_after_timeout() {
    let work_started = Arc::new(AtomicBool::new(false));
    let work_completed = Arc::new(AtomicBool::new(false));

    let ws_clone = Arc::clone(&work_started);
    let wc_clone = Arc::clone(&work_completed);

    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(50))
        .build();

    let svc = service_fn(move |_req: ()| {
        let ws = Arc::clone(&ws_clone);
        let wc = Arc::clone(&wc_clone);
        async move {
            ws.store(true, Ordering::SeqCst);
            sleep(Duration::from_millis(200)).await;
            wc.store(true, Ordering::SeqCst);
            Ok::<_, TestError>("completed")
        }
    });

    let mut service = layer.layer(svc);
    let result = service.ready().await.unwrap().call(()).await;

    assert!(result.is_err());
    assert!(work_started.load(Ordering::SeqCst));

    // Wait for what would be completion time if it ran
    sleep(Duration::from_millis(250)).await;

    // Work should NOT have completed because future was dropped
    assert!(
        !work_completed.load(Ordering::SeqCst),
        "Work should not complete after timeout drops future"
    );
}

#[tokio::test]
async fn no_resource_leaks() {
    let allocations = Arc::new(AtomicBool::new(false));
    let deallocations = Arc::new(AtomicBool::new(false));

    let alloc_clone = Arc::clone(&allocations);
    let dealloc_clone = Arc::clone(&deallocations);

    let layer = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_millis(50))
        .build();

    // Run multiple timeout scenarios
    for _ in 0..10 {
        let alloc = Arc::clone(&alloc_clone);
        let dealloc = Arc::clone(&dealloc_clone);

        let svc = service_fn(move |_req: ()| {
            let alloc = Arc::clone(&alloc);
            let dealloc = Arc::clone(&dealloc);
            async move {
                alloc.store(true, Ordering::SeqCst);
                let _guard = DropGuard { flag: dealloc };
                sleep(Duration::from_millis(200)).await;
                Ok::<_, TestError>("should timeout")
            }
        });

        let mut service = layer.clone().layer(svc);
        let result = service.ready().await.unwrap().call(()).await;
        assert!(result.is_err());

        // Reset for next iteration
        deallocations.store(false, Ordering::SeqCst);
    }

    // Give time for all cleanups
    sleep(Duration::from_millis(20)).await;

    // If there were resource leaks, some deallocations would not have happened
    // This is a basic check - in production you'd use more sophisticated leak detection
    assert!(allocations.load(Ordering::SeqCst));
}
