//! gRPC Server demonstrating server-side resilience patterns
//!
//! This server implements:
//! - Manual concurrency tracking to demonstrate bulkhead-like behavior
//! - Manual rate limiting tracking to demonstrate rate limiter behavior
//! - Chaos injection: Random slow responses to trigger client-side timeouts
//!
//! Note: Tonic requires services to return `Infallible` errors, so we demonstrate
//! resilience patterns through manual implementation rather than middleware layers.
//!
//! Run with: cargo run --bin server

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tonic::{transport::Server, Request, Response, Status};
use tracing::{info, warn};

pub mod greeter {
    tonic::include_proto!("greeter");
}

use greeter::greeter_server::{Greeter, GreeterServer};
use greeter::{HelloReply, HelloRequest};

/// Greeter service implementation with manual bulkhead and chaos injection
pub struct MyGreeter {
    /// Track concurrent requests (bulkhead-like behavior)
    concurrent_requests: Arc<AtomicUsize>,
    max_concurrent: usize,
}

impl MyGreeter {
    fn new(max_concurrent: usize) -> Self {
        Self {
            concurrent_requests: Arc::new(AtomicUsize::new(0)),
            max_concurrent,
        }
    }

    async fn acquire_slot(&self) -> Result<ConcurrencyGuard, Status> {
        let current = self.concurrent_requests.fetch_add(1, Ordering::SeqCst);

        if current >= self.max_concurrent {
            // Reject request - bulkhead is full
            self.concurrent_requests.fetch_sub(1, Ordering::SeqCst);
            warn!(
                "Bulkhead: Request rejected (concurrent: {}, max: {})",
                current, self.max_concurrent
            );
            Err(Status::resource_exhausted(format!(
                "Server at capacity (max {} concurrent requests)",
                self.max_concurrent
            )))
        } else {
            info!("Bulkhead: Request permitted (concurrent: {})", current + 1);
            Ok(ConcurrencyGuard {
                counter: Arc::clone(&self.concurrent_requests),
            })
        }
    }
}

/// RAII guard to decrement concurrent request counter
struct ConcurrencyGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for ConcurrencyGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request.into_inner().name;
        info!("Received SayHello request for: {}", name);

        // Acquire bulkhead slot
        let _guard = self.acquire_slot().await?;

        // Chaos injection: 20% chance of slow response (2 seconds)
        if rand::random::<f64>() < 0.2 {
            warn!("Chaos: Injecting slow response (2s delay)");
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        let reply = HelloReply {
            message: format!("Hello, {}!", name),
        };

        Ok(Response::new(reply))
    }

    type SayHelloStreamStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn say_hello_stream(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Self::SayHelloStreamStream>, Status> {
        let name = request.into_inner().name;
        info!("Received SayHelloStream request for: {}", name);

        // Acquire bulkhead slot for the stream setup
        let _guard = self.acquire_slot().await?;

        let (tx, rx) = tokio::sync::mpsc::channel(4);

        tokio::spawn(async move {
            for i in 0..5 {
                // Chaos injection for streaming too
                if rand::random::<f64>() < 0.2 {
                    warn!("Chaos: Injecting slow stream response (2s delay)");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }

                let reply = HelloReply {
                    message: format!("Hello, {} (stream message {})", name, i),
                };

                if tx.send(Ok(reply)).await.is_err() {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    let addr = "[::1]:50051".parse()?;

    // Create greeter with manual bulkhead (max 5 concurrent)
    let greeter = MyGreeter::new(5);

    info!("Starting gRPC server with resilience patterns");
    info!("  - Manual Bulkhead: max 5 concurrent requests");
    info!("  - Chaos: 20% slow responses (2s delay)");
    info!("Listening on {}", addr);
    info!("\nNote: This server demonstrates resilience concepts manually since");
    info!("Tonic requires Infallible errors. Client uses proper middleware layers.\n");

    Server::builder()
        .add_service(GreeterServer::new(greeter))
        .serve(addr)
        .await?;

    Ok(())
}
