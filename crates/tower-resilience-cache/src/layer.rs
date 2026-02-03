use crate::{Cache, CacheConfig};
use std::hash::Hash;
use std::sync::Arc;
use tower::Layer;

/// A Tower [`Layer`] that applies response caching to a service.
///
/// This layer wraps a service with a [`Cache`] middleware that stores
/// successful responses and returns cached values for subsequent requests
/// with the same key.
///
/// # State Isolation
///
/// **Note:** Each call to [`layer()`](Layer::layer) creates a new cache store.
/// If you need multiple services to share the same cache (e.g., when using a
/// `ServiceFactory` that creates per-session services), use
/// [`SharedCacheLayer`](crate::SharedCacheLayer) instead, or call
/// [`.shared()`](CacheLayer::shared) on this layer.
///
/// # Examples
///
/// ```
/// use tower_resilience_cache::CacheLayer;
/// use tower::ServiceBuilder;
/// use std::time::Duration;
///
/// # async fn example() {
/// let cache_layer = CacheLayer::builder()
///     .max_size(100)
///     .ttl(Duration::from_secs(60))
///     .key_extractor(|req: &String| req.clone())
///     .build();
///
/// let service = ServiceBuilder::new()
///     .layer(cache_layer)
///     .service(my_service());
/// # }
/// # fn my_service() -> impl tower::Service<String, Response = String, Error = std::io::Error> {
/// #     tower::service_fn(|req: String| async move { Ok::<_, std::io::Error>(req) })
/// # }
/// ```
#[derive(Clone)]
pub struct CacheLayer<Req, K> {
    config: Arc<CacheConfig<Req, K>>,
}

impl<Req, K> CacheLayer<Req, K>
where
    K: Hash + Eq + Clone + Send + 'static,
{
    /// Creates a new `CacheLayer` with the given configuration.
    pub fn new(config: CacheConfig<Req, K>) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    /// Creates a new builder for configuring a cache layer.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_cache::CacheLayer;
    /// use std::time::Duration;
    ///
    /// let layer = CacheLayer::builder()
    ///     .max_size(100)
    ///     .ttl(Duration::from_secs(60))
    ///     .key_extractor(|req: &String| req.clone())
    ///     .build();
    /// ```
    pub fn builder() -> crate::CacheConfigBuilder<Req, K> {
        crate::CacheConfigBuilder::new()
    }

    /// Converts this cache layer into a [`SharedCacheLayer`](crate::SharedCacheLayer) that shares
    /// the cache store across all services created via [`layer()`](Layer::layer).
    ///
    /// This is useful when multiple service instances need to share the same cache,
    /// such as when services are created per-session or per-request.
    ///
    /// # Type Parameters
    ///
    /// - `Resp`: The response type that will be cached. This must match the
    ///   `Response` type of any service this layer is applied to.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_cache::CacheLayer;
    /// use tower::ServiceBuilder;
    /// use std::time::Duration;
    ///
    /// # async fn example() {
    /// let shared_cache = CacheLayer::builder()
    ///     .max_size(100)
    ///     .ttl(Duration::from_secs(60))
    ///     .key_extractor(|req: &String| req.clone())
    ///     .build()
    ///     .shared::<String>();  // Specify the response type
    ///
    /// // Both services share the same cache
    /// let service1 = ServiceBuilder::new()
    ///     .layer(shared_cache.clone())
    ///     .service(my_service());
    ///
    /// let service2 = ServiceBuilder::new()
    ///     .layer(shared_cache)
    ///     .service(my_service());
    /// # }
    /// # fn my_service() -> impl tower::Service<String, Response = String, Error = std::io::Error> {
    /// #     tower::service_fn(|req: String| async move { Ok::<_, std::io::Error>(req) })
    /// # }
    /// ```
    pub fn shared<Resp>(self) -> crate::shared_layer::SharedCacheLayer<Req, K, Resp>
    where
        Resp: Clone + Send + 'static,
    {
        crate::shared_layer::SharedCacheLayer::from_config(self.config)
    }
}

impl<S, Req, K> Layer<S> for CacheLayer<Req, K>
where
    K: Hash + Eq + Clone + Send + 'static,
    S: tower::Service<Req>,
    S::Response: Clone + Send + 'static,
{
    type Service = Cache<S, Req, K, S::Response>;

    fn layer(&self, service: S) -> Self::Service {
        Cache::new(service, Arc::clone(&self.config))
    }
}
