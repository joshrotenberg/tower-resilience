//! Layer implementation for adaptive concurrency limiting.

use crate::{AdaptiveService, Algorithm, ConcurrencyAlgorithm};
use std::sync::Arc;
use tower_layer::Layer;

/// A Tower layer that applies adaptive concurrency limiting.
///
/// This layer dynamically adjusts the number of concurrent requests based
/// on observed latency and error rates, using algorithms like AIMD or Vegas.
///
/// # Example
///
/// ```rust,no_run
/// use tower_resilience_adaptive::{AdaptiveLimiterLayer, Aimd};
/// use std::time::Duration;
///
/// let layer = AdaptiveLimiterLayer::new(
///     Aimd::builder()
///         .initial_limit(10)
///         .latency_threshold(Duration::from_millis(100))
///         .build()
/// );
/// ```
pub struct AdaptiveLimiterLayer<A> {
    algorithm: Arc<A>,
}

impl<A> AdaptiveLimiterLayer<A>
where
    A: ConcurrencyAlgorithm,
{
    /// Create a new adaptive limiter layer with the given algorithm.
    pub fn new(algorithm: A) -> Self {
        Self {
            algorithm: Arc::new(algorithm),
        }
    }

    /// Create a builder for configuring the layer.
    pub fn builder() -> AdaptiveLimiterLayerBuilder {
        AdaptiveLimiterLayerBuilder::new()
    }
}

impl<A> Clone for AdaptiveLimiterLayer<A> {
    fn clone(&self) -> Self {
        Self {
            algorithm: Arc::clone(&self.algorithm),
        }
    }
}

impl<S, A> Layer<S> for AdaptiveLimiterLayer<A>
where
    A: ConcurrencyAlgorithm + 'static,
{
    type Service = AdaptiveService<S, A>;

    fn layer(&self, service: S) -> Self::Service {
        AdaptiveService::new(service, Arc::clone(&self.algorithm))
    }
}

/// Builder for configuring an adaptive limiter layer.
pub struct AdaptiveLimiterLayerBuilder {
    _private: (),
}

impl AdaptiveLimiterLayerBuilder {
    fn new() -> Self {
        Self { _private: () }
    }

    /// Use the AIMD algorithm.
    pub fn aimd(self) -> crate::AimdBuilder {
        crate::Aimd::builder()
    }

    /// Use the Vegas algorithm.
    pub fn vegas(self) -> crate::VegasBuilder {
        crate::Vegas::builder()
    }
}

/// Extension trait for building layers from algorithm builders.
pub trait IntoLayer {
    /// The algorithm type produced.
    type Algorithm: ConcurrencyAlgorithm;

    /// Build the layer.
    fn into_layer(self) -> AdaptiveLimiterLayer<Self::Algorithm>;
}

impl IntoLayer for crate::Aimd {
    type Algorithm = crate::Aimd;

    fn into_layer(self) -> AdaptiveLimiterLayer<Self::Algorithm> {
        AdaptiveLimiterLayer::new(self)
    }
}

impl IntoLayer for crate::Vegas {
    type Algorithm = crate::Vegas;

    fn into_layer(self) -> AdaptiveLimiterLayer<Self::Algorithm> {
        AdaptiveLimiterLayer::new(self)
    }
}

impl IntoLayer for Algorithm {
    type Algorithm = Algorithm;

    fn into_layer(self) -> AdaptiveLimiterLayer<Self::Algorithm> {
        AdaptiveLimiterLayer::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Aimd;
    use std::time::Duration;

    #[test]
    fn test_layer_creation() {
        let algorithm = Aimd::builder()
            .initial_limit(10)
            .latency_threshold(Duration::from_millis(100))
            .build();
        let layer = AdaptiveLimiterLayer::new(algorithm);
        let _ = layer.clone();
    }

    #[test]
    fn test_into_layer() {
        let layer = Aimd::builder().initial_limit(10).build().into_layer();
        let _ = layer.clone();
    }
}
