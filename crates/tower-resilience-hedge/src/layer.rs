//! Tower Layer implementation for hedging.

use crate::config::{HedgeConfig, HedgeConfigBuilder};
use crate::Hedge;
use std::marker::PhantomData;
use std::time::Duration;
use tower_layer::Layer;

/// A Tower [`Layer`] that applies hedging to a service.
///
/// See the [crate-level documentation](crate) for more details.
pub struct HedgeLayer<Req, Res, E> {
    config: HedgeConfig<Req, Res, E>,
}

impl<Req, Res, E> HedgeLayer<Req, Res, E> {
    /// Create a new `HedgeLayer` with the given delay.
    ///
    /// This creates a layer that will fire a single hedge request
    /// after the specified delay if the primary hasn't completed.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_hedge::HedgeLayer;
    /// use std::time::Duration;
    ///
    /// let layer = HedgeLayer::<(), String, std::io::Error>::new(
    ///     Duration::from_millis(100)
    /// );
    /// ```
    pub fn new(delay: Duration) -> Self {
        Self::builder().delay(delay).build()
    }

    /// Create a builder for configuring the hedge layer.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_hedge::HedgeLayer;
    /// use std::time::Duration;
    ///
    /// let layer = HedgeLayer::<(), String, std::io::Error>::builder()
    ///     .delay(Duration::from_millis(100))
    ///     .max_hedged_attempts(3)
    ///     .build();
    /// ```
    pub fn builder() -> HedgeConfigBuilder<Req, Res, E> {
        HedgeConfigBuilder::new()
    }

    /// Create a `HedgeLayer` from a configuration.
    pub(crate) fn from_config(config: HedgeConfig<Req, Res, E>) -> Self {
        Self { config }
    }
}

impl<Req, Res, E> Clone for HedgeLayer<Req, Res, E> {
    fn clone(&self) -> Self {
        Self {
            config: HedgeConfig {
                name: self.config.name.clone(),
                max_hedged_attempts: self.config.max_hedged_attempts,
                delay: self.config.delay.clone(),
                listeners: self.config.listeners.clone(),
                _phantom: PhantomData,
            },
        }
    }
}

impl<S, Req, Res, E> Layer<S> for HedgeLayer<Req, Res, E>
where
    Req: Clone,
    Res: Clone,
    E: Clone,
{
    type Service = Hedge<S, Req, Res, E>;

    fn layer(&self, service: S) -> Self::Service {
        Hedge::new(
            service,
            HedgeConfig {
                name: self.config.name.clone(),
                max_hedged_attempts: self.config.max_hedged_attempts,
                delay: self.config.delay.clone(),
                listeners: self.config.listeners.clone(),
                _phantom: PhantomData,
            },
        )
    }
}
