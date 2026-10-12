//! Bounded ETag cache for GitHub list endpoints.
//!
//! GitHub answers `If-None-Match` with `304 Not Modified` and that answer
//! does not count against the rate limit, so refreshing pipeline runs,
//! deployments, pull requests and the repository list every few seconds is
//! free when nothing changed. The cache is per process and small: 256
//! entries at a few hundred KB each is the worst case, and the oldest entry
//! is evicted first because the UI revisits the same few projects. An entry
//! also expires after [`TTL`]: a list nobody asked about for an hour is
//! not worth the memory, and a body that old is re-fetched rather than
//! trusted against a stale ETag.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

pub const CAPACITY: usize = 256;
/// How long an entry may sit unused before it is a miss.
pub const TTL: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone)]
pub struct Entry {
    pub etag: String,
    pub body: Vec<u8>,
    /// When the entry was stored or last refreshed.
    pub at: Instant,
}

#[derive(Debug)]
pub struct EtagCache {
    capacity: usize,
    ttl: Duration,
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
        Self::with_ttl(capacity, TTL)
    }

    pub fn with_ttl(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity: capacity.max(1),
            ttl,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn live(&self, url: &str) -> bool {
        self.entries
            .get(url)
            .is_some_and(|entry| entry.at.elapsed() < self.ttl)
    }

    /// Forgets `url` when its entry expired.
    fn expire(&mut self, url: &str) {
        if self.entries.contains_key(url) && !self.live(url) {
            self.entries.remove(url);
            self.order.retain(|u| u != url);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&mut self, url: &str) -> Option<&Entry> {
        self.expire(url);
        self.entries.get(url)
    }

    pub fn etag(&mut self, url: &str) -> Option<String> {
        self.expire(url);
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
            entry.at = Instant::now();
            self.order.retain(|u| u != url);
            self.order.push_back(url.to_owned());
            return;
        }
        // Expired entries go before anything live is pushed out.
        if self.entries.len() >= self.capacity {
            let ttl = self.ttl;
            self.entries.retain(|_, entry| entry.at.elapsed() < ttl);
            let entries = &self.entries;
            self.order.retain(|u| entries.contains_key(u));
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
                at: Instant::now(),
            },
        );
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// Drops every entry whose key starts with `prefix` (one workspace's
    /// slice of the cache). Returns how many went.
    pub fn remove_prefix(&mut self, prefix: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|url, _| !url.starts_with(prefix));
        self.order.retain(|url| !url.starts_with(prefix));
        before - self.entries.len()
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
    fn remove_prefix_drops_one_slice_only() {
        let mut cache = EtagCache::default();
        cache.insert("ws1|/a", "\"1\"", Vec::new());
        cache.insert("ws1|/b", "\"2\"", Vec::new());
        cache.insert("ws2|/a", "\"3\"", Vec::new());
        assert_eq!(cache.remove_prefix("ws1|"), 2);
        assert_eq!(cache.len(), 1);
        assert!(cache.get("ws2|/a").is_some());
        assert_eq!(cache.remove_prefix("ws1|"), 0);
    }

    #[test]
    fn entries_expire_after_the_ttl() {
        let mut cache = EtagCache::with_ttl(2, Duration::from_millis(20));
        cache.insert("a", "\"1\"", b"A".to_vec());
        assert!(cache.etag("a").is_some());
        std::thread::sleep(Duration::from_millis(30));
        assert!(cache.etag("a").is_none(), "an old entry is a miss");
        assert!(cache.is_empty(), "and is dropped on the way");

        // When full, expired entries make room before live ones are evicted.
        cache.insert("old", "\"1\"", Vec::new());
        std::thread::sleep(Duration::from_millis(30));
        cache.insert("live", "\"2\"", Vec::new());
        cache.insert("new", "\"3\"", Vec::new());
        assert!(cache.get("live").is_some());
        assert!(cache.get("new").is_some());
        assert!(cache.get("old").is_none());
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
