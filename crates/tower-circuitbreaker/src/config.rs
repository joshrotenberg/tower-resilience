use crate::SharedFailureClassifier;
use std::sync::Arc;
use std::time::Duration;

/// Configuration for the circuit breaker pattern.
pub struct CircuitBreakerConfig<Res, Err> {
    pub(crate) failure_rate_threshold: f64,
    pub(crate) sliding_window_size: usize,
    pub(crate) wait_duration_in_open: Duration,
    pub(crate) permitted_calls_in_half_open: usize,
    pub(crate) minimum_number_of_calls: usize,
    pub(crate) failure_classifier: SharedFailureClassifier<Res, Err>,
    #[cfg(feature = "tracing")]
    pub(crate) name: Option<String>,
}

impl<Res, Err> CircuitBreakerConfig<Res, Err> {
    /// Creates a new configuration builder.
    pub fn builder() -> CircuitBreakerConfigBuilder<Res, Err> {
        CircuitBreakerConfigBuilder::new()
    }
}

/// Builder for configuring and constructing a circuit breaker.
pub struct CircuitBreakerConfigBuilder<Res, Err> {
    failure_rate_threshold: f64,
    sliding_window_size: usize,
    wait_duration_in_open: Duration,
    permitted_calls_in_half_open: usize,
    failure_classifier: SharedFailureClassifier<Res, Err>,
    minimum_number_of_calls: Option<usize>,
    name: Option<String>,
}

impl<Res, Err> CircuitBreakerConfigBuilder<Res, Err> {
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self {
            failure_rate_threshold: 0.5,
            sliding_window_size: 100,
            wait_duration_in_open: Duration::from_secs(30),
            permitted_calls_in_half_open: 1,
            failure_classifier: Arc::new(|res| res.is_err()),
            minimum_number_of_calls: None,
            name: None,
        }
    }

    /// Sets the failure rate threshold at which the circuit will open.
    ///
    /// Default: 0.5 (50%)
    pub fn failure_rate_threshold(mut self, rate: f64) -> Self {
        self.failure_rate_threshold = rate;
        self
    }

    /// Sets the size of the sliding window for failure rate calculation.
    ///
    /// Default: 100
    pub fn sliding_window_size(mut self, size: usize) -> Self {
        self.sliding_window_size = size;
        self
    }

    /// Sets the duration the circuit remains open before transitioning to half-open.
    ///
    /// Default: 30 seconds
    pub fn wait_duration_in_open(mut self, duration: Duration) -> Self {
        self.wait_duration_in_open = duration;
        self
    }

    /// Sets the number of permitted calls in the half-open state.
    ///
    /// Default: 1
    pub fn permitted_calls_in_half_open(mut self, n: usize) -> Self {
        self.permitted_calls_in_half_open = n;
        self
    }

    /// Sets a custom failure classifier function.
    ///
    /// Default: classifies errors as failures
    pub fn failure_classifier<F>(mut self, classifier: F) -> Self
    where
        F: Fn(&Result<Res, Err>) -> bool + Send + Sync + 'static,
    {
        self.failure_classifier = Arc::new(classifier);
        self
    }

    /// Sets the minimum number of calls before failure rate is evaluated.
    ///
    /// Default: same as sliding_window_size
    pub fn minimum_number_of_calls(mut self, n: usize) -> Self {
        self.minimum_number_of_calls = Some(n);
        self
    }

    /// Give this breaker a human-readable name for logs/spans.
    ///
    /// Default: None
    pub fn name<N: Into<String>>(mut self, n: N) -> Self {
        self.name = Some(n.into());
        self
    }

    /// Builds the configuration and returns a CircuitBreakerLayer.
    pub fn build(self) -> crate::layer::CircuitBreakerLayer<Res, Err> {
        let config = CircuitBreakerConfig {
            failure_rate_threshold: self.failure_rate_threshold,
            sliding_window_size: self.sliding_window_size,
            wait_duration_in_open: self.wait_duration_in_open,
            permitted_calls_in_half_open: self.permitted_calls_in_half_open,
            failure_classifier: self.failure_classifier,
            minimum_number_of_calls: self
                .minimum_number_of_calls
                .unwrap_or(self.sliding_window_size),
            #[cfg(feature = "tracing")]
            name: self.name,
        };

        crate::layer::CircuitBreakerLayer::new(config)
    }
}

impl<Res, Err> Default for CircuitBreakerConfigBuilder<Res, Err> {
    fn default() -> Self {
        Self::new()
    }
}
