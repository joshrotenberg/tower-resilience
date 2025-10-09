//! Simple example for tower-circuitbreaker
//! Run with: cargo run --example simple
//! With tracing: RUST_LOG=debug cargo run --example simple --features tracing

use std::time::Duration;
use tokio::time::sleep;
use tower::{Service, service_fn};
use tower_resilience_circuitbreaker::CircuitBreakerLayer;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Simple service that succeeds with true and fails with false
    let boolean_service = service_fn(move |req: bool| {
        let resp = if req {
            Ok::<_, ()>(req.to_string())
        } else {
            Err::<String, _>(())
        };
        println!("Boolean service called, response is: {:?}", resp);
        async move { resp }
    });

    // Wrap it in a circuit breaker: opens if ≥50% of last 2 calls failed,
    // stays open 1s, then allows 1 trial in half-open.
    // If the trial succeeds, goes back to closed.
    let breaker_layer = CircuitBreakerLayer::<String, ()>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(2) // of the past two calls fail
        .wait_duration_in_open(Duration::from_secs(1)) // open for 1 second
        .permitted_calls_in_half_open(1) // before allowing 1 call
        .name("simple-circuit")
        .build();

    // Create a service with the circuit breaker
    let mut svc = breaker_layer.layer(boolean_service);

    // The circuit starts out closed
    println!("Circuit state (should be closed): {:?}", svc.state().await);

    // First two calls fail → breaker opens
    for i in 1..=2 {
        let result = svc.call(false).await;
        println!("Call {} result: {:?}", i, result);
    }
    println!(
        "Circuit state (should be open now after failures): {:?}",
        svc.state().await
    );

    // Wait out the open period
    sleep(Duration::from_secs(1)).await;

    // Next call transitions half-open → then closed on success
    let result = svc.call(true).await;
    println!("Trial call result: {:?}", result);
    println!("Circuit state: {:?}", svc.state().await);

    // A normal call now succeeds
    let _result = svc.call(true).await;
    println!("Circuit state: {:?}", svc.state().await);

    // And another
    let result = svc.call(true).await;
    println!("Final call result: {:?}", result);
    println!("Circuit state: {:?}", svc.state().await);
}
