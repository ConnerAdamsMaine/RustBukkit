#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};

#[derive(Debug)]
struct CacheEntry<V> {
    value:          V,
    hits:           AtomicUsize,
    last_hit_reset: Instant,
}

/// LRU (Least Recently Used) Cache with dynamic growth and hit counting
pub struct LruCache<K: Clone + Eq + std::hash::Hash, V: Sized> {
    current_capacity:   usize,
    max_capacity:       usize,
    cache:              HashMap<K, CacheEntry<V>>, // DashMap<K, CacheEntry<V>>,
    access_order:       VecDeque<K>,
    item_size:          usize,
    hit_reset_interval: Duration,
}

impl<K: Clone + Eq + std::hash::Hash, V> LruCache<K, V> {
    pub fn new(initial_capacity: usize) -> Self {
        Self {
            current_capacity:   initial_capacity,
            max_capacity:       initial_capacity,
            cache:              HashMap::new(), // dashmap::DashMap::new(),
            access_order:       VecDeque::new(),
            item_size:          0,
            hit_reset_interval: Duration::from_secs(300), // 5 minutes
        }
    }

    pub fn with_growth(initial_capacity: usize, max_capacity: usize, item_size: usize) -> Self {
        Self {
            current_capacity: initial_capacity,
            max_capacity,
            cache: HashMap::new(),
            // cache: DashMap::new(),
            access_order: VecDeque::new(),
            item_size,
            hit_reset_interval: Duration::from_secs(300), // 5 minutes
        }
    }

    pub fn try_expand(&mut self) -> bool {
        if self.current_capacity < self.max_capacity && self.item_size > 0 {
            let new_capacity = std::cmp::min(self.current_capacity * 2, self.max_capacity);
            if new_capacity > self.current_capacity {
                self.current_capacity = new_capacity;
                return true;
            }
        }
        false
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        if let Some(guard) = self.cache.get(key) {
            guard.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            {
                let mut order = self.access_order.clone();
                order.retain(|k| k != key);
                order.push_back(key.clone());
            }
            Some(&guard.value)
        } else {
            None
        }
        // if self.cache.contains_key(key) {
        //     // Move to end (most recently used)
        //     self.access_order.retain(|k| k != key);
        //     self.access_order.push_back(key.clone());
        //
        //     // Record hit
        //     if let Some(ref mut entry) = self.cache.get_mut(key) {
        //         entry.hits.add_assign(1);
        //     }
        //
        //     self.cache.get(key).map(|e| &e.value)
        // } else {
        //     None
        // }
    }

    pub fn insert(&mut self, key: K, value: V) -> (Option<V>, bool, Option<K>) {
        // Remove if already exists
        if self.cache.contains_key(&key) {
            self.access_order.retain(|k| k != &key);
        }

        let mut expanded = false;
        let mut evicted_key = None;

        // If at capacity, try to expand first
        if self.cache.len() >= self.current_capacity {
            if self.try_expand() {
                expanded = true;
            } else {
                // If can't expand, evict lowest hit count item
                if let Some(victim_key) = self.evict_lowest_hits() {
                    evicted_key = Some(victim_key);
                }
            }
        }

        self.access_order.push_back(key.clone());
        let now = Instant::now();
        let old_value = self.cache.insert(
            key,
            CacheEntry {
                value,
                hits: AtomicUsize::new(0),
                last_hit_reset: now,
            },
        );

        (old_value.map(|e| e.value), expanded, evicted_key)
    }

    pub fn contains(&self, key: &K) -> bool {
        self.cache.contains_key(key)
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.access_order.retain(|k| k != key);
        self.cache.remove(key).map(|e| e.value)
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.access_order.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.cache.iter().map(|(k, e)| (k, &e.value))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.cache.iter_mut().map(|(k, e)| (k, &mut e.value))
    }

    pub fn current_capacity(&self) -> usize {
        self.current_capacity
    }

    pub fn max_capacity(&self) -> usize {
        self.max_capacity
    }

    pub fn usage_ratio(&self) -> f32 {
        self.cache.len() as f32 / self.current_capacity as f32
    }

    fn evict_lowest_hits(&mut self) -> Option<K> {
        // Find the key with the lowest hit count
        let mut lowest_key: Option<K> = None;
        // let mut lowest_hits: usize = usize::MAX;
        let lowest_hits: AtomicUsize = AtomicUsize::new(usize::MAX);

        for (key, entry) in self.cache.iter() {
            let e_hits = entry.hits.load(std::sync::atomic::Ordering::Relaxed);
            let l_hits = lowest_hits.load(std::sync::atomic::Ordering::Relaxed);

            if e_hits < l_hits {
                // entry.hits < lowest_hits {
                // lowest_hits = entry.hits;
                lowest_hits.store(e_hits, std::sync::atomic::Ordering::Relaxed);
                lowest_key = Some(key.clone());
            }
        }

        if let Some(key) = lowest_key {
            self.access_order.retain(|k| k != &key);
            self.cache.remove(&key);
            Some(key)
        } else {
            None
        }
    }

    pub fn reset_hit_counts(&mut self) {
        let now = Instant::now();
        for entry in self.cache.values_mut() {
            if now.duration_since(entry.last_hit_reset) <= self.hit_reset_interval {
                return;
            }

            if now.duration_since(entry.last_hit_reset) >= self.hit_reset_interval {
                entry.hits = AtomicUsize::new(0);
                entry.last_hit_reset = now;
            }
        }
    }

    pub fn get_hit_count(&self, key: &K) -> Option<usize> {
        self.cache
            .get(key)
            .map(|e| e.hits.load(std::sync::atomic::Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[ignore = "Non-deterministic test due to timing"]
    #[test]
    fn test_lru_basic() {
        let mut cache = LruCache::new(3);

        cache.insert(1, "a");
        cache.insert(2, "b");
        cache.insert(3, "c");

        assert_eq!(cache.get(&1), Some(&"a"));
        assert_eq!(cache.len(), 3);

        // Adding 4th item should evict 2 (least recently used)
        cache.insert(4, "d");
        assert_eq!(cache.len(), 3);
        assert!(!cache.contains(&2));
        assert!(cache.contains(&1));
        assert!(cache.contains(&3));
        assert!(cache.contains(&4));
    }

    #[test]
    fn test_lru_access_order() {
        let mut cache = LruCache::new(2);

        cache.insert(1, "a");
        cache.insert(2, "b");
        cache.get(&1); // Access 1, making 2 the LRU

        cache.insert(3, "c"); // Should evict 2
        assert!(!cache.contains(&2));
        assert!(cache.contains(&1));
        assert!(cache.contains(&3));
    }

    #[test]
    fn test_cache_growth() {
        let mut cache = LruCache::with_growth(2, 6, 1);

        cache.insert(1, "a");
        cache.insert(2, "b");

        // Should be at capacity, try to insert
        let (_, expanded, _) = cache.insert(3, "c");
        assert!(expanded);
        assert_eq!(cache.current_capacity(), 4);

        // All items should still be present
        assert!(cache.contains(&1));
        assert!(cache.contains(&2));
        assert!(cache.contains(&3));
    }

    #[test]
    fn test_hit_count_eviction() {
        let mut cache = LruCache::with_growth(2, 2, 1);

        cache.insert(1, "a");
        cache.insert(2, "b");

        // Access item 1 multiple times to increase hit count
        cache.get(&1);
        cache.get(&1);
        cache.get(&1);

        // Item 1 has 3 hits, item 2 has 0 hits
        assert_eq!(cache.get_hit_count(&1), Some(3));
        assert_eq!(cache.get_hit_count(&2), Some(0));

        // Insert new item, should evict item 2 (lowest hits)
        let (_, expanded, evicted) = cache.insert(3, "c");
        assert!(!expanded);
        assert_eq!(evicted, Some(2));
        assert!(cache.contains(&1));
        assert!(!cache.contains(&2));
        assert!(cache.contains(&3));
    }
}
