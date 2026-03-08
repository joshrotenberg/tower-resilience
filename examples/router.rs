//! Weighted router example demonstrating canary deployment traffic splitting.
//! Run with: cargo run --example router

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::util::BoxService;
use tower::{Service, ServiceExt};
use tower_resilience_router::WeightedRouter;

type BoxSvc = BoxService<String, String, std::io::Error>;

fn make_backend(name: &'static str, counter: Arc<AtomicUsize>) -> BoxSvc {
    BoxService::new(tower::service_fn(move |req: String| {
        let c = Arc::clone(&counter);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(format!("[{}] processed: {}", name, req))
        }
    }))
}

#[tokio::main]
async fn main() {
    println!("=== Weighted Router: Canary Deployment ===\n");

    // Track how many requests each backend handles
    let stable_count = Arc::new(AtomicUsize::new(0));
    let canary_count = Arc::new(AtomicUsize::new(0));

    // Route 90% of traffic to stable, 10% to canary
    let mut router = WeightedRouter::builder()
        .name("canary-deploy")
        .route(make_backend("stable-v1", Arc::clone(&stable_count)), 90)
        .route(make_backend("canary-v2", Arc::clone(&canary_count)), 10)
        .on_request_routed(|idx, weight| {
            let label = if idx == 0 { "stable" } else { "canary" };
            println!(
                "  [ROUTER] Routed to {} (index={}, weight={})",
                label, idx, weight
            );
        })
        .build();

    println!(
        "Router '{}' configured with {} backends, weights: {:?}\n",
        router.name(),
        router.backend_count(),
        router.weights()
    );

    // Send 20 requests
    println!("Sending 20 requests...\n");
    for i in 1..=20 {
        let resp = router
            .ready()
            .await
            .unwrap()
            .call(format!("request-{}", i))
            .await
            .unwrap();
        println!("  Response: {}", resp);
    }

    let stable = stable_count.load(Ordering::SeqCst);
    let canary = canary_count.load(Ordering::SeqCst);
    println!("\n--- Results ---");
    println!("Stable handled: {} (expected 18)", stable);
    println!("Canary handled: {} (expected 2)", canary);
    println!("Total: {}", stable + canary);

    // Demonstrate random strategy
    println!("\n=== Random Strategy ===\n");

    let stable_count2 = Arc::new(AtomicUsize::new(0));
    let canary_count2 = Arc::new(AtomicUsize::new(0));

    let mut router = WeightedRouter::builder()
        .name("random-canary")
        .route(make_backend("stable-v1", Arc::clone(&stable_count2)), 80)
        .route(make_backend("canary-v2", Arc::clone(&canary_count2)), 20)
        .random()
        .build();

    for i in 1..=1000 {
        let _ = router
            .ready()
            .await
            .unwrap()
            .call(format!("req-{}", i))
            .await;
    }

    let stable = stable_count2.load(Ordering::SeqCst);
    let canary = canary_count2.load(Ordering::SeqCst);
    println!("After 1000 requests with random strategy (80/20 weights):");
    println!("  Stable: {} ({:.1}%)", stable, stable as f64 / 10.0);
    println!("  Canary: {} ({:.1}%)", canary, canary as f64 / 10.0);
}
