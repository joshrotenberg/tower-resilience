//! Example demonstrating multiple resilience patterns composed together.
//!
//! This shows how to stack circuit breaker and bulkhead middleware to protect
//! a service with both failure-based circuit breaking and concurrency limiting.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Service, ServiceBuilder};
use tower_resilience::{bulkhead::BulkheadConfig, circuitbreaker::CircuitBreakerConfig};

#[derive(Debug)]
struct ServiceError;

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Service error")
    }
}

impl std::error::Error for ServiceError {}

impl From<tower_bulkhead::BulkheadError> for ServiceError {
    fn from(_: tower_bulkhead::BulkheadError) -> Self {
        ServiceError
    }
}

#[tokio::main]
async fn main() {
    // Track concurrent calls and total calls
    let concurrent = Arc::new(AtomicUsize::new(0));
    let total_calls = Arc::new(AtomicUsize::new(0));
    let failures = Arc::new(AtomicUsize::new(0));

    // Create a service that fails 60% of the time and tracks concurrency
    let concurrent_clone = Arc::clone(&concurrent);
    let total_clone = Arc::clone(&total_calls);
    let failures_clone = Arc::clone(&failures);

    let service = tower::service_fn(move |_req: ()| {
        let concurrent = Arc::clone(&concurrent_clone);
        let total = Arc::clone(&total_clone);
        let failures = Arc::clone(&failures_clone);

        async move {
            let current = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
            let call_num = total.fetch_add(1, Ordering::SeqCst) + 1;

            println!("  [Service] Call #{call_num}, concurrent: {current}");

            // Simulate work
            sleep(Duration::from_millis(50)).await;

            concurrent.fetch_sub(1, Ordering::SeqCst);

            // Fail 60% of the time
            if call_num % 10 < 6 {
                failures.fetch_add(1, Ordering::SeqCst);
                Err(ServiceError)
            } else {
                Ok(())
            }
        }
    });

    // Build service with both patterns
    let bulkhead_count = Arc::new(AtomicUsize::new(0));
    let bulkhead_clone = Arc::clone(&bulkhead_count);

    // Apply bulkhead first
    let bulkhead_layer = BulkheadConfig::builder()
        .max_concurrent_calls(3)
        .max_wait_duration(Some(Duration::from_millis(100)))
        .on_call_rejected(move |_| {
            bulkhead_clone.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let service = ServiceBuilder::new().layer(bulkhead_layer).service(service);

    // Then wrap with circuit breaker
    let cb_layer = CircuitBreakerConfig::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .wait_duration_in_open(Duration::from_secs(2))
        .build();

    let mut service = cb_layer.layer(service);

    println!("Sending 30 requests to a service with:");
    println!("  - 60% failure rate");
    println!("  - Bulkhead limiting to 3 concurrent calls");
    println!("  - Circuit breaker with 50% failure threshold over 10 calls\n");

    // Send 30 requests
    for i in 1..=30 {
        match tower::ServiceExt::ready(&mut service)
            .await
            .unwrap()
            .call(())
            .await
        {
            Ok(()) => println!("Request {i}: Success"),
            Err(_e) => println!("Request {i}: Failed"),
        }

        // Small delay between requests
        sleep(Duration::from_millis(10)).await;
    }

    println!("\n--- Results ---");
    println!("Total calls attempted: 30");
    println!(
        "Calls that reached service: {}",
        total_calls.load(Ordering::SeqCst)
    );
    println!("Service failures: {}", failures.load(Ordering::SeqCst));
    println!(
        "Bulkhead rejections: {}",
        bulkhead_count.load(Ordering::SeqCst)
    );
}
