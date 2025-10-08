//! Message queue worker with resilience patterns
//!
//! This example demonstrates resilience patterns for processing messages from a queue:
//! - Retry with exponential backoff for transient failures
//! - Bulkhead to limit concurrent message processing
//! - Circuit breaker when downstream service fails
//! - Timeout for message processing
//!
//! Run with: cargo run --example message_queue_worker

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::time::sleep;
use tower::{Layer, Service, ServiceBuilder, ServiceExt, service_fn};
use tower_resilience_bulkhead::BulkheadConfig;
use tower_resilience_circuitbreaker::CircuitBreakerConfig;
use tower_resilience_retry::RetryConfig;

/// Message from the queue
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Message {
    id: String,
    payload: String,
    attempt: u32,
}

/// Processing result
#[derive(Debug, Clone)]
enum ProcessingResult {
    Success,
    Ack, // Acknowledged and processed
}

/// Processing errors
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum ProcessingError {
    /// Transient error - should retry (network timeout, rate limit, etc.)
    Transient(String),
    /// Permanent error - should not retry (validation error, etc.)
    Permanent(String),
    /// Downstream service unavailable
    DownstreamUnavailable,
    /// Processing timeout
    Timeout,
    /// Bulkhead full
    BulkheadFull,
    /// Circuit breaker open
    CircuitOpen,
}

impl From<tower_resilience_bulkhead::BulkheadError> for ProcessingError {
    fn from(err: tower_resilience_bulkhead::BulkheadError) -> Self {
        match err {
            tower_resilience_bulkhead::BulkheadError::Timeout => ProcessingError::Timeout,
            tower_resilience_bulkhead::BulkheadError::BulkheadFull { .. } => {
                ProcessingError::BulkheadFull
            }
        }
    }
}

impl<E> From<tower_resilience_circuitbreaker::CircuitBreakerError<E>> for ProcessingError {
    fn from(_: tower_resilience_circuitbreaker::CircuitBreakerError<E>) -> Self {
        ProcessingError::CircuitOpen
    }
}

impl<E> From<tower_resilience_timelimiter::TimeLimiterError<E>> for ProcessingError {
    fn from(_: tower_resilience_timelimiter::TimeLimiterError<E>) -> Self {
        ProcessingError::Timeout
    }
}

/// Simulated downstream service (e.g., database, API, etc.)
struct DownstreamService {
    call_count: Arc<AtomicU32>,
    fail_mode: FailMode,
}

#[derive(Clone)]
enum FailMode {
    None,
    TransientFirst(u32), // Fail first N calls with transient errors
    AlwaysFail,          // Always fail (for circuit breaker demo)
}

impl DownstreamService {
    fn new(fail_mode: FailMode) -> Self {
        Self {
            call_count: Arc::new(AtomicU32::new(0)),
            fail_mode,
        }
    }

    async fn process(&self, msg: &Message) -> Result<ProcessingResult, ProcessingError> {
        let call_num = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;

        println!(
            "[Downstream] Processing message {} (attempt {})",
            msg.id, call_num
        );

        match &self.fail_mode {
            FailMode::None => {
                sleep(Duration::from_millis(100)).await;
                Ok(ProcessingResult::Success)
            }
            FailMode::TransientFirst(count) => {
                if call_num <= *count {
                    sleep(Duration::from_millis(50)).await;
                    println!("[Downstream] Transient failure on attempt {}", call_num);
                    Err(ProcessingError::Transient("Network timeout".to_string()))
                } else {
                    sleep(Duration::from_millis(100)).await;
                    Ok(ProcessingResult::Success)
                }
            }
            FailMode::AlwaysFail => {
                sleep(Duration::from_millis(50)).await;
                Err(ProcessingError::DownstreamUnavailable)
            }
        }
    }
}

/// Message processor
async fn process_message(
    msg: Message,
    downstream: Arc<DownstreamService>,
) -> Result<ProcessingResult, ProcessingError> {
    println!(
        "[Worker] Processing message: {} (payload: {})",
        msg.id, msg.payload
    );

    // Validate message
    if msg.payload.is_empty() {
        return Err(ProcessingError::Permanent("Empty payload".to_string()));
    }

    // Process with downstream service
    downstream.process(&msg).await?;

    println!("[Worker] Successfully processed message {}", msg.id);
    Ok(ProcessingResult::Ack)
}

#[tokio::main]
async fn main() {
    println!("=== Message Queue Worker Resilience Example ===\n");

    println!("--- Scenario 1: Retry with Exponential Backoff ---");
    scenario_retry_transient().await;

    println!("\n--- Scenario 2: Bulkhead for Concurrent Processing ---");
    scenario_bulkhead().await;

    println!("\n--- Scenario 3: Circuit Breaker for Downstream Failures ---");
    scenario_circuit_breaker().await;

    println!("\n--- Scenario 4: Full Worker Stack ---");
    scenario_full_worker_stack().await;
}

async fn scenario_retry_transient() {
    // Downstream that fails first 2 times, then succeeds
    let downstream = Arc::new(DownstreamService::new(FailMode::TransientFirst(2)));

    let base_service = service_fn(move |msg: Message| {
        let downstream = Arc::clone(&downstream);
        async move { process_message(msg, downstream).await }
    });

    // Configure retry with exponential backoff
    let retry_layer = RetryConfig::<ProcessingError>::builder()
        .max_attempts(5)
        .exponential_backoff(Duration::from_millis(100))
        .retry_on(|err| {
            // Only retry transient errors
            matches!(
                err,
                ProcessingError::Transient(_) | ProcessingError::DownstreamUnavailable
            )
        })
        .on_retry(|attempt, delay| {
            println!(
                "[Retry] Attempt {} failed, retrying after {:?}",
                attempt, delay
            );
        })
        .on_error(|attempts| {
            println!("[Retry] Exhausted all {} attempts", attempts);
        })
        .build();

    let mut service = retry_layer.layer().layer(base_service);

    let msg = Message {
        id: "msg-001".to_string(),
        payload: "Order confirmation".to_string(),
        attempt: 1,
    };

    match service.ready().await.unwrap().call(msg).await {
        Ok(_) => println!("[Success] Message processed and acknowledged"),
        Err(e) => println!("[Error] Failed to process message: {:?}", e),
    }
}

async fn scenario_bulkhead() {
    let downstream = Arc::new(DownstreamService::new(FailMode::None));
    let msg_counter = Arc::new(AtomicU32::new(0));

    let base_service = service_fn(move |msg: Message| {
        let downstream = Arc::clone(&downstream);
        let counter = msg_counter.fetch_add(1, Ordering::SeqCst) + 1;
        async move {
            println!("[Worker] Started processing message #{}", counter);
            let result = process_message(msg, downstream).await;
            println!("[Worker] Completed processing message #{}", counter);
            result
        }
    });

    // Configure bulkhead: process max 2 messages concurrently
    let bulkhead = BulkheadConfig::builder()
        .max_concurrent_calls(2)
        .max_wait_duration(Some(Duration::from_millis(500)))
        .name("message-processor")
        .on_call_permitted(|current| {
            println!("[Bulkhead] Message permitted (concurrent: {})", current);
        })
        .on_call_rejected(|max| {
            println!("[Bulkhead] Message rejected - max {} workers busy", max);
        })
        .build();

    let service = bulkhead.layer(base_service);

    // Simulate 5 messages arriving rapidly
    let mut handles = vec![];

    for i in 1..=5 {
        let mut svc = service.clone();
        let handle = tokio::spawn(async move {
            let msg = Message {
                id: format!("msg-{:03}", i),
                payload: format!("Data payload {}", i),
                attempt: 1,
            };

            println!("\n[Queue] Message {} available", i);
            match svc.ready().await.unwrap().call(msg).await {
                Ok(_) => println!("[Queue] Message {} acknowledged", i),
                Err(e) => println!("[Queue] Message {} failed: {:?}", i, e),
            }
        });
        handles.push(handle);

        sleep(Duration::from_millis(30)).await;
    }

    for handle in handles {
        let _ = handle.await;
    }

    println!("\n[Note] Only 2 messages processed concurrently, others wait");
}

async fn scenario_circuit_breaker() {
    // Downstream that always fails (simulating service outage)
    let downstream = Arc::new(DownstreamService::new(FailMode::AlwaysFail));

    let base_service = service_fn(move |msg: Message| {
        let downstream = Arc::clone(&downstream);
        async move { process_message(msg, downstream).await }
    });

    // Configure circuit breaker
    let circuit_breaker = CircuitBreakerConfig::<ProcessingResult, ProcessingError>::builder()
        .failure_rate_threshold(0.5)
        .sliding_window_size(4)
        .minimum_number_of_calls(2)
        .wait_duration_in_open(Duration::from_secs(2))
        .name("downstream-api")
        .on_state_transition(|from, to| {
            println!("[Circuit Breaker] State: {:?} -> {:?}", from, to);
        })
        .on_call_rejected(|| {
            println!("[Circuit Breaker] Call rejected - circuit open, pausing message processing");
        })
        .build();

    let mut service = circuit_breaker.layer(base_service);

    // Try to process several messages
    for i in 1..=6 {
        let msg = Message {
            id: format!("msg-{:03}", i),
            payload: format!("Event data {}", i),
            attempt: 1,
        };

        println!("\n[Queue] Processing message {}", i);
        match service.ready().await.unwrap().call(msg).await {
            Ok(_) => println!("[Queue] Message {} acknowledged", i),
            Err(e) => println!("[Queue] Message {} failed: {:?}", i, e),
        }

        sleep(Duration::from_millis(100)).await;
    }

    println!("\n[Note] Circuit opened after failures, preventing wasted processing");
}

async fn scenario_full_worker_stack() {
    // Downstream with occasional transient failures
    let downstream = Arc::new(DownstreamService::new(FailMode::TransientFirst(2)));
    let msg_counter = Arc::new(AtomicU32::new(0));

    // Full worker stack: Bulkhead -> Retry
    // Note: Complex multi-layer stacks can hit trait bound limitations
    let service = ServiceBuilder::new()
        // 1. Bulkhead (outermost) - limit concurrent workers
        .layer(
            BulkheadConfig::builder()
                .max_concurrent_calls(3)
                .max_wait_duration(Some(Duration::from_secs(1)))
                .name("worker-pool")
                .on_call_permitted(|current| {
                    println!("[Bulkhead] Worker permitted (concurrent: {})", current);
                })
                .build(),
        )
        // 2. Retry - handle transient failures
        .layer(
            RetryConfig::<ProcessingError>::builder()
                .max_attempts(3)
                .exponential_backoff(Duration::from_millis(100))
                .retry_on(|err| {
                    matches!(
                        err,
                        ProcessingError::Transient(_) | ProcessingError::DownstreamUnavailable
                    )
                })
                .on_retry(|attempt, _| {
                    println!("[Retry] Retrying after attempt {}", attempt);
                })
                .build()
                .layer(),
        )
        .service_fn(move |msg: Message| {
            let downstream = Arc::clone(&downstream);
            let counter = msg_counter.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                println!("[Worker] Message #{} processing started", counter);
                process_message(msg, downstream).await
            }
        });

    let mut service = service;

    // Simulate processing a batch of messages
    for i in 1..=8 {
        let msg = Message {
            id: format!("msg-{:03}", i),
            payload: format!("Batch data item {}", i),
            attempt: 1,
        };

        println!("\n[Queue] Message {} received", i);
        match service.ready().await.unwrap().call(msg).await {
            Ok(_) => println!("[Queue] Message {} acknowledged", i),
            Err(e) => println!("[Queue] Message {} failed: {:?}", i, e),
        }

        sleep(Duration::from_millis(150)).await;
    }

    println!("\n=== Summary ===");
    println!("This example showed message queue worker resilience:");
    println!("1. Bulkhead to limit concurrent message processing");
    println!("2. Retry with exponential backoff for transient failures");
    println!("\nIn production, you would:");
    println!("- Use dead letter queues for permanently failed messages");
    println!("- Add jitter to retry backoff to prevent thundering herd");
    println!("- Set bulkhead limits based on downstream capacity");
    println!("- Add circuit breakers and timeouts at different layers");
    println!("- Track message processing latency and success rates");
    println!("\nNote: Complex multi-layer stacks may require manual composition");
    println!("      or applying patterns at different architectural points");
}
