//! Basic integration tests for outlier detection.

use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_outlier::{
    OutlierDetectionLayer, OutlierDetectionServiceError, OutlierDetector,
};

fn make_ok_svc() -> tower::util::BoxCloneService<String, String, std::io::Error> {
    tower::util::BoxCloneService::new(tower::service_fn(|req: String| async move {
        Ok::<_, std::io::Error>(req)
    }))
}

fn make_fail_svc() -> tower::util::BoxCloneService<String, String, std::io::Error> {
    tower::util::BoxCloneService::new(tower::service_fn(|_req: String| async move {
        Err::<String, _>(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "down",
        ))
    }))
}

#[tokio::test]
async fn healthy_instance_passes_through() {
    let detector = OutlierDetector::new();
    detector.register("backend-1", 5);

    let layer = OutlierDetectionLayer::builder()
        .detector(detector)
        .instance_name("backend-1")
        .error_on_ejection()
        .build();

    let mut svc = layer.layer(make_ok_svc());

    let resp = svc.ready().await.unwrap().call("hello".into()).await;
    assert!(resp.is_ok());
    assert_eq!(resp.unwrap(), "hello");
}

#[tokio::test]
async fn consecutive_errors_trigger_ejection() {
    let detector = OutlierDetector::new().max_ejection_percent(100);
    detector.register("backend-1", 3);

    let layer = OutlierDetectionLayer::builder()
        .detector(detector.clone())
        .instance_name("backend-1")
        .error_on_ejection()
        .build();

    let mut svc = layer.layer(make_fail_svc());

    // 3 consecutive errors should trigger ejection
    for _ in 0..3 {
        let resp = svc.ready().await.unwrap().call("x".into()).await;
        assert!(resp.is_err());
        assert!(resp.unwrap_err().is_inner());
    }

    // Instance should now be ejected
    assert!(detector.is_ejected("backend-1"));

    // Next call should be rejected by outlier detection
    let resp = svc.ready().await.unwrap().call("x".into()).await;
    assert!(resp.is_err());
    assert!(resp.unwrap_err().is_outlier_detection());
}

#[tokio::test]
async fn success_resets_consecutive_error_count() {
    let detector = OutlierDetector::new().max_ejection_percent(100);
    detector.register("backend-1", 3);

    // Send 2 errors
    detector.record_failure("backend-1");
    detector.record_failure("backend-1");

    // One success resets the counter
    detector.record_success("backend-1");

    // 2 more errors should NOT eject (reset happened)
    assert!(!detector.record_failure("backend-1"));
    assert!(!detector.record_failure("backend-1"));
    assert!(!detector.is_ejected("backend-1"));

    // 3rd error in this new sequence triggers ejection
    assert!(detector.record_failure("backend-1"));
    assert!(detector.is_ejected("backend-1"));
}

#[tokio::test]
async fn auto_recovery_after_ejection_duration() {
    let detector = OutlierDetector::new()
        .base_ejection_duration(Duration::from_millis(50))
        .max_ejection_percent(100);
    detector.register("backend-1", 1);

    // Trigger ejection
    assert!(detector.record_failure("backend-1"));
    assert!(detector.is_ejected("backend-1"));

    // Wait for ejection to expire
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should auto-recover on next check
    assert!(!detector.is_ejected("backend-1"));
}

#[tokio::test]
async fn exponential_backoff_on_repeated_ejections() {
    let detector = OutlierDetector::new()
        .base_ejection_duration(Duration::from_millis(50))
        .max_ejection_percent(100);
    detector.register("backend-1", 1);

    // First ejection: 50ms
    assert!(detector.record_failure("backend-1"));
    assert!(detector.is_ejected("backend-1"));

    // Wait for recovery
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(!detector.is_ejected("backend-1"));

    // Second ejection: 100ms (50 * 2^1)
    assert!(detector.record_failure("backend-1"));
    assert!(detector.is_ejected("backend-1"));

    // After 60ms, still ejected (needs 100ms)
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(detector.is_ejected("backend-1"));

    // After total 110ms, recovered
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!detector.is_ejected("backend-1"));
}

#[tokio::test]
async fn max_ejection_duration_caps_backoff() {
    let detector = OutlierDetector::new()
        .base_ejection_duration(Duration::from_millis(50))
        .max_ejection_duration(Duration::from_millis(100))
        .max_ejection_percent(100);
    detector.register("backend-1", 1);

    // First ejection: 50ms
    detector.record_failure("backend-1");
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(!detector.is_ejected("backend-1"));

    // Second ejection: 100ms (50 * 2, capped at 100)
    detector.record_failure("backend-1");
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(detector.is_ejected("backend-1"));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!detector.is_ejected("backend-1"));

    // Third ejection: still 100ms (50 * 4 = 200, capped at 100)
    detector.record_failure("backend-1");
    tokio::time::sleep(Duration::from_millis(110)).await;
    assert!(!detector.is_ejected("backend-1"));
}

#[tokio::test]
async fn error_on_ejection_returns_outlier_error() {
    let detector = OutlierDetector::new().max_ejection_percent(100);
    detector.register("backend-1", 1);

    let layer = OutlierDetectionLayer::builder()
        .detector(detector.clone())
        .instance_name("backend-1")
        .error_on_ejection()
        .build();

    let mut svc = layer.layer(make_fail_svc());

    // First call triggers ejection
    let _ = svc.ready().await.unwrap().call("x".into()).await;

    // Second call returns OutlierDetection error
    let resp = svc.ready().await.unwrap().call("x".into()).await;
    match resp {
        Err(OutlierDetectionServiceError::OutlierDetection(e)) => {
            assert_eq!(
                e.to_string(),
                "instance 'backend-1' is ejected by outlier detection"
            );
        }
        other => panic!("expected OutlierDetection error, got {:?}", other),
    }
}

#[tokio::test]
async fn event_listeners_fire_on_ejection_and_recovery() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let ejection_count = Arc::new(AtomicUsize::new(0));
    let recovery_count = Arc::new(AtomicUsize::new(0));

    let ec = Arc::clone(&ejection_count);
    let rc = Arc::clone(&recovery_count);

    let detector = OutlierDetector::new()
        .base_ejection_duration(Duration::from_millis(50))
        .max_ejection_percent(100)
        .on_ejection(move |_name, _errors| {
            ec.fetch_add(1, Ordering::SeqCst);
        })
        .on_recovery(move |_name, _duration| {
            rc.fetch_add(1, Ordering::SeqCst);
        });

    detector.register("backend-1", 1);

    // Trigger ejection
    detector.record_failure("backend-1");
    assert_eq!(ejection_count.load(Ordering::SeqCst), 1);

    // Wait for recovery
    tokio::time::sleep(Duration::from_millis(60)).await;
    detector.is_ejected("backend-1"); // triggers recovery check
    assert_eq!(recovery_count.load(Ordering::SeqCst), 1);
}
