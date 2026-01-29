//! Database connection stack examples.
//!
//! These stacks are designed for database clients (PostgreSQL, MySQL, etc.)

use std::time::Duration;

use tower::{Layer, Service, ServiceBuilder};
use tower_resilience_bulkhead::BulkheadLayer;
use tower_resilience_circuitbreaker::CircuitBreakerLayer;
use tower_resilience_retry::RetryLayer;
use tower_resilience_timelimiter::TimeLimiterLayer;

/// Test error type for database operations
#[derive(Debug, Clone)]
struct DbError {
    kind: DbErrorKind,
    message: String,
}

#[derive(Debug, Clone, PartialEq)]
enum DbErrorKind {
    ConnectionReset,
    Deadlock,
    Timeout,
    QueryFailed,
}

impl DbError {
    fn is_transient(&self) -> bool {
        matches!(
            self.kind,
            DbErrorKind::ConnectionReset | DbErrorKind::Deadlock | DbErrorKind::Timeout
        )
    }
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DbError({:?}): {}", self.kind, self.message)
    }
}

impl std::error::Error for DbError {}

/// Test query type
#[derive(Debug, Clone)]
struct Query {
    sql: String,
}

/// Test query result type
#[derive(Debug, Clone)]
struct QueryResult {
    rows: Vec<String>,
}

/// Creates a mock database client service
fn mock_db_client() -> impl Service<Query, Response = QueryResult, Error = DbError> + Clone {
    tower::service_fn(|_query: Query| async move {
        Ok(QueryResult {
            rows: vec!["row1".to_string(), "row2".to_string()],
        })
    })
}

/// Standard database stack: Timeout + Retry (transient only) + Bulkhead
#[tokio::test]
async fn standard_database_stack_compiles() {
    let bulkhead = BulkheadLayer::builder()
        .max_concurrent_calls(20) // Match connection pool size
        .build();

    let retry = RetryLayer::<Query, DbError>::builder()
        .max_attempts(2)
        .retry_on(|e: &DbError| e.is_transient())
        .build();

    let timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(5))
        .build();

    let db_client = mock_db_client();

    // Manual composition
    let with_bulkhead = bulkhead.layer(db_client);
    let with_retry = retry.layer(with_bulkhead);
    let _service = timeout.layer(with_retry);
}

/// Database stack with circuit breaker (for replicas)
#[tokio::test]
async fn database_with_circuit_breaker_compiles() {
    let bulkhead = BulkheadLayer::builder().max_concurrent_calls(20).build();

    let circuit_breaker = CircuitBreakerLayer::builder()
        .failure_rate_threshold(0.5)
        .minimum_number_of_calls(10)
        .build();

    let timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(5))
        .build();

    let db_client = mock_db_client();

    // Manual composition
    let with_bulkhead = bulkhead.layer(db_client);
    let with_cb = circuit_breaker.layer(with_bulkhead);
    let _service = timeout.layer(with_cb);
}

/// Two-layer stack via ServiceBuilder (should work reliably)
#[tokio::test]
async fn two_layer_servicebuilder_compiles() {
    let timeout = TimeLimiterLayer::builder()
        .timeout_duration(Duration::from_secs(5))
        .build();

    let retry = RetryLayer::<Query, DbError>::builder()
        .max_attempts(2)
        .build();

    let db_client = mock_db_client();

    let _service = ServiceBuilder::new()
        .layer(timeout)
        .layer(retry)
        .service(db_client);
}
