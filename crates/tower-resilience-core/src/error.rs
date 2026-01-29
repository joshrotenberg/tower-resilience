//! Common error types for tower-resilience patterns.
//!
//! This module provides [`ResilienceError`], a unified error type that eliminates
//! the need for manual `From` trait implementations when composing multiple resilience
//! layers.
//!
//! # The Problem
//!
//! When using multiple resilience layers (bulkhead, circuit breaker, rate limiter, etc.),
//! you typically need to write repetitive `From` trait implementations:
//!
//! ```rust,ignore
//! // Without ResilienceError: ~80 lines of boilerplate for 4 layers
//! impl From<BulkheadError> for ServiceError { /* ... */ }
//! impl From<CircuitBreakerError> for ServiceError { /* ... */ }
//! impl From<RateLimiterError> for ServiceError { /* ... */ }
//! impl From<TimeLimiterError> for ServiceError { /* ... */ }
//! ```
//!
//! # The Solution
//!
//! Use [`ResilienceError<E>`] as your service error type:
//!
//! ```rust
//! use tower_resilience_core::ResilienceError;
//!
//! // Your application error
//! #[derive(Debug, Clone)]
//! enum AppError {
//!     DatabaseDown,
//!     InvalidRequest,
//! }
//!
//! impl std::fmt::Display for AppError {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         match self {
//!             AppError::DatabaseDown => write!(f, "Database down"),
//!             AppError::InvalidRequest => write!(f, "Invalid request"),
//!         }
//!     }
//! }
//!
//! impl std::error::Error for AppError {}
//!
//! // That's it! Zero From implementations needed
//! type ServiceError = ResilienceError<AppError>;
//! ```
//!
//! # Benefits
//!
//! - **Zero boilerplate**: No manual `From` implementations
//! - **Works with any number of layers**: Add or remove layers without touching error code
//! - **Rich error context**: Layer names, counts, durations included
//! - **Application errors preserved**: Wrapped in `Application` variant
//! - **Convenient helpers**: `is_timeout()`, `is_rate_limited()`, etc.
//!
//! # Pattern Matching
//!
//! ```rust
//! use tower_resilience_core::ResilienceError;
//! use std::time::Duration;
//!
//! # #[derive(Debug)]
//! # struct AppError;
//! # impl std::fmt::Display for AppError {
//! #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }
//! # }
//! # impl std::error::Error for AppError {}
//! fn handle_error(error: ResilienceError<AppError>) {
//!     match error {
//!         ResilienceError::Timeout { layer } => {
//!             eprintln!("Timeout in {}", layer);
//!         }
//!         ResilienceError::CircuitOpen { name } => {
//!             eprintln!("Circuit breaker {:?} is open", name);
//!         }
//!         ResilienceError::BulkheadFull { concurrent_calls, max_concurrent } => {
//!             eprintln!("Bulkhead full: {}/{}", concurrent_calls, max_concurrent);
//!         }
//!         ResilienceError::RateLimited { retry_after } => {
//!             eprintln!("Rate limited, retry after {:?}", retry_after);
//!         }
//!         ResilienceError::Application(app_err) => {
//!             eprintln!("Application error: {}", app_err);
//!         }
//!     }
//! }
//! ```
//!
//! # Helper Methods
//!
//! ```rust
//! use tower_resilience_core::ResilienceError;
//!
//! # #[derive(Debug)]
//! # struct AppError;
//! # impl std::fmt::Display for AppError {
//! #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }
//! # }
//! # impl std::error::Error for AppError {}
//! # let error: ResilienceError<AppError> = ResilienceError::Timeout { layer: "test" };
//! if error.is_timeout() {
//!     // Handle timeout from any layer
//! } else if error.is_application() {
//!     let app_error = error.application_error().unwrap();
//!     // Handle application-specific error
//! }
//! ```
//!
//! # When to Use
//!
//! **Use `ResilienceError<E>` when:**
//! - Building new services with multiple resilience layers
//! - You want zero boilerplate error handling
//! - Standard error categorization is sufficient
//! - You're prototyping or want to move fast
//!
//! **Use manual `From` implementations when:**
//! - You need very specific error semantics
//! - Different layers require different recovery strategies
//! - Integrating with legacy error types
//! - You need specialized error logging per layer
//!
//! # Migration
//!
//! Existing code using manual `From` implementations continues to work.
//! New code can adopt `ResilienceError<E>` incrementally:
//!
//! ```rust,ignore
//! // Old code (still works)
//! type ServiceError = MyCustomError; // with manual From impls
//!
//! // New code (zero boilerplate)
//! type ServiceError = ResilienceError<MyAppError>;
//! ```

use std::fmt;
use std::time::Duration;

/// A common error type that wraps all resilience layer errors.
///
/// This allows users to compose multiple resilience patterns without
/// writing any error conversion code. Each resilience layer error automatically
/// converts into the appropriate `ResilienceError` variant.
///
/// # Type Parameters
///
/// - `E`: The application-specific error type from the wrapped service
///
/// # Examples
///
/// ```
/// use tower_resilience_core::ResilienceError;
/// use std::time::Duration;
///
/// // Your application error
/// #[derive(Debug)]
/// enum AppError {
///     Network(String),
///     InvalidData,
/// }
///
/// impl std::fmt::Display for AppError {
///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         match self {
///             AppError::Network(msg) => write!(f, "Network: {}", msg),
///             AppError::InvalidData => write!(f, "Invalid data"),
///         }
///     }
/// }
///
/// impl std::error::Error for AppError {}
///
/// // Use ResilienceError<AppError> throughout your resilience stack
/// type ServiceError = ResilienceError<AppError>;
///
/// // No From implementations needed - just use the error type!
/// fn handle_error(err: ServiceError) {
///     match err {
///         ResilienceError::Timeout { layer } => {
///             println!("Timeout in {}", layer);
///         }
///         ResilienceError::CircuitOpen { .. } => {
///             println!("Circuit breaker is open");
///         }
///         ResilienceError::Application(app_err) => {
///             println!("Application error: {}", app_err);
///         }
///         _ => println!("Other resilience error"),
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub enum ResilienceError<E> {
    /// A timeout occurred (from TimeLimiter or Bulkhead).
    Timeout {
        /// The layer that timed out (e.g., "time_limiter", "bulkhead")
        layer: &'static str,
    },

    /// Circuit breaker is open, call rejected.
    CircuitOpen {
        /// Circuit breaker name (if configured)
        name: Option<String>,
    },

    /// Bulkhead is at capacity, call rejected.
    BulkheadFull {
        /// Current number of concurrent calls
        concurrent_calls: usize,
        /// Maximum allowed concurrent calls
        max_concurrent: usize,
    },

    /// Rate limiter rejected the call.
    RateLimited {
        /// How long to wait before retrying (if available)
        retry_after: Option<Duration>,
    },

    /// The underlying application service returned an error.
    Application(E),
}

impl<E> fmt::Display for ResilienceError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

// Note: From implementations for each resilience layer error are provided
// by the individual crates (bulkhead, circuitbreaker, etc.) to avoid
// circular dependencies.

impl<E> ResilienceError<E> {
    /// Returns `true` if this is a timeout error.
    pub fn is_timeout(&self) -> bool {
        matches!(self, ResilienceError::Timeout { .. })
    }

    /// Returns `true` if this is a circuit breaker error.
    pub fn is_circuit_open(&self) -> bool {
        matches!(self, ResilienceError::CircuitOpen { .. })
    }

    /// Returns `true` if this is a bulkhead error.
    pub fn is_bulkhead_full(&self) -> bool {
        matches!(self, ResilienceError::BulkheadFull { .. })
    }

    /// Returns `true` if this is a rate limiter error.
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, ResilienceError::RateLimited { .. })
    }

    /// Returns `true` if this is an application error.
    pub fn is_application(&self) -> bool {
        matches!(self, ResilienceError::Application(_))
    }

    /// Extracts the application error, if this is an `Application` variant.
    pub fn application_error(self) -> Option<E> {
        match self {
            ResilienceError::Application(e) => Some(e),
            _ => None,
        }
    }

    /// Maps the application error using a function.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_core::ResilienceError;
    ///
    /// let err: ResilienceError<String> = ResilienceError::Application("error".to_string());
    /// let mapped: ResilienceError<usize> = err.map_application(|s| s.len());
    /// assert_eq!(mapped.application_error(), Some(5));
    /// ```
    pub fn map_application<F, T>(self, f: F) -> ResilienceError<T>
    where
        F: FnOnce(E) -> T,
    {
        match self {
            ResilienceError::Timeout { layer } => ResilienceError::Timeout { layer },
            ResilienceError::CircuitOpen { name } => ResilienceError::CircuitOpen { name },
            ResilienceError::BulkheadFull {
                concurrent_calls,
                max_concurrent,
            } => ResilienceError::BulkheadFull {
                concurrent_calls,
                max_concurrent,
            },
            ResilienceError::RateLimited { retry_after } => {
                ResilienceError::RateLimited { retry_after }
            }
            ResilienceError::Application(e) => ResilienceError::Application(f(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct TestError;

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "test error")
        }
    }

    impl std::error::Error for TestError {}

    /// Compile-time assertion that ResilienceError is Send + Sync + 'static
    /// when the inner error type is Send + Sync + 'static.
    /// This is required for compatibility with tower's BoxError.
    const _: () = {
        const fn assert_send_sync_static<T: Send + Sync + 'static>() {}
        assert_send_sync_static::<ResilienceError<TestError>>();
    };

    #[test]
    fn test_into_box_error() {
        let err: ResilienceError<TestError> = ResilienceError::Timeout { layer: "test" };
        let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(err);
        assert!(boxed.to_string().contains("Timeout"));
    }

    #[test]
    fn test_application_error_into_box_error() {
        let err: ResilienceError<TestError> = ResilienceError::Application(TestError);
        let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(err);
        assert!(boxed.to_string().contains("test error"));
    }
}
