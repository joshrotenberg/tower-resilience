//! Shared cache layer that maintains a single store across all layer() calls.

use crate::events::CacheEvent;
use crate::eviction::EvictionPolicy;
use crate::store::CacheStore;
use crate::{Cache, CacheConfig, KeyExtractor};
use std::hash::Hash;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::Layer;
use tower_resilience_core::{EventListeners, FnListener};

/// A Tower [`Layer`] that applies response caching with a shared store.
///
/// Unlike [`CacheLayer`](crate::CacheLayer), this layer shares a single cache store
/// across all services created via [`layer()`](Layer::layer). This is useful when
/// multiple service instances (e.g., per-session or per-request services) need to
/// share the same cache.
///
/// # Type Parameters
///
/// - `Req`: The request type
/// - `K`: The cache key type (extracted from requests)
/// - `Resp`: The response type that will be cached
///
/// # Examples
///
/// ```
/// use tower_resilience_cache::SharedCacheLayer;
/// use tower::ServiceBuilder;
/// use std::time::Duration;
///
/// # async fn example() {
/// // Create a shared cache layer
/// let cache_layer: SharedCacheLayer<String, String, String> = SharedCacheLayer::builder()
///     .max_size(100)
///     .ttl(Duration::from_secs(60))
///     .key_extractor(|req: &String| req.clone())
///     .build();
///
/// // Both services share the same cache
/// let service1 = ServiceBuilder::new()
///     .layer(cache_layer.clone())
///     .service(my_service());
///
/// let service2 = ServiceBuilder::new()
///     .layer(cache_layer)
///     .service(my_service());
/// # }
/// # fn my_service() -> impl tower::Service<String, Response = String, Error = std::io::Error> {
/// #     tower::service_fn(|req: String| async move { Ok::<_, std::io::Error>(req) })
/// # }
/// ```
///
/// # Creating from CacheLayer
///
/// You can also convert an existing [`CacheLayer`](crate::CacheLayer) configuration:
///
/// ```
/// use tower_resilience_cache::CacheLayer;
/// use std::time::Duration;
///
/// let shared_cache = CacheLayer::builder()
///     .max_size(100)
///     .ttl(Duration::from_secs(60))
///     .key_extractor(|req: &String| req.clone())
///     .build()
///     .shared::<String>();  // Specify the response type
/// ```
#[derive(Clone)]
pub struct SharedCacheLayer<Req, K, Resp> {
    config: Arc<CacheConfig<Req, K>>,
    store: Arc<Mutex<CacheStore<K, Resp>>>,
}

impl<Req, K, Resp> SharedCacheLayer<Req, K, Resp>
where
    K: Hash + Eq + Clone + Send + 'static,
    Resp: Clone + Send + 'static,
{
    /// Creates a new `SharedCacheLayer` with the given configuration.
    pub fn new(config: CacheConfig<Req, K>) -> Self {
        let store = Arc::new(Mutex::new(CacheStore::new(
            config.max_size,
            config.ttl,
            config.eviction_policy,
        )));
        Self {
            config: Arc::new(config),
            store,
        }
    }

    /// Creates a new `SharedCacheLayer` from an existing config Arc.
    ///
    /// This is used by [`CacheLayer::shared()`](crate::CacheLayer::shared).
    pub(crate) fn from_config(config: Arc<CacheConfig<Req, K>>) -> Self {
        let store = Arc::new(Mutex::new(CacheStore::new(
            config.max_size,
            config.ttl,
            config.eviction_policy,
        )));
        Self { config, store }
    }

    /// Creates a new builder for configuring a shared cache layer.
    ///
    /// # Examples
    ///
    /// ```
    /// use tower_resilience_cache::SharedCacheLayer;
    /// use std::time::Duration;
    ///
    /// let layer: SharedCacheLayer<String, String, String> = SharedCacheLayer::builder()
    ///     .max_size(100)
    ///     .ttl(Duration::from_secs(60))
    ///     .key_extractor(|req: &String| req.clone())
    ///     .build();
    /// ```
    pub fn builder() -> SharedCacheConfigBuilder<Req, K, Resp> {
        SharedCacheConfigBuilder::new()
    }
}

impl<S, Req, K, Resp> Layer<S> for SharedCacheLayer<Req, K, Resp>
where
    K: Hash + Eq + Clone + Send + 'static,
    S: tower::Service<Req, Response = Resp>,
    Resp: Clone + Send + 'static,
{
    type Service = Cache<S, Req, K, Resp>;

    fn layer(&self, service: S) -> Self::Service {
        Cache::with_store(service, Arc::clone(&self.config), Arc::clone(&self.store))
    }
}

/// Builder for configuring and constructing a shared cache layer.
pub struct SharedCacheConfigBuilder<Req, K, Resp> {
    max_size: usize,
    ttl: Option<Duration>,
    eviction_policy: EvictionPolicy,
    key_extractor: Option<KeyExtractor<Req, K>>,
    event_listeners: EventListeners<CacheEvent>,
    name: String,
    _resp: std::marker::PhantomData<Resp>,
}

impl<Req, K, Resp> SharedCacheConfigBuilder<Req, K, Resp>
where
    K: Hash + Eq + Clone + Send + 'static,
    Resp: Clone + Send + 'static,
{
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self {
            max_size: 100,
            ttl: None,
            eviction_policy: EvictionPolicy::default(),
            key_extractor: None,
            event_listeners: EventListeners::new(),
            name: String::from("<unnamed>"),
            _resp: std::marker::PhantomData,
        }
    }

    /// Sets the maximum number of entries in the cache.
    ///
    /// Default: 100
    pub fn max_size(mut self, size: usize) -> Self {
        self.max_size = size;
        self
    }

    /// Sets the time-to-live for cached entries.
    ///
    /// If set, entries will expire after the specified duration.
    /// Default: None (no expiration)
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    /// Sets the eviction policy for the cache.
    ///
    /// Determines which entry to evict when the cache reaches capacity.
    ///
    /// # Options
    ///
    /// - `EvictionPolicy::Lru` - Least Recently Used (default)
    /// - `EvictionPolicy::Lfu` - Least Frequently Used
    /// - `EvictionPolicy::Fifo` - First In, First Out
    ///
    /// Default: `EvictionPolicy::Lru`
    pub fn eviction_policy(mut self, policy: EvictionPolicy) -> Self {
        self.eviction_policy = policy;
        self
    }

    /// Sets the function that extracts a cache key from a request.
    ///
    /// This function must be provided before building.
    pub fn key_extractor<F>(mut self, f: F) -> Self
    where
        F: Fn(&Req) -> K + Send + Sync + 'static,
    {
        self.key_extractor = Some(Arc::new(f));
        self
    }

    /// Sets the name of this cache instance for observability.
    ///
    /// Default: `"<unnamed>"`
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Registers a callback when a cache hit occurs.
    pub fn on_hit<F>(mut self, f: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if matches!(event, CacheEvent::Hit { .. }) {
                f();
            }
        }));
        self
    }

    /// Registers a callback when a cache miss occurs.
    pub fn on_miss<F>(mut self, f: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if matches!(event, CacheEvent::Miss { .. }) {
                f();
            }
        }));
        self
    }

    /// Registers a callback when an entry is evicted from the cache.
    pub fn on_eviction<F>(mut self, f: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.event_listeners.add(FnListener::new(move |event| {
            if matches!(event, CacheEvent::Eviction { .. }) {
                f();
            }
        }));
        self
    }

    /// Builds the shared cache layer.
    ///
    /// # Panics
    ///
    /// Panics if `key_extractor` was not set.
    pub fn build(self) -> SharedCacheLayer<Req, K, Resp> {
        let key_extractor = self
            .key_extractor
            .expect("key_extractor must be set before building");

        let config = CacheConfig {
            max_size: self.max_size,
            ttl: self.ttl,
            eviction_policy: self.eviction_policy,
            key_extractor,
            event_listeners: self.event_listeners,
            name: self.name,
        };

        SharedCacheLayer::new(config)
    }
}

impl<Req, K, Resp> Default for SharedCacheConfigBuilder<Req, K, Resp>
where
    K: Hash + Eq + Clone + Send + 'static,
    Resp: Clone + Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::service_fn;
    use tower::{Service, ServiceExt};

    #[derive(Clone, Hash, Eq, PartialEq)]
    struct TestRequest {
        id: String,
    }

    #[test]
    fn test_shared_builder_defaults() {
        let _layer: SharedCacheLayer<TestRequest, String, String> = SharedCacheLayer::builder()
            .key_extractor(|req: &TestRequest| req.id.clone())
            .build();
    }

    #[test]
    fn test_shared_builder_custom_values() {
        let _layer: SharedCacheLayer<TestRequest, String, String> = SharedCacheLayer::builder()
            .max_size(500)
            .ttl(Duration::from_secs(60))
            .key_extractor(|req: &TestRequest| req.id.clone())
            .name("my-shared-cache")
            .build();
    }

    #[test]
    #[should_panic(expected = "key_extractor must be set")]
    fn test_shared_builder_panics_without_key_extractor() {
        let _config: SharedCacheLayer<TestRequest, String, String> =
            SharedCacheLayer::builder().build();
    }

    #[tokio::test]
    async fn test_shared_cache_across_layer_calls() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc1 = Arc::clone(&call_count);
        let cc2 = Arc::clone(&call_count);

        // Create two separate services
        let service1 = service_fn(move |req: String| {
            let cc = Arc::clone(&cc1);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>(format!("Response: {}", req))
            }
        });

        let service2 = service_fn(move |req: String| {
            let cc = Arc::clone(&cc2);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>(format!("Response: {}", req))
            }
        });

        // Create a shared cache layer
        let shared_layer: SharedCacheLayer<String, String, String> = SharedCacheLayer::builder()
            .max_size(10)
            .key_extractor(|req: &String| req.clone())
            .build();

        // Apply to both services
        let mut wrapped1 = shared_layer.clone().layer(service1);
        let mut wrapped2 = shared_layer.layer(service2);

        // First call on service1 - cache miss
        let response1 = (&mut wrapped1)
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        assert_eq!(response1, "Response: test");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Call on service2 with same key - should be cache HIT (shared store!)
        let response2 = (&mut wrapped2)
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        assert_eq!(response2, "Response: test");
        // Call count should still be 1 because cache was shared
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_non_shared_cache_layer_creates_separate_stores() {
        // This test demonstrates the problem that SharedCacheLayer solves
        use crate::CacheLayer;

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc1 = Arc::clone(&call_count);
        let cc2 = Arc::clone(&call_count);

        let service1 = service_fn(move |req: String| {
            let cc = Arc::clone(&cc1);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>(format!("Response: {}", req))
            }
        });

        let service2 = service_fn(move |req: String| {
            let cc = Arc::clone(&cc2);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>(format!("Response: {}", req))
            }
        });

        // Regular CacheLayer (not shared)
        let layer = CacheLayer::builder()
            .max_size(10)
            .key_extractor(|req: &String| req.clone())
            .build();

        // Apply to both services
        let mut wrapped1 = layer.clone().layer(service1);
        let mut wrapped2 = layer.layer(service2);

        // First call on service1 - cache miss
        (&mut wrapped1)
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Call on service2 with same key - ALSO a cache miss (separate stores!)
        (&mut wrapped2)
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        // Call count is 2 because stores are NOT shared
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }
}
