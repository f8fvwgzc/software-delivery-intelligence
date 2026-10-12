//! The one GitHub write the service performs (CONTRACT.md §20): posting a
//! pull request review. Access is per workspace, the same way knowledge
//! does it (§18): identity mints the token (an installation token with
//! `pull_requests: write`, or the user's OAuth token for a personal
//! workspace), it is cached in memory until shortly before it expires, and
//! `GITHUB_TOKEN` is the dev fallback only while identity is unreachable.

use std::{collections::HashMap, sync::Mutex, time::Duration as StdDuration};

use chrono::{DateTime, Duration, Utc};
use sdi_common::principal::Principal;
use serde::{Deserialize, Serialize};
use tonic::{Code, Status};
use uuid::Uuid;

use crate::gateway::IdentityGateway;

pub const PUBLIC_API: &str = "https://api.github.com";
const USER_AGENT: &str = "sdi-intelligence";
const REFRESH_MARGIN: Duration = Duration::minutes(5);
const DEFAULT_LIFETIME: Duration = Duration::minutes(55);
const DEV_RETRY: Duration = Duration::minutes(5);
const REQUEST_TIMEOUT: StdDuration = StdDuration::from_secs(30);

#[derive(Debug, Clone)]
pub struct GithubAccess {
    pub token: String,
    pub api_base_url: String,
    pub expires_at: DateTime<Utc>,
}

impl GithubAccess {
    fn usable(&self, now: DateTime<Utc>) -> bool {
        self.expires_at - REFRESH_MARGIN > now
    }
}

/// Tokens per workspace, from identity, cached while fresh.
pub struct GithubTokens {
    dev_token: Option<String>,
    cache: Mutex<HashMap<Uuid, GithubAccess>>,
}

impl GithubTokens {
    pub fn new(dev_token: Option<String>) -> Self {
        Self {
            dev_token: dev_token.filter(|t| !t.trim().is_empty()),
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub async fn access(
        &self,
        identity: &dyn IdentityGateway,
        principal: &Principal,
        workspace: Uuid,
    ) -> Result<GithubAccess, Status> {
        let now = Utc::now();
        if let Some(access) = self
            .lock()
            .get(&workspace)
            .filter(|a| a.usable(now))
            .cloned()
        {
            return Ok(access);
        }
        let access = match identity.installation_token(principal).await {
            Ok(token) if !token.token.trim().is_empty() => GithubAccess {
                token: token.token,
                api_base_url: if token.api_base_url.trim().is_empty() {
                    PUBLIC_API.to_owned()
                } else {
                    token.api_base_url.trim().trim_end_matches('/').to_owned()
                },
                expires_at: token
                    .expires_at
                    .as_ref()
                    .and_then(sdi_common::time::from_proto)
                    .unwrap_or(now + DEFAULT_LIFETIME),
            },
            Ok(_) => {
                return Err(Status::failed_precondition(
                    "identity returned no GitHub token for this workspace",
                ));
            }
            Err(error)
                if matches!(
                    error.status.code(),
                    Code::Unavailable | Code::DeadlineExceeded | Code::Unimplemented
                ) =>
            {
                match &self.dev_token {
                    Some(token) => {
                        tracing::warn!(
                            code = ?error.status.code(),
                            "identity unreachable; publishing with GITHUB_TOKEN (dev mode)"
                        );
                        GithubAccess {
                            token: token.clone(),
                            api_base_url: PUBLIC_API.to_owned(),
                            expires_at: now + DEV_RETRY + REFRESH_MARGIN,
                        }
                    }
                    None => {
                        return Err(Status::failed_precondition(format!(
                            "GitHub access is not configured: identity unreachable ({}) and GITHUB_TOKEN is not set",
                            error.status.message()
                        )));
                    }
                }
            }
            Err(error) => {
                return Err(Status::failed_precondition(format!(
                    "no GitHub access for this workspace: {}",
                    error.status.message()
                )));
            }
        };
        self.lock().insert(workspace, access.clone());
        Ok(access)
    }

    /// Drops a cached token (GitHub rejected it).
    pub fn forget(&self, workspace: Uuid) {
        self.lock().remove(&workspace);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, GithubAccess>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// One inline comment of a review: `side` is always `RIGHT` (the new
/// file) and a multi-line range carries `start_line`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewComment {
    pub path: String,
    pub line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    pub side: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_side: Option<String>,
    pub body: String,
}

/// The request body of `POST /repos/{owner}/{repo}/pulls/{number}/reviews`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewPost {
    pub commit_id: String,
    pub body: String,
    /// "COMMENT" | "REQUEST_CHANGES" | "APPROVE"
    pub event: String,
    pub comments: Vec<ReviewComment>,
}

/// What GitHub answered.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PostedReview {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub html_url: String,
}

/// The platform's `comment` / `request_changes` / `approve` to GitHub's event.
pub fn review_event(raw: &str) -> Result<&'static str, Status> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "comment" => Ok("COMMENT"),
        "request_changes" | "request-changes" => Ok("REQUEST_CHANGES"),
        "approve" => Ok("APPROVE"),
        other => Err(Status::invalid_argument(format!(
            "event must be \"comment\", \"request_changes\" or \"approve\", not \"{other}\""
        ))),
    }
}

pub struct GithubPublisher {
    http: reqwest::Client,
}

impl Default for GithubPublisher {
    fn default() -> Self {
        Self::new()
    }
}

impl GithubPublisher {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("reqwest client"),
        }
    }

    /// Posts the review. GitHub's status codes become gRPC statuses the
    /// caller can show: 401 → unauthenticated (the token is dropped by the
    /// caller), 403 → permission denied (scope), 404 → not found, 422 →
    /// failed precondition (a comment outside the diff, a stale head).
    pub async fn post_review(
        &self,
        access: &GithubAccess,
        repo_full_name: &str,
        number: u64,
        post: &ReviewPost,
    ) -> Result<PostedReview, Status> {
        let url = format!(
            "{}/repos/{}/pulls/{}/reviews",
            access.api_base_url, repo_full_name, number
        );
        let response = self
            .http
            .post(&url)
            .bearer_auth(&access.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(post)
            .send()
            .await
            .map_err(|e| Status::unavailable(format!("GitHub request failed: {e}")))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if status.is_success() {
            return serde_json::from_str(&text).map_err(|e| {
                Status::unavailable(format!("could not decode GitHub's answer: {e}"))
            });
        }
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| {
                let base = v.get("message")?.as_str()?.to_owned();
                let errors = v
                    .get("errors")
                    .and_then(|e| e.as_array())
                    .map(|errs| {
                        errs.iter()
                            .filter_map(|e| {
                                e.as_str()
                                    .map(str::to_owned)
                                    .or_else(|| e.get("message")?.as_str().map(str::to_owned))
                            })
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .filter(|s| !s.is_empty());
                Some(match errors {
                    Some(errors) => format!("{base}: {errors}"),
                    None => base,
                })
            })
            .unwrap_or_else(|| status.to_string());
        Err(match status.as_u16() {
            401 => Status::unauthenticated(format!("GitHub rejected the token: {message}")),
            403 => Status::permission_denied(format!(
                "GitHub forbade the review ({message}); the token needs pull_requests: write"
            )),
            404 => Status::not_found(format!("GitHub: {message}")),
            422 => {
                Status::failed_precondition(format!("GitHub could not post the review: {message}"))
            }
            _ => Status::unavailable(format!("GitHub answered {status}: {message}")),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;
    use crate::gateway::fake::FakeIdentity;

    /// A one-shot HTTP/1.1 server that records the request and answers
    /// `status` with `body`.
    async fn fake_github(
        status: u16,
        body: &'static str,
    ) -> (
        String,
        tokio::sync::oneshot::Receiver<(String, String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = socket.read(&mut chunk).await.unwrap();
                if n == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buffer).into_owned();
                if let Some(end) = text.find("\r\n\r\n") {
                    let headers = &text[..end];
                    let length: usize = headers
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if buffer.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&buffer).into_owned();
            let end = text.find("\r\n\r\n").unwrap();
            let (head, body_text) = (text[..end].to_owned(), text[end + 4..].to_owned());
            let request_line = head.lines().next().unwrap().to_owned();
            let reason = if status == 200 { "OK" } else { "Error" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
            let _ = tx.send((request_line, head, body_text));
        });
        (format!("http://{addr}"), rx)
    }

    fn access(base: &str) -> GithubAccess {
        GithubAccess {
            token: "ghs_test".into(),
            api_base_url: base.to_owned(),
            expires_at: Utc::now() + Duration::hours(1),
        }
    }

    #[tokio::test]
    async fn posts_the_review_in_githubs_shape() {
        let (base, seen) = fake_github(
            200,
            r#"{"id": 9, "html_url": "https://github.com/o/r/pull/1#pullrequestreview-9"}"#,
        )
        .await;
        let post = ReviewPost {
            commit_id: "abc123".into(),
            body: "## Intent\n\nx\n\n---\nReviewed by SDI".into(),
            event: "COMMENT".into(),
            comments: vec![ReviewComment {
                path: "src/main.rs".into(),
                line: 12,
                start_line: None,
                side: "RIGHT".into(),
                start_side: None,
                body: "**[high] Secret** (secrets.aws_key)\n\n```suggestion\nlet x = env();\n```"
                    .into(),
            }],
        };
        let posted = GithubPublisher::new()
            .post_review(&access(&base), "o/r", 1, &post)
            .await
            .unwrap();
        assert_eq!(posted.id, 9);
        assert!(posted.html_url.ends_with("pullrequestreview-9"));

        let (request_line, head, body) = seen.await.unwrap();
        assert_eq!(request_line, "POST /repos/o/r/pulls/1/reviews HTTP/1.1");
        let lower = head.to_ascii_lowercase();
        assert!(lower.contains("authorization: bearer ghs_test"));
        assert!(lower.contains("accept: application/vnd.github+json"));
        assert!(lower.contains("x-github-api-version: 2022-11-28"));
        assert!(lower.contains("user-agent: sdi-intelligence"));
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["commit_id"], "abc123");
        assert_eq!(json["event"], "COMMENT");
        assert!(json["body"].as_str().unwrap().ends_with("Reviewed by SDI"));
        assert_eq!(json["comments"][0]["path"], "src/main.rs");
        assert_eq!(json["comments"][0]["line"], 12);
        assert_eq!(json["comments"][0]["side"], "RIGHT");
        assert!(json["comments"][0].get("start_line").is_none());
        assert!(
            json["comments"][0]["body"]
                .as_str()
                .unwrap()
                .contains("```suggestion")
        );
    }

    #[tokio::test]
    async fn github_errors_map_to_statuses() {
        let (base, _) = fake_github(422, r#"{"message":"Unprocessable Entity","errors":["Pull request review thread line must be part of the diff"]}"#).await;
        let post = ReviewPost {
            commit_id: "abc".into(),
            body: "x".into(),
            event: "APPROVE".into(),
            comments: vec![],
        };
        let error = GithubPublisher::new()
            .post_review(&access(&base), "o/r", 2, &post)
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::FailedPrecondition);
        assert!(
            error.message().contains("part of the diff"),
            "{}",
            error.message()
        );

        let (base, _) = fake_github(
            403,
            r#"{"message":"Resource not accessible by integration"}"#,
        )
        .await;
        let error = GithubPublisher::new()
            .post_review(&access(&base), "o/r", 2, &post)
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::PermissionDenied);
        assert!(error.message().contains("pull_requests: write"));
    }

    #[test]
    fn events_map_to_githubs_names() {
        assert_eq!(review_event("").unwrap(), "COMMENT");
        assert_eq!(review_event("comment").unwrap(), "COMMENT");
        assert_eq!(review_event("request_changes").unwrap(), "REQUEST_CHANGES");
        assert_eq!(review_event("Approve").unwrap(), "APPROVE");
        assert_eq!(
            review_event("merge").unwrap_err().code(),
            Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn tokens_come_from_identity_and_are_cached() {
        let identity = Arc::new(FakeIdentity::default());
        let workspace = Uuid::now_v7();
        let principal = Principal::system(workspace.to_string());
        let tokens = GithubTokens::new(Some("ghp_dev".into()));
        // Identity answers NOT_FOUND: not an outage, so no dev fallback.
        let error = tokens
            .access(identity.as_ref(), &principal, workspace)
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::FailedPrecondition);

        identity.tokens.lock().unwrap().insert(
            workspace.to_string(),
            ("ghs_live".into(), "https://ghe.example.com/api/v3/".into()),
        );
        let access = tokens
            .access(identity.as_ref(), &principal, workspace)
            .await
            .unwrap();
        assert_eq!(access.token, "ghs_live");
        assert_eq!(access.api_base_url, "https://ghe.example.com/api/v3");
        // Cached: a changed identity answer is not seen until the token ages out.
        identity.tokens.lock().unwrap().clear();
        assert_eq!(
            tokens
                .access(identity.as_ref(), &principal, workspace)
                .await
                .unwrap()
                .token,
            "ghs_live"
        );
        tokens.forget(workspace);
        assert!(
            tokens
                .access(identity.as_ref(), &principal, workspace)
                .await
                .is_err()
        );
    }
}
