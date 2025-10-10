//! Cache eviction policies.
//!
//! This module defines different strategies for evicting entries from the cache
//! when it reaches capacity.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::num::NonZeroUsize;

/// Eviction policy for the cache.
///
/// Determines which entry to evict when the cache reaches capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionPolicy {
    /// Least Recently Used - evicts the entry that was accessed longest ago.
    ///
    /// Best for general-purpose caching where recent items are more likely
    /// to be accessed again.
    Lru,

    /// Least Frequently Used - evicts the entry with the lowest access count.
    ///
    /// Best for long-lived caches where consistently popular items should
    /// be retained regardless of recency.
    Lfu,

    /// First In, First Out - evicts the oldest entry regardless of access pattern.
    ///
    /// Best for time-based caching where age matters more than access patterns.
    Fifo,
}

impl Default for EvictionPolicy {
    fn default() -> Self {
        Self::Lru
    }
}

/// Trait for cache storage implementations with different eviction policies.
pub(crate) trait EvictionStore<K, V>: Send {
    /// Gets a value from the cache.
    fn get(&mut self, key: &K) -> Option<&V>;

    /// Inserts a value into the cache.
    /// Returns the evicted entry if the cache was full.
    fn insert(&mut self, key: K, value: V) -> Option<(K, V)>;

    /// Removes a specific key from the cache.
    fn remove(&mut self, key: &K) -> Option<V>;

    /// Returns the current number of entries.
    fn len(&self) -> usize;

    /// Clears all entries.
    fn clear(&mut self);

    /// Returns true if the cache is empty.
    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// LRU (Least Recently Used) cache storage.
pub(crate) struct LruStore<K, V> {
    cache: lru::LruCache<K, V>,
}

impl<K: Hash + Eq, V> LruStore<K, V> {
    pub(crate) fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());
        Self {
            cache: lru::LruCache::new(cap),
        }
    }
}

impl<K: Hash + Eq + Send, V: Send> EvictionStore<K, V> for LruStore<K, V> {
    fn get(&mut self, key: &K) -> Option<&V> {
        self.cache.get(key)
    }

    fn insert(&mut self, key: K, value: V) -> Option<(K, V)> {
        self.cache.push(key, value)
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        self.cache.pop(key)
    }

    fn len(&self) -> usize {
        self.cache.len()
    }

    fn clear(&mut self) {
        self.cache.clear();
    }
}

/// LFU (Least Frequently Used) cache storage.
pub(crate) struct LfuStore<K, V> {
    data: HashMap<K, V>,
    frequencies: HashMap<K, usize>,
    capacity: usize,
}

impl<K: Hash + Eq + Clone, V> LfuStore<K, V> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            data: HashMap::with_capacity(capacity),
            frequencies: HashMap::with_capacity(capacity),
            capacity: capacity.max(1),
        }
    }

    fn find_lfu_key(&self) -> Option<K> {
        self.frequencies
            .iter()
            .min_by_key(|(_, &freq)| freq)
            .map(|(k, _)| k.clone())
    }
}

impl<K: Hash + Eq + Clone + Send, V: Send> EvictionStore<K, V> for LfuStore<K, V> {
    fn get(&mut self, key: &K) -> Option<&V> {
        if self.data.contains_key(key) {
            *self.frequencies.entry(key.clone()).or_insert(0) += 1;
            self.data.get(key)
        } else {
            None
        }
    }

    fn insert(&mut self, key: K, value: V) -> Option<(K, V)> {
        // If key exists, update it
        if self.data.contains_key(&key) {
            let old_value = self.data.insert(key.clone(), value)?;
            *self.frequencies.entry(key.clone()).or_insert(0) += 1;
            return Some((key, old_value));
        }

        // If at capacity, evict LFU item
        let evicted = if self.data.len() >= self.capacity {
            self.find_lfu_key().and_then(|lfu_key| {
                let evicted_value = self.data.remove(&lfu_key)?;
                self.frequencies.remove(&lfu_key);
                Some((lfu_key, evicted_value))
            })
        } else {
            None
        };

        // Insert new item
        self.data.insert(key.clone(), value);
        self.frequencies.insert(key, 1);

        evicted
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        self.frequencies.remove(key);
        self.data.remove(key)
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn clear(&mut self) {
        self.data.clear();
        self.frequencies.clear();
    }
}

/// FIFO (First In, First Out) cache storage.
pub(crate) struct FifoStore<K, V> {
    data: HashMap<K, V>,
    order: VecDeque<K>,
    capacity: usize,
}

impl<K: Hash + Eq + Clone, V> FifoStore<K, V> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            data: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
        }
    }
}

impl<K: Hash + Eq + Clone + Send, V: Send> EvictionStore<K, V> for FifoStore<K, V> {
    fn get(&mut self, key: &K) -> Option<&V> {
        self.data.get(key)
    }

    fn insert(&mut self, key: K, value: V) -> Option<(K, V)> {
        // If key exists, update it without changing order
        if self.data.contains_key(&key) {
            let old_value = self.data.insert(key.clone(), value)?;
            return Some((key, old_value));
        }

        // If at capacity, evict oldest (first) item
        let evicted = if self.data.len() >= self.capacity {
            self.order.pop_front().and_then(|old_key| {
                let evicted_value = self.data.remove(&old_key)?;
                Some((old_key, evicted_value))
            })
        } else {
            None
        };

        // Insert new item
        self.data.insert(key.clone(), value);
        self.order.push_back(key);

        evicted
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        self.order.retain(|k| k != key);
        self.data.remove(key)
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn clear(&mut self) {
        self.data.clear();
        self.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_eviction() {
        let mut store = LruStore::new(2);

        store.insert("a", 1);
        store.insert("b", 2);

        // Access "a" to make it more recent
        assert_eq!(store.get(&"a"), Some(&1));

        // Insert "c", should evict "b" (least recently used)
        let evicted = store.insert("c", 3);
        assert_eq!(evicted, Some(("b", 2)));

        assert_eq!(store.get(&"a"), Some(&1));
        assert_eq!(store.get(&"b"), None);
        assert_eq!(store.get(&"c"), Some(&3));
    }

    #[test]
    fn test_lfu_eviction() {
        let mut store = LfuStore::new(2);

        store.insert("a", 1);
        store.insert("b", 2);

        // Access "a" multiple times
        store.get(&"a");
        store.get(&"a");
        store.get(&"a");

        // Access "b" once
        store.get(&"b");

        // Insert "c", should evict "b" (least frequently used)
        let evicted = store.insert("c", 3);
        assert_eq!(evicted.map(|(k, _)| k), Some("b"));

        assert_eq!(store.get(&"a"), Some(&1));
        assert_eq!(store.get(&"b"), None);
        assert_eq!(store.get(&"c"), Some(&3));
    }

    #[test]
    fn test_fifo_eviction() {
        let mut store = FifoStore::new(2);

        store.insert("a", 1);
        store.insert("b", 2);

        // Access "b" multiple times (shouldn't matter for FIFO)
        store.get(&"b");
        store.get(&"b");

        // Insert "c", should evict "a" (first in)
        let evicted = store.insert("c", 3);
        assert_eq!(evicted, Some(("a", 1)));

        assert_eq!(store.get(&"a"), None);
        assert_eq!(store.get(&"b"), Some(&2));
        assert_eq!(store.get(&"c"), Some(&3));
    }

    #[test]
    fn test_eviction_policy_default() {
        assert_eq!(EvictionPolicy::default(), EvictionPolicy::Lru);
    }
}
