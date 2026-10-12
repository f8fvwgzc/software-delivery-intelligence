//! Per-workspace data controls (CONTRACT.md §19), read from identity and
//! cached for a minute. They decide what the service may keep: whether file
//! contents are cached (and for how long) and whether semantic chunks hold
//! whole files or short excerpts.

use std::{sync::Mutex, time::Duration};

use chrono::{DateTime, Utc};
use sdi_common::cache::TtlCache;
use sdi_proto::identity::v1::DataControls;
use uuid::Uuid;

use crate::{auth::Caller, db::chunks::minimal_excerpt, identity::Identity};

/// How long an answer from identity is reused before it is asked again.
pub const CACHE_TTL: Duration = Duration::from_secs(60);
/// Workspaces whose controls are held at once (a minute's worth of active
/// tenants; the rest ask identity again).
pub const CACHE_CAPACITY: usize = 4096;
/// `code_cache = session` without a TTL from identity.
pub const DEFAULT_TTL_HOURS: u32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeCache {
    /// File contents are never written to the database.
    None,
    /// Cached with an expiry; swept in the background, purged on disconnect.
    Session,
    /// Cached until the next scan replaces it or a purge.
    Persistent,
}

impl CodeCache {
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim().to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "session" => Self::Session,
            "persistent" => Self::Persistent,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Controls {
    pub code_cache: CodeCache,
    pub code_cache_ttl: chrono::Duration,
    pub minimal_chunks: bool,
}

impl Default for Controls {
    /// What applies when identity has not said otherwise: the contract's
    /// defaults (`session`, 24 h, whole chunks).
    fn default() -> Self {
        Self {
            code_cache: CodeCache::Session,
            code_cache_ttl: chrono::Duration::hours(i64::from(DEFAULT_TTL_HOURS)),
            minimal_chunks: false,
        }
    }
}

impl Controls {
    pub fn from_proto(controls: &DataControls) -> Self {
        let defaults = Self::default();
        let hours = if controls.code_cache_ttl_hours == 0 {
            DEFAULT_TTL_HOURS
        } else {
            controls.code_cache_ttl_hours
        };
        Self {
            code_cache: CodeCache::parse(&controls.code_cache).unwrap_or(defaults.code_cache),
            code_cache_ttl: chrono::Duration::hours(i64::from(hours)),
            minimal_chunks: controls.minimal_chunks,
        }
    }

    /// Whether a fetched file may be written to `repo_files` at all.
    pub fn stores_files(&self) -> bool {
        self.code_cache != CodeCache::None
    }

    /// The `expires_at` a file row stored now gets: a deadline in session
    /// mode, none in persistent mode. Meaningless (and `None`) for `none`,
    /// where nothing is stored.
    pub fn file_expiry(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self.code_cache {
            CodeCache::Session => Some(now + self.code_cache_ttl),
            CodeCache::None | CodeCache::Persistent => None,
        }
    }

    /// What a chunk may hold: the whole text, or an excerpt when the
    /// workspace keeps chunks minimal. Applied before embedding.
    pub fn chunk_content(&self, content: String) -> String {
        if self.minimal_chunks {
            minimal_excerpt(&content).to_owned()
        } else {
            content
        }
    }
}

/// Controls per workspace with a one-minute cache. Never fails: when
/// identity cannot answer, the defaults apply (and are cached too, so an
/// outage costs one attempt a minute per workspace, not one per file).
pub struct ControlsProvider {
    identity: Identity,
    cache: Mutex<TtlCache<Uuid, Controls>>,
}

impl ControlsProvider {
    pub fn new(identity: Identity) -> Self {
        Self {
            identity,
            cache: Mutex::new(TtlCache::new(CACHE_CAPACITY, CACHE_TTL)),
        }
    }

    /// Entries held right now (diagnostics, tests).
    pub fn cached_workspaces(&self) -> usize {
        self.lock().len()
    }

    pub async fn get(&self, caller: &Caller) -> Controls {
        if let Some(cached) = self.cached(caller.workspace_id) {
            return cached;
        }
        let controls = match self.identity.data_controls(caller).await {
            Ok(proto) => Controls::from_proto(&proto),
            Err(status) => {
                tracing::warn!(
                    workspace = %caller.workspace_id,
                    code = ?status.code(),
                    message = status.message(),
                    "data controls unavailable, applying defaults"
                );
                Controls::default()
            }
        };
        self.store(caller.workspace_id, controls.clone());
        controls
    }

    /// Forgets what is cached for the workspace (after a purge, or when a
    /// test wants the next read to ask identity again).
    pub fn invalidate(&self, workspace_id: Uuid) {
        self.lock().remove(&workspace_id);
    }

    fn cached(&self, workspace_id: Uuid) -> Option<Controls> {
        self.lock().get(&workspace_id).cloned()
    }

    fn store(&self, workspace_id: Uuid, controls: Controls) {
        self.lock().insert(workspace_id, controls);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TtlCache<Uuid, Controls>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proto(code_cache: &str, ttl: u32, minimal: bool) -> DataControls {
        DataControls {
            code_cache: code_cache.to_owned(),
            code_cache_ttl_hours: ttl,
            minimal_chunks: minimal,
            ..Default::default()
        }
    }

    #[test]
    fn modes_parse_and_fall_back_to_the_defaults() {
        assert_eq!(CodeCache::parse(" Session "), Some(CodeCache::Session));
        assert_eq!(CodeCache::parse("none"), Some(CodeCache::None));
        assert_eq!(CodeCache::parse("persistent"), Some(CodeCache::Persistent));
        assert_eq!(CodeCache::parse("forever"), None);

        let unknown = Controls::from_proto(&proto("", 0, false));
        assert_eq!(unknown, Controls::default());
        assert_eq!(unknown.code_cache, CodeCache::Session);
        assert_eq!(unknown.code_cache_ttl, chrono::Duration::hours(24));

        let custom = Controls::from_proto(&proto("persistent", 6, true));
        assert_eq!(custom.code_cache, CodeCache::Persistent);
        assert_eq!(custom.code_cache_ttl, chrono::Duration::hours(6));
        assert!(custom.minimal_chunks);
    }

    #[test]
    fn cache_modes_decide_storage_and_expiry() {
        let now = Utc::now();

        let none = Controls::from_proto(&proto("none", 0, false));
        assert!(!none.stores_files());
        assert_eq!(none.file_expiry(now), None);

        let session = Controls::from_proto(&proto("session", 2, false));
        assert!(session.stores_files());
        assert_eq!(
            session.file_expiry(now),
            Some(now + chrono::Duration::hours(2))
        );

        let persistent = Controls::from_proto(&proto("persistent", 2, false));
        assert!(persistent.stores_files());
        assert_eq!(persistent.file_expiry(now), None);
    }

    #[test]
    fn minimal_chunks_truncate_before_storing() {
        let long = "x".repeat(1_000);
        let whole = Controls::from_proto(&proto("session", 0, false));
        assert_eq!(whole.chunk_content(long.clone()).len(), 1_000);
        let minimal = Controls::from_proto(&proto("session", 0, true));
        assert_eq!(
            minimal.chunk_content(long).len(),
            crate::db::chunks::MINIMAL_CHUNK_CHARS
        );
    }

    #[tokio::test]
    async fn defaults_apply_and_are_cached_when_identity_is_absent() {
        let provider = ControlsProvider::new(Identity::disabled());
        let caller = Caller::system(Uuid::now_v7());
        assert_eq!(provider.get(&caller).await, Controls::default());
        assert!(provider.cached(caller.workspace_id).is_some());
        provider.invalidate(caller.workspace_id);
        assert!(provider.cached(caller.workspace_id).is_none());
    }

    #[test]
    fn the_controls_cache_is_bounded() {
        let provider = ControlsProvider::new(Identity::disabled());
        for _ in 0..(CACHE_CAPACITY + 10) {
            provider.store(Uuid::now_v7(), Controls::default());
        }
        assert_eq!(provider.cached_workspaces(), CACHE_CAPACITY);
    }
}
