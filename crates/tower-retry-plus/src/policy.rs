use crate::backoff::IntervalFunction;
use std::sync::Arc;
use std::time::Duration;

/// Determines whether an error should be retried.
pub type RetryPredicate<E> = Arc<dyn Fn(&E) -> bool + Send + Sync>;

/// Policy for retry behavior.
///
/// This policy combines the interval function (backoff strategy),
/// maximum attempts, and retry predicate (which errors to retry).
pub struct RetryPolicy<E> {
    pub(crate) max_attempts: usize,
    pub(crate) interval_fn: Arc<dyn IntervalFunction>,
    pub(crate) retry_predicate: Option<RetryPredicate<E>>,
}

impl<E> RetryPolicy<E> {
    /// Creates a new retry policy.
    pub fn new(max_attempts: usize, interval_fn: Arc<dyn IntervalFunction>) -> Self {
        Self {
            max_attempts,
            interval_fn,
            retry_predicate: None,
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

    #[test]
    fn test_retry_all_by_default() {
        let policy = RetryPolicy::new(3, Arc::new(FixedInterval::new(Duration::from_secs(1))));

        let error = TestError { retryable: false };
        assert!(policy.should_retry(&error));
    }

    #[test]
    fn test_retry_predicate() {
        let policy = RetryPolicy::new(3, Arc::new(FixedInterval::new(Duration::from_secs(1))))
            .with_retry_predicate(|e: &TestError| e.retryable);

        assert!(policy.should_retry(&TestError { retryable: true }));
        assert!(!policy.should_retry(&TestError { retryable: false }));
    }

    #[test]
    fn test_backoff_computation() {
        let policy: RetryPolicy<TestError> =
            RetryPolicy::new(3, Arc::new(FixedInterval::new(Duration::from_secs(2))));

        assert_eq!(policy.next_backoff(0), Duration::from_secs(2));
        assert_eq!(policy.next_backoff(1), Duration::from_secs(2));
    }
}
