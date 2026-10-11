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
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{
    Response, StatusCode,
    header::{ACCEPT, ETAG, HeaderMap, HeaderValue, IF_NONE_MATCH},
};
use serde::de::DeserializeOwned;

use cache::EtagCache;
pub use token::{AccessKind, GithubAccess, TokenProvider};
pub use types::*;
use uuid::Uuid;

use crate::auth::Caller;

const USER_AGENT: &str = "sdi-knowledge";
const MAX_ATTEMPTS: u32 = 3;
/// Longest secondary-rate-limit pause we honour inline; beyond it the caller gets the error.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(60);
/// JSON calls are small; the archive download gets its own, longer budget.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const TARBALL_TIMEOUT: Duration = Duration::from_secs(180);

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

/// Shared by every client the hub hands out: one HTTP client and one ETag
/// cache (entries are keyed by workspace, so tenants never share a body).
struct Shared {
    http: reqwest::Client,
    cache: Mutex<EtagCache>,
}

impl Shared {
    fn cache(&self) -> std::sync::MutexGuard<'_, EtagCache> {
        // A poisoned cache only means a panic mid-insert; the data is still a cache.
        self.cache
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

    /// Drops the workspace's ETag entries.
    pub fn forget_cache(&self, workspace_id: Uuid) {
        self.shared
            .cache()
            .remove_prefix(&cache_prefix(workspace_id));
    }

    /// Entries currently cached across workspaces; for diagnostics.
    pub fn cached_entries(&self) -> usize {
        self.shared.cache().len()
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

    /// The repository archive at `reference` as gzipped tar bytes, or `None`
    /// when it is larger than `max_bytes` (the caller falls back to per-file
    /// downloads). GitHub redirects to codeload, which streams the archive
    /// without a Content-Length, so the size is enforced while reading.
    pub async fn tarball(
        &self,
        owner: &str,
        name: &str,
        reference: &str,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>> {
        let path = format!("/repos/{owner}/{name}/tarball/{reference}");
        let mut response = self.request(&path, &[], None, TARBALL_TIMEOUT).await?;
        if response
            .content_length()
            .is_some_and(|len| len > max_bytes as u64)
        {
            return Ok(None);
        }
        let mut body = Vec::with_capacity(
            response
                .content_length()
                .map_or(4 << 20, |len| len as usize),
        );
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > max_bytes {
                return Ok(None);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Some(body))
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
}

async fn decode<T: DeserializeOwned>(response: Response) -> Result<T> {
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|e| GithubError::Decode(e.to_string()))
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
