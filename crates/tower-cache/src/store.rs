//! Cache storage implementation.

use lru::LruCache;
use std::hash::Hash;
use std::num::NonZeroUsize;
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

/// LRU cache store with TTL support.
pub(crate) struct CacheStore<K, V> {
    cache: LruCache<K, CacheEntry<V>>,
    ttl: Option<Duration>,
}

impl<K: Hash + Eq, V: Clone> CacheStore<K, V> {
    /// Creates a new cache store with the given capacity and TTL.
    pub(crate) fn new(capacity: usize, ttl: Option<Duration>) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());
        Self {
            cache: LruCache::new(cap),
            ttl,
        }
    }

    /// Gets a value from the cache if it exists and is not expired.
    pub(crate) fn get(&mut self, key: &K) -> Option<V> {
        let entry = self.cache.get(key)?;

        if entry.is_expired(self.ttl) {
            // Entry expired, remove it
            self.cache.pop(key);
            None
        } else {
            Some(entry.value.clone())
        }
    }

    /// Inserts a value into the cache.
    /// Returns the evicted entry if the cache was full.
    pub(crate) fn insert(&mut self, key: K, value: V) -> Option<V> {
        let entry = CacheEntry::new(value);
        self.cache.push(key, entry).map(|(_, e)| e.value)
    }

    /// Returns the current number of entries in the cache.
    pub(crate) fn len(&self) -> usize {
        self.cache.len()
    }

    /// Clears all entries from the cache.
    #[allow(dead_code)]
    pub(crate) fn clear(&mut self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_cache_store_basic() {
        let mut store = CacheStore::new(2, None);

        // Insert and retrieve
        store.insert("key1", "value1");
        assert_eq!(store.get(&"key1"), Some("value1"));
        assert_eq!(store.len(), 1);

        // Missing key
        assert_eq!(store.get(&"key2"), None);
    }

    #[test]
    fn test_cache_store_lru_eviction() {
        let mut store = CacheStore::new(2, None);

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
        let mut store = CacheStore::new(10, Some(Duration::from_millis(50)));

        store.insert("key1", "value1");
        assert_eq!(store.get(&"key1"), Some("value1"));

        // Wait for expiration
        sleep(Duration::from_millis(60));

        // Should be expired
        assert_eq!(store.get(&"key1"), None);
    }

    #[test]
    fn test_cache_store_clear() {
        let mut store = CacheStore::new(10, None);

        store.insert("key1", "value1");
        store.insert("key2", "value2");
        assert_eq!(store.len(), 2);

        store.clear();
        assert_eq!(store.len(), 0);
        assert_eq!(store.get(&"key1"), None);
    }
}
