//! Response caching middleware for Tower services.
//!
//! This crate provides a Tower middleware for caching service responses,
//! reducing load on downstream services by storing and reusing responses
//! for identical requests.
//!
//! # Features
//!
//! - **LRU Eviction**: Least Recently Used eviction policy
//! - **TTL Support**: Optional time-to-live for cache entries
//! - **Event System**: Observability through cache events (Hit, Miss, Eviction)
//! - **Flexible Key Extraction**: User-defined key extraction from requests
//!
//! # Examples
//!
//! ```
//! use tower_resilience_cache::CacheLayer;
//! use tower::ServiceBuilder;
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a cache layer
//! let cache_layer = CacheLayer::builder()
//!     .max_size(100)
//!     .ttl(Duration::from_secs(60))
//!     .key_extractor(|req: &String| req.clone())
//!     .on_hit(|| println!("Cache hit!"))
//!     .on_miss(|| println!("Cache miss!"))
//!     .build();
//!
//! // Apply to a service
//! let service = ServiceBuilder::new()
//!     .layer(cache_layer)
//!     .service(tower::service_fn(|req: String| async move {
//!         Ok::<_, std::io::Error>(format!("Response: {}", req))
//!     }));
//! # Ok(())
//! # }
//! ```

mod config;
mod error;
mod events;
mod layer;
mod store;

pub use config::{CacheConfig, CacheConfigBuilder, KeyExtractor};
pub use error::CacheError;
pub use events::CacheEvent;
pub use layer::CacheLayer;

use futures::future::BoxFuture;
use std::hash::Hash;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Instant;
use store::CacheStore;
use tower::Service;

/// A Tower [`Service`] that caches responses.
///
/// This service wraps an inner service and caches successful responses.
/// When a request comes in, the cache checks if a valid cached response
/// exists. If so, it returns the cached value immediately without calling
/// the inner service.
///
/// Responses must implement `Clone` to be cacheable.
pub struct Cache<S, Req, K, Resp> {
    inner: S,
    config: Arc<CacheConfig<Req, K>>,
    store: Arc<Mutex<CacheStore<K, Resp>>>,
}

impl<S, Req, K, Resp> Cache<S, Req, K, Resp>
where
    K: Hash + Eq,
    Resp: Clone,
{
    /// Creates a new `Cache` wrapping the given service.
    pub fn new(inner: S, config: Arc<CacheConfig<Req, K>>) -> Self {
        let store = Arc::new(Mutex::new(CacheStore::new(config.max_size, config.ttl)));
        Self {
            inner,
            config,
            store,
        }
    }
}

impl<S, Req, K, Resp> Clone for Cache<S, Req, K, Resp>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: Arc::clone(&self.config),
            store: Arc::clone(&self.store),
        }
    }
}

impl<S, Req, K> Service<Req> for Cache<S, Req, K, S::Response>
where
    S: Service<Req>,
    S::Response: Clone + Send + 'static,
    K: Hash + Eq + Clone + Send + 'static,
    Req: Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = CacheError<S::Error>;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(CacheError::Inner)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let key = (self.config.key_extractor)(&req);

        // Check cache first
        let cached = {
            let mut store = self.store.lock().unwrap();
            store.get(&key)
        };

        if let Some(response) = cached {
            // Cache hit
            let event = CacheEvent::Hit {
                pattern_name: self.config.name.clone(),
                timestamp: Instant::now(),
            };
            self.config.event_listeners.emit(&event);
            return Box::pin(async move { Ok(response) });
        }

        // Cache miss
        let miss_event = CacheEvent::Miss {
            pattern_name: self.config.name.clone(),
            timestamp: Instant::now(),
        };
        self.config.event_listeners.emit(&miss_event);

        let future = self.inner.call(req);
        let store = Arc::clone(&self.store);
        let config = Arc::clone(&self.config);

        Box::pin(async move {
            let response = future.await.map_err(CacheError::Inner)?;

            // Store successful response in cache
            let was_evicted = {
                let mut store = store.lock().unwrap();
                let was_full = store.len() >= config.max_size;
                store.insert(key, response.clone());
                was_full
            };

            if was_evicted {
                let event = CacheEvent::Eviction {
                    pattern_name: config.name.clone(),
                    timestamp: Instant::now(),
                };
                config.event_listeners.emit(&event);
            }

            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tower::service_fn;
    use tower::Layer;
    use tower::ServiceExt;

    #[tokio::test]
    async fn cache_hit_returns_cached_response() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>(format!("Response: {}", req))
            }
        });

        let layer = CacheLayer::builder()
            .max_size(10)
            .key_extractor(|req: &String| req.clone())
            .build();

        let mut service = layer.layer(service);

        // First call - cache miss
        let response1 = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        assert_eq!(response1, "Response: test");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Second call - cache hit
        let response2 = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        assert_eq!(response2, "Response: test");
        assert_eq!(call_count.load(Ordering::SeqCst), 1); // Not called again
    }

    #[tokio::test]
    async fn cache_miss_calls_inner_service() {
        let service = service_fn(|req: String| async move {
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        });

        let layer = CacheLayer::builder()
            .max_size(10)
            .key_extractor(|req: &String| req.clone())
            .build();

        let mut service = layer.layer(service);

        let response = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        assert_eq!(response, "Response: test");
    }

    #[tokio::test]
    async fn different_keys_not_cached_together() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>(format!("Response: {}", req))
            }
        });

        let layer = CacheLayer::builder()
            .max_size(10)
            .key_extractor(|req: &String| req.clone())
            .build();

        let mut service = layer.layer(service);

        service
            .ready()
            .await
            .unwrap()
            .call("test1".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("test2".to_string())
            .await
            .unwrap();

        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn ttl_expiration_causes_cache_miss() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>(format!("Response: {}", req))
            }
        });

        let layer = CacheLayer::builder()
            .max_size(10)
            .ttl(Duration::from_millis(50))
            .key_extractor(|req: &String| req.clone())
            .build();

        let mut service = layer.layer(service);

        service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_millis(100)).await;

        service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 2); // Called again
    }

    #[tokio::test]
    async fn lru_eviction_removes_least_recently_used() {
        let service = service_fn(|req: String| async move {
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        });

        let layer = CacheLayer::builder()
            .max_size(2)
            .key_extractor(|req: &String| req.clone())
            .build();

        let mut service = layer.layer(service);

        // Fill cache with 2 items
        service
            .ready()
            .await
            .unwrap()
            .call("key1".to_string())
            .await
            .unwrap();
        service
            .ready()
            .await
            .unwrap()
            .call("key2".to_string())
            .await
            .unwrap();

        // Add third item, should evict key1
        service
            .ready()
            .await
            .unwrap()
            .call("key3".to_string())
            .await
            .unwrap();

        // Verify cache state by checking call counts
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service2 = service_fn(move |req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>(format!("Response: {}", req))
            }
        });

        let mut service2 = layer.layer(service2);

        // key1 should be evicted (cache miss)
        service2
            .ready()
            .await
            .unwrap()
            .call("key1".to_string())
            .await
            .unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn event_listeners_called() {
        let hit_count = Arc::new(AtomicUsize::new(0));
        let miss_count = Arc::new(AtomicUsize::new(0));
        let eviction_count = Arc::new(AtomicUsize::new(0));

        let hc = Arc::clone(&hit_count);
        let mc = Arc::clone(&miss_count);
        let ec = Arc::clone(&eviction_count);

        let service = service_fn(|req: String| async move {
            Ok::<_, std::io::Error>(format!("Response: {}", req))
        });

        let layer = CacheLayer::builder()
            .max_size(1)
            .key_extractor(|req: &String| req.clone())
            .on_hit(move || {
                hc.fetch_add(1, Ordering::SeqCst);
            })
            .on_miss(move || {
                mc.fetch_add(1, Ordering::SeqCst);
            })
            .on_eviction(move || {
                ec.fetch_add(1, Ordering::SeqCst);
            })
            .build();

        let mut service = layer.layer(service);

        // First call - miss
        service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        assert_eq!(miss_count.load(Ordering::SeqCst), 1);
        assert_eq!(hit_count.load(Ordering::SeqCst), 0);

        // Second call - hit
        service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await
            .unwrap();
        assert_eq!(hit_count.load(Ordering::SeqCst), 1);
        assert_eq!(miss_count.load(Ordering::SeqCst), 1);

        // Third call with different key - eviction
        service
            .ready()
            .await
            .unwrap()
            .call("other".to_string())
            .await
            .unwrap();
        assert_eq!(eviction_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn errors_not_cached() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = Arc::clone(&call_count);

        let service = service_fn(move |_req: String| {
            let cc = Arc::clone(&cc);
            async move {
                cc.fetch_add(1, Ordering::SeqCst);
                Err::<String, _>(std::io::Error::other("error"))
            }
        });

        let layer = CacheLayer::builder()
            .max_size(10)
            .key_extractor(|req: &String| req.clone())
            .build();

        let mut service = layer.layer(service);

        // First call - error
        let _ = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Second call - should call inner again (error not cached)
        let _ = service
            .ready()
            .await
            .unwrap()
            .call("test".to_string())
            .await;
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }
}
