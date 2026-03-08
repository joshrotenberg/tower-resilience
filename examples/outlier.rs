//! Outlier detection example demonstrating fleet-aware instance ejection.
//! Run with: cargo run --example outlier

use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_outlier::{OutlierDetectionLayer, OutlierDetector};

#[tokio::main]
async fn main() {
    println!("=== Outlier Detection: Fleet-Aware Ejection ===\n");

    // Create a shared detector for the fleet
    let detector = OutlierDetector::new()
        .name("my-fleet")
        .base_ejection_duration(Duration::from_secs(1))
        .max_ejection_percent(50)
        .on_ejection(|name, errors| {
            println!(
                "  [EJECTED] Instance '{}' ejected after {} consecutive errors",
                name, errors
            );
        })
        .on_recovery(|name, duration: Duration| {
            println!(
                "  [RECOVERED] Instance '{}' recovered after {:.1}s",
                name,
                duration.as_secs_f64()
            );
        });

    // Register 3 backends: eject after 3 consecutive errors
    detector.register("backend-1", 3);
    detector.register("backend-2", 3);
    detector.register("backend-3", 3);

    println!(
        "Fleet '{}' with {} instances registered\n",
        detector.pattern_name(),
        detector.instance_count()
    );

    // --- Healthy instance passes through ---
    println!("--- Healthy Instance ---");

    let layer = OutlierDetectionLayer::builder()
        .detector(detector.clone())
        .instance_name("backend-1")
        .error_on_ejection()
        .build();

    let ok_svc = tower::util::BoxCloneService::new(tower::service_fn(|req: String| async move {
        Ok::<_, std::io::Error>(format!("OK: {}", req))
    }));
    let mut svc = layer.layer(ok_svc);

    let resp = svc.ready().await.unwrap().call("hello".into()).await;
    println!("Response: {:?}", resp);
    println!("backend-1 ejected? {}\n", detector.is_ejected("backend-1"));

    // --- Consecutive errors trigger ejection ---
    println!("--- Triggering Ejection on backend-2 ---");

    let layer2 = OutlierDetectionLayer::builder()
        .detector(detector.clone())
        .instance_name("backend-2")
        .error_on_ejection()
        .build();

    let fail_svc =
        tower::util::BoxCloneService::new(tower::service_fn(|_req: String| async move {
            Err::<String, _>(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "connection refused",
            ))
        }));
    let mut svc2 = layer2.layer(fail_svc);

    for i in 1..=4 {
        let resp = svc2.ready().await.unwrap().call(format!("req-{}", i)).await;
        match &resp {
            Ok(v) => println!("  Call {}: Ok({})", i, v),
            Err(e) => println!("  Call {}: Err({})", i, e),
        }
    }

    println!("\nbackend-2 ejected? {}", detector.is_ejected("backend-2"));
    println!(
        "Ejected count: {}/{}\n",
        detector.ejected_count(),
        detector.instance_count()
    );

    // --- Max ejection percent prevents cascading ejections ---
    println!("--- Max Ejection Percent (50%) ---");
    println!("Trying to eject backend-3 (would exceed 50% threshold)...");

    // Simulate 3 consecutive failures on backend-3
    for _ in 0..3 {
        detector.record_failure("backend-3");
    }
    println!(
        "backend-3 ejected? {} (should be false - would exceed 50%)",
        detector.is_ejected("backend-3")
    );
    println!(
        "Ejected count: {}/{}\n",
        detector.ejected_count(),
        detector.instance_count()
    );

    // --- Auto-recovery after ejection duration ---
    println!("--- Auto Recovery ---");
    println!("Waiting for backend-2 to recover (1s ejection duration)...");
    tokio::time::sleep(Duration::from_millis(1100)).await;

    println!(
        "backend-2 ejected? {} (should be false after recovery)",
        detector.is_ejected("backend-2")
    );
    println!(
        "Ejected count: {}/{}",
        detector.ejected_count(),
        detector.instance_count()
    );

    println!("\nDone!");
}
