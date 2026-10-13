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
//!
//! Deployment runs (CONTRACT.md §22) use two more operations, named by the
//! step's `params.operation`: `workflow.dispatch` triggers the environment's
//! deploy workflow with the release as inputs and follows the run it
//! started to its conclusion, and `deployment.record` creates a GitHub
//! Deployment with a `success` status so the repository shows what runs in
//! which environment.
//!
//! The pull request autopilot (CONTRACT.md §23) adds five operations that
//! live in `super::pull_requests`: committing review fixes, checking that a
//! pull request can be merged, merging it, deleting its branch and turning
//! on Dependabot. They go through [`GithubAdapter::call`] like everything
//! here, so they reach GitHub as the workspace does.

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use super::{
    Adapter, GITHUB, RunContext, StepInput, StepOutcome, gitops::encode_path, jenkins::step_limit,
};
use crate::domain::diagnosis;

pub const DEFAULT_API: &str = "https://api.github.com";
pub(super) const STEP_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const USER_AGENT: &str = concat!("sdi-execution/", env!("CARGO_PKG_VERSION"));
/// What the remediation tells the human when GitHub refuses the write.
const SCOPE_HINT: &str = "GITHUB_TOKEN needs write access to the repository: the `repo` scope on a classic token, or Contents + Pull requests \"Read and write\" on a fine-grained token";
/// GitHub answers 404 (not 403) to a tree that touches `.github/workflows/`
/// when the token lacks the `workflow` scope; without the hint the failure
/// reads like a missing repository.
const WORKFLOW_SCOPE_HINT: &str = "GitHub refuses to write files under .github/workflows/ with this token: a classic token needs the `workflow` scope (`gh auth refresh -s workflow`), a fine-grained token needs Workflows \"Read and write\"";

/// `params.operation` of the deploy step of a `github_actions` environment.
pub const OP_WORKFLOW_DISPATCH: &str = "workflow.dispatch";
/// `params.operation` of the record step.
pub const OP_DEPLOYMENT_RECORD: &str = "deployment.record";
pub const DEFAULT_WORKFLOW: &str = "deploy.yml";
/// How long a dispatched workflow may run before the step gives up on it.
pub const DISPATCH_TIMEOUT_SECONDS: u64 = 30 * 60;
pub const MAX_DISPATCH_TIMEOUT_SECONDS: u64 = 6 * 60 * 60;
/// How long GitHub gets to create the run after accepting a dispatch.
const RUN_APPEARS_WITHIN: Duration = Duration::from_secs(90);
const DISPATCH_POLL: Duration = Duration::from_secs(5);
/// GitHub computes `mergeable` lazily: the first read of a pull request
/// often says `null`. `repo.verify_mergeable` asks again this often, for
/// this long in total, before saying it could not find out.
const MERGEABLE_POLL: Duration = Duration::from_secs(2);
const MERGEABLE_WAIT: Duration = Duration::from_secs(20);
/// Clock skew allowed between the dispatch and the run's `created_at`.
const DISPATCH_SKEW: chrono::Duration = chrono::Duration::seconds(5);
const ACTIONS_HINT: &str = "the GitHub token needs Actions \"Read and write\" on the repository (the `workflow` scope on a classic token; `actions: write` for a GitHub App installation)";
const DEPLOYMENTS_HINT: &str = "the GitHub token needs Deployments \"Read and write\" on the repository (the `repo` scope on a classic token; `deployments: write` for a GitHub App installation)";

pub struct GithubAdapter {
    token: Option<String>,
    api: String,
    http: reqwest::Client,
    /// Pause between polls of a dispatched workflow run.
    poll: Duration,
    /// How long a dispatched run may take to appear.
    appears_within: Duration,
    /// Pause between reads of a pull request whose mergeability GitHub has
    /// not computed yet, and how long to keep asking.
    pub(super) mergeable_poll: Duration,
    pub(super) mergeable_wait: Duration,
}

pub(super) enum ApiError {
    Status {
        method: Method,
        path: String,
        status: StatusCode,
        message: String,
    },
    Transport(String),
}

impl ApiError {
    /// `GET /repos/o/r -> 404 Not Found: Not Found`, or the transport error.
    pub(super) fn describe(&self) -> String {
        match self {
            ApiError::Status {
                method,
                path,
                status,
                message,
            } => format!("{method} {path} -> {status}: {message}"),
            ApiError::Transport(message) => message.clone(),
        }
    }

    pub(super) fn status(&self) -> Option<StatusCode> {
        match self {
            ApiError::Status { status, .. } => Some(*status),
            ApiError::Transport(_) => None,
        }
    }

    /// The outcome of a deployment step GitHub refused or failed: a refusal
    /// names the permission the identity would need and what the token
    /// lacks; everything else is the deployer failing.
    pub(super) fn into_deploy_outcome(self, permission: &str, hint: &str) -> StepOutcome {
        let raw = self.describe();
        match self.status() {
            Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) => StepOutcome::Failed {
                code: diagnosis::PERMISSION_DENIED.to_owned(),
                raw_output: format!("{raw}\n{hint}"),
                missing_permissions: vec![permission.to_owned()],
            },
            Some(StatusCode::TOO_MANY_REQUESTS) => {
                StepOutcome::failed(diagnosis::QUOTA_EXCEEDED, raw)
            }
            None if raw.contains("timed out") => StepOutcome::failed(diagnosis::TIMEOUT, raw),
            _ => StepOutcome::failed(diagnosis::DEPLOYER_FAILED, raw),
        }
    }

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
            poll: DISPATCH_POLL,
            appears_within: RUN_APPEARS_WITHIN,
            mergeable_poll: MERGEABLE_POLL,
            mergeable_wait: MERGEABLE_WAIT,
        }
    }

    /// A shorter wait for GitHub to compute a pull request's mergeability
    /// (tests).
    #[cfg(test)]
    pub fn with_mergeable_poll(mut self, poll: Duration, wait: Duration) -> Self {
        self.mergeable_poll = poll;
        self.mergeable_wait = wait;
        self
    }

    /// Shorter waits between polls and for a dispatched run to appear (tests).
    #[cfg(test)]
    pub fn with_poll(mut self, poll: Duration, appears_within: Duration) -> Self {
        self.poll = poll;
        self.appears_within = appears_within;
        self
    }

    /// One API call. Returns the status with the JSON body (`Null` when the
    /// body is empty); non-2xx statuses other than those in `allow` are
    /// errors so the caller can treat "404 on a ref" as "create it".
    pub(super) async fn call(
        &self,
        ctx: &RunContext,
        method: Method,
        path: &str,
        body: Option<Value>,
        allow: &[StatusCode],
    ) -> Result<(StatusCode, Value), ApiError> {
        let (token, api) = self.github_for(ctx);
        let url = format!("{api}{path}");
        // What an error may quote: the address without its query string
        // (a query can carry a branch, a filter, a timestamp; never needed
        // to understand the failure).
        let bare = path.split('?').next().unwrap_or(path);
        let transport = |error: reqwest::Error| {
            // reqwest's own text names the full URL and leaves the cause
            // out: "timed out" is said here for the callers that look for
            // it, and the innermost cause ("connection refused") is added.
            if error.is_timeout() {
                return ApiError::Transport(format!("{method} {api}{bare} timed out"));
            }
            let mut cause = std::error::Error::source(&error);
            while let Some(deeper) = cause.and_then(std::error::Error::source) {
                cause = Some(deeper);
            }
            let cause = cause.map_or_else(String::new, |cause| format!(": {cause}"));
            ApiError::Transport(format!(
                "{method} {api}{bare} failed: {}{cause}",
                error.without_url()
            ))
        };
        let mut request = self
            .http
            .request(method.clone(), &url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        match body {
            Some(body) => request = request.json(&body),
            // A write without a body (turning a repository setting on) is
            // sent with an explicit zero length: over HTTP/1.1 the client
            // would send none at all, and GitHub answers that with 411.
            None if matches!(method, Method::PUT | Method::POST | Method::PATCH) => {
                request = request.header(reqwest::header::CONTENT_LENGTH, "0");
            }
            None => {}
        }
        let response = request.send().await.map_err(transport)?;
        let status = response.status();
        ctx.logs.line(format!("{method} {path} -> {status}"));
        let text = response.text().await.map_err(transport)?;
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
            path: bare.to_owned(),
            status,
            message,
        })
    }

    /// The token and API base a step writes with: the workspace's
    /// (identity-minted, GHES-aware) when the context carries them, else
    /// the process-wide `GITHUB_TOKEN` / `GITHUB_API_URL`.
    pub(super) fn github_for<'a>(&'a self, ctx: &'a RunContext) -> (Option<&'a str>, &'a str) {
        match &ctx.github {
            Some(access) => (Some(access.token.as_str()), access.api_base_url.as_str()),
            None => (self.token.as_deref(), self.api.as_str()),
        }
    }

    /// The default branch of `repo`.
    pub(super) async fn default_branch(
        &self,
        ctx: &RunContext,
        repo: &str,
    ) -> Result<String, ApiError> {
        let (_, repository) = self
            .call(ctx, Method::GET, &format!("/repos/{repo}"), None, &[])
            .await?;
        Ok(default_branch_of(&repository))
    }

    // --- deployment runs (CONTRACT.md §22) ----------------------------------

    /// Dispatches the environment's deploy workflow with the release as
    /// inputs, finds the run that dispatch created and follows it until it
    /// completes. The run's page is reported as `external_url` as soon as
    /// the run exists.
    async fn workflow_dispatch(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        if self.github_for(ctx).0.is_none() {
            return StepOutcome::Failed {
                code: diagnosis::PERMISSION_DENIED.to_owned(),
                raw_output: format!(
                    "no GitHub token is available for this workspace; {ACTIONS_HINT}"
                ),
                missing_permissions: vec!["pipeline:trigger".to_owned()],
            };
        }
        let param = |key: &str| step.params.get(key).map(|v| v.trim()).unwrap_or_default();
        let Some(repo) = repo_param(step) else {
            return StepOutcome::failed(
                diagnosis::DEPLOYER_FAILED,
                "params.repo (owner/name) is required to dispatch a workflow",
            );
        };
        let workflow = match param("workflow") {
            "" => DEFAULT_WORKFLOW,
            workflow => workflow,
        };
        let limit = step_limit(step, DISPATCH_TIMEOUT_SECONDS, MAX_DISPATCH_TIMEOUT_SECONDS);
        let deadline = tokio::time::Instant::now() + limit;
        let fail = |error: ApiError| error.into_deploy_outcome("pipeline:trigger", ACTIONS_HINT);

        let git_ref = match param("ref") {
            "" => match self.default_branch(ctx, &repo).await {
                Ok(branch) => branch,
                Err(error) => return fail(error),
            },
            git_ref => git_ref.to_owned(),
        };
        // The digest the registry confirmed in this run wins over the recorded one.
        let digest = ctx
            .outputs
            .get("digest")
            .map(String::as_str)
            .filter(|d| !d.is_empty())
            .unwrap_or(param("digest"));
        let workflow_path = format!("/repos/{repo}/actions/workflows/{workflow}");
        let dispatched_at = Utc::now() - DISPATCH_SKEW;
        if let Err(error) = self
            .call(
                ctx,
                Method::POST,
                &format!("{workflow_path}/dispatches"),
                Some(json!({
                    "ref": git_ref,
                    "inputs": {
                        "environment": param("environment"),
                        "image": param("image"),
                        "digest": digest,
                        "version": param("version"),
                    }
                })),
                &[],
            )
            .await
        {
            return match error.status() {
                Some(StatusCode::NOT_FOUND) => StepOutcome::failed(
                    diagnosis::DEPLOYER_FAILED,
                    format!(
                        "{}\nGitHub has no workflow {workflow} on {repo}@{git_ref} that this token can dispatch: the file must exist under .github/workflows/ on that ref, and {ACTIONS_HINT}",
                        error.describe()
                    ),
                ),
                Some(StatusCode::UNPROCESSABLE_ENTITY) => StepOutcome::failed(
                    diagnosis::DEPLOYER_FAILED,
                    format!(
                        "{}\nThe workflow must declare `on: workflow_dispatch` with the inputs environment, image, digest and version",
                        error.describe()
                    ),
                ),
                _ => fail(error),
            };
        }
        ctx.logs.line(format!(
            "dispatched {workflow} on {repo}@{git_ref} for {}",
            param("environment")
        ));

        // The dispatch answers 204 without saying which run it started: the
        // run is the oldest `workflow_dispatch` run of this workflow created
        // since.
        let created = dispatched_at.to_rfc3339_opts(SecondsFormat::Secs, true);
        let runs_path = format!(
            "{workflow_path}/runs?event=workflow_dispatch&created=%3E%3D{created}&per_page=20"
        );
        let appears_by = tokio::time::Instant::now() + self.appears_within;
        let run = loop {
            let listed = match self.call(ctx, Method::GET, &runs_path, None, &[]).await {
                Ok((_, listed)) => listed,
                Err(error) => return fail(error),
            };
            let found = listed
                .get("workflow_runs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|run| {
                    DateTime::parse_from_rfc3339(&str_field(run, "created_at"))
                        .map(|at| at.with_timezone(&Utc) >= dispatched_at)
                        .unwrap_or(false)
                })
                .min_by_key(|run| str_field(run, "created_at"))
                .cloned();
            if let Some(run) = found {
                break run;
            }
            if tokio::time::Instant::now() + self.poll >= appears_by.min(deadline) {
                return StepOutcome::failed(
                    diagnosis::DEPLOYER_FAILED,
                    format!(
                        "GitHub accepted the dispatch of {workflow} on {repo}@{git_ref} but no run appeared within {}s",
                        self.appears_within.as_secs()
                    ),
                );
            }
            tokio::time::sleep(self.poll).await;
        };
        let run_id = run.get("id").and_then(Value::as_u64).unwrap_or(0);
        let html_url = str_field(&run, "html_url");
        ctx.logs
            .line(format!("workflow run {run_id} started: {html_url}"));
        ctx.progress.emit("external_url", html_url.clone());
        let outputs = BTreeMap::from([
            ("external_url".to_owned(), html_url.clone()),
            ("workflow_run_id".to_owned(), run_id.to_string()),
        ]);

        let run_path = format!("/repos/{repo}/actions/runs/{run_id}");
        let mut current = run;
        loop {
            if str_field(&current, "status") == "completed" {
                let conclusion = str_field(&current, "conclusion");
                ctx.logs
                    .line(format!("workflow run {run_id} concluded {conclusion}"));
                if conclusion == "success" {
                    return StepOutcome::Succeeded {
                        output: format!("{workflow} run {run_id} succeeded: {html_url}"),
                        outputs,
                    };
                }
                let culprit = self.failing_step(ctx, &run_path).await;
                return StepOutcome::failed(
                    diagnosis::DEPLOYER_FAILED,
                    format!(
                        "workflow run {html_url} concluded {}{culprit}",
                        if conclusion.is_empty() {
                            "without a conclusion"
                        } else {
                            conclusion.as_str()
                        }
                    ),
                );
            }
            if tokio::time::Instant::now() + self.poll >= deadline {
                return StepOutcome::failed(
                    diagnosis::DEPLOYER_TIMEOUT,
                    format!(
                        "workflow run {html_url} was still {} after {}s",
                        str_field(&current, "status"),
                        limit.as_secs()
                    ),
                );
            }
            tokio::time::sleep(self.poll).await;
            current = match self.call(ctx, Method::GET, &run_path, None, &[]).await {
                Ok((_, run)) => run,
                Err(error) => return fail(error),
            };
        }
    }

    /// `: job 'deploy', step 'Apply manifests'` for the first job of a run
    /// that did not succeed; empty when GitHub cannot say.
    async fn failing_step(&self, ctx: &RunContext, run_path: &str) -> String {
        let Ok((_, listed)) = self
            .call(
                ctx,
                Method::GET,
                &format!("{run_path}/jobs?per_page=100"),
                None,
                &[],
            )
            .await
        else {
            return String::new();
        };
        let failed = |value: &&Value| {
            !matches!(
                str_field(value, "conclusion").as_str(),
                "success" | "skipped" | "neutral" | ""
            )
        };
        let Some(job) = listed
            .get("jobs")
            .and_then(Value::as_array)
            .and_then(|jobs| jobs.iter().find(failed))
        else {
            return String::new();
        };
        let step = job
            .get("steps")
            .and_then(Value::as_array)
            .and_then(|steps| steps.iter().find(failed))
            .map(|step| format!(", step '{}'", str_field(step, "name")))
            .unwrap_or_default();
        format!(": job '{}'{step}", str_field(job, "name"))
    }

    /// Creates a GitHub Deployment for the release's commit and marks it
    /// successful, so the repository's Environments page shows it.
    async fn record_deployment(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        if self.github_for(ctx).0.is_none() {
            return StepOutcome::Failed {
                code: diagnosis::PERMISSION_DENIED.to_owned(),
                raw_output: format!(
                    "no GitHub token is available for this workspace; {DEPLOYMENTS_HINT}"
                ),
                missing_permissions: vec!["repository:write".to_owned()],
            };
        }
        let param = |key: &str| step.params.get(key).map(|v| v.trim()).unwrap_or_default();
        let Some(repo) = repo_param(step) else {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                "params.repo (owner/name) is required to record a deployment",
            );
        };
        let commit = param("commit_sha");
        if commit.is_empty() {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                "the release has no commit to attach the deployment to",
            );
        }
        let environment = param("environment");
        let verb = if param("run_kind") == "rollback" {
            "Rolled back to"
        } else {
            "Deployed"
        };
        let description = format!(
            "{verb} {} by Software Delivery Intelligence #{}",
            param("version"),
            ctx.execution_number
        );
        let digest = ctx
            .outputs
            .get("digest")
            .map(String::as_str)
            .filter(|d| !d.is_empty())
            .unwrap_or(param("digest"));
        let fail = |error: ApiError| match error.status() {
            Some(StatusCode::NOT_FOUND) => StepOutcome::Failed {
                code: diagnosis::PERMISSION_DENIED.to_owned(),
                raw_output: format!(
                    "{}\nGitHub answers 404 when the token may not create deployments: {DEPLOYMENTS_HINT}",
                    error.describe()
                ),
                missing_permissions: vec!["repository:write".to_owned()],
            },
            _ => error.into_deploy_outcome("repository:write", DEPLOYMENTS_HINT),
        };

        let created = self
            .call(
                ctx,
                Method::POST,
                &format!("/repos/{repo}/deployments"),
                Some(json!({
                    "ref": commit,
                    "environment": environment,
                    "description": truncate_chars(&description, 140),
                    // The image is already deployed: nothing to merge, no checks to wait for.
                    "auto_merge": false,
                    "required_contexts": [],
                    "production_environment": param("protected") == "true",
                    "transient_environment": false,
                    "payload": {
                        "image": param("image"),
                        "tag": param("tag"),
                        "digest": digest,
                        "version": param("version"),
                        "kind": param("run_kind"),
                    },
                })),
                &[],
            )
            .await;
        let deployment = match created {
            Ok((_, deployment)) => deployment,
            Err(error) => return fail(error),
        };
        let Some(id) = deployment.get("id").and_then(Value::as_u64) else {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                "GitHub created a deployment without an id",
            );
        };
        let mut status = json!({
            "state": "success",
            "environment": environment,
            "description": truncate_chars(&description, 140),
            "auto_inactive": true,
        });
        let environment_url = match param("url") {
            "" => ctx.outputs.get("container_url").map_or("", |u| u.trim()),
            url => url,
        };
        if environment_url.starts_with("https://") || environment_url.starts_with("http://") {
            status["environment_url"] = json!(environment_url);
        }
        if let Some(log_url) = ctx
            .outputs
            .get("external_url")
            .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        {
            status["log_url"] = json!(log_url);
        }
        if let Err(error) = self
            .call(
                ctx,
                Method::POST,
                &format!("/repos/{repo}/deployments/{id}/statuses"),
                Some(status),
                &[],
            )
            .await
        {
            return fail(error);
        }
        ctx.logs.line(format!(
            "recorded deployment {id} of {commit} to {environment} on {repo}"
        ));
        StepOutcome::Succeeded {
            output: format!(
                "GitHub deployment {id} recorded: {repo} runs {} in {environment}",
                param("version")
            ),
            outputs: BTreeMap::from([("github_deployment_id".to_owned(), id.to_string())]),
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
        // CONTRACT.md §23.5: a repository path that makes the step
        // unnecessary when it already exists on the default branch.
        let skip_if_exists = step
            .params
            .get("skip_if_exists")
            .map(|path| path.trim().trim_matches('/'))
            .filter(|path| !path.is_empty());

        let work = self.publish(
            ctx,
            PullRequest {
                repo: &repo,
                owner: &owner,
                branch: &branch,
                title: &title,
                body: &body,
                message: &message,
                skip_if_exists,
            },
        );
        match tokio::time::timeout(STEP_TIMEOUT, work).await {
            Ok(result) => result,
            Err(_) => StepOutcome::failed(
                diagnosis::TIMEOUT,
                "GitHub did not answer within the step's time limit",
            ),
        }
    }

    async fn publish(&self, ctx: &RunContext, pull_request: PullRequest<'_>) -> StepOutcome {
        match self.publish_inner(ctx, pull_request).await {
            Ok(outcome) => outcome,
            Err(error) => error.into_outcome(),
        }
    }

    async fn publish_inner(
        &self,
        ctx: &RunContext,
        pull_request: PullRequest<'_>,
    ) -> Result<StepOutcome, ApiError> {
        let PullRequest {
            repo,
            owner,
            branch,
            title,
            body,
            message,
            skip_if_exists,
        } = pull_request;
        let base = format!("/repos/{repo}");

        // Decided before anything is created, so a skipped step leaves no
        // branch behind: the file the pull request would add is already on
        // the default branch.
        let mut repository = None;
        if let Some(path) = skip_if_exists {
            let (_, found) = self.call(ctx, Method::GET, &base, None, &[]).await?;
            let default_branch = default_branch_of(&found);
            let (status, _) = self
                .call(
                    ctx,
                    Method::GET,
                    &format!(
                        "{base}/contents/{}?ref={}",
                        encode_path(path),
                        encode_path(&default_branch)
                    ),
                    None,
                    &[StatusCode::NOT_FOUND],
                )
                .await?;
            if status != StatusCode::NOT_FOUND {
                ctx.logs.line(format!(
                    "{path} is already on {default_branch}; nothing to open"
                ));
                return Ok(StepOutcome::ok(format!(
                    "skipped: {path} already exists on {default_branch} of {repo}, so no branch, commit or pull request was created"
                )));
            }
            repository = Some(found);
        }

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

        let repository = match repository {
            Some(repository) => repository,
            None => self.call(ctx, Method::GET, &base, None, &[]).await?.1,
        };
        let default_branch = default_branch_of(&repository);
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

        let blobs = self.create_blobs(ctx, base).await?;
        let entries = blobs
            .iter()
            .map(|(path, sha)| {
                json!({
                    "path": path,
                    "mode": "100644",
                    "type": "blob",
                    "sha": sha,
                })
            })
            .collect();

        if branch_existed && self.tree_holds(ctx, base, &base_tree, &blobs).await? {
            ctx.logs.line(format!(
                "{} files already at {parent} on {branch}; nothing to commit",
                blobs.len()
            ));
            return Ok(Some(parent.to_owned()));
        }

        let tree = self.create_tree(ctx, base, &base_tree, entries).await?;
        let (_, commit) = self
            .call(
                ctx,
                Method::POST,
                &format!("{base}/git/commits"),
                Some(json!({
                    "message": message,
                    "tree": tree,
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

    /// Uploads every artifact of the run as a blob and returns `path ->
    /// blob sha`. Git addresses blobs by content, so uploading a file that
    /// is already in the repository changes nothing and names the same sha.
    pub(super) async fn create_blobs(
        &self,
        ctx: &RunContext,
        base: &str,
    ) -> Result<BTreeMap<String, String>, ApiError> {
        let mut blobs = BTreeMap::new();
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
            blobs.insert(
                path.trim_start_matches('/').to_owned(),
                str_field(&blob, "sha"),
            );
        }
        Ok(blobs)
    }

    /// Creates the tree that is `base_tree` with `entries` on top and
    /// returns its sha. Trees are content-addressed too: the same entries
    /// on the same base always name the same tree. GitHub answers 404 to a
    /// tree touching `.github/workflows/` when the token lacks the
    /// `workflow` scope; that becomes a 403 naming the scope.
    pub(super) async fn create_tree(
        &self,
        ctx: &RunContext,
        base: &str,
        base_tree: &str,
        entries: Vec<Value>,
    ) -> Result<String, ApiError> {
        let touches_workflows = entries
            .iter()
            .any(|entry| str_field(entry, "path").starts_with(".github/workflows/"));
        match self
            .call(
                ctx,
                Method::POST,
                &format!("{base}/git/trees"),
                Some(json!({ "base_tree": base_tree, "tree": entries })),
                &[],
            )
            .await
        {
            Ok((_, tree)) => Ok(str_field(&tree, "sha")),
            Err(ApiError::Status {
                status: StatusCode::NOT_FOUND,
                method,
                path,
                message,
            }) if touches_workflows => Err(ApiError::Status {
                method,
                path,
                status: StatusCode::FORBIDDEN,
                message: format!(
                    "{message} (404 on a tree with workflow files); {WORKFLOW_SCOPE_HINT}"
                ),
            }),
            Err(error) => Err(error),
        }
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

/// What `repo.open_pull_request` publishes.
#[derive(Clone, Copy)]
struct PullRequest<'a> {
    repo: &'a str,
    owner: &'a str,
    branch: &'a str,
    title: &'a str,
    body: &'a str,
    message: &'a str,
    /// A repository path whose presence on the default branch makes the
    /// step unnecessary.
    skip_if_exists: Option<&'a str>,
}

/// The default branch a `GET /repos/{repo}` answer names.
pub(super) fn default_branch_of(repository: &Value) -> String {
    let branch = str_field(repository, "default_branch");
    if branch.is_empty() {
        "main".to_owned()
    } else {
        branch
    }
}

pub(super) fn str_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// `params.repo` as `owner/name`.
pub(super) fn repo_param(step: &StepInput) -> Option<String> {
    step.params
        .get("repo")
        .or_else(|| step.params.get("repository"))
        .map(|s| s.trim().trim_matches('/').to_owned())
        .filter(|s| s.split('/').count() == 2 && !s.starts_with('/') && !s.ends_with('/'))
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[async_trait]
impl Adapter for GithubAdapter {
    fn name(&self) -> &'static str {
        GITHUB
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        let operation = step.params.get("operation").map_or("", |op| op.trim());
        match (step.action_type.as_str(), operation) {
            (super::OPEN_PULL_REQUEST, _) => self.open_pull_request(step, ctx).await,
            (super::COMMIT_FIXES, _) => self.commit_fixes(step, ctx).await,
            (super::VERIFY_MERGEABLE, _) => self.verify_mergeable(step, ctx).await,
            (super::MERGE_PULL_REQUEST, _) => self.merge_pull_request(step, ctx).await,
            (super::DELETE_BRANCH, _) => self.delete_branch(step, ctx).await,
            (super::ENABLE_SECURITY_UPDATES, _) => self.enable_security_updates(step, ctx).await,
            (action, OP_WORKFLOW_DISPATCH) if super::is_deploy_action(action) => {
                self.workflow_dispatch(step, ctx).await
            }
            (super::DEPLOY_RECORD, _) => self.record_deployment(step, ctx).await,
            (other, _) => StepOutcome::failed(
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

/// `workflow.dispatch` and `deployment.record` against the fake GitHub.
#[cfg(test)]
mod delivery_tests {
    use serde_json::json;

    use super::*;
    use crate::{
        adapters::{GithubAccess, LogSink, OutputSink, Redactor},
        tests::fakes::github::{self as fake, FakeGithub},
    };

    const COMMIT: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";

    fn adapter() -> GithubAdapter {
        // The process-wide token is deliberately wrong: only the workspace's
        // access may reach GitHub.
        GithubAdapter::new(Some("process-wide-token".into()), "http://unused.invalid")
            .with_poll(Duration::from_millis(10), Duration::from_millis(400))
    }

    struct Run {
        ctx: RunContext,
        logs: tokio::sync::mpsc::Receiver<String>,
        progress: tokio::sync::mpsc::Receiver<(String, String)>,
    }

    fn context(api: &str) -> Run {
        let (tx, logs) = tokio::sync::mpsc::channel(256);
        let (ptx, progress) = tokio::sync::mpsc::channel(16);
        let redactor = Redactor::default();
        redactor.protect(fake::TOKEN);
        let ctx = RunContext {
            execution_number: 12,
            outputs: BTreeMap::from([("digest".to_owned(), format!("sha256:{}", "d".repeat(64)))]),
            github: Some(GithubAccess {
                token: fake::TOKEN.into(),
                api_base_url: api.to_owned(),
            }),
            logs: LogSink::new(tx).with_redactor(redactor),
            progress: OutputSink::new(ptx),
            ..Default::default()
        };
        Run {
            ctx,
            logs,
            progress,
        }
    }

    fn dispatch_step(extra: &[(&str, &str)]) -> StepInput {
        let mut params: BTreeMap<String, String> = [
            ("deployer", "github_actions"),
            ("operation", OP_WORKFLOW_DISPATCH),
            ("repo", "acme/api"),
            ("workflow", "deploy.yml"),
            ("environment", "staging"),
            ("image", "ghcr.io/acme/api"),
            ("tag", "a1b2c3d"),
            ("digest", ""),
            ("version", "a1b2c3d"),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
        for (key, value) in extra {
            params.insert((*key).to_owned(), (*value).to_owned());
        }
        StepInput {
            key: "deploy".into(),
            title: "Deploy a1b2c3d to staging".into(),
            action_type: "deploy.staging".into(),
            adapter: "github".into(),
            params,
            attempt: 1,
        }
    }

    #[tokio::test]
    async fn dispatch_follows_the_run_it_started_to_success() {
        let (github, api) = FakeGithub::default()
            .with_workflow("acme/api", "deploy.yml")
            .run_appears_after(2)
            .serve()
            .await;
        let mut run = context(&api);
        let outcome = adapter().run(&dispatch_step(&[]), &run.ctx).await;
        let StepOutcome::Succeeded { output, outputs } = outcome else {
            panic!("expected success: {outcome:?}");
        };
        // The release went in as the four inputs of the contract, on the
        // default branch GitHub reported, with the verified digest.
        let dispatches = github.dispatches.lock().unwrap().clone();
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0]["ref"], "main");
        assert_eq!(
            dispatches[0]["inputs"],
            json!({
                "environment": "staging",
                "image": "ghcr.io/acme/api",
                "digest": format!("sha256:{}", "d".repeat(64)),
                "version": "a1b2c3d",
            })
        );
        // The run found is the one this dispatch created, not the old one.
        let run_id = dispatches[0]["run_id"].as_u64().unwrap();
        let url = format!("https://github.com/acme/api/actions/runs/{run_id}");
        assert_eq!(outputs["external_url"], url);
        assert_eq!(outputs["workflow_run_id"], run_id.to_string());
        assert!(output.contains("succeeded"), "{output}");
        // The link was reported while the workflow was still running.
        assert_eq!(
            run.progress.try_recv().unwrap(),
            ("external_url".to_owned(), url)
        );
        let mut lines = Vec::new();
        while let Ok(line) = run.logs.try_recv() {
            lines.push(line);
        }
        let lines = lines.join("\n");
        assert!(lines.contains("dispatched deploy.yml on acme/api@main for staging"));
        assert!(lines.contains("concluded success"), "{lines}");
        assert!(!lines.contains(fake::TOKEN));
    }

    #[tokio::test]
    async fn a_failed_workflow_names_the_job_and_the_step() {
        let (github, api) = FakeGithub::default()
            .with_workflow("acme/api", "release.yml")
            .serve()
            .await;
        github.conclude("failure");
        let run = context(&api);
        let outcome = adapter()
            .run(
                &dispatch_step(&[("workflow", "release.yml"), ("ref", "release/1.x")]),
                &run.ctx,
            )
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(
            raw_output.contains("concluded failure: job 'deploy', step 'Apply manifests'"),
            "{raw_output}"
        );
        assert!(raw_output.contains("https://github.com/acme/api/actions/runs/"));
        assert_eq!(github.dispatches.lock().unwrap()[0]["ref"], "release/1.x");
    }

    #[tokio::test]
    async fn missing_workflows_refusals_timeouts_and_lost_runs_have_their_own_answers() {
        // No such workflow file.
        let (_, api) = FakeGithub::default().serve().await;
        let run = context(&api);
        let outcome = adapter().run(&dispatch_step(&[]), &run.ctx).await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(
            raw_output.contains("GitHub has no workflow deploy.yml on acme/api@main"),
            "{raw_output}"
        );

        // A token that may not dispatch.
        let (_, api) = FakeGithub::default()
            .with_workflow("acme/api", "deploy.yml")
            .forbid_writes()
            .serve()
            .await;
        let run = context(&api);
        let outcome = adapter().run(&dispatch_step(&[]), &run.ctx).await;
        let StepOutcome::Failed {
            code,
            missing_permissions,
            raw_output,
        } = outcome
        else {
            panic!("expected a refusal");
        };
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(missing_permissions, ["pipeline:trigger"]);
        assert!(
            raw_output.contains("Actions \"Read and write\""),
            "{raw_output}"
        );

        // The dispatch is accepted but the run never shows up.
        let (_, api) = FakeGithub::default()
            .with_workflow("acme/api", "deploy.yml")
            .run_appears_after(usize::MAX)
            .serve()
            .await;
        let run = context(&api);
        let outcome = adapter().run(&dispatch_step(&[]), &run.ctx).await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(raw_output.contains("no run appeared"), "{raw_output}");

        // Without a token nothing is attempted.
        let outcome = GithubAdapter::new(None, "http://unused.invalid")
            .run(&dispatch_step(&[]), &RunContext::default())
            .await;
        let StepOutcome::Failed { code, .. } = outcome else {
            panic!("expected a refusal");
        };
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
    }

    fn record_step(run_kind: &str) -> StepInput {
        StepInput {
            key: "record".into(),
            title: "Record deployment".into(),
            action_type: "deploy.record".into(),
            adapter: "github".into(),
            params: [
                ("operation", OP_DEPLOYMENT_RECORD),
                ("run_kind", run_kind),
                ("repo", "acme/api"),
                ("commit_sha", COMMIT),
                ("environment", "production"),
                ("url", "https://api.example.com"),
                ("protected", "true"),
                ("version", "a1b2c3d"),
                ("image", "ghcr.io/acme/api"),
                ("tag", "a1b2c3d"),
                ("digest", ""),
            ]
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
            attempt: 1,
        }
    }

    #[tokio::test]
    async fn record_creates_a_deployment_and_marks_it_successful() {
        let (github, api) = FakeGithub::default().serve().await;
        let mut run = context(&api);
        run.ctx.outputs.insert(
            "external_url".into(),
            "https://github.com/acme/api/actions/runs/101".into(),
        );
        let outcome = adapter().run(&record_step("promote"), &run.ctx).await;
        let StepOutcome::Succeeded { outputs, .. } = outcome else {
            panic!("expected success: {outcome:?}");
        };
        let deployments = github.deployments.lock().unwrap().clone();
        assert_eq!(deployments.len(), 1);
        let deployment = &deployments[0];
        assert_eq!(deployment["repo"], "acme/api");
        assert_eq!(deployment["ref"], COMMIT);
        assert_eq!(deployment["environment"], "production");
        assert_eq!(deployment["production_environment"], true);
        // Nothing to merge and no checks to wait for: the image already runs.
        assert_eq!(deployment["auto_merge"], false);
        assert_eq!(deployment["required_contexts"], json!([]));
        assert_eq!(deployment["payload"]["image"], "ghcr.io/acme/api");
        assert_eq!(
            deployment["payload"]["digest"],
            format!("sha256:{}", "d".repeat(64))
        );
        assert_eq!(
            deployment["description"],
            "Deployed a1b2c3d by Software Delivery Intelligence #12"
        );
        assert_eq!(
            outputs["github_deployment_id"],
            deployment["id"].to_string()
        );
        let statuses = github.deployment_statuses.lock().unwrap().clone();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0]["deployment_id"], deployment["id"]);
        assert_eq!(statuses[0]["state"], "success");
        assert_eq!(statuses[0]["environment_url"], "https://api.example.com");
        assert_eq!(
            statuses[0]["log_url"],
            "https://github.com/acme/api/actions/runs/101"
        );

        let outcome = adapter().run(&record_step("rollback"), &run.ctx).await;
        assert!(matches!(outcome, StepOutcome::Succeeded { .. }));
        assert_eq!(
            github.deployments.lock().unwrap()[1]["description"],
            "Rolled back to a1b2c3d by Software Delivery Intelligence #12"
        );

        // A token without the deployments permission.
        let (_, api) = FakeGithub::default().forbid_writes().serve().await;
        let run = context(&api);
        let outcome = adapter().run(&record_step("promote"), &run.ctx).await;
        let StepOutcome::Failed {
            code,
            missing_permissions,
            raw_output,
        } = outcome
        else {
            panic!("expected a refusal");
        };
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(missing_permissions, ["repository:write"]);
        assert!(
            raw_output.contains("Deployments \"Read and write\""),
            "{raw_output}"
        );
    }

    #[tokio::test]
    async fn only_deploy_steps_with_an_operation_reach_the_delivery_paths() {
        let adapter = adapter();
        let ctx = RunContext::default();
        let mut step = dispatch_step(&[]);
        step.params.remove("operation");
        let StepOutcome::Failed { raw_output, .. } = adapter.run(&step, &ctx).await else {
            panic!("a deploy step without an operation is not GitHub's");
        };
        assert!(
            raw_output.contains("cannot run deploy.staging"),
            "{raw_output}"
        );
        let mut step = dispatch_step(&[]);
        step.action_type = "container.build".into();
        let StepOutcome::Failed { raw_output, .. } = adapter.run(&step, &ctx).await else {
            panic!("the operation alone does not make a step a deployment");
        };
        assert!(
            raw_output.contains("cannot run container.build"),
            "{raw_output}"
        );
    }
}
