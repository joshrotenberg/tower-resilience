use std::sync::Arc;
use tower::layer::Layer;

use crate::{config::ReconnectConfig, service::ReconnectService, state::ReconnectState};

/// A Tower Layer that adds automatic reconnection capabilities to a service.
///
/// This layer wraps services that implement `MakeService`, allowing them to
/// automatically reconnect on connection failures with configurable backoff strategies.
///
/// # Examples
///
/// ```
/// use tower_resilience_reconnect::{ReconnectLayer, ReconnectConfig};
///
/// let layer = ReconnectLayer::new(ReconnectConfig::default());
/// ```
#[derive(Clone, Debug)]
pub struct ReconnectLayer {
    config: Arc<ReconnectConfig>,
    state: ReconnectState,
}

impl ReconnectLayer {
    /// Creates a new `ReconnectLayer` with the given configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_reconnect::{ReconnectLayer, ReconnectConfig};
    ///
    /// let config = ReconnectConfig::builder()
    ///     .max_attempts(5)
    ///     .build();
    ///
    /// let layer = ReconnectLayer::new(config);
    /// ```
    pub fn new(config: ReconnectConfig) -> Self {
        Self {
            config: Arc::new(config),
            state: ReconnectState::new(),
        }
    }

    /// Creates a new `ReconnectLayer` with default configuration.
    ///
    /// Uses exponential backoff from 100ms to 5 seconds with unlimited attempts.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_reconnect::ReconnectLayer;
    ///
    /// let layer = ReconnectLayer::default();
    /// ```
    pub fn with_defaults() -> Self {
        Self::new(ReconnectConfig::default())
    }

    /// Returns a reference to the reconnection state.
    ///
    /// This can be used to monitor the current connection state and statistics.
    pub fn state(&self) -> &ReconnectState {
        &self.state
    }
}

impl Default for ReconnectLayer {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl<S> Layer<S> for ReconnectLayer {
    type Service = ReconnectService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ReconnectService::new(inner, self.config.clone(), self.state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ReconnectPolicy;
    use std::time::Duration;

    #[test]
    fn test_layer_creation() {
        let config = ReconnectConfig::default();
        let layer = ReconnectLayer::new(config);

        // State should start as disconnected until first successful connection
        assert_eq!(
            layer.state().state(),
            crate::state::ConnectionState::Disconnected
        );
        assert_eq!(layer.state().attempts(), 0);
    }

    #[test]
    fn test_layer_default() {
        let layer = ReconnectLayer::default();
        assert_eq!(
            layer.state().state(),
            crate::state::ConnectionState::Disconnected
        );
    }

    #[test]
    fn test_layer_with_defaults() {
        let layer = ReconnectLayer::with_defaults();
        assert_eq!(
            layer.state().state(),
            crate::state::ConnectionState::Disconnected
        );
    }

    #[test]
    fn test_layer_custom_config() {
        let config = ReconnectConfig::builder()
            .policy(ReconnectPolicy::exponential(
                Duration::from_millis(200),
                Duration::from_secs(10),
            ))
            .max_attempts(5)
            .retry_on_reconnect(false)
            .build();

        let layer = ReconnectLayer::new(config);
        assert_eq!(layer.state().attempts(), 0);
    }
}
