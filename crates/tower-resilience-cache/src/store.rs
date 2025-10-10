//! Cache storage implementation.

use crate::eviction::{EvictionPolicy, EvictionStore, FifoStore, LfuStore, LruStore};
use std::hash::Hash;
use std::time::{Duration, Instant};

/// Entry in the cache with TTL tracking.
#[derive(Clone, Debug)]
struct CacheEntry<V> {
    value: V,
    inserted_at: Instant,
}

impl<V> CacheEntry<V> {
    fn new(value: V) -> Self {
        Self {
            value,
            inserted_at: Instant::now(),
        }
    }

    fn is_expired(&self, ttl: Option<Duration>) -> bool {
        if let Some(ttl) = ttl {
            self.inserted_at.elapsed() > ttl
        } else {
            false
        }
    }
}

/// Cache store with configurable eviction policy and TTL support.
pub(crate) struct CacheStore<K, V> {
    store: Box<dyn EvictionStore<K, CacheEntry<V>>>,
    ttl: Option<Duration>,
}

impl<K: Hash + Eq + Clone + Send + 'static, V: Clone + Send + 'static> CacheStore<K, V> {
    /// Creates a new cache store with the given capacity, TTL, and eviction policy.
    pub(crate) fn new(capacity: usize, ttl: Option<Duration>, policy: EvictionPolicy) -> Self {
        let store: Box<dyn EvictionStore<K, CacheEntry<V>>> = match policy {
            EvictionPolicy::Lru => Box::new(LruStore::new(capacity)),
            EvictionPolicy::Lfu => Box::new(LfuStore::new(capacity)),
            EvictionPolicy::Fifo => Box::new(FifoStore::new(capacity)),
        };

        Self { store, ttl }
    }

    /// Gets a value from the cache if it exists and is not expired.
    pub(crate) fn get(&mut self, key: &K) -> Option<V> {
        let entry = self.store.get(key)?;

        if entry.is_expired(self.ttl) {
            // Entry expired, remove it
            self.store.remove(key);
            None
        } else {
            Some(entry.value.clone())
        }
    }

    /// Inserts a value into the cache.
    /// Returns the evicted entry if the cache was full.
    pub(crate) fn insert(&mut self, key: K, value: V) -> Option<V> {
        let entry = CacheEntry::new(value);
        self.store.insert(key, entry).map(|(_, e)| e.value)
    }

    /// Returns the current number of entries in the cache.
    pub(crate) fn len(&self) -> usize {
        self.store.len()
    }

    /// Clears all entries from the cache.
    #[allow(dead_code)]
    pub(crate) fn clear(&mut self) {
        self.store.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_cache_store_basic() {
        let mut store = CacheStore::new(2, None, EvictionPolicy::Lru);

        // Insert and retrieve
        store.insert("key1", "value1");
        assert_eq!(store.get(&"key1"), Some("value1"));
        assert_eq!(store.len(), 1);

        // Missing key
        assert_eq!(store.get(&"key2"), None);
    }

    #[test]
    fn test_cache_store_lru_eviction() {
        let mut store = CacheStore::new(2, None, EvictionPolicy::Lru);

        store.insert("key1", "value1");
        store.insert("key2", "value2");

        // This should evict key1
        let evicted = store.insert("key3", "value3");
        assert_eq!(evicted, Some("value1"));

        assert_eq!(store.get(&"key1"), None);
        assert_eq!(store.get(&"key2"), Some("value2"));
        assert_eq!(store.get(&"key3"), Some("value3"));
    }

    #[test]
    fn test_cache_store_ttl_expiration() {
        let mut store = CacheStore::new(10, Some(Duration::from_millis(50)), EvictionPolicy::Lru);

        store.insert("key1", "value1");
        assert_eq!(store.get(&"key1"), Some("value1"));

        // Wait for expiration
        sleep(Duration::from_millis(60));

        // Should be expired
        assert_eq!(store.get(&"key1"), None);
    }

    #[test]
    fn test_cache_store_clear() {
        let mut store = CacheStore::new(10, None, EvictionPolicy::Lru);

        store.insert("key1", "value1");
        store.insert("key2", "value2");
        assert_eq!(store.len(), 2);

        store.clear();
        assert_eq!(store.len(), 0);
        assert_eq!(store.get(&"key1"), None);
    }
}
