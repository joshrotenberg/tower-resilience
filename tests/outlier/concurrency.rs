//! Concurrency tests for outlier detection.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_outlier::{OutlierDetectionLayer, OutlierDetector};

#[tokio::test]
async fn concurrent_failures_eject_correctly() {
    let detector = OutlierDetector::new().max_ejection_percent(100);
    detector.register("backend-1", 5);

    let layer = OutlierDetectionLayer::builder()
        .detector(detector.clone())
        .instance_name("backend-1")
        .error_on_ejection()
        .build();

    let fail_svc =
        tower::util::BoxCloneService::new(tower::service_fn(|_req: String| async move {
            Err::<String, _>(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "down",
            ))
        }));

    let svc = layer.layer(fail_svc);

    // Send 10 concurrent failing requests
    let mut handles = vec![];
    for i in 0..10 {
        let mut s = svc.clone();
        handles.push(tokio::spawn(async move {
            s.ready().await.unwrap().call(format!("req-{i}")).await
        }));
    }

    let mut inner_errors = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => {}
            Err(e) => {
                if e.is_inner() {
                    inner_errors += 1;
                }
            }
        }
    }

    // Should have some inner errors (before ejection)
    assert!(inner_errors > 0, "should have inner errors");
    assert!(detector.is_ejected("backend-1"));
}

#[tokio::test]
async fn concurrent_successes_prevent_ejection() {
    let detector = OutlierDetector::new().max_ejection_percent(100);
    detector.register("backend-1", 100);

    let success_count = Arc::new(AtomicUsize::new(0));
    let sc = Arc::clone(&success_count);

    let layer = OutlierDetectionLayer::builder()
        .detector(detector.clone())
        .instance_name("backend-1")
        .error_on_ejection()
        .build();

    let ok_svc = tower::util::BoxCloneService::new(tower::service_fn(move |req: String| {
        let c = Arc::clone(&sc);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(req)
        }
    }));

    let svc = layer.layer(ok_svc);

    let mut handles = vec![];
    for i in 0..50 {
        let mut s = svc.clone();
        handles.push(tokio::spawn(async move {
            s.ready()
                .await
                .unwrap()
                .call(format!("req-{i}"))
                .await
                .unwrap()
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert!(!detector.is_ejected("backend-1"));
    assert_eq!(success_count.load(Ordering::SeqCst), 50);
}

#[tokio::test]
async fn concurrent_multi_instance_isolation() {
    let detector = OutlierDetector::new().max_ejection_percent(100);
    detector.register("backend-1", 3);
    detector.register("backend-2", 3);

    // Backend-1 gets failures, backend-2 gets successes
    let mut handles = vec![];
    for _ in 0..5 {
        let d = detector.clone();
        handles.push(tokio::spawn(async move {
            d.record_failure("backend-1");
        }));
    }
    for _ in 0..5 {
        let d = detector.clone();
        handles.push(tokio::spawn(async move {
            d.record_success("backend-2");
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert!(detector.is_ejected("backend-1"));
    assert!(!detector.is_ejected("backend-2"));
}

#[tokio::test]
async fn recovery_under_concurrent_load() {
    let detector = OutlierDetector::new()
        .base_ejection_duration(Duration::from_millis(50))
        .max_ejection_percent(100);
    detector.register("backend-1", 1);

    // Eject
    detector.record_failure("backend-1");
    assert!(detector.is_ejected("backend-1"));

    // Wait for recovery
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Concurrent checks should all see recovered state
    let mut handles = vec![];
    for _ in 0..20 {
        let d = detector.clone();
        handles.push(tokio::spawn(async move { d.is_ejected("backend-1") }));
    }

    for handle in handles {
        let ejected = handle.await.unwrap();
        assert!(!ejected, "should be recovered");
    }
}
