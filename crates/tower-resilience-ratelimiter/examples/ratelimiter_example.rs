use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_ratelimiter::RateLimiterError;

#[tokio::main]
async fn main() {
    // Counter to track events
    let permit_count = Arc::new(AtomicUsize::new(0));
    let reject_count = Arc::new(AtomicUsize::new(0));

    let p = Arc::clone(&permit_count);
    let r = Arc::clone(&reject_count);

    // Create a rate limiter that allows 5 requests per second
    // with a 100ms timeout for waiting for permits
    let layer = tower_resilience_ratelimiter::RateLimiterLayer::builder()
        .limit_for_period(5)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_millis(100))
        .name("api-limiter")
        .on_permit_acquired(move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .on_permit_rejected(move |_| {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    // Create a simple service that returns "Hello"
    let service =
        tower::service_fn(|_req: ()| async { Ok::<_, std::convert::Infallible>("Hello") });

    // Wrap the service with rate limiting
    let mut rate_limited_service = ServiceBuilder::new().layer(layer).service(service);

    println!("Sending 10 requests (limit: 5 per second)...\n");

    // Send 10 requests rapidly
    for i in 1..=10 {
        match rate_limited_service.ready().await {
            Ok(svc) => match svc.call(()).await {
                Ok(response) => {
                    println!("Request {}: {} (permitted)", i, response);
                }
                Err(RateLimiterError::RateLimitExceeded) => {
                    println!("Request {}: Rate limited (rejected)", i);
                }
            },
            Err(_) => {
                println!("Request {}: Service not ready", i);
            }
        }
    }

    println!(
        "\nSummary: {} permits acquired, {} requests rejected",
        permit_count.load(Ordering::SeqCst),
        reject_count.load(Ordering::SeqCst)
    );

    // Wait for refresh period
    println!("\nWaiting 1 second for permit refresh...");
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Try a few more requests
    println!("\nSending 3 more requests after refresh...\n");
    for i in 11..=13 {
        match rate_limited_service.ready().await {
            Ok(svc) => match svc.call(()).await {
                Ok(response) => {
                    println!("Request {}: {} (permitted)", i, response);
                }
                Err(RateLimiterError::RateLimitExceeded) => {
                    println!("Request {}: Rate limited (rejected)", i);
                }
            },
            Err(_) => {
                println!("Request {}: Service not ready", i);
            }
        }
    }

    println!(
        "\nFinal summary: {} permits acquired, {} requests rejected",
        permit_count.load(Ordering::SeqCst),
        reject_count.load(Ordering::SeqCst)
    );
}
