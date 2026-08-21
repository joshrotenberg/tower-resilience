//! Error types for the hedging middleware.

use std::fmt;

/// Error type for the hedging service.
///
/// The typed inner error remains available through [`HedgeError::inner`] and
/// [`HedgeError::into_inner`]. It is intentionally not exposed through
/// [`std::error::Error::source`] so boxed Tower errors can be wrapped directly.
#[derive(Debug, Clone)]
pub enum HedgeError<E> {
    /// All hedged attempts failed.
    ///
    /// Contains the error from the primary request.
    AllAttemptsFailed(E),

    /// Error from the inner service.
    Inner(E),
}

impl<E: fmt::Display> fmt::Display for HedgeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HedgeError::AllAttemptsFailed(e) => {
                write!(f, "all hedged attempts failed: {}", e)
            }
            HedgeError::Inner(e) => write!(f, "{}", e),
        }
    }
}

impl<E> std::error::Error for HedgeError<E> where E: fmt::Debug + fmt::Display {}

impl<E> HedgeError<E> {
    /// Returns `true` if all hedged attempts failed.
    pub fn is_all_attempts_failed(&self) -> bool {
        matches!(self, HedgeError::AllAttemptsFailed(_))
    }

    /// Returns `true` if this is an inner service error.
    pub fn is_inner(&self) -> bool {
        matches!(self, HedgeError::Inner(_))
    }

    /// Get a reference to the inner error.
    pub fn inner(&self) -> &E {
        match self {
            HedgeError::AllAttemptsFailed(e) => e,
            HedgeError::Inner(e) => e,
        }
    }

    /// Convert into the inner error.
    pub fn into_inner(self) -> E {
        match self {
            HedgeError::AllAttemptsFailed(e) => e,
            HedgeError::Inner(e) => e,
        }
    }
}
