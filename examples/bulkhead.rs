//! Bulkhead example demonstrating concurrency limiting
//! Run with: cargo run --example bulkhead

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service};
use tower_resilience_bulkhead::{BulkheadError, BulkheadLayer};

#[derive(Debug)]
enum ExampleError {
    #[allow(dead_code)]
    Bulkhead(BulkheadError),
}

impl From<BulkheadError> for ExampleError {
    fn from(e: BulkheadError) -> Self {
        ExampleError::Bulkhead(e)
    }
}

#[tokio::main]
async fn main() {
    let concurrent_counter = Arc::new(AtomicUsize::new(0));
    let max_observed = Arc::new(AtomicUsize::new(0));

    let counter_clone = Arc::clone(&concurrent_counter);
    let max_clone = Arc::clone(&max_observed);

    // Service that simulates work and tracks concurrency
    let work_service = tower::service_fn(move |_req: u32| {
        let counter = Arc::clone(&counter_clone);
        let max = Arc::clone(&max_clone);
        async move {
            let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
            max.fetch_max(current, Ordering::SeqCst);

            println!("Working... (concurrent: {})", current);
            sleep(Duration::from_millis(100)).await;

            counter.fetch_sub(1, Ordering::SeqCst);
            Ok::<_, ExampleError>(())
        }
    });

    // Wrap with bulkhead that limits to 3 concurrent calls
    let bulkhead = BulkheadLayer::builder()
        .max_concurrent_calls(3)
        .max_wait_duration(Some(Duration::from_secs(1)))
        .name("example-bulkhead")
        .on_call_permitted(|concurrent| {
            println!("  [BULKHEAD] Permitted (concurrent: {})", concurrent);
        })
        .on_call_rejected(|max| {
            println!("  [BULKHEAD] Rejected (max: {})", max);
        })
        .build();

    let service = bulkhead.layer(work_service);

    // Spawn 10 concurrent requests
    println!("Starting 10 concurrent requests with max concurrency of 3...\n");

    let mut handles = vec![];
    for i in 0..10 {
        let mut svc = service.clone();
        handles.push(tokio::spawn(async move {
            match svc.call(i).await {
                Ok(_) => println!("Request {} completed", i),
                Err(e) => println!("Request {} failed: {:?}", i, e),
            }
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    println!(
        "\nMax concurrent observed: {}",
        max_observed.load(Ordering::SeqCst)
    );
    println!("Should be 3 or less due to bulkhead limiting");
}
