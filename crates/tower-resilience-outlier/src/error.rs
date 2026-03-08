//! Error types for the outlier detection middleware.

use tower_resilience_core::ResilienceError;

/// Errors specific to the outlier detection pattern.
#[derive(Debug, Clone, thiserror::Error)]
pub enum OutlierDetectionError {
    /// The instance has been ejected due to outlier detection.
    #[error("instance '{name}' is ejected by outlier detection")]
    Ejected {
        /// The name of the ejected instance.
        name: String,
    },
}

/// Service-level error that wraps both outlier detection and inner service errors.
#[derive(Debug)]
pub enum OutlierDetectionServiceError<E> {
    /// An outlier detection error (e.g., instance ejected).
    OutlierDetection(OutlierDetectionError),
    /// An error from the inner service.
    Inner(E),
}

impl<E> OutlierDetectionServiceError<E> {
    /// Returns `true` if this is an outlier detection error.
    pub fn is_outlier_detection(&self) -> bool {
        matches!(self, OutlierDetectionServiceError::OutlierDetection(_))
    }

    /// Returns `true` if this is an inner service error.
    pub fn is_inner(&self) -> bool {
        matches!(self, OutlierDetectionServiceError::Inner(_))
    }

    /// Consumes self and returns the inner error, if present.
    pub fn into_inner(self) -> Option<E> {
        match self {
            OutlierDetectionServiceError::Inner(e) => Some(e),
            OutlierDetectionServiceError::OutlierDetection(_) => None,
        }
    }

    /// Returns a reference to the outlier detection error, if present.
    pub fn outlier_detection_error(&self) -> Option<&OutlierDetectionError> {
        match self {
            OutlierDetectionServiceError::OutlierDetection(e) => Some(e),
            OutlierDetectionServiceError::Inner(_) => None,
        }
    }
}

impl<E: std::fmt::Display> std::fmt::Display for OutlierDetectionServiceError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutlierDetectionServiceError::OutlierDetection(e) => write!(f, "{}", e),
            OutlierDetectionServiceError::Inner(e) => write!(f, "inner service error: {}", e),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for OutlierDetectionServiceError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OutlierDetectionServiceError::OutlierDetection(e) => Some(e),
            OutlierDetectionServiceError::Inner(e) => Some(e),
        }
    }
}

impl<E> From<OutlierDetectionError> for OutlierDetectionServiceError<E> {
    fn from(err: OutlierDetectionError) -> Self {
        OutlierDetectionServiceError::OutlierDetection(err)
    }
}

impl<E> From<OutlierDetectionError> for ResilienceError<E> {
    fn from(err: OutlierDetectionError) -> Self {
        match err {
            OutlierDetectionError::Ejected { name } => ResilienceError::InstanceEjected { name },
        }
    }
}

impl<E> From<OutlierDetectionServiceError<E>> for ResilienceError<E> {
    fn from(err: OutlierDetectionServiceError<E>) -> Self {
        match err {
            OutlierDetectionServiceError::OutlierDetection(e) => e.into(),
            OutlierDetectionServiceError::Inner(e) => ResilienceError::Application(e),
        }
    }
}

// Flattening conversion for idempotent .unified() composition.
impl<E> From<OutlierDetectionServiceError<ResilienceError<E>>> for ResilienceError<E> {
    fn from(err: OutlierDetectionServiceError<ResilienceError<E>>) -> Self {
        match err {
            OutlierDetectionServiceError::OutlierDetection(e) => e.into(),
            OutlierDetectionServiceError::Inner(re) => re,
        }
    }
}
