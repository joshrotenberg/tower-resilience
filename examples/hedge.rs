//! Hedging example for reducing tail latency
//!
//! This example demonstrates hedging to reduce tail latency by
//! firing parallel requests when the primary is slow.
//! Run with: cargo run --example hedge

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_core::FnListener;
use tower_resilience_hedge::{HedgeEvent, HedgeLayer};

#[derive(Debug, Clone)]
struct MyError;
impl std::fmt::Display for MyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MyError")
    }
}
impl std::error::Error for MyError {}

#[tokio::main]
async fn main() {
    println!("=== Hedging Example ===\n");

    // Example 1: Latency mode - hedge fires after delay
    println!("--- Example 1: Latency Mode ---");
    println!("Primary request is slow (200ms), hedge fires after 50ms");
    println!();

    let call_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&call_count);

    // Service where primary is slow, hedge is fast
    let svc = service_fn(move |req: String| {
        let counter = Arc::clone(&counter);
        async move {
            let attempt = counter.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                // Primary is slow
                println!("  Primary request started (will take 200ms)");
                tokio::time::sleep(Duration::from_millis(200)).await;
                println!("  Primary request completed");
            } else {
                // Hedge is fast
                println!("  Hedge request #{} started (will be fast)", attempt);
                tokio::time::sleep(Duration::from_millis(10)).await;
                println!("  Hedge request #{} completed", attempt);
            }
            Ok::<_, MyError>(format!("Response to: {}", req))
        }
    });

    let layer = HedgeLayer::builder()
        .name("latency-hedge")
        .delay(Duration::from_millis(50)) // Fire hedge after 50ms
        .max_hedged_attempts(2)
        .on_event(FnListener::new(|e: &HedgeEvent| match e {
            HedgeEvent::PrimaryStarted { .. } => {
                println!("[Event] Primary started");
            }
            HedgeEvent::HedgeStarted { attempt, delay, .. } => {
                println!("[Event] Hedge #{} started after {:?}", attempt, delay);
            }
            HedgeEvent::PrimarySucceeded {
                duration,
                hedges_cancelled,
                ..
            } => {
                println!(
                    "[Event] Primary succeeded in {:?}, {} hedges cancelled",
                    duration, hedges_cancelled
                );
            }
            HedgeEvent::HedgeSucceeded {
                attempt, duration, ..
            } => {
                println!("[Event] Hedge #{} succeeded in {:?}", attempt, duration);
            }
            HedgeEvent::AllFailed { attempts, .. } => {
                println!("[Event] All {} attempts failed", attempts);
            }
        }))
        .build();

    let mut service = layer.layer(svc);

    let start = Instant::now();
    let result = service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await;
    let elapsed = start.elapsed();

    match result {
        Ok(response) => println!("\nResult: {} (took {:?})", response, elapsed),
        Err(_) => println!("\nFailed"),
    }

    // Wait for background tasks to complete
    tokio::time::sleep(Duration::from_millis(250)).await;
    println!("Total requests made: {}", call_count.load(Ordering::SeqCst));

    // Example 2: Parallel mode - all requests fire immediately
    println!("\n--- Example 2: Parallel Mode ---");
    println!("All 3 requests fire immediately, fastest wins");
    println!();

    let call_count2 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::clone(&call_count2);

    // Service with varying latencies
    let svc2 = service_fn(move |req: String| {
        let counter = Arc::clone(&counter2);
        async move {
            let attempt = counter.fetch_add(1, Ordering::SeqCst);
            // Different latencies for each request
            let latency = match attempt {
                0 => 100, // Primary: 100ms
                1 => 30,  // Hedge 1: 30ms (fastest)
                _ => 60,  // Hedge 2: 60ms
            };
            println!("  Request #{} started (will take {}ms)", attempt, latency);
            tokio::time::sleep(Duration::from_millis(latency)).await;
            println!("  Request #{} completed", attempt);
            Ok::<_, MyError>(format!("Response #{} to: {}", attempt, req))
        }
    });

    let layer2 = HedgeLayer::<String, String, MyError>::builder()
        .name("parallel-hedge")
        .no_delay() // Fire all immediately
        .max_hedged_attempts(3)
        .on_event(FnListener::new(|e: &HedgeEvent| match e {
            HedgeEvent::HedgeSucceeded {
                attempt, duration, ..
            } => {
                println!("[Event] Hedge #{} won the race in {:?}", attempt, duration);
            }
            HedgeEvent::PrimarySucceeded { duration, .. } => {
                println!("[Event] Primary won the race in {:?}", duration);
            }
            _ => {}
        }))
        .build();

    let mut service2 = layer2.layer(svc2);

    let start2 = Instant::now();
    let result2 = service2
        .ready()
        .await
        .unwrap()
        .call("parallel-test".to_string())
        .await;
    let elapsed2 = start2.elapsed();

    match result2 {
        Ok(response) => println!("\nResult: {} (took {:?})", response, elapsed2),
        Err(_) => println!("\nFailed"),
    }

    // Wait for background tasks to complete
    tokio::time::sleep(Duration::from_millis(150)).await;
    println!(
        "Total requests made: {}",
        call_count2.load(Ordering::SeqCst)
    );

    // Example 3: Dynamic delay based on attempt
    println!("\n--- Example 3: Dynamic Delay ---");
    println!("Increasing delays: 20ms for first hedge, 50ms for second");
    println!();

    let call_count3 = Arc::new(AtomicUsize::new(0));
    let counter3 = Arc::clone(&call_count3);

    let svc3 = service_fn(move |req: String| {
        let counter = Arc::clone(&counter3);
        async move {
            let attempt = counter.fetch_add(1, Ordering::SeqCst);
            println!("  Request #{} started", attempt);
            // All requests take the same time
            tokio::time::sleep(Duration::from_millis(200)).await;
            println!("  Request #{} completed", attempt);
            Ok::<_, MyError>(format!("Response #{} to: {}", attempt, req))
        }
    });

    let layer3 = HedgeLayer::builder()
        .name("dynamic-hedge")
        .delay_fn(|attempt| {
            // Increasing delays: 20ms, 50ms, 100ms...
            Duration::from_millis(20 * (attempt as u64).pow(2))
        })
        .max_hedged_attempts(3)
        .on_event(FnListener::new(|e: &HedgeEvent| {
            if let HedgeEvent::HedgeStarted { attempt, delay, .. } = e {
                println!("[Event] Hedge #{} fired after {:?}", attempt, delay);
            }
        }))
        .build();

    let mut service3 = layer3.layer(svc3);

    let start3 = Instant::now();
    let result3 = service3
        .ready()
        .await
        .unwrap()
        .call("dynamic-test".to_string())
        .await;
    let elapsed3 = start3.elapsed();

    match result3 {
        Ok(response) => println!("\nResult: {} (took {:?})", response, elapsed3),
        Err(_) => println!("\nFailed"),
    }

    // Wait for background tasks
    tokio::time::sleep(Duration::from_millis(250)).await;
    println!(
        "Total requests made: {}",
        call_count3.load(Ordering::SeqCst)
    );

    println!("\n=== Done ===");
}
