//! Example demonstrating chaos engineering for testing resilience patterns.
//!
//! This example shows how to use the chaos layer to test circuit breakers,
//! retries, and other resilience mechanisms.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::{Layer, Service, ServiceExt};
use tower_resilience_chaos::ChaosLayer;

#[tokio::main]
async fn main() {
    println!("=== Chaos Engineering Example ===\n");

    // Example 1: Basic error injection
    println!("1. Basic Error Injection");
    basic_error_injection().await;
    println!();

    // Example 2: Latency injection
    println!("2. Latency Injection");
    latency_injection().await;
    println!();

    // Example 3: Deterministic chaos (reproducible tests)
    println!("3. Deterministic Chaos Testing");
    deterministic_chaos().await;
    println!();

    // Example 4: Monitoring chaos with events
    println!("4. Chaos Event Monitoring");
    event_monitoring().await;
    println!();

    println!("=== Example Complete ===");
}

async fn basic_error_injection() {
    let chaos = ChaosLayer::<String, std::io::Error>::builder()
        .name("error-injector")
        .error_rate(0.3) // 30% of requests fail
        .error_fn(|_req| std::io::Error::new(std::io::ErrorKind::Other, "chaos-induced failure"))
        .build();

    let svc = tower::service_fn(|req: String| async move {
        Ok::<String, std::io::Error>(format!("Processed: {}", req))
    });

    let mut service = chaos.layer(svc);

    // Make 10 requests and count failures
    let mut successes = 0;
    let mut failures = 0;

    for i in 0..10 {
        match service
            .ready()
            .await
            .unwrap()
            .call(format!("request-{}", i))
            .await
        {
            Ok(response) => {
                successes += 1;
                println!("  ✓ Success: {}", response);
            }
            Err(e) => {
                failures += 1;
                println!("  ✗ Failed: {}", e);
            }
        }
    }

    println!(
        "  Results: {} successes, {} failures (~30% expected)",
        successes, failures
    );
}

async fn latency_injection() {
    let chaos = ChaosLayer::<String, std::io::Error>::builder()
        .name("latency-injector")
        .latency_rate(0.5) // 50% of requests delayed
        .min_latency(Duration::from_millis(50))
        .max_latency(Duration::from_millis(150))
        .build();

    let svc = tower::service_fn(|req: String| async move {
        Ok::<String, std::io::Error>(format!("Processed: {}", req))
    });

    let mut service = chaos.layer(svc);

    println!("  Making 5 requests (50% will have 50-150ms latency):");

    for i in 0..5 {
        let start = std::time::Instant::now();
        let result = service
            .ready()
            .await
            .unwrap()
            .call(format!("request-{}", i))
            .await;
        let elapsed = start.elapsed();

        match result {
            Ok(response) => {
                if elapsed.as_millis() > 40 {
                    println!(
                        "  ⏱  {}ms (latency injected): {}",
                        elapsed.as_millis(),
                        response
                    );
                } else {
                    println!("  ✓ {}ms (normal): {}", elapsed.as_millis(), response);
                }
            }
            Err(e) => {
                println!("  ✗ Failed: {}", e);
            }
        }
    }
}

async fn deterministic_chaos() {
    println!("  Running same test twice with seed=42:");

    for run in 1..=2 {
        println!("  Run {}:", run);
        let chaos = ChaosLayer::<String, std::io::Error>::builder()
            .name("deterministic-chaos")
            .error_rate(0.5)
            .error_fn(|_req| std::io::Error::new(std::io::ErrorKind::Other, "chaos"))
            .seed(42) // Same seed = same results
            .build();

        let svc = tower::service_fn(|req: String| async move {
            Ok::<String, std::io::Error>(format!("Processed: {}", req))
        });

        let mut service = chaos.layer(svc);

        let mut results = Vec::new();
        for i in 0..5 {
            let result = service
                .ready()
                .await
                .unwrap()
                .call(format!("req-{}", i))
                .await;
            results.push(result.is_ok());
        }

        println!(
            "    {}",
            results
                .iter()
                .map(|&ok| if ok { "✓" } else { "✗" })
                .collect::<Vec<_>>()
                .join(" ")
        );
    }

    println!("  Both runs should show identical patterns!");
}

async fn event_monitoring() {
    let error_count = Arc::new(AtomicUsize::new(0));
    let latency_count = Arc::new(AtomicUsize::new(0));
    let pass_count = Arc::new(AtomicUsize::new(0));

    let e = error_count.clone();
    let l = latency_count.clone();
    let p = pass_count.clone();

    let chaos = ChaosLayer::<String, std::io::Error>::builder()
        .name("monitored-chaos")
        .error_rate(0.2)
        .error_fn(|_req| std::io::Error::new(std::io::ErrorKind::Other, "chaos"))
        .latency_rate(0.3)
        .min_latency(Duration::from_millis(10))
        .max_latency(Duration::from_millis(20))
        .on_error_injected(move || {
            e.fetch_add(1, Ordering::SeqCst);
        })
        .on_latency_injected(move |delay| {
            l.fetch_add(1, Ordering::SeqCst);
            println!("    Latency injected: {:?}", delay);
        })
        .on_passed_through(move || {
            p.fetch_add(1, Ordering::SeqCst);
        })
        .build();

    let svc = tower::service_fn(|req: String| async move {
        Ok::<String, std::io::Error>(format!("Processed: {}", req))
    });

    let mut service = chaos.layer(svc);

    println!("  Making 20 requests with event monitoring:");

    for i in 0..20 {
        let _ = service
            .ready()
            .await
            .unwrap()
            .call(format!("req-{}", i))
            .await;
    }

    println!("\n  Event Summary:");
    println!(
        "    Errors injected: {}",
        error_count.load(Ordering::SeqCst)
    );
    println!(
        "    Latencies injected: {}",
        latency_count.load(Ordering::SeqCst)
    );
    println!("    Passed through: {}", pass_count.load(Ordering::SeqCst));
    println!(
        "    Total events: {}",
        error_count.load(Ordering::SeqCst)
            + latency_count.load(Ordering::SeqCst)
            + pass_count.load(Ordering::SeqCst)
    );
}
