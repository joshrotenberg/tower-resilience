//! Failure classification for circuit breaker decisions.
//!
//! This module provides the [`FailureClassifier`] trait and implementations
//! for determining whether a service call result should be considered a failure.

use std::sync::Arc;

/// Trait for classifying whether a result represents a failure.
///
/// Implementors determine whether a given `Result<Res, Err>` should be
/// counted as a failure for circuit breaker purposes.
///
/// # Type Parameters
///
/// - `Res`: The success response type
/// - `Err`: The error type
pub trait FailureClassifier<Res, Err>: Send + Sync {
    /// Determines if the given result should be classified as a failure.
    ///
    /// Returns `true` if the result represents a failure that should count
    /// toward the circuit breaker's failure rate.
    fn classify(&self, result: &Result<Res, Err>) -> bool;
}

/// Default failure classifier that treats all errors as failures.
///
/// This classifier implements `FailureClassifier<Res, Err>` for **all** `Res` and `Err` types,
/// meaning it can be used without specifying concrete response/error types at configuration time.
///
/// # Behavior
///
/// - `Ok(_)` => not a failure
/// - `Err(_)` => failure
///
/// # Example
///
/// ```rust
/// use tower_resilience_circuitbreaker::classifier::{FailureClassifier, DefaultClassifier};
///
/// let classifier = DefaultClassifier;
///
/// // Works with any Result type - type inference from the result argument
/// assert!(!FailureClassifier::<String, std::io::Error>::classify(&classifier, &Ok("success".to_string())));
/// assert!(FailureClassifier::<String, std::io::Error>::classify(&classifier, &Err(std::io::Error::other("fail"))));
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultClassifier;

impl<Res, Err> FailureClassifier<Res, Err> for DefaultClassifier {
    fn classify(&self, result: &Result<Res, Err>) -> bool {
        result.is_err()
    }
}

/// A failure classifier backed by a closure.
///
/// This allows custom failure classification logic while maintaining type safety.
/// The closure captures the concrete `Res` and `Err` types.
///
/// # Example
///
/// ```rust
/// use tower_resilience_circuitbreaker::classifier::{FailureClassifier, FnClassifier};
/// use std::io::{Error, ErrorKind};
///
/// // Don't count timeouts as failures
/// let classifier = FnClassifier::new(|result: &Result<String, Error>| {
///     match result {
///         Ok(_) => false,
///         Err(e) if e.kind() == ErrorKind::TimedOut => false,
///         Err(_) => true,
///     }
/// });
///
/// assert!(!classifier.classify(&Ok("success".to_string())));
/// assert!(!classifier.classify(&Err(Error::new(ErrorKind::TimedOut, "timeout"))));
/// assert!(classifier.classify(&Err(Error::new(ErrorKind::Other, "other error"))));
/// ```
#[derive(Clone)]
pub struct FnClassifier<F> {
    f: Arc<F>,
}

impl<F> FnClassifier<F> {
    /// Creates a new `FnClassifier` from the given closure.
    pub fn new(f: F) -> Self {
        Self { f: Arc::new(f) }
    }
}

impl<F, Res, Err> FailureClassifier<Res, Err> for FnClassifier<F>
where
    F: Fn(&Result<Res, Err>) -> bool + Send + Sync,
{
    fn classify(&self, result: &Result<Res, Err>) -> bool {
        (self.f)(result)
    }
}

impl<F> std::fmt::Debug for FnClassifier<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FnClassifier")
            .field("f", &"<closure>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_classifier_treats_errors_as_failures() {
        let classifier = DefaultClassifier;

        assert!(!FailureClassifier::<(), ()>::classify(&classifier, &Ok(())));
        assert!(FailureClassifier::<(), ()>::classify(&classifier, &Err(())));
    }

    #[test]
    fn default_classifier_works_with_any_types() {
        let classifier = DefaultClassifier;

        // String, io::Error
        assert!(!FailureClassifier::<String, std::io::Error>::classify(
            &classifier,
            &Ok("ok".to_string())
        ));
        assert!(FailureClassifier::<String, std::io::Error>::classify(
            &classifier,
            &Err(std::io::Error::other("fail"))
        ));

        // i32, &str
        assert!(!FailureClassifier::<i32, &str>::classify(
            &classifier,
            &Ok(42)
        ));
        assert!(FailureClassifier::<i32, &str>::classify(
            &classifier,
            &Err("error")
        ));
    }

    #[test]
    fn fn_classifier_custom_logic() {
        // Only count "fatal" errors as failures
        let classifier = FnClassifier::new(
            |result: &Result<(), String>| matches!(result, Err(e) if e.contains("fatal")),
        );

        assert!(!classifier.classify(&Ok(())));
        assert!(!classifier.classify(&Err("warning".to_string())));
        assert!(classifier.classify(&Err("fatal error".to_string())));
    }

    #[test]
    fn fn_classifier_can_treat_some_successes_as_failures() {
        // Treat 5xx responses as failures even though they're Ok
        let classifier = FnClassifier::new(|result: &Result<u16, ()>| match result {
            Ok(status) if *status >= 500 => true,
            Err(_) => true,
            _ => false,
        });

        assert!(!classifier.classify(&Ok(200)));
        assert!(!classifier.classify(&Ok(404)));
        assert!(classifier.classify(&Ok(500)));
        assert!(classifier.classify(&Ok(503)));
        assert!(classifier.classify(&Err(())));
    }
}
