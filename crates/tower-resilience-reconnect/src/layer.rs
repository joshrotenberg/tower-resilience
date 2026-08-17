use std::sync::Arc;

use tower::{layer::Layer, Service};

use crate::{config::ReconnectConfig, service::ReconnectService, state::ReconnectState};

/// A Tower layer that turns a service factory into a reconnecting service.
///
/// The wrapped value must be a `Service<Target>` whose response is the actual
/// request service. `ReconnectLayer::new` uses the unit target; use
/// [`ReconnectLayer::for_target`] when the factory needs a target value.
#[derive(Clone, Debug)]
pub struct ReconnectLayer<Target = ()> {
    target: Target,
    config: Arc<ReconnectConfig>,
    state: ReconnectState,
}

impl ReconnectLayer<()> {
    /// Creates a layer for a factory invoked with the unit target.
    pub fn new(config: ReconnectConfig) -> Self {
        Self::for_target((), config)
    }

    /// Creates a unit-target layer with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ReconnectConfig::default())
    }
}

impl<Target> ReconnectLayer<Target> {
    /// Creates a layer for a factory invoked with `target`.
    pub fn for_target(target: Target, config: ReconnectConfig) -> Self {
        Self {
            target,
            config: Arc::new(config),
            state: ReconnectState::new(),
        }
    }

    /// Returns the shared reconnection state.
    pub fn state(&self) -> &ReconnectState {
        &self.state
    }
}

impl Default for ReconnectLayer<()> {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl<M, Target> Layer<M> for ReconnectLayer<Target>
where
    M: Service<Target>,
    Target: Clone,
{
    type Service = ReconnectService<M, Target>;

    fn layer(&self, factory: M) -> Self::Service {
        ReconnectService::from_parts(
            factory,
            self.target.clone(),
            Arc::clone(&self.config),
            self.state.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_starts_disconnected() {
        let layer = ReconnectLayer::new(ReconnectConfig::default());
        assert_eq!(
            layer.state().state(),
            crate::state::ConnectionState::Disconnected
        );
        assert_eq!(layer.state().attempts(), 0);
    }

    #[test]
    fn custom_target_is_retained_by_the_layer() {
        let layer = ReconnectLayer::for_target("primary", ReconnectConfig::default());
        assert_eq!(layer.target, "primary");
    }
}
