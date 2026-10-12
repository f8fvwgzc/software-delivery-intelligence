//! A small bounded cache with a time to live, for the per-process answers
//! every service remembers (authorisations, tokens, data controls, ETags).
//!
//! Two rules keep memory flat on a public multi-tenant deployment: an
//! entry is never served past `ttl`, and the map never holds more than
//! `capacity` entries. When it is full, expired entries go first; if none
//! expired, the entry that was inserted or refreshed longest ago goes.
//! Nothing here is clever enough to need a background task.

use std::{
    borrow::Borrow,
    collections::HashMap,
    hash::Hash,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
struct Slot<V> {
    at: Instant,
    value: V,
}

#[derive(Debug)]
pub struct TtlCache<K, V> {
    capacity: usize,
    ttl: Duration,
    entries: HashMap<K, Slot<V>>,
}

impl<K: Eq + Hash + Clone, V> TtlCache<K, V> {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity: capacity.max(1),
            ttl,
            entries: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// A live entry, or `None` when absent or expired (expired entries are
    /// dropped on the way, so a stale value never lingers once touched).
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let fresh = self
            .entries
            .get(key)
            .is_some_and(|slot| slot.at.elapsed() < self.ttl);
        if !fresh {
            self.entries.remove(key);
            return None;
        }
        self.entries.get(key).map(|slot| &slot.value)
    }

    /// Like [`get`](Self::get) but only keeps an entry while `keep` says so
    /// (a token that expires before the cache's own TTL, for instance).
    pub fn get_if<Q>(&mut self, key: &Q, keep: impl FnOnce(&V) -> bool) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if self.get(key).is_some_and(|value| !keep(value)) {
            self.entries.remove(key);
        }
        self.get(key)
    }

    /// Stores or refreshes an entry, evicting as needed.
    pub fn insert(&mut self, key: K, value: V) {
        if !self.entries.contains_key(&key) && self.entries.len() >= self.capacity {
            self.evict();
        }
        self.entries.insert(
            key,
            Slot {
                at: Instant::now(),
                value,
            },
        );
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.entries.remove(key).map(|slot| slot.value)
    }

    /// Keeps only the entries `keep` accepts.
    pub fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        self.entries.retain(|k, slot| keep(k, &slot.value));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Drops every expired entry, then, if still full, the oldest one.
    fn evict(&mut self) {
        let ttl = self.ttl;
        self.entries.retain(|_, slot| slot.at.elapsed() < ttl);
        if self.entries.len() < self.capacity {
            return;
        }
        let oldest = self
            .entries
            .iter()
            .min_by_key(|(_, slot)| slot.at)
            .map(|(key, _)| key.clone());
        if let Some(key) = oldest {
            self.entries.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_is_never_exceeded_and_the_oldest_goes_first() {
        let mut cache = TtlCache::new(3, Duration::from_secs(60));
        for i in 0..10 {
            cache.insert(i, i * 10);
            assert!(cache.len() <= 3, "len {}", cache.len());
        }
        assert!(cache.get(&0).is_none());
        assert_eq!(cache.get(&9), Some(&90));
        assert_eq!(cache.get(&8), Some(&80));
        assert_eq!(cache.get(&7), Some(&70));
    }

    #[test]
    fn expired_entries_are_misses_and_evicted_before_live_ones() {
        let mut cache = TtlCache::new(2, Duration::from_millis(20));
        cache.insert("a", 1);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(cache.get(&"a"), None, "expired");
        assert!(cache.is_empty(), "an expired read drops the entry");

        cache.insert("b", 2);
        std::thread::sleep(Duration::from_millis(30));
        cache.insert("c", 3);
        // `b` expired, `c` is live: inserting `d` evicts `b`, not `c`.
        cache.insert("d", 4);
        assert_eq!(cache.get(&"c"), Some(&3));
        assert_eq!(cache.get(&"d"), Some(&4));
        assert_eq!(cache.get(&"b"), None);
    }

    #[test]
    fn refreshing_a_key_keeps_its_slot_and_get_if_drops_rejected_values() {
        let mut cache = TtlCache::new(2, Duration::from_secs(60));
        cache.insert("a", 1);
        cache.insert("b", 2);
        cache.insert("a", 10);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&"a"), Some(&10));
        assert_eq!(cache.get_if(&"a", |v| *v > 100), None);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.remove(&"b"), Some(2));
        assert!(cache.is_empty());
        cache.insert("z", 0);
        cache.retain(|_, v| *v > 0);
        assert!(cache.is_empty());
    }
}
