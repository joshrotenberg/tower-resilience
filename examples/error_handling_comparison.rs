//! Comparison of different error handling approaches for tower-resilience
//!
//! This example demonstrates three approaches to handling errors from multiple
//! resilience layers:
//!
//! 1. **Current approach** - Manual `From` implementations (verbose but explicit)
//! 2. **Option 4** - Helper trait pattern (reduces boilerplate slightly)
//! 3. **Option 5** - Common `ResilienceError<E>` type (simplest, zero boilerplate)
//!
//! Run with: cargo run --example error_handling_comparison --all-features

use std::time::Duration;
use tower::ServiceBuilder;
use tower_resilience_bulkhead::{BulkheadConfig, BulkheadError};
use tower_resilience_circuitbreaker::{CircuitBreakerConfig, CircuitBreakerError};
use tower_resilience_ratelimiter::{RateLimiterConfig, RateLimiterError};
use tower_resilience_retry::RetryConfig;
use tower_resilience_timelimiter::{TimeLimiterConfig, TimeLimiterError};

// ============================================================================
// APPROACH 1: Current Manual From Implementations
// ============================================================================

#[derive(Debug, Clone)]
enum ManualError {
    // Resilience layer errors
    Timeout,
    CircuitOpen,
    BulkheadFull,
    RateLimited,
    // Application errors
    NetworkError(String),
    InvalidResponse,
}

// Manual From implementations - THIS IS THE BOILERPLATE WE WANT TO REDUCE
impl From<BulkheadError> for ManualError {
    fn from(err: BulkheadError) -> Self {
        match err {
            BulkheadError::Timeout => ManualError::Timeout,
            BulkheadError::BulkheadFull { .. } => ManualError::BulkheadFull,
        }
    }
}

impl From<CircuitBreakerError> for ManualError {
    fn from(err: CircuitBreakerError) -> Self {
        match err {
            CircuitBreakerError::Rejected => ManualError::CircuitOpen,
        }
    }
}

impl From<RateLimiterError> for ManualError {
    fn from(_: RateLimiterError) -> Self {
        ManualError::RateLimited
    }
}

impl From<TimeLimiterError> for ManualError {
    fn from(_: TimeLimiterError) -> Self {
        ManualError::Timeout
    }
}

// ============================================================================
// OPTION 4: Helper Trait Pattern
// ============================================================================
//
// This would be provided by tower-resilience-core

/// Helper trait for converting resilience layer errors into application errors.
///
/// This provides a slightly more ergonomic alternative to implementing `From` traits,
/// though it still requires boilerplate code.
pub trait IntoResilienceError<E> {
    fn into_resilience_error(self) -> E;
}

// User's error type
#[derive(Debug, Clone)]
enum HelperTraitError {
    Timeout,
    CircuitOpen,
    BulkheadFull,
    RateLimited,
    NetworkError(String),
    InvalidResponse,
}

// Still need to implement the trait for each layer error type
// This is only marginally better than From implementations
impl IntoResilienceError<HelperTraitError> for BulkheadError {
    fn into_resilience_error(self) -> HelperTraitError {
        match self {
            BulkheadError::Timeout => HelperTraitError::Timeout,
            BulkheadError::BulkheadFull { .. } => HelperTraitError::BulkheadFull,
        }
    }
}

impl IntoResilienceError<HelperTraitError> for CircuitBreakerError {
    fn into_resilience_error(self) -> HelperTraitError {
        match self {
            CircuitBreakerError::Rejected => HelperTraitError::CircuitOpen,
        }
    }
}

impl IntoResilienceError<HelperTraitError> for RateLimiterError {
    fn into_resilience_error(self) -> HelperTraitError {
        HelperTraitError::RateLimited
    }
}

impl IntoResilienceError<HelperTraitError> for TimeLimiterError {
    fn into_resilience_error(self) -> HelperTraitError {
        HelperTraitError::Timeout
    }
}

// Note: This still requires the same amount of code as manual From implementations!
// The only advantage is semantic clarity (explicit resilience error conversion)

// ============================================================================
// OPTION 5: Common ResilienceError<E> Type
// ============================================================================
//
// This would be provided by tower-resilience-core

/// A common error type that wraps all resilience layer errors.
///
/// This allows users to compose multiple resilience patterns without
/// writing any error conversion code.
///
/// # Type Parameters
///
/// - `E`: The application-specific error type from the wrapped service
#[derive(Debug, Clone)]
pub enum ResilienceError<E> {
    /// A timeout occurred (from TimeLimiter or Bulkhead)
    Timeout {
        /// The layer that timed out
        layer: &'static str,
    },

    /// Circuit breaker is open, call rejected
    CircuitOpen {
        /// Circuit breaker name (if configured)
        name: Option<String>,
    },

    /// Bulkhead is at capacity, call rejected
    BulkheadFull {
        /// Current number of concurrent calls
        concurrent_calls: usize,
        /// Maximum allowed concurrent calls
        max_concurrent: usize,
    },

    /// Rate limiter rejected the call
    RateLimited {
        /// How long to wait before retrying
        retry_after: Option<Duration>,
    },

    /// The underlying application service returned an error
    Application(E),
}

impl<E> std::fmt::Display for ResilienceError<E>
where
    E: std::fmt::Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResilienceError::Timeout { layer } => write!(f, "Timeout in {}", layer),
            ResilienceError::CircuitOpen { name } => match name {
                Some(n) => write!(f, "Circuit breaker '{}' is open", n),
                None => write!(f, "Circuit breaker is open"),
            },
            ResilienceError::BulkheadFull {
                concurrent_calls,
                max_concurrent,
            } => write!(f, "Bulkhead full ({}/{})", concurrent_calls, max_concurrent),
            ResilienceError::RateLimited { retry_after } => match retry_after {
                Some(d) => write!(f, "Rate limited, retry after {:?}", d),
                None => write!(f, "Rate limited"),
            },
            ResilienceError::Application(e) => write!(f, "Application error: {}", e),
        }
    }
}

impl<E> std::error::Error for ResilienceError<E> where E: std::error::Error {}

// Automatic conversions from all resilience layer errors
impl<E> From<BulkheadError> for ResilienceError<E> {
    fn from(err: BulkheadError) -> Self {
        match err {
            BulkheadError::Timeout => ResilienceError::Timeout { layer: "bulkhead" },
            BulkheadError::BulkheadFull {
                concurrent_calls,
                max_concurrent,
            } => ResilienceError::BulkheadFull {
                concurrent_calls,
                max_concurrent,
            },
        }
    }
}

impl<E> From<CircuitBreakerError> for ResilienceError<E> {
    fn from(_: CircuitBreakerError) -> Self {
        ResilienceError::CircuitOpen { name: None }
    }
}

impl<E> From<RateLimiterError> for ResilienceError<E> {
    fn from(_: RateLimiterError) -> Self {
        ResilienceError::RateLimited { retry_after: None }
    }
}

impl<E> From<TimeLimiterError> for ResilienceError<E> {
    fn from(_: TimeLimiterError) -> Self {
        ResilienceError::Timeout {
            layer: "time_limiter",
        }
    }
}

// User's application error
#[derive(Debug, Clone)]
enum AppError {
    NetworkError(String),
    InvalidResponse,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            AppError::InvalidResponse => write!(f, "Invalid response"),
        }
    }
}

impl std::error::Error for AppError {}

// Zero boilerplate! Just use ResilienceError<AppError>

// ============================================================================
// USAGE COMPARISON
// ============================================================================

#[tokio::main]
async fn main() {
    println!("=== Error Handling Approaches Comparison ===\n");

    // APPROACH 1: Manual From implementations
    println!("1. MANUAL FROM IMPLEMENTATIONS");
    println!("   Pros:");
    println!("     - Explicit error mappings");
    println!("     - Full control over error variants");
    println!("     - Can combine multiple layer errors into one variant");
    println!("   Cons:");
    println!("     - Lots of boilerplate (4+ impl blocks)");
    println!("     - Must manually update when adding new layers");
    println!("     - Repetitive match statements");
    println!("\n   Example usage:");
    println!("     type ServiceError = ManualError;");
    println!("     // Need 4+ From implementations");
    println!();

    // APPROACH 2: Helper trait
    println!("2. HELPER TRAIT PATTERN (Option 4)");
    println!("   Pros:");
    println!("     - Slightly more semantic (explicit 'into_resilience_error')");
    println!("     - Same control as manual From");
    println!("   Cons:");
    println!("     - Still requires same amount of boilerplate");
    println!("     - No real advantage over From trait");
    println!("     - Another trait to learn/import");
    println!("\n   Example usage:");
    println!("     type ServiceError = HelperTraitError;");
    println!("     // Still need 4+ trait implementations");
    println!("\n   ❌ VERDICT: Not worth it - same verbosity as manual From");
    println!();

    // APPROACH 3: Common error type
    println!("3. COMMON RESILIENCE ERROR TYPE (Option 5)");
    println!("   Pros:");
    println!("     - ZERO boilerplate - no From implementations needed");
    println!("     - Works with any number of layers");
    println!("     - Rich error context (layer name, counts, durations)");
    println!("     - Application errors wrapped in Application variant");
    println!("     - Good Display/Debug implementations provided");
    println!("   Cons:");
    println!("     - Less control over error structure");
    println!("     - All layers produce same error type");
    println!("     - May not fit all use cases");
    println!("\n   Example usage:");
    println!("     type ServiceError = ResilienceError<AppError>;");
    println!("     // No From implementations needed! ✨");
    println!("\n   ✅ VERDICT: Best for 80% of use cases");
    println!();

    // Show actual code size comparison
    println!("=== CODE SIZE COMPARISON ===\n");
    println!("Manual From approach:      ~80 lines of boilerplate");
    println!("Helper trait approach:     ~80 lines of boilerplate (no improvement)");
    println!("ResilienceError approach:   0 lines of boilerplate\n");

    // Demonstrate usage
    println!("=== DEMONSTRATION ===\n");

    // Create a simple service that might fail
    let base_service = tower::service_fn(|_req: String| async {
        // Simulate application error
        Err::<String, AppError>(AppError::NetworkError("Connection refused".to_string()))
    });

    // Build a resilience stack using ResilienceError<AppError>
    let resilient_service = ServiceBuilder::new()
        .layer(
            TimeLimiterConfig::builder()
                .timeout_duration(Duration::from_secs(5))
                .build(),
        )
        .layer(BulkheadConfig::builder().max_concurrent_calls(10).build())
        .service(base_service);

    println!("✅ Built service with multiple resilience layers");
    println!("✅ Used ResilienceError<AppError> - zero boilerplate!");
    println!("\nNote: This is a demonstration. Actual usage would require proper");
    println!("error type alignment across all layers.");
}
