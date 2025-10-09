//! Demonstration of ResilienceError - zero-boilerplate error handling.
//!
//! This example shows how to use `ResilienceError<E>` to compose multiple
//! resilience layers without writing any `From` trait implementations.
//!
//! Compare this to examples like `server_api.rs` which require 4+ From impls.
//!
//! Run with: cargo run --example resilience_error_demo --all-features

use std::time::Duration;
use tower::ServiceBuilder;
use tower::{Service, ServiceExt};
use tower_resilience_bulkhead::BulkheadConfig;
use tower_resilience_core::ResilienceError;
use tower_resilience_ratelimiter::RateLimiterConfig;
use tower_resilience_timelimiter::TimeLimiterConfig;

// ============================================================================
// Application Error Type
// ============================================================================

/// Your application's domain errors.
///
/// Notice: No From implementations needed for resilience layer errors!
#[derive(Debug, Clone)]
enum AppError {
    DatabaseDown,
    InvalidRequest(String),
    NotFound,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::DatabaseDown => write!(f, "Database is down"),
            AppError::InvalidRequest(msg) => write!(f, "Invalid request: {}", msg),
            AppError::NotFound => write!(f, "Not found"),
        }
    }
}

impl std::error::Error for AppError {}

// ============================================================================
// Service Error Type - ZERO BOILERPLATE!
// ============================================================================

/// The error type for our resilient service.
///
/// This is all we need! No From implementations required.
type ServiceError = ResilienceError<AppError>;

// ============================================================================
// Mock Service
// ============================================================================

#[derive(Clone)]
struct DatabaseService {
    should_fail: bool,
}

impl Service<String> for DatabaseService {
    type Response = String;
    type Error = AppError;
    type Future =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, AppError>> + Send>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: String) -> Self::Future {
        let should_fail = self.should_fail;
        Box::pin(async move {
            if should_fail {
                Err(AppError::DatabaseDown)
            } else {
                Ok(format!("Response for: {}", req))
            }
        })
    }
}

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() {
    println!("=== ResilienceError Demo ===\n");
    println!("This example demonstrates using ResilienceError<AppError>");
    println!("with multiple resilience layers - NO From implementations needed!\n");

    // Create base service
    let base_service = DatabaseService { should_fail: false };

    // Build resilience stack - notice we're using ServiceError everywhere
    let mut service = ServiceBuilder::new()
        // 1. Time limiter - prevent long-running operations
        .layer(
            TimeLimiterConfig::builder()
                .timeout_duration(Duration::from_secs(2))
                .on_timeout(|| println!("  [TimeLimiter] Timeout!"))
                .build(),
        )
        // 2. Rate limiter - control request rate
        .layer(
            RateLimiterConfig::builder()
                .limit_for_period(5)
                .refresh_period(Duration::from_secs(1))
                .on_permit_rejected(|_| println!("  [RateLimiter] Request rejected!"))
                .build(),
        )
        // 3. Bulkhead - isolate resources
        .layer(
            BulkheadConfig::builder()
                .max_concurrent_calls(3)
                .on_call_rejected(|_| println!("  [Bulkhead] Too many concurrent calls!"))
                .build(),
        )
        .service(base_service);

    println!("✅ Built service with 3 resilience layers");
    println!("✅ Using ResilienceError<AppError> throughout");
    println!("✅ ZERO From implementations written!\n");

    // Test 1: Successful request
    println!("--- Test 1: Successful Request ---");
    match service
        .ready()
        .await
        .unwrap()
        .call("test request".to_string())
        .await
    {
        Ok(response) => println!("✅ Success: {}\n", response),
        Err(e) => println!("❌ Error: {}\n", e),
    }

    // Test 2: Application error
    println!("--- Test 2: Application Error ---");
    let mut failing_service = ServiceBuilder::new()
        .layer(
            TimeLimiterConfig::builder()
                .timeout_duration(Duration::from_secs(2))
                .build(),
        )
        .service(DatabaseService { should_fail: true });

    match failing_service
        .ready()
        .await
        .unwrap()
        .call("test".to_string())
        .await
    {
        Ok(_) => println!("Unexpected success"),
        Err(e) => {
            println!("❌ Error: {}", e);
            match e {
                ResilienceError::Application(app_err) => {
                    println!("   This is an application error: {:?}", app_err);
                }
                _ => println!("   This is a resilience layer error"),
            }
        }
    }
    println!();

    // Demonstrate error matching
    println!("--- Error Matching Capabilities ---");
    demonstrate_error_matching();
    println!();

    println!("=== Demo Complete ===\n");
    println!("Key Takeaways:");
    println!("1. Zero boilerplate - no From implementations");
    println!("2. Works with any number of resilience layers");
    println!("3. Rich error context (layer names, counts, durations)");
    println!("4. Application errors wrapped in Application variant");
    println!("5. Convenient helper methods (is_timeout, is_rate_limited, etc.)");
}

fn demonstrate_error_matching() {
    let errors: Vec<ServiceError> = vec![
        ResilienceError::Timeout {
            layer: "time_limiter",
        },
        ResilienceError::BulkheadFull {
            concurrent_calls: 10,
            max_concurrent: 10,
        },
        ResilienceError::RateLimited { retry_after: None },
        ResilienceError::Application(AppError::DatabaseDown),
    ];

    for err in errors {
        println!("Error: {}", err);
        println!("  is_timeout: {}", err.is_timeout());
        println!("  is_bulkhead_full: {}", err.is_bulkhead_full());
        println!("  is_rate_limited: {}", err.is_rate_limited());
        println!("  is_application: {}", err.is_application());
        println!();
    }
}
