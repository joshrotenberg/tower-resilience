//! Configuration for chaos engineering layer.

use crate::events::ChaosEvent;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::sync::Arc;
use std::time::Duration;
use tower_resilience_core::{EventListeners, FnListener};

/// Trait for error injection behavior.
///
/// This trait is implemented by both the default (no injection) and custom error
/// injectors, enabling type inference for the common case of latency-only chaos.
pub trait ErrorInjector<Req, Err>: Send + Sync {
    /// Check if an error should be injected and return it.
    fn inject_error(&self, req: &Req, roll: f64) -> Option<Err>;

    /// Get the error rate for this injector.
    fn error_rate(&self) -> f64;
}

/// No error injection - only latency chaos.
///
/// This is the default error injector. Since it never injects errors,
/// it implements `ErrorInjector<Req, Err>` for ALL types, enabling
/// type inference at the point of use.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoErrorInjection;

impl<Req, Err> ErrorInjector<Req, Err> for NoErrorInjection {
    fn inject_error(&self, _req: &Req, _roll: f64) -> Option<Err> {
        None
    }

    fn error_rate(&self) -> f64 {
        0.0
    }
}

/// Custom error injection function.
///
/// This injector calls a function to generate errors based on the request.
pub struct CustomErrorFn<F> {
    f: Arc<F>,
    rate: f64,
}

impl<F> Clone for CustomErrorFn<F> {
    fn clone(&self) -> Self {
        Self {
            f: Arc::clone(&self.f),
            rate: self.rate,
        }
    }
}

impl<F> CustomErrorFn<F> {
    /// Create a new custom error injector.
    pub fn new(f: F, rate: f64) -> Self {
        Self {
            f: Arc::new(f),
            rate: rate.clamp(0.0, 1.0),
        }
    }
}

impl<Req, Err, F> ErrorInjector<Req, Err> for CustomErrorFn<F>
where
    F: Fn(&Req) -> Err + Send + Sync + 'static,
{
    fn inject_error(&self, req: &Req, roll: f64) -> Option<Err> {
        if roll < self.rate {
            Some((self.f)(req))
        } else {
            None
        }
    }

    fn error_rate(&self) -> f64 {
        self.rate
    }
}

/// Configuration for the chaos engineering layer.
///
/// The type parameter `E` is the error injector type:
/// - `ChaosConfig<NoErrorInjection>` - latency-only chaos (works with any types)
/// - `ChaosConfig<CustomErrorFn<F>>` - custom error injection
pub struct ChaosConfig<E> {
    /// Name of this chaos layer instance for observability
    pub(crate) name: String,
    /// Error injector
    pub(crate) error_injector: E,
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

impl<E: Clone> Clone for ChaosConfig<E> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            error_injector: self.error_injector.clone(),
            latency_rate: self.latency_rate,
            min_latency: self.min_latency,
            max_latency: self.max_latency,
            seed: self.seed,
            event_listeners: self.event_listeners.clone(),
        }
    }
}

impl<E> ChaosConfig<E> {
    /// Get the RNG for this configuration.
    pub(crate) fn create_rng(&self) -> StdRng {
        match self.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_os_rng(),
        }
    }
}

/// Builder for chaos configuration.
///
/// The type parameter `E` is the error injector type. By default, this is
/// `NoErrorInjection` which works with any request/error type. When you call
/// `.error_fn()`, the type changes to `CustomErrorFn<F>`.
///
/// # Latency-Only Chaos (no type parameters needed)
///
/// ```rust
/// use tower_resilience_chaos::ChaosLayer;
/// use std::time::Duration;
///
/// // No type parameters required for latency-only chaos!
/// let layer = ChaosLayer::builder()
///     .latency_rate(0.2)  // 20% of requests delayed
///     .min_latency(Duration::from_millis(50))
///     .max_latency(Duration::from_millis(200))
///     .build();
/// ```
///
/// # Error Injection (types inferred from closure)
///
/// ```rust
/// use tower_resilience_chaos::ChaosLayer;
///
/// // Types inferred from closure signature
/// let layer = ChaosLayer::builder()
///     .error_rate(0.1)
///     .error_fn(|_req: &String| std::io::Error::other("chaos!"))
///     .build();
/// ```
pub struct ChaosConfigBuilder<E = NoErrorInjection> {
    name: String,
    error_injector: E,
    latency_rate: f64,
    min_latency: Duration,
    max_latency: Duration,
    seed: Option<u64>,
    event_listeners: EventListeners<ChaosEvent>,
}

impl Default for ChaosConfigBuilder<NoErrorInjection> {
    fn default() -> Self {
        Self::new()
    }
}

impl ChaosConfigBuilder<NoErrorInjection> {
    /// Create a new chaos configuration builder.
    ///
    /// No type parameters are required when using latency-only chaos.
    pub fn new() -> Self {
        Self {
            name: "<unnamed>".to_string(),
            error_injector: NoErrorInjection,
            latency_rate: 0.0,
            min_latency: Duration::from_millis(10),
            max_latency: Duration::from_millis(100),
            seed: None,
            event_listeners: EventListeners::new(),
        }
    }
}

impl<E> ChaosConfigBuilder<E> {
    /// Set the name of this chaos layer instance.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosLayer;
    ///
    /// let layer = ChaosLayer::builder()
    ///     .name("test-chaos")
    ///     .latency_rate(0.1)
    ///     .build();
    /// ```
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the error injection rate and function.
    ///
    /// This configures error injection with a probability (0.0 - 1.0) and
    /// a function to generate errors. Types are inferred from the closure.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosLayer;
    ///
    /// // Types inferred from closure signature
    /// let layer = ChaosLayer::builder()
    ///     .error_rate(0.1)  // 10% of requests fail
    ///     .error_fn(|_req: &String| std::io::Error::other("chaos!"))
    ///     .build();
    /// ```
    pub fn error_fn<Req, Err, F>(self, f: F) -> ChaosConfigBuilder<CustomErrorFn<F>>
    where
        F: Fn(&Req) -> Err + Send + Sync + 'static,
    {
        ChaosConfigBuilder {
            name: self.name,
            error_injector: CustomErrorFn::new(f, 0.0), // rate will be set by error_rate()
            latency_rate: self.latency_rate,
            min_latency: self.min_latency,
            max_latency: self.max_latency,
            seed: self.seed,
            event_listeners: self.event_listeners,
        }
    }

    /// Set the latency injection rate (0.0 - 1.0).
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosLayer;
    ///
    /// // No type parameters needed for latency-only chaos!
    /// let layer = ChaosLayer::builder()
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
    /// use tower_resilience_chaos::ChaosLayer;
    /// use std::time::Duration;
    ///
    /// let layer = ChaosLayer::builder()
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
    /// use tower_resilience_chaos::ChaosLayer;
    /// use std::time::Duration;
    ///
    /// let layer = ChaosLayer::builder()
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
    /// use tower_resilience_chaos::ChaosLayer;
    ///
    /// let layer = ChaosLayer::builder()
    ///     .latency_rate(0.1)
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
    /// use tower_resilience_chaos::ChaosLayer;
    ///
    /// let layer = ChaosLayer::builder()
    ///     .latency_rate(0.1)
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
    /// use tower_resilience_chaos::ChaosLayer;
    /// use std::time::Duration;
    ///
    /// let layer = ChaosLayer::builder()
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
    /// use tower_resilience_chaos::ChaosLayer;
    ///
    /// let layer = ChaosLayer::builder()
    ///     .latency_rate(0.1)
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
    pub fn build(self) -> crate::layer::ChaosLayer<E> {
        let config = ChaosConfig {
            name: self.name,
            error_injector: self.error_injector,
            latency_rate: self.latency_rate,
            min_latency: self.min_latency,
            max_latency: self.max_latency,
            seed: self.seed,
            event_listeners: self.event_listeners,
        };
        crate::layer::ChaosLayer::new(config)
    }
}

// Special impl for CustomErrorFn to set the error rate
impl<F> ChaosConfigBuilder<CustomErrorFn<F>> {
    /// Set the error injection rate (0.0 - 1.0).
    ///
    /// This should be called before `error_fn()` or the rate will need to be updated.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosLayer;
    ///
    /// let layer = ChaosLayer::builder()
    ///     .error_rate(0.1)  // 10% of requests fail
    ///     .error_fn(|_req: &String| std::io::Error::other("chaos!"))
    ///     .build();
    /// ```
    pub fn error_rate(mut self, rate: f64) -> Self {
        self.error_injector.rate = rate.clamp(0.0, 1.0);
        self
    }
}

// Also allow error_rate on any builder (for the common case of calling it before error_fn)
impl ChaosConfigBuilder<NoErrorInjection> {
    /// Set the error injection rate (0.0 - 1.0).
    ///
    /// Note: This only takes effect when combined with `error_fn()`.
    /// For latency-only chaos, this has no effect.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosLayer;
    ///
    /// let layer = ChaosLayer::builder()
    ///     .error_rate(0.1)  // Will be used when error_fn is called
    ///     .error_fn(|_req: &String| std::io::Error::other("chaos!"))
    ///     .build();
    /// ```
    pub fn error_rate(self, _rate: f64) -> ChaosConfigBuilderWithRate {
        ChaosConfigBuilderWithRate {
            name: self.name,
            error_rate: _rate.clamp(0.0, 1.0),
            latency_rate: self.latency_rate,
            min_latency: self.min_latency,
            max_latency: self.max_latency,
            seed: self.seed,
            event_listeners: self.event_listeners,
        }
    }
}

/// Builder that has an error rate set but no error function yet.
pub struct ChaosConfigBuilderWithRate {
    name: String,
    error_rate: f64,
    latency_rate: f64,
    min_latency: Duration,
    max_latency: Duration,
    seed: Option<u64>,
    event_listeners: EventListeners<ChaosEvent>,
}

impl ChaosConfigBuilderWithRate {
    /// Set the error injection function.
    ///
    /// # Example
    /// ```
    /// use tower_resilience_chaos::ChaosLayer;
    ///
    /// let layer = ChaosLayer::builder()
    ///     .error_rate(0.1)
    ///     .error_fn(|_req: &String| std::io::Error::other("chaos!"))
    ///     .build();
    /// ```
    pub fn error_fn<Req, Err, F>(self, f: F) -> ChaosConfigBuilder<CustomErrorFn<F>>
    where
        F: Fn(&Req) -> Err + Send + Sync + 'static,
    {
        ChaosConfigBuilder {
            name: self.name,
            error_injector: CustomErrorFn::new(f, self.error_rate),
            latency_rate: self.latency_rate,
            min_latency: self.min_latency,
            max_latency: self.max_latency,
            seed: self.seed,
            event_listeners: self.event_listeners,
        }
    }

    /// Set the name of this chaos layer instance.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the latency injection rate (0.0 - 1.0).
    pub fn latency_rate(mut self, rate: f64) -> Self {
        self.latency_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Set the minimum latency to inject.
    pub fn min_latency(mut self, duration: Duration) -> Self {
        self.min_latency = duration;
        self
    }

    /// Set the maximum latency to inject.
    pub fn max_latency(mut self, duration: Duration) -> Self {
        self.max_latency = duration;
        self
    }

    /// Set a seed for deterministic chaos injection.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Add a listener for error injection events.
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

    /// Add a listener for pass-through events.
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
}
