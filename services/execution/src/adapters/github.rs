//! Writes to the connected repository with `GITHUB_TOKEN` through the Git
//! Data API. `repo.open_pull_request` branches off the default branch (or
//! reuses the branch when it exists), commits the execution's artifacts as
//! blobs → tree → commit, moves the branch ref and opens the pull request.
//! The step is idempotent on content, not on the branch: a retry whose files
//! already sit at the branch head commits nothing and returns the open pull
//! request, while a later execution of the same proposal with regenerated
//! files commits them on top of the branch and reuses that pull request
//! (the branch is `sdi/<proposal>` for every execution of a proposal). A
//! 403 is reported as PERMISSION_DENIED with the token scope the remediation
//! has to explain.

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use super::{Adapter, GITHUB, RunContext, StepInput, StepOutcome};
use crate::domain::diagnosis;

pub const DEFAULT_API: &str = "https://api.github.com";
const STEP_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const USER_AGENT: &str = concat!("sdi-execution/", env!("CARGO_PKG_VERSION"));
/// What the remediation tells the human when GitHub refuses the write.
const SCOPE_HINT: &str = "GITHUB_TOKEN needs write access to the repository: the `repo` scope on a classic token, or Contents + Pull requests \"Read and write\" on a fine-grained token";
/// GitHub answers 404 (not 403) to a tree that touches `.github/workflows/`
/// when the token lacks the `workflow` scope; without the hint the failure
/// reads like a missing repository.
const WORKFLOW_SCOPE_HINT: &str = "GitHub refuses to write files under .github/workflows/ with this token: a classic token needs the `workflow` scope (`gh auth refresh -s workflow`), a fine-grained token needs Workflows \"Read and write\"";

pub struct GithubAdapter {
    token: Option<String>,
    api: String,
    http: reqwest::Client,
}

enum ApiError {
    Status {
        method: Method,
        path: String,
        status: StatusCode,
        message: String,
    },
    Transport(String),
}

impl ApiError {
    fn into_outcome(self) -> StepOutcome {
        match self {
            ApiError::Status {
                method,
                path,
                status,
                message,
            } => {
                let raw = format!("{method} {path} -> {status}: {message}");
                match status {
                    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => StepOutcome::Failed {
                        code: diagnosis::PERMISSION_DENIED.to_owned(),
                        raw_output: format!("{raw}\n{SCOPE_HINT}"),
                        missing_permissions: vec!["repository:write".to_owned()],
                    },
                    StatusCode::TOO_MANY_REQUESTS => {
                        StepOutcome::failed(diagnosis::QUOTA_EXCEEDED, raw)
                    }
                    _ => StepOutcome::failed(diagnosis::UNKNOWN, raw),
                }
            }
            ApiError::Transport(message) => {
                if message.contains("timed out") {
                    StepOutcome::failed(diagnosis::TIMEOUT, message)
                } else {
                    StepOutcome::failed(diagnosis::UNKNOWN, message)
                }
            }
        }
    }
}

impl GithubAdapter {
    pub fn new(token: Option<String>, api: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest client");
        Self {
            token,
            api: api.trim_end_matches('/').to_owned(),
            http,
        }
    }

    /// One API call. Returns the status with the JSON body (`Null` when the
    /// body is empty); non-2xx statuses other than those in `allow` are
    /// errors so the caller can treat "404 on a ref" as "create it".
    async fn call(
        &self,
        ctx: &RunContext,
        method: Method,
        path: &str,
        body: Option<Value>,
        allow: &[StatusCode],
    ) -> Result<(StatusCode, Value), ApiError> {
        let (token, api) = self.github_for(ctx);
        let url = format!("{api}{path}");
        let mut request = self
            .http
            .request(method.clone(), &url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|e| ApiError::Transport(format!("{method} {url} failed: {e}")))?;
        let status = response.status();
        ctx.logs.line(format!("{method} {path} -> {status}"));
        let text = response
            .text()
            .await
            .map_err(|e| ApiError::Transport(format!("{method} {url}: {e}")))?;
        let value: Value = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text.clone()))
        };
        if status.is_success() || allow.contains(&status) {
            return Ok((status, value));
        }
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| super::truncate_line(text.trim()), str::to_owned);
        Err(ApiError::Status {
            method,
            path: path.to_owned(),
            status,
            message,
        })
    }

    /// The token and API base a step writes with: the workspace's
    /// (identity-minted, GHES-aware) when the context carries them, else
    /// the process-wide `GITHUB_TOKEN` / `GITHUB_API_URL`.
    fn github_for<'a>(&'a self, ctx: &'a RunContext) -> (Option<&'a str>, &'a str) {
        match &ctx.github {
            Some(access) => (Some(access.token.as_str()), access.api_base_url.as_str()),
            None => (self.token.as_deref(), self.api.as_str()),
        }
    }

    async fn open_pull_request(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        if self.github_for(ctx).0.is_none() {
            return StepOutcome::Failed {
                code: diagnosis::PERMISSION_DENIED.to_owned(),
                raw_output: format!("GITHUB_TOKEN is not configured; {SCOPE_HINT}"),
                missing_permissions: vec!["repository:write".to_owned()],
            };
        }
        let Some(repo) = step
            .params
            .get("repo")
            .or_else(|| step.params.get("repository"))
            .map(|s| s.trim().trim_matches('/').to_owned())
            .filter(|s| s.split('/').count() == 2)
        else {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                "params.repo (owner/name) is required for repo.open_pull_request",
            );
        };
        let owner = repo.split('/').next().unwrap_or_default().to_owned();
        let branch = step
            .params
            .get("branch")
            .map(|b| b.trim().trim_matches('/').to_owned())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| {
                if ctx.proposal_id.trim().is_empty() {
                    format!("sdi/{}", ctx.short_id())
                } else {
                    format!("sdi/{}", ctx.proposal_id.trim())
                }
            });
        let title = step
            .params
            .get("title")
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .map_or_else(
                || format!("Software Delivery Intelligence: {}", step.title),
                str::to_owned,
            );
        let mut body = step
            .params
            .get("body")
            .map(|b| b.trim().to_owned())
            .unwrap_or_default();
        if !ctx.artifacts.is_empty() {
            body.push_str("\n\n## Generated files\n");
            for path in ctx.artifacts.keys() {
                body.push_str(&format!("- `{path}`\n"));
            }
        }
        body.push_str(&format!(
            "\nOpened by Software Delivery Intelligence execution #{}\n",
            ctx.execution_number
        ));
        let body = body.trim_start().to_owned();
        let message = step
            .params
            .get("commit_message")
            .map(|m| m.trim())
            .filter(|m| !m.is_empty())
            .map_or_else(|| title.clone(), str::to_owned);

        let work = self.publish(ctx, &repo, &owner, &branch, &title, &body, &message);
        match tokio::time::timeout(STEP_TIMEOUT, work).await {
            Ok(result) => result,
            Err(_) => StepOutcome::failed(
                diagnosis::TIMEOUT,
                "GitHub did not answer within the step's time limit",
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn publish(
        &self,
        ctx: &RunContext,
        repo: &str,
        owner: &str,
        branch: &str,
        title: &str,
        body: &str,
        message: &str,
    ) -> StepOutcome {
        match self
            .publish_inner(ctx, repo, owner, branch, title, body, message)
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => error.into_outcome(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn publish_inner(
        &self,
        ctx: &RunContext,
        repo: &str,
        owner: &str,
        branch: &str,
        title: &str,
        body: &str,
        message: &str,
    ) -> Result<StepOutcome, ApiError> {
        let base = format!("/repos/{repo}");
        let mut outputs = BTreeMap::from([("branch".to_owned(), branch.to_owned())]);

        // An open pull request for the branch is reused, never duplicated;
        // whether anything has to be committed is decided by content below.
        let (_, open) = self
            .call(
                ctx,
                Method::GET,
                &format!("{base}/pulls?state=open&head={owner}:{branch}"),
                None,
                &[],
            )
            .await?;
        let existing_pr = open
            .as_array()
            .and_then(|prs| prs.first())
            .map(|pr| str_field(pr, "html_url"))
            .filter(|url| !url.is_empty());
        if let Some(url) = &existing_pr {
            ctx.logs.line(format!("pull request already open: {url}"));
        }

        let (_, repository) = self.call(ctx, Method::GET, &base, None, &[]).await?;
        let default_branch = str_field(&repository, "default_branch");
        let default_branch = if default_branch.is_empty() {
            "main".to_owned()
        } else {
            default_branch
        };
        let (_, head) = self
            .call(
                ctx,
                Method::GET,
                &format!("{base}/git/ref/heads/{default_branch}"),
                None,
                &[],
            )
            .await?;
        let base_sha = str_field(head.get("object").unwrap_or(&Value::Null), "sha");

        // Reuse the branch when it exists (a previous attempt or execution
        // created it).
        let (status, existing) = self
            .call(
                ctx,
                Method::GET,
                &format!("{base}/git/ref/heads/{branch}"),
                None,
                &[StatusCode::NOT_FOUND],
            )
            .await?;
        let branch_existed = status != StatusCode::NOT_FOUND;
        let parent = if branch_existed {
            let sha = str_field(existing.get("object").unwrap_or(&Value::Null), "sha");
            ctx.logs
                .line(format!("reusing refs/heads/{branch} at {sha}"));
            sha
        } else {
            self.call(
                ctx,
                Method::POST,
                &format!("{base}/git/refs"),
                Some(json!({ "ref": format!("refs/heads/{branch}"), "sha": base_sha })),
                &[],
            )
            .await?;
            ctx.logs
                .line(format!("created refs/heads/{branch} from {default_branch}"));
            base_sha.clone()
        };

        let committed = if ctx.artifacts.is_empty() {
            ctx.logs.line("no artifacts to commit");
            None
        } else {
            self.commit_artifacts(ctx, &base, branch, &parent, branch_existed, message)
                .await?
        };
        if let Some(sha) = &committed {
            outputs.insert("commit".to_owned(), sha.clone());
        }

        if let Some(url) = existing_pr {
            outputs.insert("pull_request_url".to_owned(), url.clone());
            return Ok(StepOutcome::Succeeded {
                output: format!(
                    "pull request for {branch} already open: {url} ({} files{})",
                    ctx.artifacts.len(),
                    committed.map_or(String::new(), |sha| format!(", at commit {sha}"))
                ),
                outputs,
            });
        }

        let (_, pr) = self
            .call(
                ctx,
                Method::POST,
                &format!("{base}/pulls"),
                Some(json!({
                    "title": title,
                    "head": branch,
                    "base": default_branch,
                    "body": body,
                })),
                &[],
            )
            .await?;
        let url = str_field(&pr, "html_url");
        outputs.insert("pull_request_url".to_owned(), url.clone());
        ctx.logs.line(format!("opened {url}"));
        Ok(StepOutcome::Succeeded {
            output: format!(
                "opened {url} ({branch} -> {default_branch}, {} files{})",
                ctx.artifacts.len(),
                committed.map_or(String::new(), |sha| format!(", commit {sha}"))
            ),
            outputs,
        })
    }

    /// Commits the artifacts on top of `parent` and moves the branch, unless
    /// the branch head already holds every file with the same content: git
    /// addresses blobs by content, so the blob shas the API hands back are
    /// compared with the head tree's entries. Returns the commit the
    /// artifacts sit at (the new one, or the head when nothing changed).
    async fn commit_artifacts(
        &self,
        ctx: &RunContext,
        base: &str,
        branch: &str,
        parent: &str,
        branch_existed: bool,
        message: &str,
    ) -> Result<Option<String>, ApiError> {
        let (_, commit) = self
            .call(
                ctx,
                Method::GET,
                &format!("{base}/git/commits/{parent}"),
                None,
                &[],
            )
            .await?;
        let base_tree = str_field(commit.get("tree").unwrap_or(&Value::Null), "sha");

        let mut entries = Vec::with_capacity(ctx.artifacts.len());
        let mut blobs: BTreeMap<String, String> = BTreeMap::new();
        for (path, content) in &ctx.artifacts {
            let (_, blob) = self
                .call(
                    ctx,
                    Method::POST,
                    &format!("{base}/git/blobs"),
                    Some(json!({ "content": content, "encoding": "utf-8" })),
                    &[],
                )
                .await?;
            let sha = str_field(&blob, "sha");
            let path = path.trim_start_matches('/').to_owned();
            entries.push(json!({
                "path": path,
                "mode": "100644",
                "type": "blob",
                "sha": sha,
            }));
            blobs.insert(path, sha);
        }

        if branch_existed && self.tree_holds(ctx, base, &base_tree, &blobs).await? {
            ctx.logs.line(format!(
                "{} files already at {parent} on {branch}; nothing to commit",
                blobs.len()
            ));
            return Ok(Some(parent.to_owned()));
        }

        let touches_workflows = blobs.keys().any(|p| p.starts_with(".github/workflows/"));
        let (_, tree) = match self
            .call(
                ctx,
                Method::POST,
                &format!("{base}/git/trees"),
                Some(json!({ "base_tree": base_tree, "tree": entries })),
                &[],
            )
            .await
        {
            Ok(tree) => tree,
            Err(ApiError::Status {
                status: StatusCode::NOT_FOUND,
                method,
                path,
                message,
            }) if touches_workflows => {
                return Err(ApiError::Status {
                    method,
                    path,
                    status: StatusCode::FORBIDDEN,
                    message: format!(
                        "{message} (404 on a tree with workflow files); {WORKFLOW_SCOPE_HINT}"
                    ),
                });
            }
            Err(error) => return Err(error),
        };
        let (_, commit) = self
            .call(
                ctx,
                Method::POST,
                &format!("{base}/git/commits"),
                Some(json!({
                    "message": message,
                    "tree": str_field(&tree, "sha"),
                    "parents": [parent],
                })),
                &[],
            )
            .await?;
        let sha = str_field(&commit, "sha");
        self.call(
            ctx,
            Method::PATCH,
            &format!("{base}/git/refs/heads/{branch}"),
            Some(json!({ "sha": sha, "force": false })),
            &[],
        )
        .await?;
        ctx.logs.line(format!(
            "committed {} files as {sha} on {branch}",
            ctx.artifacts.len()
        ));
        Ok(Some(sha))
    }

    /// Whether `tree` already contains every `path -> blob sha` in `blobs`.
    /// A tree too large for one recursive listing is reported truncated by
    /// GitHub; then nothing is assumed and the files are committed again.
    async fn tree_holds(
        &self,
        ctx: &RunContext,
        base: &str,
        tree: &str,
        blobs: &BTreeMap<String, String>,
    ) -> Result<bool, ApiError> {
        let (_, listing) = self
            .call(
                ctx,
                Method::GET,
                &format!("{base}/git/trees/{tree}?recursive=1"),
                None,
                &[],
            )
            .await?;
        if listing
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(false);
        }
        let Some(entries) = listing.get("tree").and_then(Value::as_array) else {
            return Ok(false);
        };
        let present: BTreeMap<String, String> = entries
            .iter()
            .filter(|e| str_field(e, "type") == "blob")
            .map(|e| (str_field(e, "path"), str_field(e, "sha")))
            .collect();
        Ok(blobs
            .iter()
            .all(|(path, sha)| present.get(path) == Some(sha)))
    }
}

fn str_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

#[async_trait]
impl Adapter for GithubAdapter {
    fn name(&self) -> &'static str {
        GITHUB
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        match step.action_type.as_str() {
            "repo.open_pull_request" => self.open_pull_request(step, ctx).await,
            other => StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("the github adapter cannot run {other}"),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use axum::{
        Json, Router,
        extract::{Path, Query, State},
        http::StatusCode,
        routing::{get, post},
    };
    use uuid::Uuid;

    use super::*;

    /// A fake GitHub that records every call and can be told what exists.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        branch_exists: bool,
        pr_exists: bool,
        forbid_writes: bool,
        /// GitHub without the `workflow` scope: trees touching
        /// `.github/workflows/` answer 404, everything else succeeds.
        no_workflow_scope: bool,
        /// Blobs at the head of an existing branch, as `GET /git/trees/{sha}`
        /// lists them: (path, sha). The fake's blob shas are `blob-<len>`.
        head_tree: Vec<(&'static str, &'static str)>,
    }

    type Shared = Arc<Fake>;

    fn record(fake: &Fake, line: String) {
        fake.calls.lock().unwrap().push(line);
    }

    async fn repo(State(fake): State<Shared>) -> Json<Value> {
        record(&fake, "GET repo".into());
        Json(json!({ "default_branch": "main" }))
    }

    async fn get_ref(
        State(fake): State<Shared>,
        Path((_o, _r, reference)): Path<(String, String, String)>,
    ) -> (StatusCode, Json<Value>) {
        record(&fake, format!("GET ref {reference}"));
        if reference == "heads/main" {
            return (
                StatusCode::OK,
                Json(json!({ "object": { "sha": "base000" } })),
            );
        }
        if fake.branch_exists {
            return (
                StatusCode::OK,
                Json(json!({ "object": { "sha": "head111" } })),
            );
        }
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "message": "Not Found" })),
        )
    }

    async fn create_ref(
        State(fake): State<Shared>,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        record(
            &fake,
            format!("POST refs {}", body["ref"].as_str().unwrap()),
        );
        if fake.forbid_writes {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "message": "Resource not accessible by integration" })),
            );
        }
        (
            StatusCode::CREATED,
            Json(json!({ "object": { "sha": body["sha"] } })),
        )
    }

    async fn get_commit(
        State(fake): State<Shared>,
        Path((_o, _r, sha)): Path<(String, String, String)>,
    ) -> Json<Value> {
        record(&fake, format!("GET commit {sha}"));
        Json(json!({ "sha": sha, "tree": { "sha": format!("tree-of-{sha}") } }))
    }

    async fn get_tree(
        State(fake): State<Shared>,
        Path((_o, _r, sha)): Path<(String, String, String)>,
    ) -> Json<Value> {
        record(&fake, format!("GET tree {sha}"));
        let tree: Vec<Value> = fake
            .head_tree
            .iter()
            .map(|(path, sha)| json!({ "path": path, "type": "blob", "sha": sha }))
            .collect();
        Json(json!({ "sha": sha, "truncated": false, "tree": tree }))
    }

    async fn create_blob(
        State(fake): State<Shared>,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        let content = body["content"].as_str().unwrap().to_owned();
        record(&fake, format!("POST blob {}", content.len()));
        (
            StatusCode::CREATED,
            Json(json!({ "sha": format!("blob-{}", content.len()) })),
        )
    }

    async fn create_tree(
        State(fake): State<Shared>,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        let paths: Vec<String> = body["tree"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["path"].as_str().unwrap().to_owned())
            .collect();
        record(
            &fake,
            format!(
                "POST tree base={} {}",
                body["base_tree"].as_str().unwrap(),
                paths.join(",")
            ),
        );
        if fake.no_workflow_scope && paths.iter().any(|p| p.starts_with(".github/workflows/")) {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "message": "Not Found" })),
            );
        }
        (StatusCode::CREATED, Json(json!({ "sha": "tree222" })))
    }

    async fn create_commit(
        State(fake): State<Shared>,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        record(
            &fake,
            format!(
                "POST commit tree={} parent={}",
                body["tree"].as_str().unwrap(),
                body["parents"][0].as_str().unwrap()
            ),
        );
        (StatusCode::CREATED, Json(json!({ "sha": "commit333" })))
    }

    async fn update_ref(
        State(fake): State<Shared>,
        Path((_o, _r, reference)): Path<(String, String, String)>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        record(
            &fake,
            format!("PATCH ref {reference} -> {}", body["sha"].as_str().unwrap()),
        );
        Json(json!({ "object": { "sha": body["sha"] } }))
    }

    async fn pulls(
        State(fake): State<Shared>,
        Query(query): Query<HashMap<String, String>>,
    ) -> Json<Value> {
        record(&fake, format!("GET pulls head={}", query["head"]));
        if fake.pr_exists {
            return Json(json!([{ "html_url": "https://github.com/o/r/pull/7", "number": 7 }]));
        }
        Json(json!([]))
    }

    async fn create_pull(
        State(fake): State<Shared>,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        record(
            &fake,
            format!(
                "POST pulls {} -> {} body={}",
                body["head"].as_str().unwrap(),
                body["base"].as_str().unwrap(),
                body["body"].as_str().unwrap().replace('\n', "|")
            ),
        );
        (
            StatusCode::CREATED,
            Json(json!({ "html_url": "https://github.com/o/r/pull/42", "number": 42 })),
        )
    }

    async fn serve(fake: Fake) -> (Shared, String) {
        serve_at(fake, "").await
    }

    /// The fake under `prefix` (`/api/v3` for an enterprise host).
    async fn serve_at(fake: Fake, prefix: &'static str) -> (Shared, String) {
        let fake = Arc::new(fake);
        let routes = Router::new()
            .route("/repos/{o}/{r}", get(repo))
            .route("/repos/{o}/{r}/git/ref/{*reference}", get(get_ref))
            .route("/repos/{o}/{r}/git/refs", post(create_ref))
            .route(
                "/repos/{o}/{r}/git/refs/{*reference}",
                axum::routing::patch(update_ref),
            )
            .route("/repos/{o}/{r}/git/commits/{sha}", get(get_commit))
            .route("/repos/{o}/{r}/git/trees/{sha}", get(get_tree))
            .route("/repos/{o}/{r}/git/blobs", post(create_blob))
            .route("/repos/{o}/{r}/git/trees", post(create_tree))
            .route("/repos/{o}/{r}/git/commits", post(create_commit))
            .route("/repos/{o}/{r}/pulls", get(pulls).post(create_pull))
            .with_state(Arc::clone(&fake));
        let app = if prefix.is_empty() {
            routes
        } else {
            Router::new().nest(prefix, routes)
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (fake, format!("http://{addr}"))
    }

    fn ctx(artifacts: &[(&str, &str)]) -> RunContext {
        RunContext {
            execution_id: Uuid::now_v7(),
            execution_number: 8241,
            proposal_id: "prop-1".into(),
            artifacts: artifacts
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..Default::default()
        }
    }

    fn step(params: &[(&str, &str)]) -> StepInput {
        StepInput {
            key: "pr".into(),
            title: "Open pull request".into(),
            action_type: "repo.open_pull_request".into(),
            adapter: GITHUB.into(),
            params: params
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            attempt: 1,
        }
    }

    /// An organisation on GitHub Enterprise Server: identity minted the
    /// token and the `/api/v3` base, which the context carries; the
    /// process-wide defaults (dead port, no token) are never used.
    #[tokio::test]
    async fn writes_through_the_workspace_api_base_on_enterprise_hosts() {
        let (fake, base) = serve_at(Fake::default(), "/api/v3").await;
        let adapter = GithubAdapter::new(None, "http://127.0.0.1:1");
        let mut ctx = ctx(&[("Dockerfile", "FROM scratch\n")]);
        ctx.github = Some(super::super::GithubAccess {
            token: "ghs_enterprise".into(),
            api_base_url: format!("{base}/api/v3"),
        });
        let outcome = adapter
            .run(&step(&[("repo", "o/r"), ("title", "Containerize")]), &ctx)
            .await;
        let StepOutcome::Succeeded { outputs, .. } = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(
            outputs["pull_request_url"],
            "https://github.com/o/r/pull/42"
        );
        let calls = fake.calls.lock().unwrap().clone();
        assert!(!calls.is_empty());

        // Without any access at all the step is PERMISSION_DENIED with the
        // scope hint, before a single request.
        let outcome = adapter
            .run(&step(&[("repo", "o/r")]), &ctx_without_github(&ctx))
            .await;
        assert!(
            matches!(outcome, StepOutcome::Failed { ref code, .. } if code == diagnosis::PERMISSION_DENIED),
            "{outcome:?}"
        );
    }

    fn ctx_without_github(ctx: &RunContext) -> RunContext {
        RunContext {
            github: None,
            ..ctx.clone()
        }
    }

    #[tokio::test]
    async fn opens_a_pull_request_through_blobs_tree_commit_and_ref() {
        let (fake, api) = serve(Fake::default()).await;
        let adapter = GithubAdapter::new(Some("t0k3n".into()), &api);
        let ctx = ctx(&[
            ("Dockerfile", "FROM scratch\n"),
            (".dockerignore", "target\n"),
        ]);
        let outcome = adapter
            .run(
                &step(&[
                    ("repo", "o/r"),
                    ("title", "Containerize"),
                    ("body", "Adds a Dockerfile."),
                ]),
                &ctx,
            )
            .await;
        let StepOutcome::Succeeded { output, outputs } = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(
            outputs["pull_request_url"],
            "https://github.com/o/r/pull/42"
        );
        assert_eq!(outputs["branch"], "sdi/prop-1");
        assert_eq!(outputs["commit"], "commit333");
        assert!(output.contains("pull/42"));

        let calls = fake.calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![
                "GET pulls head=o:sdi/prop-1",
                "GET repo",
                "GET ref heads/main",
                "GET ref heads/sdi/prop-1",
                "POST refs refs/heads/sdi/prop-1",
                "GET commit base000",
                "POST blob 7",
                "POST blob 13",
                "POST tree base=tree-of-base000 .dockerignore,Dockerfile",
                "POST commit tree=tree222 parent=base000",
                "PATCH ref heads/sdi/prop-1 -> commit333",
                "POST pulls sdi/prop-1 -> main body=Adds a Dockerfile.||## Generated files|- `.dockerignore`|- `Dockerfile`||Opened by Software Delivery Intelligence execution #8241|",
            ]
        );
    }

    #[tokio::test]
    async fn reuses_an_existing_branch_and_returns_an_existing_pull_request() {
        let (fake, api) = serve(Fake {
            branch_exists: true,
            ..Default::default()
        })
        .await;
        let adapter = GithubAdapter::new(Some("t0k3n".into()), &api);
        let outcome = adapter
            .run(
                &step(&[("repo", "o/r"), ("branch", "feature/x")]),
                &ctx(&[("a.txt", "a")]),
            )
            .await;
        assert!(
            matches!(outcome, StepOutcome::Succeeded { .. }),
            "{outcome:?}"
        );
        let calls = fake.calls.lock().unwrap().clone();
        assert!(calls.contains(&"GET ref heads/feature/x".to_string()));
        assert!(!calls.iter().any(|c| c.starts_with("POST refs")));
        assert!(calls.contains(&"POST commit tree=tree222 parent=head111".to_string()));

        // The same files are already at the head: a retry commits nothing
        // and reports the open pull request with the head as the commit.
        let (fake, api) = serve(Fake {
            branch_exists: true,
            pr_exists: true,
            head_tree: vec![("a.txt", "blob-1")],
            ..Default::default()
        })
        .await;
        let adapter = GithubAdapter::new(Some("t0k3n".into()), &api);
        let outcome = adapter
            .run(&step(&[("repo", "o/r")]), &ctx(&[("a.txt", "a")]))
            .await;
        let StepOutcome::Succeeded { outputs, .. } = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(outputs["pull_request_url"], "https://github.com/o/r/pull/7");
        assert_eq!(outputs["commit"], "head111");
        let calls = fake.calls.lock().unwrap().clone();
        assert!(
            !calls.iter().any(|c| {
                c.starts_with("POST tree")
                    || c.starts_with("POST commit")
                    || c.starts_with("PATCH ref")
                    || c.starts_with("POST pulls")
                    || c.starts_with("POST refs")
            }),
            "nothing written on a retry: {calls:?}"
        );
    }

    /// A later execution of the same proposal regenerates the files: they
    /// go on top of the branch and the open pull request is reused.
    #[tokio::test]
    async fn changed_artifacts_are_committed_onto_an_open_pull_request() {
        let (fake, api) = serve(Fake {
            branch_exists: true,
            pr_exists: true,
            head_tree: vec![("Dockerfile", "blob-7")],
            ..Default::default()
        })
        .await;
        let adapter = GithubAdapter::new(Some("t0k3n".into()), &api);
        let outcome = adapter
            .run(
                &step(&[("repo", "o/r")]),
                &ctx(&[("Dockerfile", "FROM scratch\nUSER app\n")]),
            )
            .await;
        let StepOutcome::Succeeded { outputs, output } = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(outputs["pull_request_url"], "https://github.com/o/r/pull/7");
        assert_eq!(outputs["commit"], "commit333");
        assert!(output.contains("pull/7"), "{output}");
        let calls = fake.calls.lock().unwrap().clone();
        assert!(calls.contains(&"GET tree tree-of-head111".to_string()));
        assert!(calls.contains(&"POST commit tree=tree222 parent=head111".to_string()));
        assert!(calls.contains(&"PATCH ref heads/sdi/prop-1 -> commit333".to_string()));
        assert!(
            !calls.iter().any(|c| c.starts_with("POST pulls")),
            "{calls:?}"
        );
    }

    #[tokio::test]
    async fn a_forbidden_write_is_permission_denied_with_the_scope_hint() {
        let (_fake, api) = serve(Fake {
            forbid_writes: true,
            ..Default::default()
        })
        .await;
        let adapter = GithubAdapter::new(Some("read-only".into()), &api);
        match adapter
            .run(&step(&[("repo", "o/r")]), &ctx(&[("a", "b")]))
            .await
        {
            StepOutcome::Failed {
                code,
                raw_output,
                missing_permissions,
            } => {
                assert_eq!(code, diagnosis::PERMISSION_DENIED);
                assert_eq!(missing_permissions, vec!["repository:write".to_string()]);
                assert!(raw_output.contains("403"), "{raw_output}");
                assert!(raw_output.contains("repo"), "{raw_output}");
            }
            other => panic!("{other:?}"),
        }

        let adapter = GithubAdapter::new(None, &api);
        match adapter.run(&step(&[("repo", "o/r")]), &ctx(&[])).await {
            StepOutcome::Failed {
                code, raw_output, ..
            } => {
                assert_eq!(code, diagnosis::PERMISSION_DENIED);
                assert!(raw_output.contains("GITHUB_TOKEN is not configured"));
            }
            other => panic!("{other:?}"),
        }
        match adapter.run(&step(&[]), &ctx(&[])).await {
            StepOutcome::Failed { code, .. } => assert_eq!(code, diagnosis::PERMISSION_DENIED),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_workflow_file_without_the_workflow_scope_names_the_missing_scope() {
        let (_fake, api) = serve(Fake {
            no_workflow_scope: true,
            ..Default::default()
        })
        .await;
        let adapter = GithubAdapter::new(Some("repo-only".into()), &api);
        match adapter
            .run(
                &step(&[("repo", "o/r")]),
                &ctx(&[
                    (".github/workflows/ci.yml", "name: CI"),
                    ("Dockerfile", "FROM x"),
                ]),
            )
            .await
        {
            StepOutcome::Failed {
                code,
                raw_output,
                missing_permissions,
            } => {
                assert_eq!(code, diagnosis::PERMISSION_DENIED);
                assert_eq!(missing_permissions, vec!["repository:write".to_string()]);
                assert!(raw_output.contains("workflow"), "{raw_output}");
                assert!(raw_output.contains("404"), "{raw_output}");
            }
            other => panic!("{other:?}"),
        }
        // Without workflow files the same token commits fine.
        match adapter
            .run(&step(&[("repo", "o/r")]), &ctx(&[("Dockerfile", "FROM x")]))
            .await
        {
            StepOutcome::Succeeded { outputs, .. } => {
                assert!(outputs.contains_key("pull_request_url"));
            }
            other => panic!("{other:?}"),
        }
    }
}
