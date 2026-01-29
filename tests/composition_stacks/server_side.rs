//! Server-side (inbound) stack examples.
//!
//! These stacks are designed for protecting your service from incoming traffic.

use std::time::Duration;

use tower::{Layer, Service};
use tower_resilience_bulkhead::BulkheadLayer;
use tower_resilience_ratelimiter::RateLimiterLayer;
use tower_resilience_timelimiter::TimeLimiterLayer;

/// Test error type for request handling
#[derive(Debug, Clone)]
struct HandlerError(String);

impl std::fmt::Display for HandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HandlerError: {}", self.0)
    }
}

impl std::error::Error for HandlerError {}

/// Incoming HTTP request
#[derive(Debug, Clone)]
struct HttpRequest {
    path: String,
    tenant_id: Option<String>,
}

/// HTTP response
#[derive(Debug, Clone)]
struct HttpResponse {
    status: u16,
    body: String,
}

/// Creates a mock request handler service
fn mock_handler() -> impl Service<HttpRequest, Response = HttpResponse, Error = HandlerError> + Clone
{
    tower::service_fn(|req: HttpRequest| async move {
        Ok(HttpResponse {
            status: 200,
            body: format!("OK: {}", req.path),
        })
    })
}

/// Server-side stack: RateLimiter + Bulkhead + Timeout
#[tokio::test]
async fn server_side_stack_compiles() {
    let timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(30))
        .build();

    let bulkhead = BulkheadLayer::builder()
        .max_concurrent_calls(100)
        .max_wait_duration(Duration::from_secs(1))
        .build();

    let rate_limiter = RateLimiterLayer::builder()
        .limit_for_period(1000)
        .refresh_period(Duration::from_secs(1))
        .build();

    let handler = mock_handler();

    // Manual composition (outside-in for server-side)
    // Rate limiter is outermost: reject over-limit immediately
    // Bulkhead is next: isolate resources
    // Timeout is innermost: bound handler execution
    let with_timeout = timeout.layer(handler);
    let with_bulkhead = bulkhead.layer(with_timeout);
    let _service = rate_limiter.layer(with_bulkhead);
}

/// Rate limiter only (simplest server protection)
#[tokio::test]
async fn rate_limiter_only_compiles() {
    let rate_limiter = RateLimiterLayer::builder()
        .limit_for_period(100)
        .refresh_period(Duration::from_secs(1))
        .timeout_duration(Duration::from_millis(100)) // Wait up to 100ms for permit
        .build();

    let handler = mock_handler();

    let _service = rate_limiter.layer(handler);
}

/// Bulkhead for tenant isolation
#[tokio::test]
async fn bulkhead_tenant_isolation_compiles() {
    let bulkhead = BulkheadLayer::builder()
        .max_concurrent_calls(10) // Per-tenant limit
        .max_wait_duration(Duration::from_secs(5))
        .build();

    let timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(30))
        .build();

    let handler = mock_handler();

    let with_timeout = timeout.layer(handler);
    let _service = bulkhead.layer(with_timeout);
}

/// Timeout only (prevent runaway requests)
#[tokio::test]
async fn timeout_only_compiles() {
    let timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(30))
        .cancel_running_future(true)
        .build();

    let handler = mock_handler();

    let _service = timeout.layer(handler);
}
