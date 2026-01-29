//! Error types for bulkhead pattern.

use tower_resilience_core::ResilienceError;

/// Errors that can occur when using a bulkhead.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BulkheadError {
    /// The bulkhead rejected the call because it's at capacity.
    #[error("bulkhead is full: max concurrent calls ({max_concurrent_calls}) reached")]
    BulkheadFull {
        /// Maximum concurrent calls allowed.
        max_concurrent_calls: usize,
    },
    /// Timeout waiting for a permit.
    #[error("timeout waiting for bulkhead permit")]
    Timeout,
}

/// Result type for bulkhead operations.
pub type Result<T> = std::result::Result<T, BulkheadError>;

/// Service-level error that wraps inner service errors.
///
/// This error type is returned by the [`Bulkhead`](crate::Bulkhead) service and
/// allows services with any error type to be wrapped without requiring
/// `From<BulkheadError>` implementations.
///
/// # Examples
///
/// ```rust
/// use tower_resilience_bulkhead::BulkheadServiceError;
///
/// // Match on the error to determine the cause
/// fn handle_error<E: std::fmt::Debug>(err: BulkheadServiceError<E>) {
///     match err {
///         BulkheadServiceError::Bulkhead(e) => {
///             println!("Bulkhead error: {}", e);
///         }
///         BulkheadServiceError::Inner(e) => {
///             println!("Inner service error: {:?}", e);
///         }
///     }
/// }
/// ```
#[derive(Debug)]
pub enum BulkheadServiceError<E> {
    /// Bulkhead-specific error (full or timeout).
    Bulkhead(BulkheadError),
    /// Error from the inner service.
    Inner(E),
}

impl<E> BulkheadServiceError<E> {
    /// Returns true if this is a bulkhead-specific error.
    pub fn is_bulkhead(&self) -> bool {
        matches!(self, BulkheadServiceError::Bulkhead(_))
    }

    /// Returns true if this is an inner service error.
    pub fn is_inner(&self) -> bool {
        matches!(self, BulkheadServiceError::Inner(_))
    }

    /// Converts this error into the inner error, if any.
    pub fn into_inner(self) -> Option<E> {
        match self {
            BulkheadServiceError::Bulkhead(_) => None,
            BulkheadServiceError::Inner(e) => Some(e),
        }
    }

    /// Returns a reference to the bulkhead error, if any.
    pub fn bulkhead_error(&self) -> Option<&BulkheadError> {
        match self {
            BulkheadServiceError::Bulkhead(e) => Some(e),
            BulkheadServiceError::Inner(_) => None,
        }
    }
}

impl<E: std::fmt::Display> std::fmt::Display for BulkheadServiceError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BulkheadServiceError::Bulkhead(e) => write!(f, "{}", e),
            BulkheadServiceError::Inner(e) => write!(f, "inner service error: {}", e),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BulkheadServiceError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BulkheadServiceError::Bulkhead(e) => Some(e),
            BulkheadServiceError::Inner(e) => Some(e),
        }
    }
}

impl<E> From<BulkheadError> for BulkheadServiceError<E> {
    fn from(err: BulkheadError) -> Self {
        BulkheadServiceError::Bulkhead(err)
    }
}

// Conversion to ResilienceError for zero-boilerplate error handling
impl<E> From<BulkheadError> for ResilienceError<E> {
    fn from(err: BulkheadError) -> Self {
        match err {
            BulkheadError::Timeout => ResilienceError::Timeout { layer: "bulkhead" },
            BulkheadError::BulkheadFull {
                max_concurrent_calls,
            } => ResilienceError::BulkheadFull {
                concurrent_calls: max_concurrent_calls,
                max_concurrent: max_concurrent_calls,
            },
        }
    }
}

impl<E> From<BulkheadServiceError<E>> for ResilienceError<E> {
    fn from(err: BulkheadServiceError<E>) -> Self {
        match err {
            BulkheadServiceError::Bulkhead(e) => e.into(),
            BulkheadServiceError::Inner(e) => ResilienceError::Application(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time assertion that BulkheadError is Send + Sync + 'static.
    /// This is required for compatibility with tower's BoxError.
    const _: () = {
        const fn assert_send_sync_static<T: Send + Sync + 'static>() {}
        assert_send_sync_static::<BulkheadError>();
    };

    #[test]
    fn test_into_box_error() {
        let err = BulkheadError::BulkheadFull {
            max_concurrent_calls: 10,
        };
        let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(err);
        assert!(boxed.to_string().contains("bulkhead is full"));
    }

    #[test]
    fn test_timeout_into_box_error() {
        let err = BulkheadError::Timeout;
        let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(err);
        assert!(boxed.to_string().contains("timeout"));
    }
}
