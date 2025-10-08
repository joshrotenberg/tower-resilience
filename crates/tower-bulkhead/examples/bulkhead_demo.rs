use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_bulkhead::{BulkheadConfig, BulkheadError};

#[tokio::main]
async fn main() {
    println!("=== Tower Bulkhead Pattern Demo ===\n");

    // Counters for tracking events
    let permitted = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    let p = Arc::clone(&permitted);
    let r = Arc::clone(&rejected);
    let fin = Arc::clone(&finished);
    let fail = Arc::clone(&failed);

    println!("Creating bulkhead with:");
    println!("  - Max concurrent calls: 3");
    println!("  - Max wait duration: 100ms");
    println!("  - Event listeners enabled\n");

    // Create bulkhead configuration
    let config = BulkheadConfig::builder()
        .max_concurrent_calls(3)
        .max_wait_duration(Some(Duration::from_millis(100)))
        .name("demo-bulkhead")
        .on_call_permitted(move |concurrent| {
            p.fetch_add(1, Ordering::SeqCst);
            println!("  ✓ Call permitted (concurrent: {})", concurrent);
        })
        .on_call_rejected(move |max| {
            r.fetch_add(1, Ordering::SeqCst);
            println!("  ✗ Call rejected (max: {})", max);
        })
        .on_call_finished(move |duration| {
            fin.fetch_add(1, Ordering::SeqCst);
            println!("  → Call finished ({:?})", duration);
        })
        .on_call_failed(move |duration| {
            fail.fetch_add(1, Ordering::SeqCst);
            println!("  ⚠ Call failed ({:?})", duration);
        })
        .build();

    // Create a service that simulates work
    let service = tower::service_fn(|req: (usize, Duration, bool)| async move {
        let (id, duration, should_fail) = req;
        println!("    [Request {}] Processing...", id);
        tokio::time::sleep(duration).await;

        if should_fail {
            Err::<(), BulkheadError>(BulkheadError::BulkheadFull {
                max_concurrent_calls: 3,
            })
        } else {
            Ok(())
        }
    });

    // Wrap with bulkhead
    let mut bulkhead_service = ServiceBuilder::new().layer(config).service(service);

    println!("--- Test 1: Within capacity (3 concurrent requests) ---\n");

    let mut handles = vec![];
    for i in 1..=3 {
        let mut svc = bulkhead_service.clone();
        let handle = tokio::spawn(async move {
            svc.ready().await.unwrap();
            svc.call((i, Duration::from_millis(200), false)).await
        });
        handles.push(handle);
    }

    // Let them start
    tokio::time::sleep(Duration::from_millis(50)).await;

    println!("\n--- Test 2: Exceeding capacity (should reject) ---\n");

    // Try to make more calls while first 3 are still running
    for i in 4..=6 {
        match bulkhead_service.ready().await {
            Ok(svc) => match svc.call((i, Duration::from_millis(50), false)).await {
                Ok(_) => println!("  Request {} succeeded", i),
                Err(e) => println!("  Request {} error: {}", i, e),
            },
            Err(e) => println!("  Service not ready for request {}: {}", i, e),
        }
    }

    // Wait for initial requests to complete
    for handle in handles {
        let _ = handle.await;
    }

    println!("\n--- Test 3: After capacity freed (should succeed) ---\n");

    for i in 7..=9 {
        match bulkhead_service.ready().await {
            Ok(svc) => match svc.call((i, Duration::from_millis(50), false)).await {
                Ok(_) => println!("  Request {} succeeded", i),
                Err(e) => println!("  Request {} error: {}", i, e),
            },
            Err(e) => println!("  Service not ready for request {}: {}", i, e),
        }
    }

    println!("\n--- Test 4: Simulating failures ---\n");

    for i in 10..=11 {
        match bulkhead_service.ready().await {
            Ok(svc) => match svc.call((i, Duration::from_millis(50), true)).await {
                Ok(_) => println!("  Request {} succeeded", i),
                Err(e) => println!("  Request {} error: {}", i, e),
            },
            Err(e) => println!("  Service not ready for request {}: {}", i, e),
        }
    }

    // Final summary
    println!("\n=== Summary ===");
    println!("Calls permitted: {}", permitted.load(Ordering::SeqCst));
    println!("Calls rejected:  {}", rejected.load(Ordering::SeqCst));
    println!("Calls finished:  {}", finished.load(Ordering::SeqCst));
    println!("Calls failed:    {}", failed.load(Ordering::SeqCst));
}
