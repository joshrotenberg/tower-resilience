//! Outlier detection stress tests - shared fleet state under load
//!
//! `OutlierDetector` holds fleet-wide ejection state behind an internal
//! `Arc<Mutex<...>>` shared across every per-instance service. These tests push
//! concurrent traffic through a fleet and assert that ejection, the
//! `max_ejection_percent` cap, and time-based readmission stay consistent under
//! load.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tower::util::BoxCloneService;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_outlier::{
    OutlierDetectionLayer, OutlierDetectionServiceError, OutlierDetector,
};

/// Uniform, cloneable per-instance service type for a fleet. We box so every
/// instance shares one type and can be stored in a `Vec` and cloned per task.
type FleetService = BoxCloneService<usize, usize, OutlierDetectionServiceError<std::io::Error>>;

/// Builds one fleet instance sharing `detector`, in error-on-ejection mode so
/// ejected instances surface an explicit error instead of parking `poll_ready`
/// (which would hang the driving loop).
fn make_instance(detector: OutlierDetector, name: &str, healthy: Arc<AtomicBool>) -> FleetService {
    let inner = tower::service_fn(move |req: usize| {
        let healthy = Arc::clone(&healthy);
        async move {
            if healthy.load(Ordering::Relaxed) {
                Ok::<usize, std::io::Error>(req)
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "down",
                ))
            }
        }
    });

    let layer = OutlierDetectionLayer::builder()
        .detector(detector)
        .instance_name(name)
        .error_on_ejection()
        .build();

    BoxCloneService::new(layer.layer(inner))
}

/// Test: Healthy fleet sustains high concurrent throughput with no ejections.
#[tokio::test]
#[ignore]
async fn stress_healthy_fleet_throughput() {
    let fleet_size = 10;
    let detector = OutlierDetector::new().max_ejection_percent(100);

    let mut fleet = Vec::with_capacity(fleet_size);
    for i in 0..fleet_size {
        let name = format!("backend-{}", i);
        detector.register(&name, 5);
        fleet.push(make_instance(
            detector.clone(),
            &name,
            Arc::new(AtomicBool::new(true)),
        ));
    }

    let total = 30_000;
    let start = Instant::now();
    let mut handles = Vec::with_capacity(total);

    for i in 0..total {
        let mut svc = fleet[i % fleet_size].clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(i).await
        }));
    }

    let mut success = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            success += 1;
        }
    }

    let elapsed = start.elapsed();
    println!(
        "Healthy fleet throughput: {} requests across {} instances",
        total, fleet_size
    );
    println!("Completed in: {:?}", elapsed);
    println!(
        "Throughput: {:.0} req/sec",
        total as f64 / elapsed.as_secs_f64()
    );

    assert_eq!(success, total, "healthy fleet should serve every request");
    assert_eq!(detector.ejected_count(), 0, "no instance should be ejected");
    assert_eq!(
        detector.instance_count(),
        fleet_size,
        "fleet membership is stable"
    );
}

/// Test: The `max_ejection_percent` cap holds under concurrent failures.
///
/// Half the fleet fails every call. With a 30% cap on a 10-instance fleet, at
/// most 3 instances may be ejected simultaneously even though 5 are unhealthy.
#[tokio::test]
#[ignore]
async fn stress_concurrent_ejection_cap() {
    let fleet_size = 10;
    let failing = 5;
    // Long ejection duration so nothing recovers mid-test.
    let detector = OutlierDetector::new()
        .max_ejection_percent(30)
        .base_ejection_duration(Duration::from_secs(60));

    let mut fleet = Vec::with_capacity(fleet_size);
    for i in 0..fleet_size {
        let name = format!("backend-{}", i);
        detector.register(&name, 3);
        let healthy = Arc::new(AtomicBool::new(i >= failing));
        fleet.push(make_instance(detector.clone(), &name, healthy));
    }

    let total = 20_000;
    let start = Instant::now();
    let mut handles = Vec::with_capacity(total);

    for i in 0..total {
        let mut svc = fleet[i % fleet_size].clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(i).await
        }));
    }

    for handle in handles {
        let _ = handle.await.unwrap();
    }

    let elapsed = start.elapsed();
    let ejected = detector.ejected_count();
    println!(
        "Concurrent ejection cap: {} requests, {} failing instances",
        total, failing
    );
    println!("Completed in: {:?}", elapsed);
    println!("Ejected count: {} (cap = 3)", ejected);

    // 30% of 10 instances = at most 3 ejected; all 5 failing reach threshold so
    // the cap is the binding constraint.
    assert!(
        ejected <= 3,
        "must never exceed the 30% ejection cap, got {}",
        ejected
    );
    assert_eq!(
        ejected, 3,
        "cap should be saturated by the failing instances"
    );
    assert_eq!(
        detector.instance_count(),
        fleet_size,
        "fleet membership is stable"
    );
}

/// Test: Ejection and time-based readmission stay consistent across cycles.
///
/// A single unhealthy instance is repeatedly ejected and, after its (capped)
/// ejection duration elapses, readmitted. Over many cycles both ejection and
/// recovery events must keep firing -- the detector must not get stuck.
#[tokio::test]
#[ignore]
async fn stress_ejection_readmission_cycle() {
    let ejections = Arc::new(AtomicUsize::new(0));
    let recoveries = Arc::new(AtomicUsize::new(0));
    let ej = Arc::clone(&ejections);
    let rec = Arc::clone(&recoveries);

    // Cap the ejection duration so every ejection lasts ~50ms regardless of the
    // exponential backoff applied on repeated ejections.
    let detector = OutlierDetector::new()
        .max_ejection_percent(100)
        .base_ejection_duration(Duration::from_millis(50))
        .max_ejection_duration(Duration::from_millis(50))
        .on_ejection(move |_name, _errors| {
            ej.fetch_add(1, Ordering::Relaxed);
        })
        .on_recovery(move |_name, _dur| {
            rec.fetch_add(1, Ordering::Relaxed);
        });

    detector.register("backend-1", 2);
    let svc = make_instance(
        detector.clone(),
        "backend-1",
        Arc::new(AtomicBool::new(false)),
    );

    let cycles = 6;
    for _ in 0..cycles {
        // Drive enough failing calls to (re-)eject the instance.
        for i in 0..5 {
            let mut s = svc.clone();
            let _ = s.ready().await.unwrap().call(i).await;
        }
        // Wait past the ejection duration so the instance becomes eligible for
        // readmission on the next probe.
        sleep(Duration::from_millis(120)).await;
        // A probe call triggers the time-based recovery path.
        let mut s = svc.clone();
        let _ = s.ready().await.unwrap().call(0).await;
    }

    let ej = ejections.load(Ordering::Relaxed);
    let rec = recoveries.load(Ordering::Relaxed);
    println!("Ejection/readmission cycle: {} cycles", cycles);
    println!("Ejections: {}, Recoveries: {}", ej, rec);

    // Generous lower bounds: timing on CI is noisy, but the detector must keep
    // ejecting and readmitting rather than wedging in one state.
    assert!(ej >= 3, "should re-eject across cycles, got {}", ej);
    assert!(rec >= 2, "should readmit across cycles, got {}", rec);
}

/// Test: Mixed healthy/unhealthy fleet under high concurrency stays consistent.
///
/// A 20-instance fleet with 10 unhealthy instances and a 50% cap: all 10
/// unhealthy instances should eject (50% of 20), healthy traffic keeps
/// succeeding, and fleet membership never changes.
#[tokio::test]
#[ignore]
async fn stress_high_concurrency_mixed_load() {
    let fleet_size = 20;
    let failing = 10;
    let detector = OutlierDetector::new()
        .max_ejection_percent(50)
        .base_ejection_duration(Duration::from_secs(60));

    let mut fleet = Vec::with_capacity(fleet_size);
    for i in 0..fleet_size {
        let name = format!("backend-{}", i);
        detector.register(&name, 3);
        let healthy = Arc::new(AtomicBool::new(i >= failing));
        fleet.push(make_instance(detector.clone(), &name, healthy));
    }

    let total = 20_000;
    let start = Instant::now();
    let mut handles = Vec::with_capacity(total);

    for i in 0..total {
        let mut svc = fleet[i % fleet_size].clone();
        handles.push(tokio::spawn(async move {
            svc.ready().await.unwrap().call(i).await.is_ok()
        }));
    }

    let mut success = 0;
    for handle in handles {
        if handle.await.unwrap() {
            success += 1;
        }
    }

    let elapsed = start.elapsed();
    let ejected = detector.ejected_count();
    println!(
        "Mixed load: {} requests, {}/{} instances unhealthy",
        total, failing, fleet_size
    );
    println!("Completed in: {:?}", elapsed);
    println!("Successful responses: {}", success);
    println!("Ejected count: {} (cap = 10)", ejected);

    assert_eq!(
        detector.instance_count(),
        fleet_size,
        "fleet membership is stable"
    );
    assert!(
        ejected <= failing,
        "ejected must not exceed unhealthy count"
    );
    assert_eq!(ejected, 10, "50% of 20 instances should be ejected");
    assert!(success > 0, "healthy instances must keep serving traffic");
}
