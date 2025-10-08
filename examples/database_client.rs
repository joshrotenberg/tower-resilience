//! Database client with resilience patterns
//!
//! This example demonstrates how to wrap database operations with multiple
//! resilience patterns for production robustness:
//! - Circuit breaker for replica failover
//! - Retry with exponential backoff for transient errors (deadlocks, connection pool exhaustion)
//! - Query timeouts to prevent runaway queries
//!
//! Run with: cargo run --example database_client

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_resilience_circuitbreaker::CircuitBreakerConfig;
use tower_resilience_retry::RetryConfig;

/// Simulated database error types
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum DbError {
    /// Transient error that should be retried (e.g., deadlock, connection timeout)
    Transient(String),
    /// Permanent error that should not be retried (e.g., syntax error, constraint violation)
    Permanent(String),
    /// Query timeout
    Timeout,
}

/// Database query request
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct DbQuery {
    sql: String,
    timeout: Duration,
}

/// Database query result
type DbResult = Result<Vec<String>, DbError>;

/// Simulated database service that can fail in various ways
struct DatabaseService {
    call_count: Arc<AtomicU32>,
    /// Fail first N calls with transient errors
    fail_transient_count: u32,
    /// Fail calls after N with permanent errors
    fail_permanent_after: Option<u32>,
}

impl DatabaseService {
    fn new() -> Self {
        Self {
            call_count: Arc::new(AtomicU32::new(0)),
            fail_transient_count: 0,
            fail_permanent_after: None,
        }
    }

    fn with_transient_failures(mut self, count: u32) -> Self {
        self.fail_transient_count = count;
        self
    }

    async fn execute(&self, query: DbQuery) -> DbResult {
        let call_num = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;

        println!("[DB] Executing query #{}: {}", call_num, query.sql);

        // Simulate transient failures (deadlocks, connection issues)
        if call_num <= self.fail_transient_count {
            println!("[DB] Simulating transient error (call #{})", call_num);
            sleep(Duration::from_millis(10)).await;
            return Err(DbError::Transient(format!(
                "Deadlock detected on attempt {}",
                call_num
            )));
        }

        // Simulate permanent failure
        if let Some(fail_after) = self.fail_permanent_after
            && call_num > fail_after
        {
            println!("[DB] Simulating permanent error (call #{})", call_num);
            return Err(DbError::Permanent("Invalid SQL syntax".to_string()));
        }

        // Simulate query execution time
        sleep(Duration::from_millis(50)).await;

        Ok(vec![format!("Result from query: {}", query.sql)])
    }
}

#[tokio::main]
async fn main() {
    println!("=== Database Client Resilience Example ===\n");

    // Scenario 1: Retry with transient failures
    println!("--- Scenario 1: Retry with Transient Failures ---");
    scenario_retry_transient_errors().await;

    println!("\n--- Scenario 2: Circuit Breaker for Failing Replica ---");
    scenario_circuit_breaker().await;

    println!("\n--- Scenario 3: Full Stack (Timeout + Circuit Breaker + Retry) ---");
    scenario_full_stack().await;
}

async fn scenario_retry_transient_errors() {
    // Database that fails first 2 times with deadlocks, then succeeds
    let db = DatabaseService::new().with_transient_failures(2);
    let call_count = Arc::clone(&db.call_count);

    let base_service = service_fn(move |req: DbQuery| {
        let db = DatabaseService {
            call_count: Arc::clone(&call_count),
            fail_transient_count: 2,
            fail_permanent_after: None,
        };
        async move { db.execute(req).await }
    });

    // Configure retry: retry transient errors with exponential backoff
    let retry_layer = RetryConfig::<DbError>::builder()
        .max_attempts(5)
        .exponential_backoff(Duration::from_millis(100))
        .retry_on(|err: &DbError| {
            // Only retry transient errors, not permanent ones
            matches!(err, DbError::Transient(_))
        })
        .on_retry(|attempt, delay| {
            println!(
                "[Retry] Attempt {} failed, retrying after {:?}",
                attempt, delay
            );
        })
        .build();

    let mut service = retry_layer.layer().layer(base_service);

    let query = DbQuery {
        sql: "SELECT * FROM users WHERE id = 1".to_string(),
        timeout: Duration::from_secs(5),
    };

    match service.ready().await.unwrap().call(query).await {
        Ok(results) => println!("[Success] Got results: {:?}", results),
        Err(e) => println!("[Error] Query failed: {:?}", e),
    }
}

async fn scenario_circuit_breaker() {
    // Database that always fails (simulating a down replica)
    let failing_db_count = Arc::new(AtomicU32::new(0));

    let base_service = service_fn(move |_req: DbQuery| {
        let count = failing_db_count.fetch_add(1, Ordering::SeqCst) + 1;
        async move {
            println!("[DB] Replica query attempt #{}", count);
            sleep(Duration::from_millis(10)).await;
            Err::<Vec<String>, DbError>(DbError::Permanent(
                "Connection refused - replica is down".to_string(),
            ))
        }
    });

    // Configure circuit breaker: open after 50% failure rate over 4 calls
    let circuit_breaker = CircuitBreakerConfig::<Vec<String>, DbError>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(4)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_secs(2))
        .name("db-replica-1")
        .on_state_transition(|from, to| {
            println!("[Circuit Breaker] State transition: {:?} -> {:?}", from, to);
        })
        .on_call_rejected(|| {
            println!("[Circuit Breaker] Call rejected - circuit is open, failing fast");
        })
        .build();

    let mut service = circuit_breaker.layer(base_service);

    // Make several calls - circuit should open after failures
    for i in 1..=6 {
        let query = DbQuery {
            sql: format!("SELECT * FROM users LIMIT {}", i),
            timeout: Duration::from_secs(1),
        };

        println!("\n[Client] Executing query #{}", i);
        match service.ready().await.unwrap().call(query).await {
            Ok(_) => println!("[Client] Query {} succeeded", i),
            Err(e) => println!("[Client] Query {} failed: {:?}", i, e),
        }

        sleep(Duration::from_millis(100)).await;
    }
}

async fn scenario_full_stack() {
    // Database with occasional transient failures
    let db_count = Arc::new(AtomicU32::new(0));

    let base_service = service_fn(move |_req: DbQuery| {
        let count = db_count.fetch_add(1, Ordering::SeqCst) + 1;
        async move {
            println!("[DB] Executing query #{}", count);

            // Every 3rd call fails transiently
            if count.is_multiple_of(3) {
                sleep(Duration::from_millis(10)).await;
                return Err::<Vec<String>, DbError>(DbError::Transient(
                    "Deadlock detected".to_string(),
                ));
            }

            // Normal execution
            sleep(Duration::from_millis(50)).await;
            Ok(vec![format!("Row data {}", count)])
        }
    });

    // Combine circuit breaker with retry
    // Apply retry first (innermost layer), then circuit breaker
    let retry_layer = RetryConfig::<DbError>::builder()
        .max_attempts(3)
        .exponential_backoff(Duration::from_millis(50))
        .retry_on(|err| matches!(err, DbError::Transient(_)))
        .build();

    let circuit_breaker = CircuitBreakerConfig::<Vec<String>, DbError>::builder()
        .failure_rate_threshold(0.6)
        .sliding_window_size(10)
        .minimum_number_of_calls(3)
        .wait_duration_in_open(Duration::from_secs(3))
        .name("primary-db")
        .build();

    // Compose: base -> retry -> circuit breaker
    let service_with_retry = retry_layer.layer().layer(base_service);
    let mut service = circuit_breaker.layer(service_with_retry);

    // Execute several queries
    for i in 1..=8 {
        let query = DbQuery {
            sql: format!("SELECT * FROM orders WHERE user_id = {}", i),
            timeout: Duration::from_secs(1),
        };

        println!("\n[Client] Query {} starting", i);
        match service.ready().await.unwrap().call(query).await {
            Ok(results) => println!("[Client] Query {} succeeded: {:?}", i, results),
            Err(e) => println!("[Client] Query {} failed: {:?}", i, e),
        }

        sleep(Duration::from_millis(100)).await;
    }

    println!("\n=== Summary ===");
    println!("This example showed:");
    println!("1. Retry handling transient database errors (deadlocks)");
    println!("2. Circuit breaker failing fast when replica is down");
    println!("3. Combining circuit breaker with retry for robust error handling");
    println!("\nIn production, you would:");
    println!("- Configure timeouts based on query SLAs");
    println!("- Set circuit breaker thresholds based on acceptable error rates");
    println!("- Only retry idempotent queries or use transaction IDs");
    println!("- Monitor circuit breaker state and retry rates");
    println!("- Add metrics and tracing for observability");
}
