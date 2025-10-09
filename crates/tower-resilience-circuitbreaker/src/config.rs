use crate::events::CircuitBreakerEvent;
use crate::SharedFailureClassifier;
use std::sync::Arc;
use std::time::Duration;
use tower_resilience_core::EventListeners;

/// Type of sliding window used for tracking calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlidingWindowType {
    /// Count-based window tracks the last N calls.
    CountBased,
    /// Time-based window tracks calls within a time duration.
    TimeBased,
}

/// Configuration for the circuit breaker pattern.
pub struct CircuitBreakerConfig<Res, Err> {
    pub(crate) failure_rate_threshold: f64,
    pub(crate) sliding_window_type: SlidingWindowType,
    pub(crate) sliding_window_size: usize,
    pub(crate) sliding_window_duration: Option<Duration>,
    pub(crate) wait_duration_in_open: Duration,
    pub(crate) permitted_calls_in_half_open: usize,
    pub(crate) minimum_number_of_calls: usize,
    pub(crate) failure_classifier: SharedFailureClassifier<Res, Err>,
    pub(crate) slow_call_duration_threshold: Option<Duration>,
    pub(crate) slow_call_rate_threshold: f64,
    pub(crate) event_listeners: EventListeners<CircuitBreakerEvent>,
    pub(crate) name: String,
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
    sliding_window_type: SlidingWindowType,
    sliding_window_size: usize,
    sliding_window_duration: Option<Duration>,
    wait_duration_in_open: Duration,
    permitted_calls_in_half_open: usize,
    failure_classifier: SharedFailureClassifier<Res, Err>,
    minimum_number_of_calls: Option<usize>,
    slow_call_duration_threshold: Option<Duration>,
    slow_call_rate_threshold: f64,
    event_listeners: EventListeners<CircuitBreakerEvent>,
    name: String,
}

impl<Res, Err> CircuitBreakerConfigBuilder<Res, Err> {
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self {
            failure_rate_threshold: 0.5,
            sliding_window_type: SlidingWindowType::CountBased,
            sliding_window_size: 100,
            sliding_window_duration: None,
            wait_duration_in_open: Duration::from_secs(30),
            permitted_calls_in_half_open: 1,
            failure_classifier: Arc::new(|res| res.is_err()),
            minimum_number_of_calls: None,
            slow_call_duration_threshold: None,
            slow_call_rate_threshold: 1.0,
            event_listeners: EventListeners::new(),
            name: String::from("<unnamed>"),
        }
    }

    /// Sets the failure rate threshold at which the circuit will open.
    ///
    /// Default: 0.5 (50%)
    pub fn failure_rate_threshold(mut self, rate: f64) -> Self {
        self.failure_rate_threshold = rate;
        self
    }

    /// Sets the type of sliding window to use.
    ///
    /// Default: CountBased
    pub fn sliding_window_type(mut self, window_type: SlidingWindowType) -> Self {
        self.sliding_window_type = window_type;
        self
    }

    /// Sets the size of the sliding window for failure rate calculation (count-based).
    ///
    /// For count-based windows, this is the number of calls to track.
    /// For time-based windows, this is used as the minimum calls threshold if not set explicitly.
    ///
    /// Default: 100
    pub fn sliding_window_size(mut self, size: usize) -> Self {
        self.sliding_window_size = size;
        self
    }

    /// Sets the duration of the sliding window (time-based only).
    ///
    /// Only used when `sliding_window_type` is `TimeBased`.
    /// Calls older than this duration are excluded from failure rate calculation.
    ///
    /// Default: None (must be set for time-based windows)
    pub fn sliding_window_duration(mut self, duration: Duration) -> Self {
        self.sliding_window_duration = Some(duration);
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

    /// Sets the duration threshold for considering a call "slow".
    ///
    /// When set, calls exceeding this duration will be tracked and can trigger
    /// circuit opening based on `slow_call_rate_threshold`.
    ///
    /// Default: None (slow call detection disabled)
    pub fn slow_call_duration_threshold(mut self, duration: Duration) -> Self {
        self.slow_call_duration_threshold = Some(duration);
        self
    }

    /// Sets the slow call rate threshold at which the circuit will open.
    ///
    /// Only applies when `slow_call_duration_threshold` is set.
    ///
    /// Default: 1.0 (100%, effectively disabled)
    pub fn slow_call_rate_threshold(mut self, rate: f64) -> Self {
        self.slow_call_rate_threshold = rate;
        self
    }

    /// Give this breaker a human-readable name for observability.
    ///
    /// Default: `<unnamed>`
    pub fn name<N: Into<String>>(mut self, n: N) -> Self {
        self.name = n.into();
        self
    }

    /// Registers a callback when the circuit breaker transitions between states.
    ///
    /// The circuit breaker has three states: Closed (normal operation), Open (failing),
    /// and HalfOpen (testing recovery). This callback is invoked whenever the circuit
    /// transitions from one state to another.
    ///
    /// # Callback Signature
    /// `Fn(CircuitState, CircuitState)` - Called with two parameters:
    /// - First parameter: The state the circuit is transitioning **from**
    /// - Second parameter: The state the circuit is transitioning **to**
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_circuitbreaker::{CircuitBreakerConfig, CircuitState};
    ///
    /// let config = CircuitBreakerConfig::<(), ()>::builder()
    ///     .on_state_transition(|from, to| {
    ///         println!("Circuit breaker: {:?} -> {:?}", from, to);
    ///         match to {
    ///             CircuitState::Open => println!("WARNING: Circuit opened - service degraded"),
    ///             CircuitState::HalfOpen => println!("Testing recovery..."),
    ///             CircuitState::Closed => println!("Circuit recovered - normal operation"),
    ///         }
    ///     })
    ///     .build();
    /// ```
    pub fn on_state_transition<F>(mut self, f: F) -> Self
    where
        F: Fn(crate::CircuitState, crate::CircuitState) + Send + Sync + 'static,
    {
        use tower_resilience_core::FnListener;
        self.event_listeners
            .add(FnListener::new(move |event: &CircuitBreakerEvent| {
                if let CircuitBreakerEvent::StateTransition {
                    from_state,
                    to_state,
                    ..
                } = event
                {
                    f(*from_state, *to_state);
                }
            }));
        self
    }

    /// Registers a callback when a call is permitted through the circuit breaker.
    ///
    /// This callback is invoked when the circuit breaker allows a request to proceed to
    /// the underlying service. Calls are permitted in the Closed and HalfOpen states,
    /// but rejected in the Open state.
    ///
    /// # Callback Signature
    /// `Fn(CircuitState)` - Called with the current state of the circuit when the call was permitted.
    /// Will typically be `CircuitState::Closed` or `CircuitState::HalfOpen`.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_circuitbreaker::{CircuitBreakerConfig, CircuitState};
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let call_count = Arc::new(AtomicUsize::new(0));
    /// let counter = Arc::clone(&call_count);
    ///
    /// let config = CircuitBreakerConfig::<(), ()>::builder()
    ///     .on_call_permitted(move |state| {
    ///         let count = counter.fetch_add(1, Ordering::SeqCst);
    ///         println!("Call #{} permitted in state: {:?}", count + 1, state);
    ///     })
    ///     .build();
    /// ```
    pub fn on_call_permitted<F>(mut self, f: F) -> Self
    where
        F: Fn(crate::CircuitState) + Send + Sync + 'static,
    {
        self.event_listeners
            .add(tower_resilience_core::FnListener::new(
                move |event: &CircuitBreakerEvent| {
                    if let CircuitBreakerEvent::CallPermitted { state, .. } = event {
                        f(*state);
                    }
                },
            ));
        self
    }

    /// Registers a callback when a call is rejected by the circuit breaker.
    ///
    /// This callback is invoked when the circuit breaker rejects a request without
    /// allowing it to reach the underlying service. Calls are rejected when the circuit
    /// is in the Open state, indicating that the service is currently failing and the
    /// circuit is protecting it from additional load.
    ///
    /// # Callback Signature
    /// `Fn()` - Called with no parameters when a call is rejected.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_circuitbreaker::CircuitBreakerConfig;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let rejection_count = Arc::new(AtomicUsize::new(0));
    /// let counter = Arc::clone(&rejection_count);
    ///
    /// let config = CircuitBreakerConfig::<(), ()>::builder()
    ///     .on_call_rejected(move || {
    ///         let count = counter.fetch_add(1, Ordering::SeqCst);
    ///         println!("Call rejected - circuit is open (total: {})", count + 1);
    ///     })
    ///     .build();
    /// ```
    pub fn on_call_rejected<F>(mut self, f: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.event_listeners
            .add(tower_resilience_core::FnListener::new(
                move |event: &CircuitBreakerEvent| {
                    if matches!(event, CircuitBreakerEvent::CallRejected { .. }) {
                        f();
                    }
                },
            ));
        self
    }

    /// Registers a callback when a successful call is recorded.
    ///
    /// This callback is invoked when a call completes successfully (as determined by the
    /// failure classifier). Successful calls contribute to closing the circuit or keeping
    /// it closed, demonstrating that the underlying service is healthy.
    ///
    /// # Callback Signature
    /// `Fn(CircuitState)` - Called with the current state of the circuit when the success was recorded.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_circuitbreaker::{CircuitBreakerConfig, CircuitState};
    ///
    /// let config = CircuitBreakerConfig::<(), ()>::builder()
    ///     .on_success(|state| {
    ///         match state {
    ///             CircuitState::HalfOpen => println!("Success in half-open - may recover soon"),
    ///             CircuitState::Closed => println!("Success - circuit healthy"),
    ///             _ => {}
    ///         }
    ///     })
    ///     .build();
    /// ```
    pub fn on_success<F>(mut self, f: F) -> Self
    where
        F: Fn(crate::CircuitState) + Send + Sync + 'static,
    {
        self.event_listeners
            .add(tower_resilience_core::FnListener::new(
                move |event: &CircuitBreakerEvent| {
                    if let CircuitBreakerEvent::SuccessRecorded { state, .. } = event {
                        f(*state);
                    }
                },
            ));
        self
    }

    /// Registers a callback when a failed call is recorded.
    ///
    /// This callback is invoked when a call fails (as determined by the failure classifier).
    /// Failed calls contribute to opening the circuit when the failure rate exceeds the
    /// configured threshold, protecting the underlying service from additional load.
    ///
    /// # Callback Signature
    /// `Fn(CircuitState)` - Called with the current state of the circuit when the failure was recorded.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_circuitbreaker::{CircuitBreakerConfig, CircuitState};
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use std::sync::Arc;
    ///
    /// let failure_count = Arc::new(AtomicUsize::new(0));
    /// let counter = Arc::clone(&failure_count);
    ///
    /// let config = CircuitBreakerConfig::<(), ()>::builder()
    ///     .on_failure(move |state| {
    ///         let count = counter.fetch_add(1, Ordering::SeqCst);
    ///         println!("Failure #{} recorded in state: {:?}", count + 1, state);
    ///         if matches!(state, CircuitState::HalfOpen) {
    ///             println!("Warning: failure during recovery attempt");
    ///         }
    ///     })
    ///     .build();
    /// ```
    pub fn on_failure<F>(mut self, f: F) -> Self
    where
        F: Fn(crate::CircuitState) + Send + Sync + 'static,
    {
        self.event_listeners
            .add(tower_resilience_core::FnListener::new(
                move |event: &CircuitBreakerEvent| {
                    if let CircuitBreakerEvent::FailureRecorded { state, .. } = event {
                        f(*state);
                    }
                },
            ));
        self
    }

    /// Registers a callback when a slow call is detected.
    ///
    /// This callback is invoked when a call takes longer than the configured
    /// `slow_call_duration_threshold`. Slow calls are tracked separately from failures,
    /// and if the slow call rate exceeds `slow_call_rate_threshold`, the circuit will open
    /// even if the failure rate is acceptable. This protects against performance degradation.
    ///
    /// Note: This callback is only invoked when `slow_call_duration_threshold` is configured.
    ///
    /// # Callback Signature
    /// `Fn(Duration)` - Called with the actual duration the slow call took to complete.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tower_resilience_circuitbreaker::CircuitBreakerConfig;
    /// use std::time::Duration;
    ///
    /// let config = CircuitBreakerConfig::<(), ()>::builder()
    ///     .slow_call_duration_threshold(Duration::from_secs(2))
    ///     .slow_call_rate_threshold(0.5) // Open if >50% of calls are slow
    ///     .on_slow_call(|duration| {
    ///         println!("Slow call detected: {:?}", duration);
    ///         if duration > Duration::from_secs(10) {
    ///             println!("WARNING: Extremely slow call!");
    ///         }
    ///     })
    ///     .build();
    /// ```
    pub fn on_slow_call<F>(mut self, f: F) -> Self
    where
        F: Fn(Duration) + Send + Sync + 'static,
    {
        use tower_resilience_core::FnListener;
        self.event_listeners
            .add(FnListener::new(move |event: &CircuitBreakerEvent| {
                if let CircuitBreakerEvent::SlowCallDetected { duration, .. } = event {
                    f(*duration);
                }
            }));
        self
    }

    /// Builds the configuration and returns a CircuitBreakerLayer.
    pub fn build(self) -> crate::layer::CircuitBreakerLayer<Res, Err> {
        // Validate time-based window configuration
        if self.sliding_window_type == SlidingWindowType::TimeBased
            && self.sliding_window_duration.is_none()
        {
            panic!("sliding_window_duration must be set when using TimeBased sliding window");
        }

        let config = CircuitBreakerConfig {
            failure_rate_threshold: self.failure_rate_threshold,
            sliding_window_type: self.sliding_window_type,
            sliding_window_size: self.sliding_window_size,
            sliding_window_duration: self.sliding_window_duration,
            wait_duration_in_open: self.wait_duration_in_open,
            permitted_calls_in_half_open: self.permitted_calls_in_half_open,
            failure_classifier: self.failure_classifier,
            minimum_number_of_calls: self
                .minimum_number_of_calls
                .unwrap_or(self.sliding_window_size),
            slow_call_duration_threshold: self.slow_call_duration_threshold,
            slow_call_rate_threshold: self.slow_call_rate_threshold,
            event_listeners: self.event_listeners,
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
