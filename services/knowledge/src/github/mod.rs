//! Read-only GitHub REST client. Only what the scanner and the delivery
//! observer need; every call is a GET and failures are typed so the gRPC
//! layer can answer with the right status.

pub mod types;

use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{
    Response, StatusCode,
    header::{ACCEPT, HeaderMap, HeaderValue},
};
use serde::de::DeserializeOwned;

pub use types::*;

const API: &str = "https://api.github.com";
const USER_AGENT: &str = "sdi-knowledge";
const MAX_ATTEMPTS: u32 = 3;
/// Longest secondary-rate-limit pause we honour inline; beyond it the caller gets the error.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    #[error("GITHUB_TOKEN is not configured")]
    NoToken,
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

#[derive(Clone)]
pub struct GithubClient {
    http: reqwest::Client,
    token: Option<String>,
    base: String,
}

impl GithubClient {
    pub fn new(token: Option<String>) -> anyhow::Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            token,
            base: API.to_owned(),
        })
    }

    pub fn token_configured(&self) -> bool {
        self.token.is_some()
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Response> {
        let token = self.token.as_ref().ok_or(GithubError::NoToken)?;
        let url = format!("{}{}{}", self.base, encode_path(path), query_string(query));
        for attempt in 1..=MAX_ATTEMPTS {
            let response = self.http.get(&url).bearer_auth(token).send().await?;
            let status = response.status();
            if status.is_success() {
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

    async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let response = self.get(path, query).await?;
        decode(response).await
    }

    /// Follows `page=` until a short page; `per_page` is already 100.
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
            let batch: Vec<T> = self.get_json(path, &q).await?;
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

    pub async fn workflow_runs(
        &self,
        owner: &str,
        name: &str,
        per_page: u32,
    ) -> Result<Vec<WorkflowRun>> {
        let page: WorkflowRuns = self
            .get_json(
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
        self.get_json(
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
}
