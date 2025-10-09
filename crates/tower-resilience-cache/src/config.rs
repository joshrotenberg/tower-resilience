//! Configuration for cache.

use crate::events::CacheEvent;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;
use tower_resilience_core::{EventListeners, FnListener};

/// Function that extracts a cache key from a request.
pub type KeyExtractor<Req, K> = Arc<dyn Fn(&Req) -> K + Send + Sync>;

/// Configuration for the cache pattern.
pub struct CacheConfig<Req, K> {
    pub(crate) max_size: usize,
    pub(crate) ttl: Option<Duration>,
    pub(crate) key_extractor: KeyExtractor<Req, K>,
    pub(crate) event_listeners: EventListeners<CacheEvent>,
    pub(crate) name: String,
}

impl<Req, K> CacheConfig<Req, K>
where
    K: Hash + Eq + Clone + Send + 'static,
{
    /// Creates a new configuration builder.
    pub fn builder() -> CacheConfigBuilder<Req, K> {
        CacheConfigBuilder::new()
    }
}

/// Builder for configuring and constructing a cache.
pub struct CacheConfigBuilder<Req, K> {
    max_size: usize,
    ttl: Option<Duration>,
    key_extractor: Option<KeyExtractor<Req, K>>,
    event_listeners: EventListeners<CacheEvent>,
    name: String,
}

impl<Req, K> CacheConfigBuilder<Req, K>
where
    K: Hash + Eq + Clone + Send + 'static,
{
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self {
            max_size: 100,
            ttl: None,
            key_extractor: None,
            event_listeners: EventListeners::new(),
            name: String::from("<unnamed>"),
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

    /// Registers a callback to be invoked when a cache hit occurs.
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

    /// Registers a callback to be invoked when a cache miss occurs.
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

    /// Registers a callback to be invoked when an entry is evicted.
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

    /// Builds the cache layer.
    ///
    /// # Panics
    ///
    /// Panics if `key_extractor` was not set.
    pub fn build(self) -> crate::CacheLayer<Req, K> {
        let key_extractor = self
            .key_extractor
            .expect("key_extractor must be set before building");

        let config = CacheConfig {
            max_size: self.max_size,
            ttl: self.ttl,
            key_extractor,
            event_listeners: self.event_listeners,
            name: self.name,
        };

        crate::CacheLayer::new(config)
    }
}

impl<Req, K> Default for CacheConfigBuilder<Req, K>
where
    K: Hash + Eq + Clone + Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Hash, Eq, PartialEq)]
    struct TestRequest {
        id: String,
    }

    #[test]
    fn test_builder_defaults() {
        let _layer = CacheConfig::<TestRequest, String>::builder()
            .key_extractor(|req| req.id.clone())
            .build();
        // If this compiles and doesn't panic, the builder works
    }

    #[test]
    fn test_builder_custom_values() {
        let _layer = CacheConfig::<TestRequest, String>::builder()
            .max_size(500)
            .ttl(Duration::from_secs(60))
            .key_extractor(|req| req.id.clone())
            .name("my-cache")
            .build();
        // If this compiles and doesn't panic, the builder works
    }

    #[test]
    fn test_event_listeners() {
        let _layer = CacheConfig::<TestRequest, String>::builder()
            .key_extractor(|req| req.id.clone())
            .on_hit(|| {})
            .on_miss(|| {})
            .on_eviction(|| {})
            .build();
        // If this compiles and doesn't panic, the event listener registration works
    }

    #[test]
    #[should_panic(expected = "key_extractor must be set")]
    fn test_builder_panics_without_key_extractor() {
        let _config = CacheConfig::<TestRequest, String>::builder().build();
    }
}
