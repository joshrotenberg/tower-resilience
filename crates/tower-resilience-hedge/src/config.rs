//! Configuration for the hedging middleware.

use crate::events::HedgeEvent;
use crate::layer::HedgeLayer;
use std::sync::Arc;
use std::time::Duration;
use tower_resilience_core::{EventListener, EventListeners};

/// Delay strategy for hedged requests.
#[derive(Clone)]
pub enum HedgeDelay {
    /// Fixed delay before each hedge attempt.
    Fixed(Duration),
    /// No delay - fire all attempts immediately (parallel mode).
    Immediate,
    /// Dynamic delay based on attempt number.
    Dynamic(Arc<dyn Fn(usize) -> Duration + Send + Sync>),
}

impl HedgeDelay {
    /// Get the delay for the given attempt number (1-indexed).
    pub fn get_delay(&self, attempt: usize) -> Option<Duration> {
        match self {
            HedgeDelay::Fixed(d) => Some(*d),
            HedgeDelay::Immediate => Some(Duration::ZERO),
            HedgeDelay::Dynamic(f) => Some(f(attempt)),
        }
    }
}

impl Default for HedgeDelay {
    fn default() -> Self {
        HedgeDelay::Fixed(Duration::from_secs(1))
    }
}

/// Configuration for the hedging service.
///
/// This configuration is type-agnostic - it doesn't depend on the request,
/// response, or error types. Types are only constrained when the layer is
/// applied to a service.
#[derive(Clone)]
pub struct HedgeConfig {
    /// Name for metrics/tracing.
    pub(crate) name: Option<String>,
    /// Maximum number of hedged attempts (including original).
    pub(crate) max_hedged_attempts: usize,
    /// Delay before firing each hedge.
    pub(crate) delay: HedgeDelay,
    /// Event listeners.
    pub(crate) listeners: EventListeners<HedgeEvent>,
}

impl Default for HedgeConfig {
    fn default() -> Self {
        Self {
            name: None,
            max_hedged_attempts: 2,
            delay: HedgeDelay::default(),
            listeners: EventListeners::default(),
        }
    }
}

/// Builder for [`HedgeConfig`].
///
/// No type parameters needed - types are inferred when the layer is applied to a service.
///
/// # Example
///
/// ```rust
/// use tower_resilience_hedge::HedgeLayer;
/// use std::time::Duration;
///
/// // No type parameters needed!
/// let layer = HedgeLayer::builder()
///     .delay(Duration::from_millis(100))
///     .max_hedged_attempts(3)
///     .build();
/// ```
pub struct HedgeConfigBuilder {
    config: HedgeConfig,
}

impl Default for HedgeConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl HedgeConfigBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self {
            config: HedgeConfig::default(),
        }
    }

    /// Set the name for this hedge instance (used in metrics/tracing).
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.config.name = Some(name.into());
        self
    }

    /// Set the maximum number of hedged attempts (including the original request).
    ///
    /// Default is 2 (1 original + 1 hedge).
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_hedge::HedgeLayer;
    ///
    /// // Allow up to 3 parallel attempts - no type parameters needed!
    /// let layer = HedgeLayer::builder()
    ///     .max_hedged_attempts(3)
    ///     .build();
    /// ```
    pub fn max_hedged_attempts(mut self, n: usize) -> Self {
        self.config.max_hedged_attempts = n.max(1);
        self
    }

    /// Set a fixed delay before firing hedge requests.
    ///
    /// After this delay, if the primary request hasn't completed,
    /// a hedge request will be fired.
    ///
    /// Default is 1 second.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_hedge::HedgeLayer;
    /// use std::time::Duration;
    ///
    /// // Fire hedge after 100ms - no type parameters needed!
    /// let layer = HedgeLayer::builder()
    ///     .delay(Duration::from_millis(100))
    ///     .build();
    /// ```
    pub fn delay(mut self, delay: Duration) -> Self {
        self.config.delay = HedgeDelay::Fixed(delay);
        self
    }

    /// Fire all hedge requests immediately (parallel mode).
    ///
    /// All requests are fired simultaneously and the first successful
    /// response is returned. Use when latency is critical and you can
    /// afford the additional resource usage.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_hedge::HedgeLayer;
    ///
    /// // Fire 3 requests immediately - no type parameters needed!
    /// let layer = HedgeLayer::builder()
    ///     .no_delay()
    ///     .max_hedged_attempts(3)
    ///     .build();
    /// ```
    pub fn no_delay(mut self) -> Self {
        self.config.delay = HedgeDelay::Immediate;
        self
    }

    /// Set a dynamic delay generator based on attempt number.
    ///
    /// The function receives the attempt number (1-indexed) and returns
    /// the delay before that attempt should fire.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_hedge::HedgeLayer;
    /// use std::time::Duration;
    ///
    /// // Increasing delays: 50ms, 100ms, 150ms...
    /// let layer = HedgeLayer::builder()
    ///     .delay_fn(|attempt| Duration::from_millis(50 * attempt as u64))
    ///     .max_hedged_attempts(3)
    ///     .build();
    /// ```
    pub fn delay_fn<F>(mut self, f: F) -> Self
    where
        F: Fn(usize) -> Duration + Send + Sync + 'static,
    {
        self.config.delay = HedgeDelay::Dynamic(Arc::new(f));
        self
    }

    /// Add an event listener for hedge events.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_hedge::{HedgeLayer, HedgeEvent};
    /// use tower_resilience_core::FnListener;
    ///
    /// let layer = HedgeLayer::builder()
    ///     .on_event(FnListener::new(|event: &HedgeEvent| {
    ///         match event {
    ///             HedgeEvent::HedgeSucceeded { attempt, duration, .. } => {
    ///                 println!("Hedge {} succeeded in {:?}", attempt, duration);
    ///             }
    ///             _ => {}
    ///         }
    ///     }))
    ///     .build();
    /// ```
    pub fn on_event<L>(mut self, listener: L) -> Self
    where
        L: EventListener<HedgeEvent> + 'static,
    {
        self.config.listeners.add(listener);
        self
    }

    /// Build the [`HedgeLayer`].
    pub fn build(self) -> HedgeLayer {
        HedgeLayer::from_config(self.config)
    }
}
