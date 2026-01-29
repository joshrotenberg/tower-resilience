//! Tower Layer implementation for hedging.

use crate::config::{HedgeConfig, HedgeConfigBuilder};
use crate::Hedge;
use std::time::Duration;
use tower_layer::Layer;

/// A Tower [`Layer`] that applies hedging to a service.
///
/// No type parameters needed - types are inferred from the service.
///
/// See the [crate-level documentation](crate) for more details.
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
#[derive(Clone)]
pub struct HedgeLayer {
    config: HedgeConfig,
}

impl HedgeLayer {
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
    /// // No type parameters needed!
    /// let layer = HedgeLayer::new(Duration::from_millis(100));
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
    /// // No type parameters needed!
    /// let layer = HedgeLayer::builder()
    ///     .delay(Duration::from_millis(100))
    ///     .max_hedged_attempts(3)
    ///     .build();
    /// ```
    pub fn builder() -> HedgeConfigBuilder {
        HedgeConfigBuilder::new()
    }

    /// Create a `HedgeLayer` from a configuration.
    pub(crate) fn from_config(config: HedgeConfig) -> Self {
        Self { config }
    }
}

impl<S> Layer<S> for HedgeLayer {
    type Service = Hedge<S>;

    fn layer(&self, service: S) -> Self::Service {
        Hedge::new(service, self.config.clone())
    }
}
