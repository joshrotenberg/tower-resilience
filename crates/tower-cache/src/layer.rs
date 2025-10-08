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
/// # Examples
///
/// ```
/// use tower_cache::{CacheConfig, CacheLayer};
/// use tower::ServiceBuilder;
/// use std::time::Duration;
///
/// # async fn example() {
/// let cache_layer = CacheConfig::builder()
///     .max_size(100)
///     .ttl(Duration::from_secs(60))
///     .key_extractor(|req: &String| req.clone())
///     .build()
///     .layer();
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

impl<Req, K> CacheLayer<Req, K> {
    /// Creates a new `CacheLayer` with the given configuration.
    pub fn new(config: CacheConfig<Req, K>) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

impl<S, Req, K> Layer<S> for CacheLayer<Req, K>
where
    K: Hash + Eq,
    S: tower::Service<Req>,
    S::Response: Clone,
{
    type Service = Cache<S, Req, K, S::Response>;

    fn layer(&self, service: S) -> Self::Service {
        Cache::new(service, Arc::clone(&self.config))
    }
}
