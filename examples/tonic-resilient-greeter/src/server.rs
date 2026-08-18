//! gRPC server demonstrating server-side resilience patterns.
//!
//! Real tower-resilience layers protect the greeting logic:
//! - `RateLimiterLayer` caps the accepted request rate (10 req/sec).
//! - `BulkheadLayer` caps in-flight concurrent requests (max 5).
//! - `ChaosLayer` injects latency to exercise the layers above -- and the
//!   client's circuit breaker. This is a *test fixture* that simulates an
//!   unreliable backend; it is not itself a resilience pattern.
//!
//! Tonic requires `Greeter::say_hello` to return `Result<_, tonic::Status>`,
//! not the layer-specific error types tower-resilience middleware produces,
//! so the pipeline below is built once at startup as a real `tower::Service`
//! and invoked from inside the handler, translating its error into a
//! `Status`. This mirrors the pattern in `examples/axum-resilient-kv-store`,
//! which guards a `CircuitBreaker`-wrapped service behind an `Arc<Mutex<_>>`
//! and calls it manually from the handler.
//!
//! Run with: cargo run --bin server

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tonic::{transport::Server, Request, Response, Status};
use tower::util::BoxService;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_resilience_bulkhead::{BulkheadLayer, BulkheadServiceError};
use tower_resilience_chaos::ChaosLayer;
use tower_resilience_ratelimiter::{RateLimiterLayer, RateLimiterServiceError};
use tracing::{info, warn};

pub mod greeter {
    tonic::include_proto!("greeter");
}

use greeter::greeter_server::{Greeter, GreeterServer};
use greeter::{HelloReply, HelloRequest};

/// Error produced by the rate-limiter -> bulkhead -> chaos -> greeting
/// pipeline. The greeting logic itself never fails (`Infallible`); every
/// real failure comes from one of the resilience layers.
type PipelineError = RateLimiterServiceError<BulkheadServiceError<Infallible>>;

type GreetPipeline = BoxService<HelloRequest, HelloReply, PipelineError>;

fn build_pipeline() -> GreetPipeline {
    let greet = tower::service_fn(|req: HelloRequest| async move {
        Ok::<HelloReply, Infallible>(HelloReply {
            message: format!("Hello, {}!", req.name),
        })
    });

    let chaos_layer = ChaosLayer::builder()
        .name("greeter-chaos")
        .latency_rate(0.2)
        .min_latency(Duration::from_secs(2))
        .max_latency(Duration::from_secs(2))
        .on_latency_injected(|delay| warn!("Chaos: injecting slow response ({:?})", delay))
        .build();

    let bulkhead_layer = BulkheadLayer::builder()
        .name("greeter-bulkhead")
        .max_concurrent_calls(5)
        .reject_when_full()
        .on_call_rejected(|active| {
            warn!(
                "Bulkhead: request rejected (server at capacity, {} active)",
                active
            )
        })
        .build();

    let ratelimiter_layer = RateLimiterLayer::builder()
        .name("greeter-ratelimiter")
        .limit_for_period(10)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::ZERO)
        .on_permit_rejected(|retry_after| {
            warn!(
                "RateLimiter: request rejected (retry after {:?})",
                retry_after
            )
        })
        .build();

    let service = ServiceBuilder::new()
        .layer(ratelimiter_layer)
        .layer(bulkhead_layer)
        .layer(chaos_layer)
        .service(greet);

    BoxService::new(service)
}

fn pipeline_error_to_status(err: PipelineError) -> Status {
    match err {
        RateLimiterServiceError::RateLimited => Status::resource_exhausted("rate limit exceeded"),
        RateLimiterServiceError::Inner(BulkheadServiceError::Bulkhead(e)) => {
            Status::resource_exhausted(format!("server at capacity: {e}"))
        }
        RateLimiterServiceError::Inner(BulkheadServiceError::Inner(never)) => match never {},
    }
}

/// Greeter service implementation backed by a real tower-resilience pipeline.
pub struct MyGreeter {
    pipeline: Arc<Mutex<GreetPipeline>>,
}

impl MyGreeter {
    fn new() -> Self {
        Self {
            pipeline: Arc::new(Mutex::new(build_pipeline())),
        }
    }

    async fn handle(&self, req: HelloRequest) -> Result<HelloReply, Status> {
        let mut pipeline = self.pipeline.lock().await;
        let ready = pipeline.ready().await.map_err(pipeline_error_to_status)?;
        ready.call(req).await.map_err(pipeline_error_to_status)
    }
}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let req = request.into_inner();
        info!("Received SayHello request for: {}", req.name);

        let reply = self.handle(req).await?;
        Ok(Response::new(reply))
    }

    type SayHelloStreamStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn say_hello_stream(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Self::SayHelloStreamStream>, Status> {
        let req = request.into_inner();
        info!("Received SayHelloStream request for: {}", req.name);

        // Admission-gate the stream through the same resilience pipeline
        // used for unary calls (rate limiter, bulkhead, chaos), then stream
        // the reply messages independently of the pipeline.
        self.handle(req.clone()).await?;

        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let name = req.name;

        tokio::spawn(async move {
            for i in 0..5 {
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
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    let addr = "[::1]:50051".parse()?;
    let greeter = MyGreeter::new();

    info!("Starting gRPC server with a real tower-resilience pipeline");
    info!("  - RateLimiter: 10 requests/sec");
    info!("  - Bulkhead: max 5 concurrent requests");
    info!("  - Chaos: 20% chance of 2s injected latency (test fixture, not a resilience pattern)");
    info!("Listening on {}", addr);

    Server::builder()
        .add_service(GreeterServer::new(greeter))
        .serve(addr)
        .await?;

    Ok(())
}
