//! Message queue stack examples.
//!
//! These stacks are designed for Kafka, RabbitMQ, SQS, etc.

use std::time::Duration;

use tower::{Layer, Service};
use tower_resilience_bulkhead::BulkheadLayer;
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_retry::RetryLayer;
use tower_resilience_timelimiter::TimeLimiterLayer;

/// Test error type for message operations
#[derive(Debug, Clone)]
struct MessageError {
    retriable: bool,
    message: String,
}

impl MessageError {
    fn retriable(msg: &str) -> Self {
        Self {
            retriable: true,
            message: msg.to_string(),
        }
    }
}

impl std::fmt::Display for MessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MessageError: {}", self.message)
    }
}

impl std::error::Error for MessageError {}

/// Incoming message to be processed
#[derive(Debug, Clone)]
struct Message {
    id: String,
    payload: Vec<u8>,
}

/// Result of processing a message
#[derive(Debug, Clone)]
struct ProcessResult {
    success: bool,
}

/// Message to be published
#[derive(Debug, Clone)]
struct PublishRequest {
    topic: String,
    payload: Vec<u8>,
}

/// Result of publishing
#[derive(Debug, Clone)]
struct PublishResult {
    message_id: String,
}

/// Creates a mock message handler service
fn mock_message_handler()
-> impl Service<Message, Response = ProcessResult, Error = MessageError> + Clone {
    tower::service_fn(|_msg: Message| async move { Ok(ProcessResult { success: true }) })
}

/// Creates a mock queue producer service
fn mock_queue_producer()
-> impl Service<PublishRequest, Response = PublishResult, Error = MessageError> + Clone {
    tower::service_fn(|_req: PublishRequest| async move {
        Ok(PublishResult {
            message_id: "msg-123".to_string(),
        })
    })
}

/// Consumer stack: Timeout + Retry (with backoff) + CircuitBreaker
#[tokio::test]
async fn consumer_stack_compiles() {
    let circuit_breaker = CircuitBreakerLayer::<Message, MessageError>::builder()
        .failure_rate_threshold(0.5)
        .wait_duration_in_open(Duration::from_secs(60))
        .build();

    let retry = RetryLayer::<Message, MessageError>::builder()
        .max_attempts(5)
        .exponential_backoff(Duration::from_secs(1))
        .build();

    let timeout = TimeLimiterLayer::<Message>::builder()
        .timeout_duration(Duration::from_secs(30))
        .build();

    let message_handler = mock_message_handler();

    // Manual composition
    let with_cb = circuit_breaker.layer::<_, Message>(message_handler);
    let with_retry = retry.layer(with_cb);
    let _service = timeout.layer(with_retry);
}

/// Producer stack: Timeout + Retry + Bulkhead
#[tokio::test]
async fn producer_stack_compiles() {
    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(50).build();

    let retry = RetryLayer::<PublishRequest, MessageError>::builder()
        .max_attempts(3)
        .exponential_backoff(Duration::from_millis(100))
        .build();

    let timeout = TimeLimiterLayer::<PublishRequest>::builder()
        .timeout_duration(Duration::from_secs(5))
        .build();

    let queue_producer = mock_queue_producer();

    // Manual composition
    let with_bulkhead = bulkhead.layer(queue_producer);
    let with_retry = retry.layer(with_bulkhead);
    let _service = timeout.layer(with_retry);
}

/// Consumer with retry predicate
#[tokio::test]
async fn consumer_with_retry_predicate_compiles() {
    let retry = RetryLayer::<Message, MessageError>::builder()
        .max_attempts(3)
        .retry_on(|e: &MessageError| e.retriable)
        .exponential_backoff(Duration::from_secs(1))
        .build();

    let timeout = TimeLimiterLayer::<Message>::builder()
        .timeout_duration(Duration::from_secs(30))
        .build();

    let message_handler = mock_message_handler();

    let with_retry = retry.layer(message_handler);
    let _service = timeout.layer(with_retry);
}
