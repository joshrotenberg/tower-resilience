use crate::backoff::{ExponentialBackoff, FixedInterval, IntervalFunction};
use crate::budget::RetryBudget;
use crate::events::RetryEvent;
use crate::policy::{RetryPolicy, RetryPredicate};
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;
use tower_resilience_core::events::{EventListeners, FnListener};

/// Source for determining the maximum number of retry attempts.
///
/// This enum allows configuring either a fixed max attempts for all requests
/// or a dynamic max attempts extracted from each request.
#[derive(Clone)]
pub enum MaxAttemptsSource<Req> {
    /// Fixed max attempts for all requests.
    Fixed(usize),
    /// Dynamic max attempts extracted from the request.
    ///
    /// The function receives a reference to the request and returns
    /// the max attempts to use for that specific request.
    Dynamic(Arc<dyn Fn(&Req) -> usize + Send + Sync>),
}

impl<Req> MaxAttemptsSource<Req> {
    /// Get the max attempts for a request.
    pub fn get_max_attempts(&self, req: &Req) -> usize {
        match self {
            MaxAttemptsSource::Fixed(n) => *n,
            MaxAttemptsSource::Dynamic(f) => f(req),
        }
    }
}

impl<Req> Default for MaxAttemptsSource<Req> {
    fn default() -> Self {
        MaxAttemptsSource::Fixed(3)
    }
}

/// Configuration for the retry middleware.
pub struct RetryConfig<Req, E> {
    pub(crate) policy: RetryPolicy<E>,
    pub(crate) max_attempts_source: MaxAttemptsSource<Req>,
    pub(crate) event_listeners: EventListeners<RetryEvent>,
    pub(crate) name: String,
    pub(crate) budget: Option<Arc<dyn RetryBudget>>,
}

/// Builder for [`RetryConfig`].
pub struct RetryConfigBuilder<Req, E> {
    max_attempts_source: MaxAttemptsSource<Req>,
    interval_fn: Option<Arc<dyn IntervalFunction>>,
    retry_predicate: Option<RetryPredicate<E>>,
    event_listeners: EventListeners<RetryEvent>,
    name: String,
    budget: Option<Arc<dyn RetryBudget>>,
    _phantom: PhantomData<Req>,
}

impl<Req, E> Default for RetryConfigBuilder<Req, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Req, E> RetryConfigBuilder<Req, E> {
    /// Creates a new builder with defaults.
    ///
    /// Defaults:
    /// - max_attempts: 3
    /// - backoff: Exponential with 100ms initial interval
    /// - name: `"<unnamed>"`
    /// - budget: None (unlimited retries)
    pub fn new() -> Self {
        Self {
            max_attempts_source: MaxAttemptsSource::default(),
            interval_fn: None,
            retry_predicate: None,
            event_listeners: EventListeners::new(),
            name: "<unnamed>".to_string(),
            budget: None,
            _phantom: PhantomData,
        }
    }

    /// Sets a fixed maximum number of retry attempts for all requests.
    ///
    /// This includes the initial attempt, so max_attempts=3 means
    /// 1 initial attempt + 2 retries.
    pub fn max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts_source = MaxAttemptsSource::Fixed(max_attempts);
        self
    }

    /// Sets a dynamic max attempts extractor function.
    ///
    /// The function receives a reference to the request and returns
    /// the maximum number of attempts to use for that specific request.
    /// This enables per-request retry configuration based on request properties.
    ///
    /// # Use Cases
    ///
    /// - Idempotent requests can retry more aggressively
    /// - Critical requests may have more retries
    /// - Different operations may have different retry budgets
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_retry::RetryLayer;
    /// use std::time::Duration;
    ///
    /// #[derive(Clone)]
    /// struct MyRequest {
    ///     is_idempotent: bool,
    ///     // ... other fields
    /// }
    ///
    /// #[derive(Debug, Clone)]
    /// struct MyError;
    ///
    /// let layer = RetryLayer::<MyRequest, MyError>::builder()
    ///     .max_attempts_fn(|req: &MyRequest| {
    ///         if req.is_idempotent { 5 } else { 1 }
    ///     })
    ///     .exponential_backoff(Duration::from_millis(100))
    ///     .build();
    /// ```
    pub fn max_attempts_fn<F>(mut self, f: F) -> Self
    where
        F: Fn(&Req) -> usize + Send + Sync + 'static,
    {
        self.max_attempts_source = MaxAttemptsSource::Dynamic(Arc::new(f));
        self
    }

    /// Sets a fixed backoff interval.
    pub fn fixed_backoff(mut self, duration: Duration) -> Self {
        self.interval_fn = Some(Arc::new(FixedInterval::new(duration)));
        self
    }

    /// Sets exponential backoff with default settings.
    pub fn exponential_backoff(mut self, initial_interval: Duration) -> Self {
        self.interval_fn = Some(Arc::new(ExponentialBackoff::new(initial_interval)));
        self
    }

    /// Sets a custom interval function for backoff.
    pub fn backoff<I>(mut self, interval_fn: I) -> Self
    where
        I: IntervalFunction + 'static,
    {
        self.interval_fn = Some(Arc::new(interval_fn));
        self
    }

    /// Sets a predicate to determine which errors should be retried.
    pub fn retry_on<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&E) -> bool + Send + Sync + 'static,
    {
        self.retry_predicate = Some(Arc::new(predicate));
        self
    }

    /// Sets the name for this retry instance (used in events).
    pub fn name<S: Into<String>>(mut self, name: S) -> Self {
        self.name = name.into();
        self
    }

    /// Sets a retry budget to limit total retries across all requests.
    ///
    /// Retry budgets prevent retry storms by limiting the total number of
    /// retries that can occur, regardless of how many concurrent requests
    /// are being processed.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_retry::{RetryLayer, RetryBudgetBuilder};
    /// use std::time::Duration;
    ///
    /// // Create a token bucket budget: 10 retries/sec, max burst of 100
    /// let budget = RetryBudgetBuilder::new()
    ///     .token_bucket()
    ///     .tokens_per_second(10.0)
    ///     .max_tokens(100)
    ///     .build();
    ///
    /// let layer = RetryLayer::<(), std::io::Error>::builder()
    ///     .max_attempts(5)
    ///     .exponential_backoff(Duration::from_millis(100))
    ///     .budget(budget)
    ///     .build();
    /// ```
    pub fn budget(mut self, budget: Arc<dyn RetryBudget>) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Registers a callback when a retry is skipped due to budget exhaustion.
    ///
    /// This callback is invoked when a retry would have been attempted, but
    /// the retry budget has been exhausted. The request will fail immediately
    /// instead of retrying.
    ///
    /// # Callback Signature
    /// `Fn(usize)` - Called with the attempt number that was skipped.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use tower_resilience_retry::RetryLayer;
    /// use std::time::Duration;
    ///
    /// let layer = RetryLayer::<(), std::io::Error>::builder()
    ///     .max_attempts(5)
    ///     .on_budget_exhausted(|attempt| {
    ///         println!("Retry {} skipped - budget exhausted", attempt);
    ///     })
    ///     .build();
    /// ```
    pub fn on_budget_exhausted<F>(mut self, f: F) -> Self
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let RetryEvent::BudgetExhausted { attempt, .. } = event {
                f(*attempt);
            }
        }));
        self
    }

    /// Registers a callback when a retry attempt is about to be made.
    ///
    /// This callback is invoked after a failed attempt and before the retry delay begins.
    /// It provides visibility into the retry behavior and allows for custom logging,
    /// metrics collection, or other side effects.
    ///
    /// # Callback Signature
    /// `Fn(usize, Duration)` - Called with two parameters:
    /// - First parameter: The retry attempt number (1-indexed, so 1 = first retry)
    /// - Second parameter: The delay duration before the next attempt
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_retry::RetryLayer;
    /// use std::time::Duration;
    ///
    /// let layer = RetryLayer::<(), std::io::Error>::builder()
    ///     .max_attempts(5)
    ///     .exponential_backoff(Duration::from_millis(100))
    ///     .on_retry(|attempt, delay| {
    ///         println!("Retry attempt {} after {:?} delay", attempt, delay);
    ///         if attempt >= 3 {
    ///             println!("Warning: multiple retries required");
    ///         }
    ///     })
    ///     .build();
    /// ```
    pub fn on_retry<F>(mut self, f: F) -> Self
    where
        F: Fn(usize, Duration) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let RetryEvent::Retry { attempt, delay, .. } = event {
                f(*attempt, *delay);
            }
        }));
        self
    }

    /// Registers a callback when an operation succeeds.
    ///
    /// This callback is invoked when the operation completes successfully, either on
    /// the first attempt or after one or more retries. This is useful for tracking
    /// how many attempts were needed to achieve success.
    ///
    /// # Callback Signature
    /// `Fn(usize)` - Called with the total number of attempts made (including the initial attempt).
    /// - Value of 1 means success on first try (no retries)
    /// - Value > 1 means retries were needed
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_retry::RetryLayer;
    /// use std::time::Duration;
    ///
    /// let layer = RetryLayer::<(), std::io::Error>::builder()
    ///     .max_attempts(3)
    ///     .on_success(|attempts| {
    ///         if attempts == 1 {
    ///             println!("Success on first attempt");
    ///         } else {
    ///             println!("Success after {} attempts", attempts);
    ///         }
    ///     })
    ///     .build();
    /// ```
    pub fn on_success<F>(mut self, f: F) -> Self
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let RetryEvent::Success { attempts, .. } = event {
                f(*attempts);
            }
        }));
        self
    }

    /// Registers a callback when all retry attempts are exhausted.
    ///
    /// This callback is invoked when the operation fails and the maximum number of
    /// retry attempts has been reached. The operation will return the final error
    /// to the caller after this callback is invoked.
    ///
    /// # Callback Signature
    /// `Fn(usize)` - Called with the total number of attempts made (including the initial attempt).
    /// This will typically equal `max_attempts` configured in the builder.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_retry::RetryLayer;
    /// use std::time::Duration;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let failure_count = Arc::new(AtomicUsize::new(0));
    /// let counter = Arc::clone(&failure_count);
    ///
    /// let layer = RetryLayer::<(), std::io::Error>::builder()
    ///     .max_attempts(3)
    ///     .on_error(move |attempts| {
    ///         let count = counter.fetch_add(1, Ordering::SeqCst);
    ///         println!("Operation failed after {} attempts (total failures: {})",
    ///                  attempts, count + 1);
    ///     })
    ///     .build();
    /// ```
    pub fn on_error<F>(mut self, f: F) -> Self
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let RetryEvent::Error { attempts, .. } = event {
                f(*attempts);
            }
        }));
        self
    }

    /// Registers a callback when an error is ignored and not retried.
    ///
    /// This callback is invoked when an error occurs but the retry predicate determines
    /// that it should not be retried. The error is returned immediately to the caller
    /// without any retry attempts. This is useful for distinguishing between retryable
    /// and non-retryable errors.
    ///
    /// # Callback Signature
    /// `Fn()` - Called with no parameters when an error is ignored.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_retry::RetryLayer;
    /// use std::time::Duration;
    /// use std::io::{Error, ErrorKind};
    ///
    /// let layer = RetryLayer::<(), Error>::builder()
    ///     .max_attempts(3)
    ///     .retry_on(|err| {
    ///         // Only retry transient errors
    ///         matches!(err.kind(), ErrorKind::ConnectionRefused | ErrorKind::TimedOut)
    ///     })
    ///     .on_ignored_error(|| {
    ///         println!("Error occurred but was not retried (non-retryable error type)");
    ///     })
    ///     .build();
    /// ```
    pub fn on_ignored_error<F>(mut self, f: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if matches!(event, RetryEvent::IgnoredError { .. }) {
                f();
            }
        }));
        self
    }

    /// Builds the retry layer.
    pub fn build(self) -> crate::RetryLayer<Req, E> {
        let interval_fn = self
            .interval_fn
            .unwrap_or_else(|| Arc::new(ExponentialBackoff::new(Duration::from_millis(100))));

        let mut policy = RetryPolicy::new(interval_fn);
        if let Some(predicate) = self.retry_predicate {
            policy.retry_predicate = Some(predicate);
        }

        let config = RetryConfig {
            policy,
            max_attempts_source: self.max_attempts_source,
            event_listeners: self.event_listeners,
            name: self.name,
            budget: self.budget,
        };

        crate::RetryLayer::new(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RetryLayer;

    #[test]
    fn test_builder_defaults() {
        let _layer = RetryLayer::<(), std::io::Error>::builder().build();
    }

    #[test]
    fn test_builder_custom_values() {
        let _layer = RetryLayer::<(), std::io::Error>::builder()
            .max_attempts(5)
            .fixed_backoff(Duration::from_secs(2))
            .name("test-retry")
            .build();
    }

    #[test]
    fn test_event_listeners() {
        let _layer = RetryLayer::<(), std::io::Error>::builder()
            .on_retry(|_, _| {})
            .on_success(|_| {})
            .build();
    }

    #[test]
    fn test_max_attempts_fn() {
        #[derive(Clone)]
        struct MyRequest {
            is_idempotent: bool,
        }

        let _layer = RetryLayer::<MyRequest, std::io::Error>::builder()
            .max_attempts_fn(|req: &MyRequest| if req.is_idempotent { 5 } else { 1 })
            .build();
    }

    #[test]
    fn test_max_attempts_source_fixed() {
        let source: MaxAttemptsSource<()> = MaxAttemptsSource::Fixed(5);
        assert_eq!(source.get_max_attempts(&()), 5);
    }

    #[test]
    fn test_max_attempts_source_dynamic() {
        #[derive(Clone)]
        struct Req {
            retries: usize,
        }

        let source: MaxAttemptsSource<Req> =
            MaxAttemptsSource::Dynamic(Arc::new(|req: &Req| req.retries));
        let req = Req { retries: 10 };
        assert_eq!(source.get_max_attempts(&req), 10);
    }

    #[test]
    fn test_preset_exponential_backoff() {
        let _layer = RetryLayer::<(), std::io::Error>::exponential_backoff().build();
    }

    #[test]
    fn test_preset_aggressive() {
        let _layer = RetryLayer::<(), std::io::Error>::aggressive().build();
    }

    #[test]
    fn test_preset_conservative() {
        let _layer = RetryLayer::<(), std::io::Error>::conservative().build();
    }

    #[test]
    fn test_preset_with_customization() {
        // Verify presets can be further customized
        let _layer = RetryLayer::<(), std::io::Error>::exponential_backoff()
            .max_attempts(10)
            .name("custom")
            .build();
    }
}
