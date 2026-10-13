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
//!
//! A cache may also be given a byte budget ([`EtagCache::with_budget`]):
//! then no single body larger than the per-entry limit is kept, and the
//! oldest entries go until the total fits. The client's second cache, the
//! one holding decoded slices of the pull request calls (CONTRACT.md §23),
//! is bounded that way, because its entries are many and of very different
//! sizes.

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
    /// Largest body one entry may hold; a larger one is not cached.
    max_entry_bytes: usize,
    /// Most body bytes held at once, across every entry.
    max_total_bytes: usize,
    total_bytes: usize,
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
            max_entry_bytes: usize::MAX,
            max_total_bytes: usize::MAX,
            total_bytes: 0,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// A cache bounded by bytes as well as by entries: a body over
    /// `max_entry_bytes` is never kept, and the oldest entries are evicted
    /// until everything held fits `max_total_bytes`.
    pub fn with_budget(capacity: usize, max_entry_bytes: usize, max_total_bytes: usize) -> Self {
        Self {
            max_entry_bytes: max_entry_bytes.min(max_total_bytes),
            max_total_bytes,
            ..Self::new(capacity)
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
            self.forget(url);
        }
    }

    /// Drops one entry and its place in the eviction order.
    fn forget(&mut self, url: &str) {
        if let Some(entry) = self.entries.remove(url) {
            self.total_bytes -= entry.body.len();
            self.order.retain(|u| u != url);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Body bytes held right now.
    pub fn bytes(&self) -> usize {
        self.total_bytes
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
    /// without consuming another slot. A body over the per-entry limit is
    /// not stored, and whatever was held for that url is dropped (its ETag
    /// no longer describes anything this cache can serve).
    pub fn insert(&mut self, url: &str, etag: &str, body: Vec<u8>) {
        if etag.is_empty() {
            return;
        }
        if body.len() > self.max_entry_bytes {
            self.forget(url);
            return;
        }
        // A refresh keeps its slot; only a new url can push something out
        // of the entry limit. Either may push older entries out of the byte
        // budget, so the old body leaves the total first.
        self.forget(url);
        // Expired entries go before anything live is pushed out.
        if self.entries.len() >= self.capacity
            || self.total_bytes + body.len() > self.max_total_bytes
        {
            let ttl = self.ttl;
            let mut freed = 0;
            self.entries.retain(|_, entry| {
                let live = entry.at.elapsed() < ttl;
                if !live {
                    freed += entry.body.len();
                }
                live
            });
            self.total_bytes -= freed;
            let entries = &self.entries;
            self.order.retain(|u| entries.contains_key(u));
        }
        while self.entries.len() >= self.capacity
            || self.total_bytes + body.len() > self.max_total_bytes
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.total_bytes -= entry.body.len();
            }
        }
        self.total_bytes += body.len();
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
        self.total_bytes = 0;
    }

    /// Drops every entry whose key starts with `prefix` (one workspace's
    /// slice of the cache). Returns how many went.
    pub fn remove_prefix(&mut self, prefix: &str) -> usize {
        let before = self.entries.len();
        let mut freed = 0;
        self.entries.retain(|url, entry| {
            let keep = !url.starts_with(prefix);
            if !keep {
                freed += entry.body.len();
            }
            keep
        });
        self.total_bytes -= freed;
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

    #[test]
    fn a_byte_budget_bounds_the_total_and_refuses_oversized_bodies() {
        // Four entries at most, 10 bytes each at most, 25 bytes in all.
        let mut cache = EtagCache::with_budget(4, 10, 25);
        cache.insert("a", "\"1\"", vec![0; 10]);
        cache.insert("b", "\"2\"", vec![0; 10]);
        assert_eq!((cache.len(), cache.bytes()), (2, 20));

        // A third 10-byte body does not fit next to both: the oldest goes.
        cache.insert("c", "\"3\"", vec![0; 10]);
        assert_eq!((cache.len(), cache.bytes()), (2, 20));
        assert!(cache.get("a").is_none());
        assert!(cache.get("b").is_some() && cache.get("c").is_some());

        // Too large for one entry: not kept, and what was held for that
        // url is dropped rather than left to answer a stale ETag.
        cache.insert("c", "\"3b\"", vec![0; 11]);
        assert!(cache.get("c").is_none());
        assert_eq!((cache.len(), cache.bytes()), (1, 10));

        // A refresh replaces the body in the total instead of adding to it.
        cache.insert("b", "\"2b\"", vec![0; 4]);
        assert_eq!((cache.len(), cache.bytes()), (1, 4));
        assert_eq!(cache.etag("b").as_deref(), Some("\"2b\""));

        // Removing a slice and clearing both give the bytes back.
        cache.insert("ws|x", "\"4\"", vec![0; 6]);
        assert_eq!(cache.remove_prefix("ws|"), 1);
        assert_eq!(cache.bytes(), 4);
        cache.clear();
        assert_eq!((cache.len(), cache.bytes()), (0, 0));
    }
}
