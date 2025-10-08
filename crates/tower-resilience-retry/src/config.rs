use crate::backoff::{ExponentialBackoff, FixedInterval, IntervalFunction};
use crate::events::RetryEvent;
use crate::policy::{RetryPolicy, RetryPredicate};
use std::sync::Arc;
use std::time::Duration;
use tower_resilience_core::events::{EventListeners, FnListener};

/// Configuration for the retry middleware.
pub struct RetryConfig<E> {
    pub(crate) policy: RetryPolicy<E>,
    pub(crate) event_listeners: EventListeners<RetryEvent>,
    pub(crate) name: String,
}

impl<E> RetryConfig<E> {
    /// Creates a new builder for retry configuration.
    pub fn builder() -> RetryConfigBuilder<E> {
        RetryConfigBuilder::new()
    }

    /// Returns a layer that can be applied to a service.
    pub fn layer(self) -> crate::RetryLayer<E> {
        crate::RetryLayer::new(self)
    }
}

/// Builder for [`RetryConfig`].
pub struct RetryConfigBuilder<E> {
    max_attempts: usize,
    interval_fn: Option<Arc<dyn IntervalFunction>>,
    retry_predicate: Option<RetryPredicate<E>>,
    event_listeners: EventListeners<RetryEvent>,
    name: String,
}

impl<E> Default for RetryConfigBuilder<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E> RetryConfigBuilder<E> {
    /// Creates a new builder with defaults.
    ///
    /// Defaults:
    /// - max_attempts: 3
    /// - backoff: Exponential with 100ms initial interval
    /// - name: `"<unnamed>"`
    pub fn new() -> Self {
        Self {
            max_attempts: 3,
            interval_fn: None,
            retry_predicate: None,
            event_listeners: EventListeners::new(),
            name: "<unnamed>".to_string(),
        }
    }

    /// Sets the maximum number of retry attempts.
    ///
    /// This includes the initial attempt, so max_attempts=3 means
    /// 1 initial attempt + 2 retries.
    pub fn max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts = max_attempts;
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

    /// Registers a callback for retry events.
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

    /// Registers a callback for success events.
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

    /// Registers a callback for error events (exhausted retries).
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

    /// Registers a callback for ignored error events.
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

    /// Builds the retry configuration.
    pub fn build(self) -> RetryConfig<E> {
        let interval_fn = self
            .interval_fn
            .unwrap_or_else(|| Arc::new(ExponentialBackoff::new(Duration::from_millis(100))));

        let mut policy = RetryPolicy::new(self.max_attempts, interval_fn);
        if let Some(predicate) = self.retry_predicate {
            policy.retry_predicate = Some(predicate);
        }

        RetryConfig {
            policy,
            event_listeners: self.event_listeners,
            name: self.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_builder_defaults() {
        let config: RetryConfig<std::io::Error> = RetryConfig::builder().build();
        assert_eq!(config.policy.max_attempts, 3);
        assert_eq!(config.name, "<unnamed>");
    }

    #[test]
    fn test_builder_custom_values() {
        let config: RetryConfig<std::io::Error> = RetryConfig::builder()
            .max_attempts(5)
            .fixed_backoff(Duration::from_secs(2))
            .name("test-retry")
            .build();

        assert_eq!(config.policy.max_attempts, 5);
        assert_eq!(config.policy.next_backoff(0), Duration::from_secs(2));
        assert_eq!(config.name, "test-retry");
    }

    #[test]
    fn test_event_listeners() {
        let retry_count = Arc::new(AtomicUsize::new(0));
        let success_count = Arc::new(AtomicUsize::new(0));

        let rc = Arc::clone(&retry_count);
        let sc = Arc::clone(&success_count);

        let config: RetryConfig<std::io::Error> = RetryConfig::builder()
            .on_retry(move |_, _| {
                rc.fetch_add(1, Ordering::SeqCst);
            })
            .on_success(move |_| {
                sc.fetch_add(1, Ordering::SeqCst);
            })
            .build();

        // Emit events
        let event = RetryEvent::Retry {
            pattern_name: "test".to_string(),
            timestamp: std::time::Instant::now(),
            attempt: 1,
            delay: Duration::from_secs(1),
        };
        config.event_listeners.emit(&event);

        let event = RetryEvent::Success {
            pattern_name: "test".to_string(),
            timestamp: std::time::Instant::now(),
            attempts: 2,
        };
        config.event_listeners.emit(&event);

        assert_eq!(retry_count.load(Ordering::SeqCst), 1);
        assert_eq!(success_count.load(Ordering::SeqCst), 1);
    }
}
