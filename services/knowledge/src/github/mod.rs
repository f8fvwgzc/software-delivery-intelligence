//! Read-only GitHub REST client. Only what the scanner and the delivery
//! observer need; every call is a GET and failures are typed so the gRPC
//! layer can answer with the right status.
//!
//! Access is per workspace (CONTRACT.md §18): [`Github`] is the process-wide
//! hub (HTTP client, ETag cache, token provider) and hands out a
//! [`GithubClient`] bound to one workspace's token and API base.

pub mod cache;
pub mod token;
pub mod types;

use std::{
    collections::VecDeque,
    hash::{Hash, Hasher},
    io::{Seek, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{
    Response, StatusCode,
    header::{ACCEPT, ETAG, HeaderMap, HeaderValue, IF_NONE_MATCH},
};
use serde::{Serialize, de::DeserializeOwned};

use cache::EtagCache;
pub use token::{AccessKind, GithubAccess, TokenProvider};
pub use types::*;
use uuid::Uuid;

use crate::auth::Caller;

const USER_AGENT: &str = "sdi-knowledge";
const MAX_ATTEMPTS: u32 = 3;
/// Subtree requests spent completing a tree GitHub truncated (the recursive
/// endpoint stops at 100 000 entries / 7 MB). Each request lists one
/// directory, so this bounds both the quota and the time a scan of a huge
/// monorepo may spend on its tree.
pub const TREE_FALLBACK_MAX_REQUESTS: usize = 400;
/// Longest secondary-rate-limit pause we honour inline; beyond it the caller gets the error.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(60);
/// JSON calls are small; the archive download gets its own, longer budget.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const TARBALL_TIMEOUT: Duration = Duration::from_secs(180);
/// Largest JSON body the pull request and security calls read. A page of a
/// hundred alerts or a comparison with its file list is a few MB at worst;
/// anything larger is refused instead of being buffered.
pub const MAX_JSON_BYTES: usize = 16 << 20;
/// The slim cache (decoded slices, see [`GithubClient::get_slim`]): entries
/// held, the largest one kept, and the bytes held in all. A page of fifty
/// pull requests is some tens of KB once reduced to the fields read here,
/// so the budget covers thousands of polled projects.
pub const SLIM_CAPACITY: usize = 8192;
pub const SLIM_MAX_ENTRY_BYTES: usize = 2 << 20;
pub const SLIM_MAX_TOTAL_BYTES: usize = 64 << 20;
/// Pages of a hundred read of a head commit's check runs, of its commit
/// statuses, and of a pull request's reviews. More than that is reported
/// as [`Listed::more`], never silently cut: a merge is not judged on part
/// of a list.
pub const MAX_STATUS_PAGES: u32 = 5;
/// Open Dependabot alerts read per repository (CONTRACT.md §23.4).
pub const MAX_ALERTS: usize = 300;

#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    #[error("GitHub access is not configured: {0}")]
    NoToken(String),
    #[error("GitHub rejected the token")]
    Unauthorized,
    #[error("GitHub forbade the request: {0}")]
    Forbidden(String),
    #[error("not found on GitHub: {0}")]
    NotFound(String),
    #[error("the repository is empty (no commits yet): {0}")]
    EmptyRepository(String),
    #[error("GitHub rate limit reached, retry after {retry_after}s")]
    RateLimited { retry_after: u64 },
    #[error("GitHub answered {status}: {message}")]
    Api { status: u16, message: String },
    #[error("GitHub request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("could not decode GitHub response: {0}")]
    Decode(String),
}

pub type Result<T> = std::result::Result<T, GithubError>;

/// What [`GithubClient::file_raw`] read: at most the requested bytes, whether
/// the file went on past them, and the size GitHub announced, when it did.
#[derive(Debug, Clone)]
pub struct RawFile {
    pub bytes: Vec<u8>,
    pub truncated: bool,
    pub content_length: Option<u64>,
}

/// A value read through [`GithubClient::get_slim`].
#[derive(Debug, Clone)]
pub struct Slim<T> {
    pub value: T,
    /// GitHub answered `304 Not Modified`: the value is the cached one and
    /// the request cost nothing against the rate limit.
    pub not_modified: bool,
    /// A hash of the fields this client reads. Two answers with the same
    /// fingerprint are the same for every purpose here, however they were
    /// obtained; stable within a process, never stored.
    pub fingerprint: u64,
}

/// What [`GithubClient::pulls_listing`] read.
#[derive(Debug, Clone)]
pub struct PullListing {
    pub pulls: Vec<Pull>,
    /// Every page was a `304`: the poll cost no rate limit.
    pub not_modified: bool,
    /// Identifies this exact listing (see [`Slim::fingerprint`]): a caller
    /// that already applied a listing with this fingerprint has nothing to
    /// write.
    pub fingerprint: u64,
}

/// A list read up to a page cap.
#[derive(Debug, Clone, Default)]
pub struct Listed<T> {
    pub items: Vec<T>,
    /// GitHub has more than were read.
    pub more: bool,
}

/// The severity filters of the two passes of an alert listing, worst first.
const ALERT_SEVERITY_PASSES: [&str; 2] = ["critical,high", "medium,low"];

/// What [`GithubClient::dependabot_alerts`] read.
#[derive(Debug, Clone, Default)]
pub struct AlertListing {
    pub alerts: Vec<Alert>,
    /// GitHub has more open alerts than the limit that was read.
    pub truncated: bool,
}

/// Shared by every client the hub hands out: one HTTP client and two ETag
/// caches (entries are keyed by workspace, so tenants never share a body).
/// `cache` holds GitHub's bodies as they came, for the lists the scanner
/// and the delivery observer read. `slim` holds decoded slices for the
/// calls that are polled per pull request (CONTRACT.md §23); it has its own
/// budget so a burst of status checks can never evict a polled list.
struct Shared {
    http: reqwest::Client,
    cache: Mutex<EtagCache>,
    slim: Mutex<EtagCache>,
}

impl Shared {
    fn cache(&self) -> std::sync::MutexGuard<'_, EtagCache> {
        // A poisoned cache only means a panic mid-insert; the data is still a cache.
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn slim(&self) -> std::sync::MutexGuard<'_, EtagCache> {
        self.slim
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The process-wide hub. Resolves a workspace's token through the
/// [`TokenProvider`] and binds a [`GithubClient`] to it.
pub struct Github {
    shared: Arc<Shared>,
    tokens: TokenProvider,
}

impl Github {
    pub fn new(tokens: TokenProvider) -> anyhow::Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );
        // Redirects are followed (the tarball endpoint answers 302 to
        // codeload); reqwest drops the Authorization header when the host
        // changes, which is what GitHub expects.
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .timeout(REQUEST_TIMEOUT)
            .build()?;
        Ok(Self {
            shared: Arc::new(Shared {
                http,
                cache: Mutex::new(EtagCache::default()),
                slim: Mutex::new(EtagCache::with_budget(
                    SLIM_CAPACITY,
                    SLIM_MAX_ENTRY_BYTES,
                    SLIM_MAX_TOTAL_BYTES,
                )),
            }),
            tokens,
        })
    }

    /// Whether the dev-mode `GITHUB_TOKEN` fallback exists.
    pub fn dev_token_configured(&self) -> bool {
        self.tokens.dev_token_configured()
    }

    /// A client for the caller's workspace. `cache` mirrors the workspace's
    /// `settings.cache_github`; turning it off also forgets what that
    /// workspace had cached.
    pub async fn client(&self, caller: &Caller, cache: bool) -> Result<GithubClient> {
        let access = self.tokens.access(caller).await?;
        Ok(self.bind(caller.workspace_id, access, cache))
    }

    /// A client bound to a known token, bypassing identity (tests, tools).
    pub fn bind(&self, workspace_id: Uuid, access: GithubAccess, cache: bool) -> GithubClient {
        if !cache {
            self.forget_cache(workspace_id);
        }
        GithubClient {
            shared: Arc::clone(&self.shared),
            access: Arc::new(access),
            workspace_id,
            cache_enabled: cache,
        }
    }

    /// Drops the workspace's cached token (GitHub rejected it).
    pub fn forget_token(&self, workspace_id: Uuid) {
        self.tokens.forget(workspace_id);
    }

    /// Holds `access` for the workspace as if identity had just minted it,
    /// until it expires (tests and tools that run without identity).
    pub fn remember_token(&self, workspace_id: Uuid, access: GithubAccess) {
        self.tokens.remember(workspace_id, access);
    }

    /// Drops the workspace's ETag entries, of both caches.
    pub fn forget_cache(&self, workspace_id: Uuid) {
        let prefix = cache_prefix(workspace_id);
        self.shared.cache().remove_prefix(&prefix);
        self.shared.slim().remove_prefix(&prefix);
    }

    /// Entries currently cached across workspaces; for diagnostics.
    pub fn cached_entries(&self) -> usize {
        self.shared.cache().len()
    }

    /// Entries and body bytes of the slim cache across workspaces; for
    /// diagnostics.
    pub fn slim_cached(&self) -> (usize, usize) {
        let slim = self.shared.slim();
        (slim.len(), slim.bytes())
    }
}

fn cache_prefix(workspace_id: Uuid) -> String {
    format!("{}|", workspace_id.simple())
}

/// One workspace's view of GitHub: its token, its API base (github.com or
/// a GHES host) and its slice of the ETag cache.
#[derive(Clone)]
pub struct GithubClient {
    shared: Arc<Shared>,
    access: Arc<GithubAccess>,
    workspace_id: Uuid,
    cache_enabled: bool,
}

impl GithubClient {
    pub fn workspace_id(&self) -> Uuid {
        self.workspace_id
    }

    pub fn access(&self) -> &GithubAccess {
        &self.access
    }

    pub fn cache_enabled(&self) -> bool {
        self.cache_enabled
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, EtagCache> {
        self.shared.cache()
    }

    fn cache_key(&self, path: &str, query: &[(&str, String)]) -> String {
        format!(
            "{}{}{}",
            cache_prefix(self.workspace_id),
            encode_path(path),
            query_string(query)
        )
    }

    /// One GET with the retry loop for rate limits. `etag` turns the call into
    /// a conditional request; `304` is returned as a success for the caller to
    /// resolve against the cache.
    async fn request(
        &self,
        path: &str,
        query: &[(&str, String)],
        etag: Option<&str>,
        timeout: Duration,
    ) -> Result<Response> {
        self.request_as(path, query, etag, timeout, None).await
    }

    /// [`request`](Self::request) with an `Accept` other than the JSON
    /// default, for the raw media types.
    async fn request_as(
        &self,
        path: &str,
        query: &[(&str, String)],
        etag: Option<&str>,
        timeout: Duration,
        accept: Option<&'static str>,
    ) -> Result<Response> {
        let token = &self.access.token;
        let url = format!(
            "{}{}{}",
            self.access.api_base_url,
            encode_path(path),
            query_string(query)
        );
        for attempt in 1..=MAX_ATTEMPTS {
            let mut builder = self
                .shared
                .http
                .get(&url)
                .bearer_auth(token)
                .timeout(timeout);
            if let Some(etag) = etag {
                builder = builder.header(IF_NONE_MATCH, etag);
            }
            if let Some(accept) = accept {
                builder = builder.header(ACCEPT, accept);
            }
            let response = builder.send().await?;
            let status = response.status();
            if status.is_success() || status == StatusCode::NOT_MODIFIED {
                return Ok(response);
            }
            match status {
                StatusCode::UNAUTHORIZED => return Err(GithubError::Unauthorized),
                StatusCode::NOT_FOUND => return Err(GithubError::NotFound(path.to_owned())),
                // GitHub's answer for commits, trees and archives of a
                // repository without a single commit.
                StatusCode::CONFLICT => return Err(GithubError::EmptyRepository(path.to_owned())),
                StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => {
                    let retry_after = retry_after(&response);
                    let exhausted =
                        status == StatusCode::TOO_MANY_REQUESTS || remaining_is_zero(&response);
                    if let Some(wait) = retry_after
                        && attempt < MAX_ATTEMPTS
                        && wait <= MAX_RETRY_WAIT
                    {
                        tracing::warn!(path, wait = wait.as_secs(), "GitHub rate limited, waiting");
                        tokio::time::sleep(wait).await;
                        continue;
                    }
                    if exhausted || retry_after.is_some() {
                        return Err(GithubError::RateLimited {
                            retry_after: retry_after.map_or(60, |d| d.as_secs()),
                        });
                    }
                    return Err(GithubError::Forbidden(error_message(response).await));
                }
                _ => {
                    return Err(GithubError::Api {
                        status: status.as_u16(),
                        message: error_message(response).await,
                    });
                }
            }
        }
        Err(GithubError::RateLimited { retry_after: 60 })
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Response> {
        self.request(path, query, None, REQUEST_TIMEOUT).await
    }

    /// Body bytes, served from the ETag cache when GitHub says nothing
    /// changed. Only list endpoints go through here; everything else is
    /// fetched once per scan and would only fill the cache.
    async fn get_bytes(&self, path: &str, query: &[(&str, String)]) -> Result<Vec<u8>> {
        let use_cache = self.cache_enabled();
        let key = self.cache_key(path, query);
        let etag = if use_cache {
            self.cache().etag(&key)
        } else {
            None
        };
        let response = self
            .request(path, query, etag.as_deref(), REQUEST_TIMEOUT)
            .await?;
        if response.status() == StatusCode::NOT_MODIFIED {
            if let Some(entry) = self.cache().get(&key) {
                tracing::debug!(path, "GitHub 304, served from cache");
                return Ok(entry.body.clone());
            }
            // Only possible when the cache was cleared mid-request; fetch fresh.
            let response = self.request(path, query, None, REQUEST_TIMEOUT).await?;
            return Ok(response.bytes().await?.to_vec());
        }
        let new_etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = response.bytes().await?.to_vec();
        if use_cache && let Some(new_etag) = new_etag {
            self.cache().insert(&key, &new_etag, body.clone());
        }
        Ok(body)
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let response = self.get(path, query).await?;
        decode(response).await
    }

    async fn get_json_cached<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let bytes = self.get_bytes(path, query).await?;
        serde_json::from_slice(&bytes).map_err(|e| GithubError::Decode(e.to_string()))
    }

    /// A conditional GET that remembers the decoded value instead of the
    /// body. GitHub's payloads carry far more than this client reads (a
    /// pull request in a list is some 20 KB, of which a few hundred bytes
    /// matter), and the calls of CONTRACT.md §23 are repeated per pull
    /// request, per minute. So what sits next to the ETag is `T`
    /// re-encoded: on a `304` it decodes back to the same `T`, at a
    /// fraction of the memory.
    ///
    /// With the workspace's `cache_github` off the request is unconditional
    /// and nothing is kept. The body is read up to [`MAX_JSON_BYTES`].
    async fn get_slim<T>(&self, path: &str, query: &[(&str, String)]) -> Result<Slim<T>>
    where
        T: DeserializeOwned + Serialize,
    {
        self.get_slim_when(path, query, |_| true).await
    }

    /// [`get_slim`](Self::get_slim) that only remembers an answer `settled`
    /// accepts. An answer GitHub itself calls unfinished is asked for again
    /// plainly, never revalidated: a `304` to it would only confirm that
    /// nothing is known yet.
    async fn get_slim_when<T>(
        &self,
        path: &str,
        query: &[(&str, String)],
        settled: impl Fn(&T) -> bool,
    ) -> Result<Slim<T>>
    where
        T: DeserializeOwned + Serialize,
    {
        let use_cache = self.cache_enabled();
        // The type is part of the key: one url is never decoded as two shapes.
        let key = format!(
            "{}{}|{}{}",
            cache_prefix(self.workspace_id),
            std::any::type_name::<T>(),
            encode_path(path),
            query_string(query)
        );
        let etag = if use_cache {
            self.shared.slim().etag(&key)
        } else {
            None
        };
        let mut response = self
            .request(path, query, etag.as_deref(), REQUEST_TIMEOUT)
            .await?;
        if response.status() == StatusCode::NOT_MODIFIED {
            let cached = self.shared.slim().get(&key).map(|entry| entry.body.clone());
            if let Some(body) = cached {
                tracing::debug!(path, "GitHub 304, served from the slim cache");
                let value = serde_json::from_slice(&body)
                    .map_err(|e| GithubError::Decode(format!("cached {path}: {e}")))?;
                return Ok(Slim {
                    value,
                    not_modified: true,
                    fingerprint: fingerprint(&body),
                });
            }
            // Only possible when the entry was evicted mid-request; fetch fresh.
            response = self.request(path, query, None, REQUEST_TIMEOUT).await?;
        }
        let new_etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = read_bounded(response, MAX_JSON_BYTES, path).await?;
        let value: T =
            serde_json::from_slice(&body).map_err(|e| GithubError::Decode(e.to_string()))?;
        let slim = serde_json::to_vec(&value).map_err(|e| GithubError::Decode(e.to_string()))?;
        let fingerprint = fingerprint(&slim);
        if use_cache
            && let Some(new_etag) = new_etag
            && settled(&value)
        {
            self.shared.slim().insert(&key, &new_etag, slim);
        }
        Ok(Slim {
            value,
            not_modified: false,
            fingerprint,
        })
    }

    /// Follows `page=` until a short page; `per_page` is already 100. Pages
    /// are cached by ETag: a list that did not change costs no quota.
    async fn get_paginated<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Vec<T>> {
        let mut items = Vec::new();
        for page in 1..=20_u32 {
            let mut q = query.to_vec();
            q.push(("per_page", "100".to_owned()));
            q.push(("page", page.to_string()));
            let batch: Vec<T> = self.get_json_cached(path, &q).await?;
            let last = batch.len() < 100;
            items.extend(batch);
            if last {
                break;
            }
        }
        Ok(items)
    }

    pub async fn user(&self) -> Result<(User, Vec<String>)> {
        let response = self.get("/user", &[]).await?;
        let scopes = response
            .headers()
            .get("x-oauth-scopes")
            .and_then(|v| v.to_str().ok())
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Ok((decode(response).await?, scopes))
    }

    /// The repositories a user token can see.
    pub async fn repos(&self) -> Result<Vec<Repo>> {
        self.get_paginated(
            "/user/repos",
            &[
                ("affiliation", "owner,collaborator".to_owned()),
                ("sort", "pushed".to_owned()),
            ],
        )
        .await
    }

    /// The repositories an installation grants (`all` or the selection).
    pub async fn installation_repos(&self) -> Result<Vec<Repo>> {
        let mut repos = Vec::new();
        for page in 1..=20_u32 {
            let batch: InstallationRepos = self
                .get_json_cached(
                    "/installation/repositories",
                    &[("per_page", "100".to_owned()), ("page", page.to_string())],
                )
                .await?;
            let last = batch.repositories.len() < 100;
            repos.extend(batch.repositories);
            if last {
                break;
            }
        }
        Ok(repos)
    }

    /// Whatever the workspace's token grants: the installation's
    /// repositories for an organisation, the user's for a personal one.
    pub async fn accessible_repos(&self) -> Result<Vec<Repo>> {
        match self.access.kind {
            AccessKind::Installation => self.installation_repos().await,
            AccessKind::User => self.repos().await,
        }
    }

    pub async fn repo(&self, owner: &str, name: &str) -> Result<Repo> {
        self.get_json(&format!("/repos/{owner}/{name}"), &[]).await
    }

    pub async fn languages(
        &self,
        owner: &str,
        name: &str,
    ) -> Result<std::collections::BTreeMap<String, u64>> {
        self.get_json(&format!("/repos/{owner}/{name}/languages"), &[])
            .await
    }

    pub async fn branch_sha(&self, owner: &str, name: &str, branch: &str) -> Result<String> {
        let commit: Commit = self
            .get_json(&format!("/repos/{owner}/{name}/commits/{branch}"), &[])
            .await?;
        Ok(commit.sha)
    }

    pub async fn tree(&self, owner: &str, name: &str, reference: &str) -> Result<Tree> {
        self.get_json(
            &format!("/repos/{owner}/{name}/git/trees/{reference}"),
            &[("recursive", "1".to_owned())],
        )
        .await
    }

    /// One level of a tree (no `recursive`): what the fallback walks when
    /// the recursive listing was truncated.
    pub async fn tree_shallow(&self, owner: &str, name: &str, sha: &str) -> Result<Tree> {
        self.get_json(&format!("/repos/{owner}/{name}/git/trees/{sha}"), &[])
            .await
    }

    /// The whole tree at `reference`. GitHub truncates the recursive
    /// listing of very large repositories; when it does, the directories
    /// the listing did reach are walked one level at a time until the tree
    /// is complete or [`TREE_FALLBACK_MAX_REQUESTS`] were spent, in which
    /// case `truncated` stays set and the scan works with what it has.
    pub async fn full_tree(&self, owner: &str, name: &str, reference: &str) -> Result<Tree> {
        let mut tree = self.tree(owner, name, reference).await?;
        if !tree.truncated {
            return Ok(tree);
        }
        tracing::info!(
            entries = tree.tree.len(),
            "GitHub truncated the recursive tree; completing it per directory"
        );
        let complete = self
            .complete_tree(owner, name, &mut tree.tree, TREE_FALLBACK_MAX_REQUESTS)
            .await?;
        tree.truncated = !complete;
        Ok(tree)
    }

    /// Lists every directory of `nodes` that has no entries yet, breadth
    /// first, until `budget` requests were spent. Returns whether the tree
    /// is complete. Paths with spaces or non-ASCII characters are kept as
    /// GitHub gives them (the request goes by sha, never by path).
    async fn complete_tree(
        &self,
        owner: &str,
        name: &str,
        nodes: &mut Vec<TreeNode>,
        budget: usize,
    ) -> Result<bool> {
        use std::collections::HashSet;
        let mut listed: HashSet<String> = HashSet::new();
        for node in nodes.iter() {
            if let Some((dir, _)) = node.path.rsplit_once('/') {
                listed.insert(dir.to_owned());
            }
        }
        let mut pending: VecDeque<TreeNode> = nodes
            .iter()
            .filter(|n| n.is_dir() && !listed.contains(&n.path) && !n.sha.is_empty())
            .cloned()
            .collect();
        let mut requests = 0;
        while let Some(dir) = pending.pop_front() {
            if requests >= budget {
                tracing::warn!(
                    requests,
                    left = pending.len() + 1,
                    "tree fallback budget spent; the tree stays truncated"
                );
                return Ok(false);
            }
            requests += 1;
            let level = match self.tree_shallow(owner, name, &dir.sha).await {
                Ok(level) => level,
                // A directory that vanished between calls is not worth failing a scan over.
                Err(GithubError::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            listed.insert(dir.path.clone());
            for mut child in level.tree {
                child.path = format!("{}/{}", dir.path, child.path);
                if child.is_dir() && !listed.contains(&child.path) {
                    pending.push_back(child.clone());
                }
                nodes.push(child);
            }
        }
        nodes.sort_by(|a, b| a.path.cmp(&b.path));
        nodes.dedup_by(|a, b| a.path == b.path);
        Ok(true)
    }

    /// File contents at `reference`, base64-decoded. Binary bytes are replaced lossily.
    pub async fn file(
        &self,
        owner: &str,
        name: &str,
        path: &str,
        reference: &str,
    ) -> Result<String> {
        let content: Content = self
            .get_json(
                &format!("/repos/{owner}/{name}/contents/{path}"),
                &[("ref", reference.to_owned())],
            )
            .await?;
        content_text(content, path)
    }

    /// A text file on the repository's default branch (no `ref`, so GitHub
    /// picks the branch), or `None` when the path does not exist there.
    /// Conditional like every list: asking again for an unchanged file
    /// costs no quota.
    pub async fn default_branch_file(
        &self,
        owner: &str,
        name: &str,
        path: &str,
    ) -> Result<Option<String>> {
        let api_path = format!("/repos/{owner}/{name}/contents/{path}");
        match self.get_slim::<Content>(&api_path, &[]).await {
            Ok(content) => content_text(content.value, path).map(Some),
            Err(GithubError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// One file's bytes at `reference` through the contents API with the raw
    /// media type, which serves files the JSON encoding refuses (over 1 MB)
    /// and never base64-pads. Reading stops after `max_bytes`: the result is
    /// then `truncated` and holds exactly the first `max_bytes` bytes.
    /// `NotFound` when `path` is a directory, a submodule or absent.
    pub async fn file_raw(
        &self,
        owner: &str,
        name: &str,
        path: &str,
        reference: &str,
        max_bytes: usize,
    ) -> Result<RawFile> {
        let api_path = format!("/repos/{owner}/{name}/contents/{path}");
        let mut response = self
            .request_as(
                &api_path,
                &[("ref", reference.to_owned())],
                None,
                TARBALL_TIMEOUT,
                Some("application/vnd.github.raw+json"),
            )
            .await?;
        // Raw content comes back as text/plain or an octet stream; JSON means
        // GitHub described the path (a directory listing, a submodule) instead.
        let is_json = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.starts_with("application/json"));
        if is_json {
            return Err(GithubError::NotFound(format!("{path} is not a file")));
        }
        let content_length = response.content_length();
        let mut bytes = Vec::with_capacity(
            content_length.map_or(64 << 10, |len| (len as usize).min(max_bytes)),
        );
        let mut truncated = false;
        while let Some(chunk) = response.chunk().await? {
            let room = max_bytes.saturating_sub(bytes.len());
            if chunk.len() > room {
                bytes.extend_from_slice(&chunk[..room]);
                truncated = true;
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(RawFile {
            bytes,
            truncated,
            content_length,
        })
    }

    /// The repository archive at `reference`, streamed into an anonymous
    /// temporary file (never held in memory), or `None` when it is larger
    /// than `max_bytes` (the caller falls back to per-file downloads).
    /// GitHub redirects to codeload, which streams the archive without a
    /// Content-Length, so the size is enforced while reading. The file is
    /// positioned at its start and unlinked already: it goes with the handle.
    pub async fn tarball(
        &self,
        owner: &str,
        name: &str,
        reference: &str,
        max_bytes: usize,
    ) -> Result<Option<std::fs::File>> {
        let path = format!("/repos/{owner}/{name}/tarball/{reference}");
        let mut response = self.request(&path, &[], None, TARBALL_TIMEOUT).await?;
        if response
            .content_length()
            .is_some_and(|len| len > max_bytes as u64)
        {
            return Ok(None);
        }
        let mut file = tokio::task::spawn_blocking(tempfile::tempfile)
            .await
            .map_err(|e| GithubError::Decode(format!("temp file: {e}")))?
            .map_err(|e| GithubError::Decode(format!("temp file: {e}")))?;
        let mut written = 0usize;
        while let Some(chunk) = response.chunk().await? {
            written += chunk.len();
            if written > max_bytes {
                return Ok(None);
            }
            // Writes of a few hundred KB to a temp file are fast enough to
            // stay on the runtime thread; the unpack later is what blocks.
            file.write_all(&chunk)
                .map_err(|e| GithubError::Decode(format!("temp file: {e}")))?;
        }
        file.flush()
            .and_then(|()| file.rewind())
            .map_err(|e| GithubError::Decode(format!("temp file: {e}")))?;
        Ok(Some(file))
    }

    pub async fn workflow_runs(
        &self,
        owner: &str,
        name: &str,
        per_page: u32,
    ) -> Result<Vec<WorkflowRun>> {
        let page: WorkflowRuns = self
            .get_json_cached(
                &format!("/repos/{owner}/{name}/actions/runs"),
                &[("per_page", per_page.to_string())],
            )
            .await?;
        Ok(page.workflow_runs)
    }

    pub async fn run_jobs(&self, owner: &str, name: &str, run_id: i64) -> Result<Vec<Job>> {
        let page: Jobs = self
            .get_json(
                &format!("/repos/{owner}/{name}/actions/runs/{run_id}/jobs"),
                &[("per_page", "100".to_owned())],
            )
            .await?;
        Ok(page.jobs)
    }

    pub async fn deployments(
        &self,
        owner: &str,
        name: &str,
        per_page: u32,
    ) -> Result<Vec<Deployment>> {
        self.get_json_cached(
            &format!("/repos/{owner}/{name}/deployments"),
            &[("per_page", per_page.to_string())],
        )
        .await
    }

    pub async fn deployment_statuses(
        &self,
        owner: &str,
        name: &str,
        id: i64,
    ) -> Result<Vec<DeploymentStatus>> {
        self.get_json(
            &format!("/repos/{owner}/{name}/deployments/{id}/statuses"),
            &[("per_page", "10".to_owned())],
        )
        .await
    }

    pub async fn open_pulls(&self, owner: &str, name: &str) -> Result<Vec<Pull>> {
        self.get_paginated(
            &format!("/repos/{owner}/{name}/pulls"),
            &[("state", "open".to_owned())],
        )
        .await
    }

    /// Up to `limit` pull requests in `state` (`open` | `closed` | `all`),
    /// most recently updated first. Pages are ETag-cached like every list.
    pub async fn pulls(
        &self,
        owner: &str,
        name: &str,
        state: &str,
        limit: usize,
    ) -> Result<Vec<Pull>> {
        Ok(self.pulls_listing(owner, name, state, limit).await?.pulls)
    }

    /// [`pulls`](Self::pulls) with what a poller needs to know: whether
    /// GitHub said nothing changed, and a fingerprint of the listing. Every
    /// page is a conditional request, so polling an unchanged list costs
    /// `304`s only (CONTRACT.md §23.1).
    pub async fn pulls_listing(
        &self,
        owner: &str,
        name: &str,
        state: &str,
        limit: usize,
    ) -> Result<PullListing> {
        let path = format!("/repos/{owner}/{name}/pulls");
        let mut pulls = Vec::new();
        let mut not_modified = true;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for page in 1..=20_u32 {
            let per_page = limit.saturating_sub(pulls.len()).clamp(1, 100);
            let batch: Slim<Vec<Pull>> = self
                .get_slim(
                    &path,
                    &[
                        ("state", state.to_owned()),
                        ("sort", "updated".to_owned()),
                        ("direction", "desc".to_owned()),
                        ("per_page", per_page.to_string()),
                        ("page", page.to_string()),
                    ],
                )
                .await?;
            not_modified &= batch.not_modified;
            batch.fingerprint.hash(&mut hasher);
            let last = batch.value.len() < per_page;
            pulls.extend(batch.value);
            if last || pulls.len() >= limit {
                break;
            }
        }
        pulls.truncate(limit);
        Ok(PullListing {
            pulls,
            not_modified,
            fingerprint: hasher.finish(),
        })
    }

    /// One pull request with its counters and, once GitHub has computed
    /// it, its mergeability. Conditional: re-reading an unchanged pull
    /// request costs no quota. The exception is an open pull request whose
    /// `mergeable` is still `null`: GitHub computes it in the background
    /// after the first read, so that answer is not kept and the next read
    /// is a plain one.
    pub async fn pull(&self, owner: &str, name: &str, number: u64) -> Result<Pull> {
        // Closed and merged pull requests keep `null` for good.
        let settled = |pull: &Pull| pull.mergeable.is_some() || pull.state != "open";
        Ok(self
            .get_slim_when(
                &format!("/repos/{owner}/{name}/pulls/{number}"),
                &[],
                settled,
            )
            .await?
            .value)
    }

    /// Which merge methods the repository allows (CONTRACT.md §23.3).
    pub async fn merge_settings(&self, owner: &str, name: &str) -> Result<RepoMergeSettings> {
        Ok(self
            .get_slim(&format!("/repos/{owner}/{name}"), &[])
            .await?
            .value)
    }

    /// The check runs of a commit (GitHub lists the latest run of every
    /// check), at most [`MAX_STATUS_PAGES`] pages of a hundred.
    pub async fn check_runs(&self, owner: &str, name: &str, sha: &str) -> Result<Listed<CheckRun>> {
        let path = format!("/repos/{owner}/{name}/commits/{sha}/check-runs");
        let mut listed = Listed {
            items: Vec::new(),
            more: true,
        };
        for page in 1..=MAX_STATUS_PAGES {
            let batch: CheckRuns = self
                .get_slim(
                    &path,
                    &[("per_page", "100".to_owned()), ("page", page.to_string())],
                )
                .await?
                .value;
            let short = batch.check_runs.len() < 100;
            listed.items.extend(batch.check_runs);
            // A short page ends the list; so does having as many as GitHub counted.
            if short || (batch.total_count > 0 && listed.items.len() as u64 >= batch.total_count) {
                listed.more = false;
                break;
            }
        }
        Ok(listed)
    }

    /// The commit statuses of a commit, the latest of every context, at
    /// most [`MAX_STATUS_PAGES`] pages of a hundred.
    pub async fn commit_statuses(
        &self,
        owner: &str,
        name: &str,
        sha: &str,
    ) -> Result<Listed<CommitStatus>> {
        let path = format!("/repos/{owner}/{name}/commits/{sha}/status");
        let mut listed = Listed {
            items: Vec::new(),
            more: true,
        };
        for page in 1..=MAX_STATUS_PAGES {
            let batch: CombinedStatus = self
                .get_slim(
                    &path,
                    &[("per_page", "100".to_owned()), ("page", page.to_string())],
                )
                .await?
                .value;
            let short = batch.statuses.len() < 100;
            listed.items.extend(batch.statuses);
            if short || (batch.total_count > 0 && listed.items.len() as u64 >= batch.total_count) {
                listed.more = false;
                break;
            }
        }
        Ok(listed)
    }

    /// The reviews of a pull request, oldest first, at most
    /// [`MAX_STATUS_PAGES`] pages of a hundred. GitHub does not say how
    /// many there are, so a last page that came back full counts as "more".
    pub async fn pull_reviews(
        &self,
        owner: &str,
        name: &str,
        number: u64,
    ) -> Result<Listed<Review>> {
        let path = format!("/repos/{owner}/{name}/pulls/{number}/reviews");
        let mut listed = Listed {
            items: Vec::new(),
            more: true,
        };
        for page in 1..=MAX_STATUS_PAGES {
            let batch: Vec<Review> = self
                .get_slim(
                    &path,
                    &[("per_page", "100".to_owned()), ("page", page.to_string())],
                )
                .await?
                .value;
            let short = batch.len() < 100;
            listed.items.extend(batch);
            if short {
                listed.more = false;
                break;
            }
        }
        Ok(listed)
    }

    /// Whether GitHub protects `branch` (its rules apply at merge time).
    pub async fn branch_protected(&self, owner: &str, name: &str, branch: &str) -> Result<bool> {
        let branch: Slim<Branch> = self
            .get_slim(&format!("/repos/{owner}/{name}/branches/{branch}"), &[])
            .await?;
        Ok(branch.value.protected)
    }

    /// How many commits `head` is behind `base`. Only the counters are
    /// kept; the commits are asked for one at a time and the file list is
    /// dropped while decoding.
    pub async fn behind_by(&self, owner: &str, name: &str, base: &str, head: &str) -> Result<u32> {
        let comparison: Slim<Comparison> = self
            .get_slim(
                &format!("/repos/{owner}/{name}/compare/{base}...{head}"),
                &[("per_page", "1".to_owned())],
            )
            .await?;
        Ok(comparison.value.behind_by)
    }

    /// Succeeds when Dependabot alerts are on for the repository (GitHub
    /// answers `204`); `NotFound` when they are off.
    pub async fn vulnerability_alerts(&self, owner: &str, name: &str) -> Result<()> {
        self.get(&format!("/repos/{owner}/{name}/vulnerability-alerts"), &[])
            .await
            .map(|_| ())
    }

    /// Whether Dependabot security updates are on, and paused.
    pub async fn security_fixes(&self, owner: &str, name: &str) -> Result<SecurityFixes> {
        let response = self
            .get(
                &format!("/repos/{owner}/{name}/automated-security-fixes"),
                &[],
            )
            .await?;
        let path = "automated-security-fixes";
        let body = read_bounded(response, MAX_JSON_BYTES, path).await?;
        serde_json::from_slice(&body).map_err(|e| GithubError::Decode(e.to_string()))
    }

    /// The repository's open Dependabot alerts, at most `max` of them, read
    /// in two passes so that a cut never drops the worse ones: critical and
    /// high first, then medium and low while there is room. GitHub cannot
    /// sort by severity, only filter by it. The endpoint pages by cursor:
    /// the `after` value of each `Link: rel="next"` is sent back on this
    /// client's own path, so the token never follows a URL GitHub handed out.
    pub async fn dependabot_alerts(
        &self,
        owner: &str,
        name: &str,
        max: usize,
    ) -> Result<AlertListing> {
        let path = format!("/repos/{owner}/{name}/dependabot/alerts");
        let mut listing = AlertListing::default();
        let mut held = std::collections::HashSet::new();
        for severities in ALERT_SEVERITY_PASSES {
            if listing.alerts.len() >= max {
                // Full already. Whether anything less severe exists decides
                // "truncated"; one alert is enough to know.
                if !listing.truncated {
                    let (probe, _) = self.alert_page(&path, severities, 1, None).await?;
                    listing.truncated = !probe.is_empty();
                }
                break;
            }
            let mut after: Option<String> = None;
            // One page more than `max` needs can never be asked for: the
            // loop ends as soon as `max` alerts are held.
            for _ in 0..max.div_ceil(100).max(1) {
                let (page, next) = self
                    .alert_page(&path, severities, 100, after.as_deref())
                    .await?;
                after = next;
                let empty = page.is_empty();
                // An alert re-rated between the two passes must not show twice.
                listing
                    .alerts
                    .extend(page.into_iter().filter(|alert| held.insert(alert.number)));
                if after.is_none() || empty || listing.alerts.len() >= max {
                    break;
                }
            }
            if listing.alerts.len() > max {
                listing.alerts.truncate(max);
                listing.truncated = true;
            }
            // A cursor left over means GitHub had another page.
            listing.truncated |= after.is_some();
        }
        Ok(listing)
    }

    /// One page of open alerts of the given severities and the cursor of
    /// the next page, when there is one.
    async fn alert_page(
        &self,
        path: &str,
        severities: &str,
        per_page: usize,
        after: Option<&str>,
    ) -> Result<(Vec<Alert>, Option<String>)> {
        let mut query = vec![
            ("state", "open".to_owned()),
            ("per_page", per_page.to_string()),
            ("severity", severities.to_owned()),
        ];
        if let Some(cursor) = after {
            query.push(("after", cursor.to_owned()));
        }
        let response = self.get(path, &query).await?;
        let next = response
            .headers()
            .get(reqwest::header::LINK)
            .and_then(|v| v.to_str().ok())
            .and_then(next_cursor);
        let body = read_bounded(response, MAX_JSON_BYTES, path).await?;
        let page: Vec<Alert> =
            serde_json::from_slice(&body).map_err(|e| GithubError::Decode(e.to_string()))?;
        Ok((page, next))
    }

    /// The changed files of a pull request, at most `max_files` of them
    /// (GitHub lists up to 3000; the review reads a bounded diff).
    pub async fn pull_files(
        &self,
        owner: &str,
        name: &str,
        number: u64,
        max_files: usize,
    ) -> Result<Vec<PullFile>> {
        let path = format!("/repos/{owner}/{name}/pulls/{number}/files");
        let mut files = Vec::new();
        for page in 1..=30_u32 {
            let batch: Vec<PullFile> = self
                .get_json(
                    &path,
                    &[("per_page", "100".to_owned()), ("page", page.to_string())],
                )
                .await?;
            let last = batch.len() < 100;
            files.extend(batch);
            if last || files.len() > max_files {
                break;
            }
        }
        Ok(files)
    }
}

async fn decode<T: DeserializeOwned>(response: Response) -> Result<T> {
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|e| GithubError::Decode(e.to_string()))
}

/// The whole body, unless it is larger than `max`: then an error, never a
/// cut document (half a JSON value decodes to nothing useful).
async fn read_bounded(mut response: Response, max: usize, what: &str) -> Result<Vec<u8>> {
    let too_large =
        || GithubError::Decode(format!("{what}: the response is larger than {max} bytes"));
    if response
        .content_length()
        .is_some_and(|len| len > max as u64)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > max {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// The text of a contents-API answer: base64-decoded, binary bytes replaced
/// lossily.
fn content_text(content: Content, path: &str) -> Result<String> {
    match content.encoding.as_deref() {
        Some("base64") => {
            let raw: String = content
                .content
                .unwrap_or_default()
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            let bytes = STANDARD
                .decode(raw)
                .map_err(|e| GithubError::Decode(format!("{path}: {e}")))?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        _ => Ok(content.content.unwrap_or_default()),
    }
}

/// In-process identity of a slim body (see [`Slim::fingerprint`]).
fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// The `after` cursor of the `rel="next"` entry of a `Link` header, when
/// there is one. Only the cursor is taken: the URL around it is GitHub's
/// business (it names the repository by id, and on an enterprise host may
/// carry another base than the one this workspace is configured with).
pub fn next_cursor(link: &str) -> Option<String> {
    let next = link.split(',').find(|part| {
        part.split(';')
            .skip(1)
            .any(|param| param.trim().replace(' ', "") == "rel=\"next\"")
    })?;
    let url = next.split(';').next()?.trim();
    let url = url.strip_prefix('<')?.strip_suffix('>')?;
    let query = url.split_once('?')?.1;
    let query = query.split('#').next().unwrap_or(query);
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "after" && !value.is_empty()).then(|| percent_decode(value))
    })
}

/// Undoes percent-encoding (and `+` for a space) in a query value.
/// Malformed escapes are kept as they are.
fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(high), Some(low)) => {
                    out.push((high * 16 + low) as u8);
                    i += 3;
                    continue;
                }
                _ => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn error_message(response: Response) -> String {
    let status = response.status();
    match response.json::<ApiError>().await {
        Ok(body) if !body.message.is_empty() => body.message,
        _ => status.to_string(),
    }
}

fn retry_after(response: &Response) -> Option<Duration> {
    let headers = response.headers();
    if let Some(seconds) = headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
    {
        return Some(Duration::from_secs(seconds.max(1)));
    }
    let reset = headers
        .get("x-ratelimit-reset")
        .and_then(|v| v.to_str().ok()?.parse::<i64>().ok())?;
    let now = chrono::Utc::now().timestamp();
    Some(Duration::from_secs((reset - now).clamp(1, i64::MAX) as u64))
}

fn remaining_is_zero(response: &Response) -> bool {
    response
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim() == "0")
}

/// Percent-encodes everything outside the unreserved set, keeping `/` so
/// repository paths stay paths. reqwest's `query` feature is not enabled.
fn encode(raw: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn encode_path(path: &str) -> String {
    encode(path, true)
}

fn query_string(query: &[(&str, String)]) -> String {
    if query.is_empty() {
        return String::new();
    }
    let pairs: Vec<String> = query
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k, false), encode(v, false)))
        .collect();
    format!("?{}", pairs.join("&"))
}

/// `owner/name` → `(owner, name)`.
pub fn split_full_name(full_name: &str) -> Option<(&str, &str)> {
    let (owner, name) = full_name.trim().split_once('/')?;
    let valid = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (valid(owner) && valid(name)).then_some((owner, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_name_validation() {
        assert_eq!(
            split_full_name("f8fvwgzc/view-engine"),
            Some(("f8fvwgzc", "view-engine"))
        );
        assert_eq!(split_full_name("nope"), None);
        assert_eq!(split_full_name("a/b/c"), None);
        assert_eq!(split_full_name("a/b c"), None);
    }

    #[test]
    fn urls_are_encoded() {
        assert_eq!(
            encode_path("/repos/o/r/contents/src/my file.rs"),
            "/repos/o/r/contents/src/my%20file.rs"
        );
        assert_eq!(
            query_string(&[
                ("affiliation", "owner,collaborator".to_owned()),
                ("page", "2".to_owned())
            ]),
            "?affiliation=owner%2Ccollaborator&page=2"
        );
        assert_eq!(query_string(&[]), "");
    }

    #[test]
    fn the_next_cursor_is_taken_from_the_link_header_and_nothing_else() {
        // GitHub's shape: the repository by id, the cursor percent-encoded.
        let link = "<https://api.github.com/repositories/42/dependabot/alerts?state=open&per_page=100&after=Y3Vyc29yOnYyOpHOAAE%3D>; rel=\"next\", \
                    <https://api.github.com/repositories/42/dependabot/alerts?state=open&per_page=100&before=Y3Vyc29yOnYy>; rel=\"prev\"";
        assert_eq!(next_cursor(link).as_deref(), Some("Y3Vyc29yOnYyOpHOAAE="));
        // Order and spacing of the entries do not matter.
        let reordered = "<https://x.example/a?before=b>; rel=\"prev\",<https://x.example/a?after=c%2Fd&per_page=100>;rel=\"next\"";
        assert_eq!(next_cursor(reordered).as_deref(), Some("c/d"));
        // The last page has no `next`; a `next` without a cursor is not one.
        assert_eq!(
            next_cursor("<https://x.example/a?before=b>; rel=\"prev\""),
            None
        );
        assert_eq!(
            next_cursor("<https://x.example/a?page=2>; rel=\"next\""),
            None
        );
        assert_eq!(
            next_cursor("<https://x.example/a?after=>; rel=\"next\""),
            None
        );
        assert_eq!(next_cursor(""), None);
        assert_eq!(next_cursor("garbage"), None);
        // A broken escape is kept as it is rather than guessed at.
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("a%zzb+c"), "a%zzb c");
    }

    #[test]
    fn slim_keys_are_per_workspace_and_per_shape_and_the_switch_clears_them() {
        let hub = Github::new(TokenProvider::new(
            crate::identity::Identity::disabled(),
            None,
        ))
        .unwrap();
        let workspace = Uuid::now_v7();
        let prefix = cache_prefix(workspace);
        hub.shared
            .slim()
            .insert(&format!("{prefix}Pull|/x"), "\"e\"", b"{}".to_vec());
        hub.shared.slim().insert(
            &format!("{}Pull|/x", cache_prefix(Uuid::now_v7())),
            "\"e\"",
            b"{}".to_vec(),
        );
        assert_eq!(hub.slim_cached(), (2, 4));
        // The raw cache is another cache: nothing of this shows there.
        assert_eq!(hub.cached_entries(), 0);
        hub.forget_cache(workspace);
        assert_eq!(hub.slim_cached(), (1, 2));

        // A remembered token is served until it expires, without identity.
        let mut access = GithubAccess::dev("t");
        access.api_base_url = "http://127.0.0.1:1".to_owned();
        hub.remember_token(workspace, access);
        assert_eq!(hub.tokens.cached_tokens(), 1);
        hub.forget_token(workspace);
        assert_eq!(hub.tokens.cached_tokens(), 0);
    }

    #[test]
    fn cache_entries_are_per_workspace_and_cleared_by_the_switch() {
        let hub = Github::new(TokenProvider::new(
            crate::identity::Identity::disabled(),
            None,
        ))
        .unwrap();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let client_a = hub.bind(a, GithubAccess::dev("ta"), true);
        let client_b = hub.bind(b, GithubAccess::dev("tb"), true);
        assert_ne!(client_a.cache_key("/x", &[]), client_b.cache_key("/x", &[]));
        let key_a = client_a.cache_key("/x", &[]);
        let key_b = client_b.cache_key("/x", &[]);
        client_a.cache().insert(&key_a, "\"e\"", b"{}".to_vec());
        client_b.cache().insert(&key_b, "\"e\"", b"{}".to_vec());
        assert_eq!(hub.cached_entries(), 2);

        // Turning the cache off for one workspace forgets only its entries.
        let client_a = hub.bind(a, GithubAccess::dev("ta"), false);
        assert!(!client_a.cache_enabled());
        assert_eq!(hub.cached_entries(), 1);
        assert!(client_b.cache().get(&key_b).is_some());
        assert_eq!(client_a.access().api_base_url, token::PUBLIC_API);
    }
}
