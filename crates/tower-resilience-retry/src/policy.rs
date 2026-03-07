use crate::backoff::IntervalFunction;
use std::sync::Arc;
use std::time::Duration;

/// Determines whether an error should be retried.
pub type RetryPredicate<E> = Arc<dyn Fn(&E) -> bool + Send + Sync>;

/// Determines whether a successful response should be retried.
///
/// When this predicate returns `true`, the response is treated as a
/// retryable failure — consuming a budget token, applying backoff,
/// and re-sending the request.
pub type ResponsePredicate<R> = Arc<dyn Fn(&R) -> bool + Send + Sync>;

/// Policy for retry behavior.
///
/// This policy combines the interval function (backoff strategy)
/// and retry predicates (which errors/responses to retry). Maximum attempts
/// are configured separately via `MaxAttemptsSource` in the config.
pub struct RetryPolicy<R, E> {
    pub(crate) interval_fn: Arc<dyn IntervalFunction>,
    pub(crate) retry_predicate: Option<RetryPredicate<E>>,
    pub(crate) response_predicate: Option<ResponsePredicate<R>>,
}

impl<R, E> RetryPolicy<R, E> {
    /// Creates a new retry policy.
    pub fn new(interval_fn: Arc<dyn IntervalFunction>) -> Self {
        Self {
            interval_fn,
            retry_predicate: None,
            response_predicate: None,
        }
    }

    /// Sets a predicate to determine which errors should be retried.
    pub fn with_retry_predicate<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&E) -> bool + Send + Sync + 'static,
    {
        self.retry_predicate = Some(Arc::new(predicate));
        self
    }

    /// Checks if the given error should be retried.
    pub fn should_retry(&self, error: &E) -> bool {
        if let Some(predicate) = &self.retry_predicate {
            predicate(error)
        } else {
            true // Retry all errors by default
        }
    }

    /// Checks if the given response should be retried.
    ///
    /// Returns `true` if a response predicate is set and it matches.
    /// Returns `false` if no response predicate is set (default behavior).
    pub fn should_retry_response(&self, response: &R) -> bool {
        if let Some(predicate) = &self.response_predicate {
            predicate(response)
        } else {
            false // Never retry responses by default
        }
    }

    /// Computes the delay before the next retry attempt.
    pub fn next_backoff(&self, attempt: usize) -> Duration {
        self.interval_fn.next_interval(attempt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backoff::FixedInterval;

    #[derive(Debug)]
    struct TestError {
        retryable: bool,
    }

    #[derive(Debug)]
    struct TestResponse {
        has_error: bool,
    }

    #[test]
    fn test_retry_all_by_default() {
        let policy: RetryPolicy<TestResponse, TestError> =
            RetryPolicy::new(Arc::new(FixedInterval::new(Duration::from_secs(1))));

        let error = TestError { retryable: false };
        assert!(policy.should_retry(&error));
    }

    #[test]
    fn test_retry_predicate() {
        let policy: RetryPolicy<TestResponse, TestError> =
            RetryPolicy::new(Arc::new(FixedInterval::new(Duration::from_secs(1))))
                .with_retry_predicate(|e: &TestError| e.retryable);

        assert!(policy.should_retry(&TestError { retryable: true }));
        assert!(!policy.should_retry(&TestError { retryable: false }));
    }

    #[test]
    fn test_no_response_retry_by_default() {
        let policy: RetryPolicy<TestResponse, TestError> =
            RetryPolicy::new(Arc::new(FixedInterval::new(Duration::from_secs(1))));

        assert!(!policy.should_retry_response(&TestResponse { has_error: true }));
    }

    #[test]
    fn test_response_predicate() {
        let mut policy: RetryPolicy<TestResponse, TestError> =
            RetryPolicy::new(Arc::new(FixedInterval::new(Duration::from_secs(1))));
        policy.response_predicate = Some(Arc::new(|r: &TestResponse| r.has_error));

        assert!(policy.should_retry_response(&TestResponse { has_error: true }));
        assert!(!policy.should_retry_response(&TestResponse { has_error: false }));
    }

    #[test]
    fn test_backoff_computation() {
        let policy: RetryPolicy<TestResponse, TestError> =
            RetryPolicy::new(Arc::new(FixedInterval::new(Duration::from_secs(2))));

        assert_eq!(policy.next_backoff(0), Duration::from_secs(2));
        assert_eq!(policy.next_backoff(1), Duration::from_secs(2));
    }
}
