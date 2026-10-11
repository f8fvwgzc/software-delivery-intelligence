//! Bounded ETag cache for GitHub list endpoints.
//!
//! GitHub answers `If-None-Match` with `304 Not Modified` and that answer
//! does not count against the rate limit, so refreshing pipeline runs,
//! deployments, pull requests and the repository list every few seconds is
//! free when nothing changed. The cache is per process and small: 256
//! entries at a few hundred KB each is the worst case, and the oldest entry
//! is evicted first because the UI revisits the same few projects.

use std::collections::{HashMap, VecDeque};

pub const CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub struct Entry {
    pub etag: String,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub struct EtagCache {
    capacity: usize,
    entries: HashMap<String, Entry>,
    /// Insertion order; the front is evicted first.
    order: VecDeque<String>,
}

impl Default for EtagCache {
    fn default() -> Self {
        Self::new(CAPACITY)
    }
}

impl EtagCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, url: &str) -> Option<&Entry> {
        self.entries.get(url)
    }

    pub fn etag(&self, url: &str) -> Option<String> {
        self.entries.get(url).map(|e| e.etag.clone())
    }

    /// Stores a fresh body. Re-inserting a known url refreshes it in place
    /// without consuming another slot.
    pub fn insert(&mut self, url: &str, etag: &str, body: Vec<u8>) {
        if etag.is_empty() {
            return;
        }
        if let Some(entry) = self.entries.get_mut(url) {
            // A refresh keeps its slot; only a new url can push something out.
            entry.etag = etag.to_owned();
            entry.body = body;
            self.order.retain(|u| u != url);
            self.order.push_back(url.to_owned());
            return;
        }
        while self.entries.len() >= self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.order.push_back(url.to_owned());
        self.entries.insert(
            url.to_owned(),
            Entry {
                etag: etag.to_owned(),
                body,
            },
        );
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_oldest_when_full_and_refreshes_in_place() {
        let mut cache = EtagCache::new(2);
        cache.insert("a", "\"1\"", b"A".to_vec());
        cache.insert("b", "\"2\"", b"B".to_vec());
        assert_eq!(cache.len(), 2);

        // Refreshing `a` must not evict anything.
        cache.insert("a", "\"1b\"", b"A2".to_vec());
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.etag("a").as_deref(), Some("\"1b\""));
        assert_eq!(cache.get("a").unwrap().body, b"A2");

        // `b` is now the oldest and goes first.
        cache.insert("c", "\"3\"", b"C".to_vec());
        assert_eq!(cache.len(), 2);
        assert!(cache.get("b").is_none());
        assert!(cache.get("a").is_some());
        assert!(cache.get("c").is_some());
    }

    #[test]
    fn empty_etags_are_not_cached_and_clear_empties() {
        let mut cache = EtagCache::default();
        cache.insert("a", "", b"A".to_vec());
        assert!(cache.is_empty());
        cache.insert("a", "\"1\"", b"A".to_vec());
        assert_eq!(cache.len(), 1);
        cache.clear();
        assert!(cache.is_empty());
        assert!(cache.etag("a").is_none());
    }

    #[test]
    fn capacity_is_bounded_at_the_default() {
        let mut cache = EtagCache::default();
        for i in 0..(CAPACITY * 2) {
            cache.insert(&format!("u{i}"), "\"e\"", Vec::new());
        }
        assert_eq!(cache.len(), CAPACITY);
        assert!(cache.get("u0").is_none());
        assert!(cache.get(&format!("u{}", CAPACITY * 2 - 1)).is_some());
    }
}
