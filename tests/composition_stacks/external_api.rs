//! External API client stack examples.
//!
//! These stacks are designed for calling third-party APIs (Stripe, Twilio, AWS, etc.)

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tower::{Layer, Service, ServiceBuilder, ServiceExt};
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_core::testing::ServiceProbe;
use tower_resilience_fallback::FallbackLayer;
use tower_resilience_hedge::HedgeLayer;
use tower_resilience_retry::RetryLayer;
use tower_resilience_timelimiter::TimeLimiterLayer;

/// Test error type
#[derive(Debug, Clone)]
pub struct ApiError(pub String);

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ApiError: {}", self.0)
    }
}

impl std::error::Error for ApiError {}

/// Test request type
#[derive(Debug, Clone)]
pub struct ApiRequest {
    pub endpoint: String,
}

impl ApiRequest {
    pub fn new(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
        }
    }
}

/// Test response type
#[derive(Debug, Clone)]
pub struct ApiResponse {
    pub body: String,
}

impl ApiResponse {
    pub fn new(body: &str) -> Self {
        Self {
            body: body.to_string(),
        }
    }
}

/// Creates a mock HTTP client service for testing
fn mock_http_client() -> impl Service<ApiRequest, Response = ApiResponse, Error = ApiError> + Clone
{
    tower::service_fn(|req: ApiRequest| async move {
        Ok(ApiResponse {
            body: format!("Response from {}", req.endpoint),
        })
    })
}

/// Minimal stack: Timeout + Retry
#[tokio::test]
async fn minimal_stack_compiles() {
    let retry = RetryLayer::<ApiRequest, ApiResponse, ApiError>::builder()
        .max_attempts(3)
        .exponential_backoff(Duration::from_millis(100))
        .build();

    let timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(10))
        .build();

    let http_client = mock_http_client();

    // Build inside-out: retry is innermost, timeout is outermost
    let _service = ServiceBuilder::new()
        .layer(timeout) // Outermost: bounds total time
        .layer(retry) // Innermost: retries within timeout
        .service(http_client);
}

/// Timeout + Retry drives fresh inner readiness for every retry attempt.
#[tokio::test]
async fn minimal_stack_repolls_readiness_for_internal_attempts() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_service = Arc::clone(&attempts);
    let http_client = tower::service_fn(move |_req: ApiRequest| {
        let attempt = attempts_for_service.fetch_add(1, Ordering::SeqCst);
        async move {
            if attempt < 2 {
                Err(ApiError(format!("transient failure {}", attempt + 1)))
            } else {
                Ok(ApiResponse::new("success on attempt 3"))
            }
        }
    });
    let probe = ServiceProbe::new(http_client);
    let probe_handle = probe.handle();
    let retry = RetryLayer::<ApiRequest, ApiResponse, ApiError>::builder()
        .max_attempts(3)
        .fixed_backoff(Duration::ZERO)
        .build();
    let timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(1))
        .build();

    let with_retry = retry.layer(probe);
    let mut service = timeout.layer(with_retry);
    let response = service
        .ready()
        .await
        .unwrap()
        .call(ApiRequest::new("payments"))
        .await
        .unwrap();

    assert_eq!(response.body, "success on attempt 3");
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    let snapshot = probe_handle.snapshot();
    assert_eq!(snapshot.calls, 3);
    assert_eq!(snapshot.readiness_successes, 3);
    probe_handle.assert_ready_contract();
    probe_handle.assert_quiescent();
}

/// Standard stack: Total Timeout + Retry + CircuitBreaker + Per-attempt Timeout
#[tokio::test]
async fn standard_stack_compiles() {
    let per_attempt_timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(10))
        .build();

    let circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .build()
        .unwrap();

    let retry = RetryLayer::<ApiRequest, ApiResponse, ApiError>::builder()
        .max_attempts(3)
        .exponential_backoff(Duration::from_millis(100))
        .build();

    let total_timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(30))
        .build();

    let http_client = mock_http_client();

    // Manual composition (recommended for 3+ layers)
    let with_timeout = per_attempt_timeout.layer(http_client);
    let with_cb = circuit_breaker.layer(with_timeout);
    let with_retry = retry.layer(with_cb);
    let _service = total_timeout.layer(with_retry);
}

/// Full stack with fallback
#[tokio::test]
async fn full_stack_with_fallback_compiles() {
    let cached_response = ApiResponse {
        body: "Cached fallback response".to_string(),
    };

    let per_attempt_timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(10))
        .build();

    let circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .wait_duration_in_open(Duration::from_secs(30))
        .build()
        .unwrap();

    let retry = RetryLayer::<ApiRequest, ApiResponse, ApiError>::builder()
        .max_attempts(3)
        .exponential_backoff(Duration::from_millis(100))
        .build();

    let total_timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(30))
        .build();

    let fallback = FallbackLayer::<ApiRequest, ApiResponse, ApiError>::value(cached_response);

    let http_client = mock_http_client();

    // Manual composition
    let with_timeout = per_attempt_timeout.layer(http_client);
    let with_cb = circuit_breaker.layer(with_timeout);
    let with_retry = retry.layer(with_cb);
    let with_total_timeout = total_timeout.layer(with_retry);
    let _service = fallback.layer(with_total_timeout);
}

/// Stack with hedging for latency-sensitive idempotent calls.
///
/// Hedge positioning rationale:
/// - Hedge is INSIDE circuit breaker: CB sees hedge failures, preventing
///   a broken service from triggering endless hedge attempts
/// - Hedge is OUTSIDE per-attempt timeout: each hedged request gets its
///   own timeout, so a slow primary doesn't block the hedge from winning
#[tokio::test]
async fn stack_with_hedging_compiles() {
    let per_attempt_timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(10))
        .build();

    let hedge = HedgeLayer::builder()
        .delay(Duration::from_millis(50))
        .max_hedged_attempts(2)
        .build();

    let circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .build()
        .unwrap();

    let retry = RetryLayer::<ApiRequest, ApiResponse, ApiError>::builder()
        .max_attempts(3)
        .exponential_backoff(Duration::from_millis(100))
        .build();

    let total_timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(30))
        .build();

    let http_client = mock_http_client();

    // Manual composition (innermost to outermost):
    // 1. Per-attempt timeout wraps raw client
    // 2. Hedge wraps timeout (each hedge attempt gets own timeout)
    // 3. CB wraps hedge (sees hedge failures, can trip on broken service)
    // 4. Retry wraps CB (retries after CB rejects or hedge fails)
    // 5. Total timeout bounds everything
    let with_timeout = per_attempt_timeout.layer(http_client);
    let with_hedge = hedge.layer(with_timeout);
    let with_cb = circuit_breaker.layer(with_hedge);
    let with_retry = retry.layer(with_cb);
    let _service = total_timeout.layer(with_retry);
}
