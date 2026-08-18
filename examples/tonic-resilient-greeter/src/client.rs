//! gRPC client demonstrating client-side resilience patterns.
//!
//! Real tower-resilience layers protect the outbound call:
//! - `CircuitBreakerLayer` (outer) fails fast once the call keeps failing.
//! - `RetryLayer` (inner) retries transient failures with exponential
//!   backoff before the circuit breaker sees a failure.
//!
//! Tonic's `Channel` operates on non-`Clone` HTTP bodies, so retrying at the
//! transport level isn't possible. Instead the resilience layers wrap a
//! small `tower::Service<HelloRequest>` that re-issues the RPC from the
//! (`Clone`) protobuf request each attempt -- the same "wrap the logical
//! request type, not the raw transport" approach used by
//! `examples/axum-resilient-kv-store` and `examples/composition_outbound.rs`.
//!
//! Run with: cargo run --bin client
//! (Make sure the server is running first)

use std::time::Duration;
use tonic::{transport::Channel, Request, Status};
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_circuitbreaker::{CircuitBreakerError, CircuitBreakerLayer};
use tower_resilience_retry::{ExponentialBackoff, RetryLayer};
use tracing::{error, info, warn};

pub mod greeter {
    tonic::include_proto!("greeter");
}

use greeter::greeter_client::GreeterClient;
use greeter::HelloRequest;

/// Error produced by the circuit-breaker -> retry -> RPC call pipeline.
type CallError = CircuitBreakerError<Status>;

fn build_call_service(
    client: GreeterClient<Channel>,
) -> impl Service<HelloRequest, Response = String, Error = CallError> + Clone {
    let rpc = tower::service_fn(move |req: HelloRequest| {
        let mut client = client.clone();
        async move {
            let response = client.say_hello(Request::new(req)).await?;
            Ok::<_, Status>(response.into_inner().message)
        }
    });

    let retry_layer = RetryLayer::<HelloRequest, String, Status>::builder()
        .max_attempts(3)
        .backoff(
            ExponentialBackoff::new(Duration::from_millis(100))
                .multiplier(2.0)
                .max_interval(Duration::from_secs(2)),
        )
        .on_retry(|attempt, delay| {
            warn!("Retry: attempt {} after {:?}", attempt, delay);
        })
        .build();

    let circuit_breaker_layer = CircuitBreakerLayer::builder()
        .name("greeter-client")
        .failure_rate_threshold(0.5)
        .sliding_window_size(10)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_secs(3))
        .on_state_transition(|from, to| {
            warn!("CircuitBreaker: {:?} -> {:?}", from, to);
        })
        .on_call_rejected(|| {
            warn!("CircuitBreaker: call rejected (circuit OPEN)");
        })
        .build()
        .expect("valid circuit breaker config");

    ServiceBuilder::new()
        .layer(circuit_breaker_layer)
        .layer(retry_layer)
        .service(rpc)
}

async fn make_requests() -> Result<(), Box<dyn std::error::Error>> {
    info!("Connecting to gRPC server at http://[::1]:50051");

    let channel = Channel::from_static("http://[::1]:50051")
        .timeout(Duration::from_millis(500))
        .connect()
        .await?;

    let mut service = build_call_service(GreeterClient::new(channel));

    info!("Connected successfully");
    info!("Resilience pipeline: CircuitBreaker(RetryLayer(rpc call))");
    info!("  - CircuitBreaker: 50% failure threshold, opens after 3 calls, 3s open wait");
    info!("  - Retry: max 3 attempts, exponential backoff (100ms base)");

    let mut success_count = 0u32;
    let mut failure_count = 0u32;
    let mut circuit_rejected_count = 0u32;

    info!("\n=== Making 20 requests to demonstrate resilience patterns ===\n");

    for i in 1..=20 {
        info!("Request {}/20: Calling SayHello for User{}", i, i);

        let outcome = match service.ready().await {
            Ok(ready) => {
                ready
                    .call(HelloRequest {
                        name: format!("User{i}"),
                    })
                    .await
            }
            Err(e) => Err(e),
        };

        match outcome {
            Ok(message) => {
                success_count += 1;
                info!("  ✓ Response: {}", message);
            }
            Err(CircuitBreakerError::OpenCircuit) => {
                circuit_rejected_count += 1;
                failure_count += 1;
                error!("  ✗ Circuit Breaker: call rejected (circuit is OPEN)");
            }
            Err(CircuitBreakerError::Inner(status)) => {
                failure_count += 1;
                error!("  ✗ Error: {}", status);
            }
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    info!("\n=== Summary ===");
    info!("Successful requests: {}", success_count);
    info!("Failed requests: {}", failure_count);
    info!("Circuit breaker rejections: {}", circuit_rejected_count);
    info!("\nThe circuit breaker and retry layers protected the client from cascading failures.");
    info!("Timeouts and server overload were handled by real tower-resilience middleware.");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    if let Err(e) = make_requests().await {
        error!("Client error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
