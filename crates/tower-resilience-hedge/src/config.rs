//! Configuration for the hedging middleware.

use crate::events::HedgeEvent;
use crate::layer::HedgeLayer;
use std::marker::PhantomData;
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
pub struct HedgeConfig<Req, Res, E> {
    /// Name for metrics/tracing.
    pub(crate) name: Option<String>,
    /// Maximum number of hedged attempts (including original).
    pub(crate) max_hedged_attempts: usize,
    /// Delay before firing each hedge.
    pub(crate) delay: HedgeDelay,
    /// Event listeners.
    pub(crate) listeners: EventListeners<HedgeEvent>,
    /// Phantom data for type parameters.
    pub(crate) _phantom: PhantomData<(Req, Res, E)>,
}

impl<Req, Res, E> Default for HedgeConfig<Req, Res, E> {
    fn default() -> Self {
        Self {
            name: None,
            max_hedged_attempts: 2,
            delay: HedgeDelay::default(),
            listeners: EventListeners::default(),
            _phantom: PhantomData,
        }
    }
}

/// Builder for [`HedgeConfig`].
pub struct HedgeConfigBuilder<Req, Res, E> {
    config: HedgeConfig<Req, Res, E>,
}

impl<Req, Res, E> Default for HedgeConfigBuilder<Req, Res, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Req, Res, E> HedgeConfigBuilder<Req, Res, E> {
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
    /// // Allow up to 3 parallel attempts
    /// let layer = HedgeLayer::<(), String, std::io::Error>::builder()
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
    /// // Fire hedge after 100ms
    /// let layer = HedgeLayer::<(), String, std::io::Error>::builder()
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
    /// // Fire 3 requests immediately
    /// let layer = HedgeLayer::<(), String, std::io::Error>::builder()
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
    /// let layer = HedgeLayer::<(), String, std::io::Error>::builder()
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
    /// let layer = HedgeLayer::<(), String, std::io::Error>::builder()
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
    pub fn build(self) -> HedgeLayer<Req, Res, E> {
        HedgeLayer::from_config(self.config)
    }
}
