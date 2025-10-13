//! Configuration for chaos engineering layer.

use crate::events::ChaosEvent;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::sync::Arc;
use std::time::Duration;
use tower_resilience_core::{EventListeners, FnListener};

/// Type alias for error generation function
type ErrorFn<Req, Err> = Arc<dyn Fn(&Req) -> Err + Send + Sync>;

/// Configuration for the chaos engineering layer.
pub struct ChaosConfig<Req, Err> {
    /// Name of this chaos layer instance for observability
    pub(crate) name: String,
    /// Probability of injecting an error (0.0 - 1.0)
    pub(crate) error_rate: f64,
    /// Function to generate errors when injecting
    pub(crate) error_fn: Option<ErrorFn<Req, Err>>,
    /// Probability of injecting latency (0.0 - 1.0)
    pub(crate) latency_rate: f64,
    /// Minimum latency to inject
    pub(crate) min_latency: Duration,
    /// Maximum latency to inject
    pub(crate) max_latency: Duration,
    /// Optional seed for deterministic chaos
    pub(crate) seed: Option<u64>,
    /// Event listeners
    pub(crate) event_listeners: EventListeners<ChaosEvent>,
}

impl<Req, Err> Clone for ChaosConfig<Req, Err> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            error_rate: self.error_rate,
            error_fn: self.error_fn.clone(),
            latency_rate: self.latency_rate,
            min_latency: self.min_latency,
            max_latency: self.max_latency,
            seed: self.seed,
            event_listeners: self.event_listeners.clone(),
        }
    }
}

impl<Req, Err> ChaosConfig<Req, Err> {
    /// Create a new builder for chaos configuration.
    pub fn builder() -> ChaosConfigBuilder<Req, Err> {
        ChaosConfigBuilder::new()
    }

    /// Get the RNG for this configuration.
    pub(crate) fn create_rng(&self) -> StdRng {
        match self.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_os_rng(),
        }
    }
}

/// Builder for chaos configuration.
pub struct ChaosConfigBuilder<Req, Err> {
    name: String,
    error_rate: f64,
    error_fn: Option<ErrorFn<Req, Err>>,
    latency_rate: f64,
    min_latency: Duration,
    max_latency: Duration,
    seed: Option<u64>,
    event_listeners: EventListeners<ChaosEvent>,
}

impl<Req, Err> ChaosConfigBuilder<Req, Err> {
    /// Create a new chaos configuration builder.
    pub fn new() -> Self {
        Self {
            name: "<unnamed>".to_string(),
            error_rate: 0.0,
            error_fn: None,
            latency_rate: 0.0,
            min_latency: Duration::from_millis(10),
            max_latency: Duration::from_millis(100),
            seed: None,
            event_listeners: EventListeners::new(),
        }
    }

    /// Set the name of this chaos layer instance.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosConfig;
    ///
    /// let config = ChaosConfig::<(), ()>::builder()
    ///     .name("test-chaos")
    ///     .build();
    /// ```
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the error injection rate (0.0 - 1.0).
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosConfig;
    ///
    /// let config = ChaosConfig::<(), ()>::builder()
    ///     .error_rate(0.1)  // 10% of requests will fail
    ///     .build();
    /// ```
    pub fn error_rate(mut self, rate: f64) -> Self {
        self.error_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Set the function to generate errors.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosConfig;
    ///
    /// let config = ChaosConfig::<(), std::io::Error>::builder()
    ///     .error_rate(0.1)
    ///     .error_fn(|_req| {
    ///         std::io::Error::new(std::io::ErrorKind::Other, "chaos!")
    ///     })
    ///     .build();
    /// ```
    pub fn error_fn<F>(mut self, f: F) -> Self
    where
        F: Fn(&Req) -> Err + Send + Sync + 'static,
    {
        self.error_fn = Some(Arc::new(f));
        self
    }

    /// Set the latency injection rate (0.0 - 1.0).
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosConfig;
    ///
    /// let config = ChaosConfig::<(), ()>::builder()
    ///     .latency_rate(0.2)  // 20% of requests will be delayed
    ///     .build();
    /// ```
    pub fn latency_rate(mut self, rate: f64) -> Self {
        self.latency_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Set the minimum latency to inject.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosConfig;
    /// use std::time::Duration;
    ///
    /// let config = ChaosConfig::<(), ()>::builder()
    ///     .latency_rate(0.2)
    ///     .min_latency(Duration::from_millis(50))
    ///     .build();
    /// ```
    pub fn min_latency(mut self, duration: Duration) -> Self {
        self.min_latency = duration;
        self
    }

    /// Set the maximum latency to inject.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosConfig;
    /// use std::time::Duration;
    ///
    /// let config = ChaosConfig::<(), ()>::builder()
    ///     .latency_rate(0.2)
    ///     .max_latency(Duration::from_millis(500))
    ///     .build();
    /// ```
    pub fn max_latency(mut self, duration: Duration) -> Self {
        self.max_latency = duration;
        self
    }

    /// Set a seed for deterministic chaos injection.
    ///
    /// Useful for reproducible tests.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosConfig;
    ///
    /// let config = ChaosConfig::<(), ()>::builder()
    ///     .error_rate(0.1)
    ///     .seed(42)  // Deterministic behavior
    ///     .build();
    /// ```
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Add a listener for error injection events.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosConfig;
    ///
    /// let config = ChaosConfig::<(), ()>::builder()
    ///     .error_rate(0.1)
    ///     .on_error_injected(|| {
    ///         println!("Chaos: error injected!");
    ///     })
    ///     .build();
    /// ```
    pub fn on_error_injected<F>(mut self, f: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if matches!(event, ChaosEvent::ErrorInjected { .. }) {
                f();
            }
        }));
        self
    }

    /// Add a listener for latency injection events.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosConfig;
    /// use std::time::Duration;
    ///
    /// let config = ChaosConfig::<(), ()>::builder()
    ///     .latency_rate(0.2)
    ///     .on_latency_injected(|delay: Duration| {
    ///         println!("Chaos: injected {:?} latency", delay);
    ///     })
    ///     .build();
    /// ```
    pub fn on_latency_injected<F>(mut self, f: F) -> Self
    where
        F: Fn(Duration) + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if let ChaosEvent::LatencyInjected { delay, .. } = event {
                f(*delay);
            }
        }));
        self
    }

    /// Add a listener for pass-through events (no chaos injected).
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosConfig;
    ///
    /// let config = ChaosConfig::<(), ()>::builder()
    ///     .error_rate(0.1)
    ///     .on_passed_through(|| {
    ///         println!("Chaos: request passed through");
    ///     })
    ///     .build();
    /// ```
    pub fn on_passed_through<F>(mut self, f: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if matches!(event, ChaosEvent::PassedThrough { .. }) {
                f();
            }
        }));
        self
    }

    /// Build the chaos configuration and return a ChaosLayer.
    pub fn build(self) -> crate::layer::ChaosLayer<Req, Err> {
        let config = ChaosConfig {
            name: self.name,
            error_rate: self.error_rate,
            error_fn: self.error_fn,
            latency_rate: self.latency_rate,
            min_latency: self.min_latency,
            max_latency: self.max_latency,
            seed: self.seed,
            event_listeners: self.event_listeners,
        };
        crate::layer::ChaosLayer::new(config)
    }
}

impl<Req, Err> Default for ChaosConfigBuilder<Req, Err> {
    fn default() -> Self {
        Self::new()
    }
}
