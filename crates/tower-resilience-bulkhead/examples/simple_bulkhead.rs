use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_bulkhead::{BulkheadConfig, BulkheadError};

#[tokio::main]
async fn main() {
    println!("Simple Bulkhead Example\n");

    // Create a bulkhead that allows max 5 concurrent calls
    let config = BulkheadConfig::builder()
        .max_concurrent_calls(5)
        .name("api-bulkhead")
        .build();

    // Create a simple service
    let service = tower::service_fn(|req: String| async move {
        println!("Processing: {}", req);
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok::<_, BulkheadError>(format!("Response to: {}", req))
    });

    // Wrap with bulkhead
    let mut bulkhead_service = ServiceBuilder::new().layer(config).service(service);

    // Make some requests
    for i in 1..=3 {
        match bulkhead_service.ready().await {
            Ok(svc) => {
                let response = svc.call(format!("Request {}", i)).await.unwrap();
                println!("{}", response);
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }

    println!("\nAll requests completed!");
}
