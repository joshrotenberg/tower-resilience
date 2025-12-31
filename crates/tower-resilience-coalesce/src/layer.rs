//! Layer implementation for request coalescing.

use crate::{CoalesceConfig, CoalesceService};
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;
use tower_layer::Layer;

/// A Tower layer that coalesces concurrent identical requests.
///
/// This layer wraps a service and ensures that concurrent requests with the
/// same key are coalesced into a single execution. All callers receive a
/// clone of the result.
///
/// # Example
///
/// ```rust
/// use tower_resilience_coalesce::CoalesceLayer;
/// use tower::ServiceBuilder;
///
/// # #[derive(Clone, Hash, Eq, PartialEq)]
/// # struct Request { id: String }
/// # #[derive(Debug, Clone)]
/// # struct MyError;
/// # async fn example() {
/// # let backend = tower::service_fn(|_req: Request| async { Ok::<_, MyError>(()) });
/// let layer = CoalesceLayer::new(|req: &Request| req.id.clone());
///
/// let service = ServiceBuilder::new()
///     .layer(layer)
///     .service(backend);
/// # }
/// ```
pub struct CoalesceLayer<K, Req, F> {
    config: Arc<CoalesceConfig<K, F>>,
    _req: PhantomData<Req>,
}

impl<K, Req, F> CoalesceLayer<K, Req, F>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    F: Fn(&Req) -> K + Clone + Send + Sync + 'static,
{
    /// Create a new coalesce layer with the given key extractor.
    ///
    /// The key extractor function is called for each request to determine
    /// its coalescing key. Requests with the same key will be coalesced.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_coalesce::CoalesceLayer;
    ///
    /// // Coalesce by request ID
    /// # #[derive(Clone)]
    /// # struct Request { id: u64 }
    /// let layer = CoalesceLayer::new(|req: &Request| req.id);
    ///
    /// // Coalesce by string key
    /// let layer = CoalesceLayer::new(|req: &String| req.clone());
    /// ```
    pub fn new(key_extractor: F) -> Self {
        Self {
            config: Arc::new(CoalesceConfig::new(key_extractor)),
            _req: PhantomData,
        }
    }

    /// Create a new coalesce layer with a configuration.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_coalesce::{CoalesceLayer, CoalesceConfig};
    ///
    /// let config = CoalesceConfig::builder(|req: &String| req.clone())
    ///     .name("my-coalesce")
    ///     .build();
    ///
    /// let layer = CoalesceLayer::<String, String, _>::with_config(config);
    /// ```
    pub fn with_config(config: CoalesceConfig<K, F>) -> Self {
        Self {
            config: Arc::new(config),
            _req: PhantomData,
        }
    }

    /// Create a builder for more configuration options.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tower_resilience_coalesce::CoalesceLayer;
    ///
    /// let layer = CoalesceLayer::builder(|req: &String| req.clone())
    ///     .name("user-lookup")
    ///     .build();
    /// ```
    pub fn builder(key_extractor: F) -> CoalesceLayerBuilder<K, Req, F> {
        CoalesceLayerBuilder::new(key_extractor)
    }
}

impl<K, Req, F> Clone for CoalesceLayer<K, Req, F> {
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            _req: PhantomData,
        }
    }
}

impl<S, K, Req, F> Layer<S> for CoalesceLayer<K, Req, F>
where
    S: tower_service::Service<Req>,
    S::Response: Clone,
    S::Error: Clone,
    K: Hash + Eq + Clone + Send + Sync + 'static,
    F: Fn(&Req) -> K + Clone + Send + Sync + 'static,
{
    type Service = CoalesceService<S, K, Req, F>;

    fn layer(&self, service: S) -> Self::Service {
        CoalesceService::new(service, Arc::clone(&self.config))
    }
}

/// Builder for CoalesceLayer.
pub struct CoalesceLayerBuilder<K, Req, F> {
    key_extractor: F,
    name: Option<String>,
    _key: PhantomData<K>,
    _req: PhantomData<Req>,
}

impl<K, Req, F> CoalesceLayerBuilder<K, Req, F>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    F: Fn(&Req) -> K + Clone + Send + Sync + 'static,
{
    /// Create a new builder with the given key extractor.
    pub fn new(key_extractor: F) -> Self {
        Self {
            key_extractor,
            name: None,
            _key: PhantomData,
            _req: PhantomData,
        }
    }

    /// Set a name for this coalesce instance.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Build the layer.
    pub fn build(self) -> CoalesceLayer<K, Req, F> {
        let mut config_builder = CoalesceConfig::builder(self.key_extractor);
        if let Some(name) = self.name {
            config_builder = config_builder.name(name);
        }
        CoalesceLayer::with_config(config_builder.build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_creation() {
        let layer = CoalesceLayer::new(|req: &String| req.clone());
        let _ = layer.clone();
    }

    #[test]
    fn test_layer_builder() {
        let layer = CoalesceLayer::builder(|req: &String| req.clone())
            .name("test")
            .build();
        let _ = layer.clone();
    }
}
