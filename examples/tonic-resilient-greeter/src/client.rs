//! gRPC Client demonstrating client-side resilience patterns
//!
//! This client demonstrates resilience patterns manually implemented around gRPC calls:
//! - Circuit Breaker-like behavior: Tracks failures and stops calling when failure rate is high
//! - Retry: Automatically retries failed requests with exponential backoff
//! - Comprehensive logging to observe resilience patterns in action
//!
//! Note: Due to tonic's requirement that request bodies be non-Clone and the specific
//! Service trait bounds, we demonstrate resilience patterns through manual implementation
//! rather than Tower middleware layers. This shows the same concepts in a practical way.
//!
//! Run with: cargo run --bin client
//! (Make sure the server is running first)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tonic::{transport::Channel, Request, Status};
use tracing::{error, info, warn};

pub mod greeter {
    tonic::include_proto!("greeter");
}

use greeter::greeter_client::GreeterClient;
use greeter::HelloRequest;

/// Simple circuit breaker state tracker
struct CircuitBreakerState {
    failures: AtomicUsize,
    successes: AtomicUsize,
    total_calls: AtomicUsize,
    is_open: std::sync::atomic::AtomicBool,
}

impl CircuitBreakerState {
    fn new() -> Self {
        Self {
            failures: AtomicUsize::new(0),
            successes: AtomicUsize::new(0),
            total_calls: AtomicUsize::new(0),
            is_open: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn record_success(&self) {
        self.successes.fetch_add(1, Ordering::SeqCst);
        self.total_calls.fetch_add(1, Ordering::SeqCst);

        // Close circuit if we get successful calls
        let total = self.total_calls.load(Ordering::SeqCst);
        if total >= 3 {
            let failures = self.failures.load(Ordering::SeqCst);
            let failure_rate = failures as f64 / total as f64;

            if failure_rate < 0.3 && self.is_open.load(Ordering::SeqCst) {
                warn!(
                    "Circuit Breaker: Closing circuit (failure rate: {:.1}%)",
                    failure_rate * 100.0
                );
                self.is_open.store(false, Ordering::SeqCst);
            }
        }
    }

    fn record_failure(&self) {
        self.failures.fetch_add(1, Ordering::SeqCst);
        self.total_calls.fetch_add(1, Ordering::SeqCst);

        // Open circuit if failure rate exceeds threshold
        let total = self.total_calls.load(Ordering::SeqCst);
        if total >= 3 {
            let failures = self.failures.load(Ordering::SeqCst);
            let failure_rate = failures as f64 / total as f64;

            if failure_rate >= 0.5 && !self.is_open.load(Ordering::SeqCst) {
                warn!(
                    "Circuit Breaker: Opening circuit (failure rate: {:.1}%)",
                    failure_rate * 100.0
                );
                self.is_open.store(true, Ordering::SeqCst);
            }
        }
    }

    fn should_allow_call(&self) -> bool {
        !self.is_open.load(Ordering::SeqCst)
    }
}

async fn make_request_with_retry(
    client: &mut GreeterClient<Channel>,
    name: String,
    max_attempts: usize,
) -> Result<String, Status> {
    let mut attempt = 0;
    let mut backoff = Duration::from_millis(100);

    loop {
        attempt += 1;
        let request = Request::new(HelloRequest { name: name.clone() });

        match client.say_hello(request).await {
            Ok(response) => {
                if attempt > 1 {
                    info!("  Retry: Success after {} attempts", attempt);
                }
                return Ok(response.into_inner().message);
            }
            Err(e) => {
                if attempt >= max_attempts {
                    return Err(e);
                }
                warn!(
                    "  Retry: Attempt {} failed, retrying after {:?}",
                    attempt, backoff
                );
                tokio::time::sleep(backoff).await;
                backoff *= 2; // Exponential backoff
            }
        }
    }
}

async fn make_requests() -> Result<(), Box<dyn std::error::Error>> {
    info!("Connecting to gRPC server at http://[::1]:50051");

    // Create base channel with timeout
    let channel = Channel::from_static("http://[::1]:50051")
        .timeout(Duration::from_millis(500))
        .connect()
        .await?;

    let mut client = GreeterClient::new(channel);

    info!("Connected successfully");
    info!("Resilience patterns (manual implementation):");
    info!("  - Circuit Breaker: 50% failure threshold, opens after 3 calls");
    info!("  - Retry: max 3 attempts with exponential backoff (100ms base)");

    let circuit_breaker = Arc::new(CircuitBreakerState::new());
    let success_count = Arc::new(AtomicUsize::new(0));
    let failure_count = Arc::new(AtomicUsize::new(0));
    let circuit_rejected_count = Arc::new(AtomicUsize::new(0));

    info!("\n=== Making 20 requests to demonstrate resilience patterns ===\n");

    for i in 1..=20 {
        info!("Request {}/20: Calling SayHello for User{}", i, i);

        // Check circuit breaker
        if !circuit_breaker.should_allow_call() {
            error!("  Circuit Breaker: Call rejected (circuit is OPEN)");
            circuit_rejected_count.fetch_add(1, Ordering::SeqCst);
            failure_count.fetch_add(1, Ordering::SeqCst);

            // Simulate wait before next attempt
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }

        // Make request with retry
        match make_request_with_retry(&mut client, format!("User{}", i), 3).await {
            Ok(message) => {
                success_count.fetch_add(1, Ordering::SeqCst);
                circuit_breaker.record_success();
                info!("  ✓ Response: {}", message);
            }
            Err(status) => {
                failure_count.fetch_add(1, Ordering::SeqCst);
                circuit_breaker.record_failure();
                error!("  ✗ Error: {}", status);

                // Log error type
                if status.message().contains("timeout") {
                    warn!("  Server timeout detected (chaos injection)");
                } else if status.message().contains("capacity") {
                    warn!("  Server at capacity (bulkhead limit reached)");
                }
            }
        }

        // Small delay between requests to make output readable
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    info!("\n=== Summary ===");
    info!(
        "Successful requests: {}",
        success_count.load(Ordering::SeqCst)
    );
    info!("Failed requests: {}", failure_count.load(Ordering::SeqCst));
    info!(
        "Circuit breaker rejections: {}",
        circuit_rejected_count.load(Ordering::SeqCst)
    );
    info!("\nThe circuit breaker and retry patterns protected the client from cascading failures.");
    info!("Timeouts and server overload were handled gracefully.");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    info!("\nNote: This client demonstrates resilience patterns manually since");
    info!("tonic's Service requirements (non-Clone request bodies) make direct");
    info!("Tower middleware integration complex. The concepts are the same!\n");

    if let Err(e) = make_requests().await {
        error!("Client error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
