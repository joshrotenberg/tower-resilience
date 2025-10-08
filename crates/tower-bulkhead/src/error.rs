//! Error types for bulkhead pattern.

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
