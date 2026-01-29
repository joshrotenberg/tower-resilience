//! Internal microservices stack examples.
//!
//! These stacks are designed for calling other services you control.

use std::time::Duration;

use tower::{Layer, Service, ServiceBuilder};
use tower_resilience_adaptive::{AdaptiveLimiterLayer, Vegas};
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_retry::RetryLayer;
use tower_resilience_timelimiter::TimeLimiterLayer;

/// Test error type for gRPC/internal service calls
#[derive(Debug, Clone)]
struct ServiceError {
    code: i32,
    message: String,
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ServiceError({}): {}", self.code, self.message)
    }
}

impl std::error::Error for ServiceError {}

/// Test request type
#[derive(Debug, Clone)]
struct GrpcRequest {
    method: String,
    payload: Vec<u8>,
}

/// Test response type
#[derive(Debug, Clone)]
struct GrpcResponse {
    payload: Vec<u8>,
}

/// Creates a mock gRPC client service
fn mock_grpc_client()
-> impl Service<GrpcRequest, Response = GrpcResponse, Error = ServiceError> + Clone {
    tower::service_fn(|_req: GrpcRequest| async move {
        Ok(GrpcResponse {
            payload: vec![1, 2, 3],
        })
    })
}

/// Standard microservices stack: Timeout + Retry + CircuitBreaker
#[tokio::test]
async fn standard_microservices_stack_compiles() {
    let circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.6)
        .slow_call_rate_threshold(0.8)
        .slow_call_duration_threshold(Duration::from_secs(2))
        .build();

    let retry = RetryLayer::<GrpcRequest, ServiceError>::builder()
        .max_attempts(2)
        .fixed_backoff(Duration::from_millis(50))
        .build();

    let timeout = TimeLimiterLayer::<GrpcRequest>::builder()
        .timeout_duration(Duration::from_secs(5))
        .build();

    let grpc_client = mock_grpc_client();

    // Manual composition
    let with_cb = circuit_breaker.layer(grpc_client);
    let with_retry = retry.layer(with_cb);
    let _service = timeout.layer(with_retry);
}

/// Microservices stack with adaptive concurrency
#[tokio::test]
async fn microservices_with_adaptive_concurrency_compiles() {
    let retry = RetryLayer::<GrpcRequest, ServiceError>::builder()
        .max_attempts(2)
        .build();

    let adaptive = AdaptiveLimiterLayer::new(Vegas::builder().build());

    let timeout = TimeLimiterLayer::<GrpcRequest>::builder()
        .timeout_duration(Duration::from_secs(5))
        .build();

    let grpc_client = mock_grpc_client();

    // Manual composition
    let with_retry = retry.layer(grpc_client);
    let with_adaptive = adaptive.layer(with_retry);
    let _service = timeout.layer(with_adaptive);
}

/// Two-layer stack via ServiceBuilder
#[tokio::test]
async fn two_layer_servicebuilder_compiles() {
    let timeout = TimeLimiterLayer::<GrpcRequest>::builder()
        .timeout_duration(Duration::from_secs(5))
        .build();

    let retry = RetryLayer::<GrpcRequest, ServiceError>::builder()
        .max_attempts(2)
        .build();

    let grpc_client = mock_grpc_client();

    let _service = ServiceBuilder::new()
        .layer(timeout)
        .layer(retry)
        .service(grpc_client);
}
