//! GitHub access per workspace (CONTRACT.md §18, §19).
//!
//! Identity mints the token: a short-lived installation token for an
//! organisation workspace, the user's OAuth token for a personal one, each
//! with the API base of its GitHub host (GHES works through it). Tokens are
//! cached in memory until shortly before they expire and never written
//! anywhere. Dev mode: when identity cannot be reached and `GITHUB_TOKEN`
//! is set, that token is used for every workspace and a warning is logged
//! once.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use chrono::{DateTime, Duration, Utc};
use sdi_common::cache::TtlCache;
use uuid::Uuid;

use super::{GithubError, Result};
use crate::{auth::Caller, identity::Identity};

pub const PUBLIC_API: &str = "https://api.github.com";

/// A token is refreshed this long before identity says it expires, so a
/// request started with it never fails half-way through.
const REFRESH_MARGIN: Duration = Duration::minutes(5);
/// Installation tokens live an hour; a token identity reports without an
/// expiry (OAuth) is re-read after this long anyway.
const DEFAULT_LIFETIME: Duration = Duration::minutes(55);
/// How long the dev-mode fallback is reused before identity is tried again.
const DEV_RETRY: Duration = Duration::minutes(5);
/// Workspaces whose token is held at once. A public deployment serves many
/// more over a day, but only the ones active in the last hour matter; the
/// rest are re-minted by identity when they come back.
pub const CACHE_CAPACITY: usize = 4096;
/// Longest a cached token is kept whatever `expires_at` says (OAuth tokens
/// have none, so this is their bound).
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// What kind of principal the token represents, which decides how
/// repositories are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
    /// A GitHub App installation: repositories come from
    /// `/installation/repositories`.
    Installation,
    /// A user token (OAuth or `GITHUB_TOKEN`): `/user/repos`.
    User,
}

#[derive(Debug, Clone)]
pub struct GithubAccess {
    pub token: String,
    pub api_base_url: String,
    pub kind: AccessKind,
    /// When the token stops working; the cache drops it a margin earlier.
    pub expires_at: DateTime<Utc>,
    /// Login of the org or user the workspace maps to, when identity said.
    pub slug: String,
}

impl GithubAccess {
    /// A user token against github.com, as dev mode and tests use. It is
    /// reused for `DEV_RETRY` (the refresh margin is added back so the
    /// cache keeps it that long) before identity is tried again.
    pub fn dev(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            api_base_url: PUBLIC_API.to_owned(),
            kind: AccessKind::User,
            expires_at: Utc::now() + DEV_RETRY + REFRESH_MARGIN,
            slug: String::new(),
        }
    }

    fn usable(&self, now: DateTime<Utc>) -> bool {
        self.expires_at - REFRESH_MARGIN > now
    }
}

pub struct TokenProvider {
    identity: Identity,
    dev_token: Option<String>,
    cache: Mutex<TtlCache<Uuid, GithubAccess>>,
    warned: AtomicBool,
}

impl TokenProvider {
    pub fn new(identity: Identity, dev_token: Option<String>) -> Self {
        Self {
            identity,
            dev_token: dev_token.filter(|t| !t.trim().is_empty()),
            cache: Mutex::new(TtlCache::new(CACHE_CAPACITY, CACHE_TTL)),
            warned: AtomicBool::new(false),
        }
    }

    /// Tokens held right now (diagnostics, tests).
    pub fn cached_tokens(&self) -> usize {
        self.lock().len()
    }

    pub fn dev_token_configured(&self) -> bool {
        self.dev_token.is_some()
    }

    /// The access for the caller's workspace, from the cache while it is
    /// fresh, otherwise from identity (or the dev fallback).
    pub async fn access(&self, caller: &Caller) -> Result<GithubAccess> {
        let now = Utc::now();
        if let Some(access) = self.cached(caller.workspace_id, now) {
            return Ok(access);
        }
        let access = match self.identity.installation_token(caller).await {
            Ok(token) => {
                let expires_at = token
                    .expires_at
                    .as_ref()
                    .and_then(sdi_common::time::from_proto)
                    .unwrap_or(now + DEFAULT_LIFETIME);
                let api_base_url = if token.api_base_url.trim().is_empty() {
                    PUBLIC_API.to_owned()
                } else {
                    token.api_base_url.trim().trim_end_matches('/').to_owned()
                };
                if token.token.trim().is_empty() {
                    return Err(GithubError::NoToken(
                        "identity returned no GitHub token for this workspace".to_owned(),
                    ));
                }
                let (kind, slug) = self.kind_of(caller).await;
                GithubAccess {
                    token: token.token,
                    api_base_url,
                    kind,
                    expires_at,
                    slug,
                }
            }
            Err(status) if crate::identity::unreachable(&status) => match &self.dev_token {
                Some(token) => {
                    if !self.warned.swap(true, Ordering::Relaxed) {
                        tracing::warn!(
                            code = ?status.code(),
                            "identity service unreachable; using GITHUB_TOKEN for every workspace (dev mode)"
                        );
                    }
                    GithubAccess::dev(token.clone())
                }
                None => {
                    return Err(GithubError::NoToken(format!(
                        "identity service unreachable ({}) and GITHUB_TOKEN is not set",
                        status.message()
                    )));
                }
            },
            Err(status) => {
                return Err(GithubError::NoToken(format!(
                    "no GitHub access for this workspace: {}",
                    status.message()
                )));
            }
        };
        self.store(caller.workspace_id, access.clone());
        Ok(access)
    }

    /// Whether the workspace is an installation, from identity; a personal
    /// workspace when identity cannot say.
    async fn kind_of(&self, caller: &Caller) -> (AccessKind, String) {
        match self.identity.workspace(caller).await {
            Ok(workspace) if workspace.installation_id > 0 => {
                (AccessKind::Installation, workspace.slug)
            }
            Ok(workspace) => (AccessKind::User, workspace.slug),
            Err(status) => {
                tracing::warn!(
                    workspace = %caller.workspace_id,
                    message = status.message(),
                    "workspace kind unknown, treating the token as a user token"
                );
                (AccessKind::User, String::new())
            }
        }
    }

    /// Drops a cached token (GitHub rejected it, or a test wants a refresh).
    pub fn forget(&self, workspace_id: Uuid) {
        self.lock().remove(&workspace_id);
    }

    /// Holds `access` for the workspace as if identity had minted it: it
    /// is served until it stops being usable, then identity is asked as
    /// always. For tests and tools that point a workspace at a GitHub of
    /// their own without running identity.
    pub fn remember(&self, workspace_id: Uuid, access: GithubAccess) {
        self.store(workspace_id, access);
    }

    fn cached(&self, workspace_id: Uuid, now: DateTime<Utc>) -> Option<GithubAccess> {
        let mut cache = self.lock();
        cache
            .get_if(&workspace_id, |access| access.usable(now))
            .cloned()
    }

    fn store(&self, workspace_id: Uuid, access: GithubAccess) {
        self.lock().insert(workspace_id, access);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TtlCache<Uuid, GithubAccess>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing listens on port 1; the lazy channel fails to connect at once.
    fn unreachable_identity() -> Identity {
        Identity::connect("http://127.0.0.1:1").unwrap()
    }

    #[tokio::test]
    async fn falls_back_to_the_dev_token_when_identity_is_unreachable() {
        let provider = TokenProvider::new(unreachable_identity(), Some("ghp_dev".to_owned()));
        let caller = Caller::system(Uuid::now_v7());
        let access = provider.access(&caller).await.unwrap();
        assert_eq!(access.token, "ghp_dev");
        assert_eq!(access.api_base_url, PUBLIC_API);
        assert_eq!(access.kind, AccessKind::User);
        assert!(provider.warned.load(Ordering::Relaxed));
        // The fallback is cached so identity is not retried per request.
        assert!(provider.cached(caller.workspace_id, Utc::now()).is_some());
        provider.forget(caller.workspace_id);
        assert!(provider.cached(caller.workspace_id, Utc::now()).is_none());
    }

    #[tokio::test]
    async fn without_a_dev_token_an_unreachable_identity_is_no_token() {
        let provider = TokenProvider::new(unreachable_identity(), Some("   ".to_owned()));
        assert!(!provider.dev_token_configured());
        let caller = Caller::system(Uuid::now_v7());
        let error = provider.access(&caller).await.unwrap_err();
        assert!(matches!(error, GithubError::NoToken(_)), "{error}");
        assert!(error.to_string().contains("GITHUB_TOKEN"));

        let disabled = TokenProvider::new(Identity::disabled(), None);
        assert!(matches!(
            disabled.access(&caller).await.unwrap_err(),
            GithubError::NoToken(_)
        ));
    }

    #[test]
    fn the_token_cache_never_outgrows_its_capacity() {
        let provider = TokenProvider::new(Identity::disabled(), None);
        for _ in 0..(CACHE_CAPACITY + 100) {
            provider.store(Uuid::now_v7(), GithubAccess::dev("t"));
        }
        assert_eq!(provider.cached_tokens(), CACHE_CAPACITY);
    }

    #[test]
    fn tokens_are_refreshed_before_they_expire() {
        let now = Utc::now();
        let mut access = GithubAccess::dev("t");
        access.expires_at = now + Duration::minutes(10);
        assert!(access.usable(now));
        access.expires_at = now + Duration::minutes(4);
        assert!(!access.usable(now));
    }
}
