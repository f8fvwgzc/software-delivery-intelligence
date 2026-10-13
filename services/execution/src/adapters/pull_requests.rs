//! The repository operations of the pull request autopilot (CONTRACT.md
//! §23), run by the `github` adapter with the workspace's GitHub access:
//!
//! - `repo.commit_fixes` puts the run's artifacts on a pull request's
//!   branch as one commit on top of the head the review saw, and moves the
//!   branch without force;
//! - `repo.verify_mergeable` reads the pull request again and refuses a
//!   moved head, a conflict, a failing or running check, a blocked merge;
//! - `repo.merge_pull_request` merges exactly the head that was verified;
//! - `repo.delete_branch` removes the merged branch when it is safe to;
//! - `repo.enable_security_updates` turns on Dependabot alerts and
//!   security updates.
//!
//! The runner resumes executions after a restart, so every operation reads
//! GitHub's current state first and treats "already done" as done: a commit
//! that is already the branch head, a pull request that is already merged,
//! a branch that is already gone. None of them reports a success for
//! something that did not happen: a branch that is left in place says so
//! and why.

use std::{collections::BTreeMap, future::Future, time::Duration};

use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::time::Instant;

use super::{
    RunContext, StepInput, StepOutcome,
    github::{ApiError, GithubAdapter, STEP_TIMEOUT, default_branch_of, repo_param, str_field},
    gitops::encode_path,
};
use crate::domain::{diagnosis, policy::REPOSITORY_ADMIN as ADMIN};

const READ: &str = "repository:read";
const WRITE: &str = "repository:write";

const READ_HINT: &str = "the GitHub access has to read the repository, its pull requests, check runs and commit statuses: the `repo` scope on a classic token; `contents: read`, `pull_requests: read`, `checks: read` and `statuses: read` for the GitHub App";
const WRITE_HINT: &str = "the GitHub access has to write to the repository: a classic token needs the `repo` scope and write access to the repository; the GitHub App needs `contents: write` and `pull_requests: write`";
const ADMIN_HINT: &str = "changing these repository settings needs admin rights on the repository: a token of a user who is an admin of it; `administration: write` for the GitHub App";

/// Check names a `CHECKS_FAILING` failure spells out.
const NAMED_CHECKS: usize = 10;
/// Pages of 100 read from a list before it is taken as complete.
const MAX_PAGES: usize = 20;

/// A step's outcome, decided either way: `Err` ends the operation early
/// with the failure it carries.
type Decided = Result<StepOutcome, StepOutcome>;

/// Runs one operation under the step's time limit.
async fn bounded(work: impl Future<Output = Decided>) -> StepOutcome {
    match tokio::time::timeout(STEP_TIMEOUT, work).await {
        Ok(Ok(outcome) | Err(outcome)) => outcome,
        Err(_) => StepOutcome::failed(
            diagnosis::TIMEOUT,
            "GitHub did not answer within the step's time limit",
        ),
    }
}

/// A plan this adapter cannot act on: a missing or malformed parameter,
/// nothing to commit, a branch it must not write to. Nothing was changed.
fn bad_plan(message: impl Into<String>) -> StepOutcome {
    StepOutcome::failed(diagnosis::UNKNOWN, message)
}

fn head_moved(message: impl Into<String>) -> StepOutcome {
    StepOutcome::failed(diagnosis::HEAD_MOVED, message)
}

fn blocked(message: impl Into<String>) -> StepOutcome {
    StepOutcome::failed(diagnosis::MERGE_BLOCKED, message)
}

/// What a call GitHub refused or that never completed means for an
/// operation that needed `permission`. GitHub answers 404, not 403, to a
/// token that may not see what it asked for, so both name the permission.
fn refused(error: ApiError, permission: &str, hint: &str) -> StepOutcome {
    let raw = error.describe();
    let rate_limited = raw.to_ascii_lowercase().contains("rate limit");
    match error.status() {
        Some(StatusCode::TOO_MANY_REQUESTS) => StepOutcome::failed(diagnosis::QUOTA_EXCEEDED, raw),
        Some(StatusCode::FORBIDDEN) if rate_limited => {
            StepOutcome::failed(diagnosis::QUOTA_EXCEEDED, raw)
        }
        Some(
            status @ (StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
        ) => {
            let not_found = if status == StatusCode::NOT_FOUND {
                "GitHub answers 404 both when this does not exist and when the token may not see or change it; "
            } else {
                ""
            };
            StepOutcome::Failed {
                code: diagnosis::PERMISSION_DENIED.to_owned(),
                raw_output: format!("{raw}\n{not_found}{hint}"),
                missing_permissions: vec![permission.to_owned()],
            }
        }
        None if raw.contains("timed out") => StepOutcome::failed(diagnosis::TIMEOUT, raw),
        _ => StepOutcome::failed(diagnosis::UNKNOWN, raw),
    }
}

fn param<'a>(step: &'a StepInput, key: &str) -> &'a str {
    step.params.get(key).map_or("", |value| value.trim())
}

fn required<'a>(step: &'a StepInput, key: &str) -> Result<&'a str, StepOutcome> {
    match param(step, key) {
        "" => Err(bad_plan(format!(
            "params.{key} is required for {}",
            step.action_type
        ))),
        value => Ok(value),
    }
}

/// `params.repo` as `owner/name`: the repository of the execution's project.
/// Both halves go into request paths, so they are held to what GitHub
/// allows in them.
fn repo_of(step: &StepInput) -> Result<String, StepOutcome> {
    let named = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    repo_param(step)
        .filter(|repo| repo.split('/').all(named))
        .ok_or_else(|| {
            bad_plan(format!(
                "params.repo (owner/name) is required for {}",
                step.action_type
            ))
        })
}

/// `params.branch` as a branch name (`refs/heads/` is accepted and dropped).
/// A name git itself would refuse is a bad plan: it is about to be part of
/// a request path, where `..` would address something else.
fn branch_of(step: &StepInput) -> Result<&str, StepOutcome> {
    let raw = required(step, "branch")?;
    let branch = raw.strip_prefix("refs/heads/").unwrap_or(raw);
    let well_formed = !branch.is_empty()
        && !branch.contains("..")
        && !branch.contains("@{")
        && !branch.ends_with(".lock")
        && branch
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with('.'))
        && !branch
            .chars()
            .any(|c| c.is_control() || " ~^:?*[\\".contains(c));
    if !well_formed {
        return Err(bad_plan(format!(
            "params.branch of {} is not a branch name: '{raw}'",
            step.action_type
        )));
    }
    Ok(branch)
}

/// Whether `path` names a file inside a repository: no empty, `.` or `..`
/// segment, nothing inside `.git`.
fn is_repository_path(path: &str) -> bool {
    !path.is_empty()
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && part != ".git")
}

/// What a path of the fixes is in the tree they go on top of.
enum Existing {
    /// An entry with this git mode (a file, or something that is not one).
    Entry(String),
    /// One of its parent directories is this file.
    Under(String),
}

/// `params.pull_request`: the number as decimal text (§23.5).
fn pull_number(step: &StepInput) -> Result<u64, StepOutcome> {
    let raw = required(step, "pull_request")?;
    raw.parse::<u64>()
        .ok()
        .filter(|number| *number > 0 && raw.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| {
            bad_plan(format!(
                "params.pull_request must be the pull request number, got '{raw}'"
            ))
        })
}

/// `params.expected_head_sha`: the full commit the plan was made for. An
/// abbreviated sha would never equal what GitHub reports and every run
/// would look like a moved head, so it is refused as a bad plan instead.
fn expected_head(step: &StepInput) -> Result<String, StepOutcome> {
    let raw = required(step, "expected_head_sha")?;
    if !matches!(raw.len(), 40 | 64) || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad_plan(format!(
            "params.expected_head_sha must be a full commit sha, got '{raw}'"
        )));
    }
    Ok(raw.to_ascii_lowercase())
}

/// `"true"` or `"false"`; absent means false.
fn flag(step: &StepInput, key: &str) -> Result<bool, StepOutcome> {
    match param(step, key) {
        "true" => Ok(true),
        "" | "false" => Ok(false),
        other => Err(bad_plan(format!(
            "params.{key} must be true or false, got '{other}'"
        ))),
    }
}

/// The first characters of a sha, as people read them.
fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// GitHub's sentence in an answer, without the closing period (it is
/// quoted inside one of ours).
fn said(answer: &Value) -> String {
    let message = match answer {
        Value::String(text) => text.clone(),
        other => str_field(other, "message"),
    };
    let message = message.trim().trim_end_matches('.').to_owned();
    if message.is_empty() {
        "no reason given".to_owned()
    } else {
        message
    }
}

fn span(duration: Duration) -> String {
    if duration >= Duration::from_secs(1) {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn object_sha(reference: &Value) -> String {
    str_field(reference.get("object").unwrap_or(&Value::Null), "sha")
}

fn tree_of(commit: &Value) -> String {
    str_field(commit.get("tree").unwrap_or(&Value::Null), "sha")
}

fn parents_of(commit: &Value) -> Vec<String> {
    commit
        .get("parents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|parent| str_field(parent, "sha"))
        .collect()
}

/// `2 checks`, `1 check`.
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// What the operations read from `GET /pulls/{n}`.
struct Pull {
    url: String,
    open: bool,
    merged: bool,
    draft: bool,
    head_sha: String,
    head_ref: String,
    /// `owner/name` of the repository the head branch lives in; empty when
    /// that repository (a fork) no longer exists.
    head_repo: String,
    base_ref: String,
    /// `None` while GitHub has not computed it.
    mergeable: Option<bool>,
    mergeable_state: String,
    /// Only meaningful once merged: before that GitHub keeps the sha of
    /// its own test merge here.
    merge_commit_sha: String,
}

impl Pull {
    fn read(value: &Value) -> Self {
        let head = value.get("head").unwrap_or(&Value::Null);
        let base = value.get("base").unwrap_or(&Value::Null);
        let flag = |key: &str| value.get(key).and_then(Value::as_bool).unwrap_or(false);
        Self {
            url: str_field(value, "html_url"),
            open: str_field(value, "state") == "open",
            merged: flag("merged"),
            draft: flag("draft"),
            head_sha: str_field(head, "sha").to_ascii_lowercase(),
            head_ref: str_field(head, "ref"),
            head_repo: str_field(head.get("repo").unwrap_or(&Value::Null), "full_name"),
            base_ref: str_field(base, "ref"),
            mergeable: value.get("mergeable").and_then(Value::as_bool),
            mergeable_state: str_field(value, "mergeable_state"),
            merge_commit_sha: str_field(value, "merge_commit_sha"),
        }
    }

    /// ` (merge commit a1b2c3d)` once merged and known.
    fn merge_commit_note(&self) -> String {
        if self.merged && !self.merge_commit_sha.is_empty() {
            format!(" (merge commit {})", short(&self.merge_commit_sha))
        } else {
            String::new()
        }
    }

    fn outputs(&self) -> BTreeMap<String, String> {
        let mut outputs = BTreeMap::new();
        if !self.url.is_empty() {
            outputs.insert("pull_request_url".to_owned(), self.url.clone());
        }
        outputs
    }
}

/// One check of a commit, folded from its check runs and commit statuses.
/// Ordered by how bad it is for a merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Check {
    Passed,
    Running,
    Failed,
}

impl Check {
    /// A check run (CONTRACT.md §23.3): not finished is running; finished
    /// with success, neutral or skipped passed; anything else failed.
    fn of_run(run: &Value) -> Self {
        match str_field(run, "status").as_str() {
            "queued" | "in_progress" | "waiting" | "requested" | "pending" => Self::Running,
            _ => match str_field(run, "conclusion").as_str() {
                "success" | "neutral" | "skipped" => Self::Passed,
                _ => Self::Failed,
            },
        }
    }

    /// A commit status: success passed, pending is running, failure and
    /// error (and anything unheard of) failed.
    fn of_status(state: &str) -> Self {
        match state {
            "success" => Self::Passed,
            "pending" => Self::Running,
            _ => Self::Failed,
        }
    }
}

/// Keeps the worst state reported under one name.
fn fold(checks: &mut BTreeMap<String, Check>, name: String, state: Check) {
    let name = if name.trim().is_empty() {
        "unnamed check".to_owned()
    } else {
        name
    };
    let slot = checks.entry(name).or_insert(state);
    *slot = (*slot).max(state);
}

/// `2 checks failed (lint, unit) and 1 check is still running (build)`.
fn checks_sentence(checks: &BTreeMap<String, Check>) -> Option<String> {
    let names = |wanted: Check| -> Vec<&str> {
        checks
            .iter()
            .filter(|(_, state)| **state == wanted)
            .map(|(name, _)| name.as_str())
            .collect()
    };
    let failed = names(Check::Failed);
    let running = names(Check::Running);
    if failed.is_empty() && running.is_empty() {
        return None;
    }
    // At most NAMED_CHECKS names in all, the failed ones first.
    let mut budget = NAMED_CHECKS;
    let mut list = |all: &[&str]| -> String {
        let shown = all.len().min(budget);
        budget -= shown;
        let mut text = all[..shown].join(", ");
        if shown < all.len() {
            if shown > 0 {
                text.push_str(", ");
            }
            text.push_str(&format!("{} more", all.len() - shown));
        }
        text
    };
    let mut parts = Vec::new();
    if !failed.is_empty() {
        parts.push(format!(
            "{} failed ({})",
            count(failed.len(), "check"),
            list(&failed)
        ));
    }
    if !running.is_empty() {
        parts.push(format!(
            "{} still running ({})",
            if running.len() == 1 {
                "1 check is".to_owned()
            } else {
                format!("{} checks are", running.len())
            },
            list(&running)
        ));
    }
    Some(parts.join(" and "))
}

impl GithubAdapter {
    /// `Err` before any request when the workspace has no GitHub access.
    fn require_access(
        &self,
        ctx: &RunContext,
        permission: &str,
        hint: &str,
    ) -> Result<(), StepOutcome> {
        if self.github_for(ctx).0.is_some() {
            return Ok(());
        }
        Err(StepOutcome::Failed {
            code: diagnosis::PERMISSION_DENIED.to_owned(),
            raw_output: format!("no GitHub access is configured for this workspace; {hint}"),
            missing_permissions: vec![permission.to_owned()],
        })
    }

    async fn pull(&self, ctx: &RunContext, repo: &str, number: u64) -> Result<Pull, ApiError> {
        let (_, value) = self
            .call(
                ctx,
                Method::GET,
                &format!("/repos/{repo}/pulls/{number}"),
                None,
                &[],
            )
            .await?;
        Ok(Pull::read(&value))
    }

    /// Every item of a paged list whose answer is `{total_count, <key>:
    /// [...]}`. `path` already carries `?per_page=100`.
    async fn listed(
        &self,
        ctx: &RunContext,
        path: &str,
        key: &str,
    ) -> Result<Vec<Value>, ApiError> {
        let mut items = Vec::new();
        for page in 1..=MAX_PAGES {
            let paged = if page == 1 {
                path.to_owned()
            } else {
                format!("{path}&page={page}")
            };
            let (_, body) = self.call(ctx, Method::GET, &paged, None, &[]).await?;
            let batch = body
                .get(key)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let total = body
                .get("total_count")
                .and_then(Value::as_u64)
                .map_or(0, |n| n as usize);
            let got = batch.len();
            items.extend(batch);
            if got == 0 || items.len() >= total {
                break;
            }
        }
        Ok(items)
    }

    // --- repo.commit_fixes --------------------------------------------------

    /// Commits the run's artifacts (full file contents keyed by path) to a
    /// pull request's branch: one commit whose parent is
    /// `params.expected_head_sha`, then the branch moved to it without
    /// force. A branch that is anywhere else fails `HEAD_MOVED` and nothing
    /// is committed, unless its head is this very commit, left there by an
    /// attempt that was interrupted before it could report.
    pub(super) async fn commit_fixes(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        bounded(self.commit_fixes_inner(step, ctx)).await
    }

    async fn commit_fixes_inner(&self, step: &StepInput, ctx: &RunContext) -> Decided {
        self.require_access(ctx, WRITE, WRITE_HINT)?;
        let repo = repo_of(step)?;
        let branch = branch_of(step)?;
        let expected = expected_head(step)?;
        let number = match param(step, "pull_request") {
            "" => None,
            _ => Some(pull_number(step)?),
        };
        // Never the first word of a sentence: the diagnosis capitalises it.
        let target = number.map_or_else(
            || format!("branch {branch}"),
            |number| format!("branch {branch} of pull request #{number}"),
        );
        let message = match param(step, "message") {
            "" => number.map_or_else(
                || "Apply review fixes".to_owned(),
                |number| format!("Apply review fixes to #{number}"),
            ),
            message => message.to_owned(),
        };
        if ctx.artifacts.is_empty() {
            return Err(bad_plan(format!(
                "there is nothing to commit to {target}: the execution carries no files. No commit was created."
            )));
        }

        let base = format!("/repos/{repo}");
        let write = |error: ApiError| refused(error, WRITE, WRITE_HINT);
        let (_, repository) = self
            .call(ctx, Method::GET, &base, None, &[])
            .await
            .map_err(write)?;
        let default_branch = default_branch_of(&repository);
        if branch == default_branch {
            return Err(bad_plan(format!(
                "refusing to commit review fixes to {branch}: it is the default branch of {repo}, and fixes only go to a pull request's own branch. No commit was created."
            )));
        }

        let ref_path = format!("{base}/git/ref/heads/{}", encode_path(branch));
        let (status, reference) = self
            .call(ctx, Method::GET, &ref_path, None, &[StatusCode::NOT_FOUND])
            .await
            .map_err(write)?;
        if status == StatusCode::NOT_FOUND {
            return Err(head_moved(format!(
                "branch {branch} no longer exists on {repo}; the fixes were written for it at {}. Nothing was committed.",
                short(&expected)
            )));
        }
        let head = object_sha(&reference).to_ascii_lowercase();
        let moved = |now: &str| {
            head_moved(format!(
                "{target} is at {} now, not at {} where the fixes were written: the branch has commits the review never saw. Nothing was committed.",
                short(now),
                short(&expected)
            ))
        };
        // The only other state this step accepts is its own commit on the
        // branch: an earlier attempt committed the fixes and was
        // interrupted before the step was checkpointed. That commit's only
        // parent is the expected head and it carries this message; whether
        // its tree is the one these files produce is decided below. Any
        // other history is a branch that moved, and nothing is sent to it.
        let earlier = if head == expected {
            Vec::new()
        } else {
            let earlier = self
                .earlier_attempts(ctx, &base, &expected, &head, &message)
                .await
                .map_err(write)?;
            if earlier.is_empty() {
                return Err(moved(&head));
            }
            earlier
        };

        let (_, parent) = self
            .call(
                ctx,
                Method::GET,
                &format!("{base}/git/commits/{expected}"),
                None,
                &[],
            )
            .await
            .map_err(write)?;
        let base_tree = tree_of(&parent);
        let paths: Vec<String> = ctx
            .artifacts
            .keys()
            .map(|path| path.trim_start_matches('/').to_owned())
            .collect();
        if let Some(path) = paths.iter().find(|path| !is_repository_path(path)) {
            return Err(bad_plan(format!(
                "'{path}' is not a path inside the repository. No commit was created."
            )));
        }
        // A fix replaces lines of a file: the file keeps what it is (an
        // executable script stays executable), and something that is not a
        // regular file is never replaced by one.
        let existing = self
            .existing(ctx, &base, &base_tree, &paths)
            .await
            .map_err(write)?;
        for (path, what) in &existing {
            let problem = match what {
                Existing::Entry(mode) if matches!(mode.as_str(), "100644" | "100755") => continue,
                Existing::Entry(mode) => format!("is not a regular file (git mode {mode})"),
                Existing::Under(file) => format!("is inside {file}, which is a file"),
            };
            return Err(bad_plan(format!(
                "{path} {problem} at {}; a review fix cannot replace it. No commit was created.",
                short(&expected)
            )));
        }
        let blobs = self.create_blobs(ctx, &base).await.map_err(write)?;
        let entries = blobs
            .iter()
            .map(|(path, sha)| {
                let mode = match existing.get(path) {
                    Some(Existing::Entry(mode)) => mode.as_str(),
                    _ => "100644",
                };
                json!({ "path": path, "mode": mode, "type": "blob", "sha": sha })
            })
            .collect();
        // Trees are addressed by content: this is the tree the commit has,
        // whether it is created now or was created by an earlier attempt.
        let tree = self
            .create_tree(ctx, &base, &base_tree, entries)
            .await
            .map_err(write)?;

        if head != expected {
            let Some(commit) = earlier
                .iter()
                .find(|commit| tree_of(commit.get("commit").unwrap_or(&Value::Null)) == tree)
            else {
                return Err(moved(&head));
            };
            let sha = str_field(commit, "sha");
            ctx.logs.line(format!(
                "{branch} already has {sha}, the commit of these files on top of {expected}"
            ));
            let files = if blobs.len() == 1 {
                "the file is".to_owned()
            } else {
                format!("the {} files are", blobs.len())
            };
            let since = if sha == head {
                String::new()
            } else {
                format!(" (the branch has moved on to {} since)", short(&head))
            };
            return Ok(Self::committed(
                &repository,
                commit,
                &sha,
                format!(
                    "{files} already on {target} as {}{since}: an earlier attempt of this step made that commit, nothing was committed again",
                    short(&sha)
                ),
            ));
        }
        if tree == base_tree {
            return Err(bad_plan(format!(
                "there is nothing to commit to {target}: every file already has this content at {}. No commit was created.",
                short(&expected)
            )));
        }

        let (_, commit) = self
            .call(
                ctx,
                Method::POST,
                &format!("{base}/git/commits"),
                Some(json!({
                    "message": message,
                    "tree": tree,
                    "parents": [expected],
                })),
                &[],
            )
            .await
            .map_err(write)?;
        let sha = str_field(&commit, "sha");
        let (status, answer) = self
            .call(
                ctx,
                Method::PATCH,
                &format!("{base}/git/refs/heads/{}", encode_path(branch)),
                Some(json!({ "sha": sha, "force": false })),
                &[StatusCode::UNPROCESSABLE_ENTITY],
            )
            .await
            .map_err(write)?;
        if status == StatusCode::UNPROCESSABLE_ENTITY {
            // Not a fast-forward when somebody pushed between the read and
            // the write. Look again instead of guessing from the wording.
            let (status, reference) = self
                .call(ctx, Method::GET, &ref_path, None, &[StatusCode::NOT_FOUND])
                .await
                .map_err(write)?;
            let now = object_sha(&reference).to_ascii_lowercase();
            return Err(if status == StatusCode::NOT_FOUND {
                head_moved(format!(
                    "branch {branch} was deleted from {repo} while the fixes were being committed ({}). Nothing was committed.",
                    said(&answer)
                ))
            } else if now != expected {
                moved(&now)
            } else {
                StepOutcome::failed(
                    diagnosis::UNKNOWN,
                    format!(
                        "GitHub refused to move {branch} to the commit of the fixes: {}. The branch is unchanged.",
                        said(&answer)
                    ),
                )
            });
        }
        ctx.logs.line(format!(
            "committed {} as {sha} on {branch}",
            count(blobs.len(), "file")
        ));
        Ok(Self::committed(
            &repository,
            &commit,
            &sha,
            format!(
                "committed {} to {target} as {}",
                count(blobs.len(), "file"),
                short(&sha)
            ),
        ))
    }

    /// The success of `repo.commit_fixes` with `commit_sha` and
    /// `commit_url` (the commit's own `html_url`; the repository's page for
    /// the commit when the git object carries none).
    fn committed(repository: &Value, commit: &Value, sha: &str, what: String) -> StepOutcome {
        let mut url = str_field(commit, "html_url");
        if url.is_empty() {
            let page = str_field(repository, "html_url");
            if !page.is_empty() {
                url = format!("{}/commit/{sha}", page.trim_end_matches('/'));
            }
        }
        let mut outputs = BTreeMap::from([("commit_sha".to_owned(), sha.to_owned())]);
        let output = if url.is_empty() {
            what
        } else {
            outputs.insert("commit_url".to_owned(), url.clone());
            format!("{what}: {url}")
        };
        StepOutcome::Succeeded { output, outputs }
    }

    /// The commits between `expected` and `head` that an earlier attempt
    /// of this step could have made: `expected` is their only parent and
    /// `message` their message. Empty when `head` does not descend from
    /// `expected` (the history was rewritten) or nothing matches. Items are
    /// the commits as the compare API lists them (`sha`, `html_url`,
    /// `commit.tree.sha`), oldest first.
    async fn earlier_attempts(
        &self,
        ctx: &RunContext,
        base: &str,
        expected: &str,
        head: &str,
        message: &str,
    ) -> Result<Vec<Value>, ApiError> {
        let (status, comparison) = self
            .call(
                ctx,
                Method::GET,
                &format!("{base}/compare/{expected}...{head}?per_page=100"),
                None,
                &[StatusCode::NOT_FOUND],
            )
            .await?;
        if status == StatusCode::NOT_FOUND || str_field(&comparison, "status") != "ahead" {
            return Ok(Vec::new());
        }
        Ok(comparison
            .get("commits")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|commit| parents_of(commit) == [expected])
            .filter(|commit| {
                str_field(commit.get("commit").unwrap_or(&Value::Null), "message").trim_end()
                    == message.trim_end()
            })
            .cloned()
            .collect())
    }

    /// What each of `paths` is in the tree `root`, for those that are there
    /// (or cannot be, because a file is in the way). Found by walking only
    /// the directories those paths go through: a recursive listing of a
    /// large repository is megabytes, and GitHub truncates it. A path that
    /// is simply not there yet is absent from the map.
    async fn existing(
        &self,
        ctx: &RunContext,
        base: &str,
        root: &str,
        paths: &[String],
    ) -> Result<BTreeMap<String, Existing>, ApiError> {
        // Directory path ("" is the root) -> name -> (mode, type, sha).
        let mut listings: BTreeMap<String, BTreeMap<String, (String, String, String)>> =
            BTreeMap::new();
        listings.insert(String::new(), self.tree_entries(ctx, base, root).await?);
        let mut modes = BTreeMap::new();
        for path in paths {
            let mut directory = String::new();
            let mut names = path.split('/').peekable();
            while let Some(name) = names.next() {
                let Some((mode, kind, sha)) = listings
                    .get(&directory)
                    .and_then(|entries| entries.get(name))
                    .cloned()
                else {
                    break;
                };
                let here = if directory.is_empty() {
                    name.to_owned()
                } else {
                    format!("{directory}/{name}")
                };
                if names.peek().is_none() {
                    modes.insert(path.clone(), Existing::Entry(mode));
                    break;
                }
                if kind != "tree" {
                    // A file where the path needs a directory: writing the
                    // path would replace that file with a directory.
                    modes.insert(path.clone(), Existing::Under(here));
                    break;
                }
                if !listings.contains_key(&here) {
                    let entries = self.tree_entries(ctx, base, &sha).await?;
                    listings.insert(here.clone(), entries);
                }
                directory = here;
            }
        }
        Ok(modes)
    }

    /// The direct entries of one tree: name -> (mode, type, sha).
    async fn tree_entries(
        &self,
        ctx: &RunContext,
        base: &str,
        tree: &str,
    ) -> Result<BTreeMap<String, (String, String, String)>, ApiError> {
        let (_, listing) = self
            .call(
                ctx,
                Method::GET,
                &format!("{base}/git/trees/{tree}"),
                None,
                &[],
            )
            .await?;
        Ok(listing
            .get("tree")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|entry| {
                (
                    str_field(entry, "path"),
                    (
                        str_field(entry, "mode"),
                        str_field(entry, "type"),
                        str_field(entry, "sha"),
                    ),
                )
            })
            .collect())
    }

    // --- repo.verify_mergeable ----------------------------------------------

    /// Reads the pull request again, right before the merge: still open,
    /// not a draft, at the head the merge was requested for, without
    /// conflicts, with every check passed and nothing on GitHub blocking it.
    pub(super) async fn verify_mergeable(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        bounded(self.verify_mergeable_inner(step, ctx)).await
    }

    async fn verify_mergeable_inner(&self, step: &StepInput, ctx: &RunContext) -> Decided {
        self.require_access(ctx, READ, READ_HINT)?;
        let repo = repo_of(step)?;
        let number = pull_number(step)?;
        let expected = expected_head(step)?;
        let read = |error: ApiError| refused(error, READ, READ_HINT);

        // GitHub computes `mergeable` in the background after the first
        // read: ask a few times before saying it is not known.
        let give_up_at = Instant::now() + self.mergeable_wait;
        let pull = loop {
            let pull = self.pull(ctx, &repo, number).await.map_err(read)?;
            if pull.merged {
                return Ok(StepOutcome::Succeeded {
                    output: format!(
                        "pull request #{number} is already merged{}; there is nothing left to verify",
                        pull.merge_commit_note()
                    ),
                    outputs: pull.outputs(),
                });
            }
            if !pull.open {
                return Err(blocked(format!(
                    "pull request #{number} is closed and was not merged; reopen it on GitHub before merging it. Nothing was merged."
                )));
            }
            if pull.draft {
                return Err(blocked(format!(
                    "pull request #{number} is still a draft; mark it ready for review on GitHub first. Nothing was merged."
                )));
            }
            if pull.head_sha != expected {
                return Err(head_moved(format!(
                    "pull request #{number} is at {} now; this merge was requested for {}. Nothing was merged.",
                    short(&pull.head_sha),
                    short(&expected)
                )));
            }
            if pull.mergeable.is_some() {
                break pull;
            }
            if Instant::now() + self.mergeable_poll > give_up_at {
                return Err(blocked(format!(
                    "GitHub has not finished computing whether pull request #{number} can be merged (asked for {}). Nothing was merged; retry in a moment.",
                    span(self.mergeable_wait)
                )));
            }
            ctx.logs.line(format!(
                "GitHub is still computing whether #{number} can be merged; asking again"
            ));
            tokio::time::sleep(self.mergeable_poll).await;
        };
        if pull.mergeable == Some(false) {
            return Err(StepOutcome::failed(
                diagnosis::MERGE_CONFLICT,
                format!(
                    "pull request #{number} conflicts with {}: {} cannot be merged into it until the conflicts are resolved. Nothing was merged.",
                    pull.base_ref, pull.head_ref
                ),
            ));
        }

        let checks = self.checks(ctx, &repo, &expected).await.map_err(read)?;
        if let Some(sentence) = checks_sentence(&checks) {
            return Err(StepOutcome::failed(
                diagnosis::CHECKS_FAILING,
                format!(
                    "pull request #{number} is not ready to merge at {}: {sentence}. Nothing was merged.",
                    short(&expected)
                ),
            ));
        }
        if pull.mergeable_state == "blocked" {
            return Err(blocked(format!(
                "GitHub reports pull request #{number} as blocked: a branch protection rule on {} is not satisfied (a required review, a required check that has not reported, or a restriction on who may merge). Nothing was merged.",
                pull.base_ref
            )));
        }
        Ok(StepOutcome::Succeeded {
            output: format!(
                "pull request #{number} can be merged: {} at {} has no conflicts with {} and {}",
                pull.head_ref,
                short(&expected),
                pull.base_ref,
                match checks.len() {
                    0 => "no checks reported on it".to_owned(),
                    1 => "its 1 check passed".to_owned(),
                    n => format!("all {n} checks passed"),
                }
            ),
            outputs: pull.outputs(),
        })
    }

    /// The checks of a commit, one state per name, from its check runs and
    /// its commit statuses (CONTRACT.md §23.3). GitHub lists the latest run
    /// of each check; where a name is still reported more than once (two
    /// apps, a run and a status) the worst state counts.
    async fn checks(
        &self,
        ctx: &RunContext,
        repo: &str,
        sha: &str,
    ) -> Result<BTreeMap<String, Check>, ApiError> {
        let commit = format!("/repos/{repo}/commits/{sha}");
        let mut checks = BTreeMap::new();
        for run in self
            .listed(
                ctx,
                &format!("{commit}/check-runs?per_page=100"),
                "check_runs",
            )
            .await?
        {
            fold(&mut checks, str_field(&run, "name"), Check::of_run(&run));
        }
        for status in self
            .listed(ctx, &format!("{commit}/status?per_page=100"), "statuses")
            .await?
        {
            fold(
                &mut checks,
                str_field(&status, "context"),
                Check::of_status(&str_field(&status, "state")),
            );
        }
        Ok(checks)
    }

    // --- repo.merge_pull_request --------------------------------------------

    /// Merges the pull request at exactly `params.expected_head_sha`:
    /// GitHub refuses the merge itself (409) when the head is another
    /// commit by then. A pull request that is already merged is success
    /// with its merge commit, so a re-run never merges twice.
    pub(super) async fn merge_pull_request(
        &self,
        step: &StepInput,
        ctx: &RunContext,
    ) -> StepOutcome {
        bounded(self.merge_pull_request_inner(step, ctx)).await
    }

    async fn merge_pull_request_inner(&self, step: &StepInput, ctx: &RunContext) -> Decided {
        self.require_access(ctx, WRITE, WRITE_HINT)?;
        let repo = repo_of(step)?;
        let number = pull_number(step)?;
        let expected = expected_head(step)?;
        let method = match param(step, "merge_method") {
            method @ ("merge" | "squash" | "rebase") => method,
            other => {
                return Err(bad_plan(format!(
                    "params.merge_method must be merge, squash or rebase, got '{other}'"
                )));
            }
        };

        let pull = self
            .pull(ctx, &repo, number)
            .await
            .map_err(|error| refused(error, READ, READ_HINT))?;
        if pull.merged {
            let mut outputs = pull.outputs();
            if !pull.merge_commit_sha.is_empty() {
                outputs.insert("merge_commit_sha".to_owned(), pull.merge_commit_sha.clone());
            }
            return Ok(StepOutcome::Succeeded {
                output: format!(
                    "pull request #{number} is already merged{}; nothing was merged again",
                    pull.merge_commit_note()
                ),
                outputs,
            });
        }
        if !pull.open {
            return Err(blocked(format!(
                "pull request #{number} is closed and was not merged; reopen it on GitHub before merging it. Nothing was merged."
            )));
        }
        if pull.head_sha != expected {
            return Err(head_moved(format!(
                "pull request #{number} is at {} now; this merge was requested for {}. Nothing was merged.",
                short(&pull.head_sha),
                short(&expected)
            )));
        }

        let mut body = json!({ "sha": expected, "merge_method": method });
        let title = param(step, "commit_title");
        if !title.is_empty() {
            body["commit_title"] = json!(title);
        }
        let (status, answer) = self
            .call(
                ctx,
                Method::PUT,
                &format!("/repos/{repo}/pulls/{number}/merge"),
                Some(body),
                &[
                    StatusCode::CONFLICT,
                    StatusCode::METHOD_NOT_ALLOWED,
                    StatusCode::UNPROCESSABLE_ENTITY,
                ],
            )
            .await
            .map_err(|error| refused(error, WRITE, WRITE_HINT))?;
        match status {
            StatusCode::CONFLICT => Err(head_moved(format!(
                "GitHub refused to merge pull request #{number} at {}: {}. Nothing was merged.",
                short(&expected),
                said(&answer)
            ))),
            StatusCode::METHOD_NOT_ALLOWED | StatusCode::UNPROCESSABLE_ENTITY => {
                Err(blocked(format!(
                    "GitHub refused to merge pull request #{number}: {}. Nothing was merged.",
                    said(&answer)
                )))
            }
            _ => {
                let sha = str_field(&answer, "sha");
                let merged = answer
                    .get("merged")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if !merged || sha.is_empty() {
                    return Err(blocked(format!(
                        "GitHub answered the merge of pull request #{number} without a merge commit: {}",
                        said(&answer)
                    )));
                }
                ctx.logs
                    .line(format!("merged #{number} with {method} as {sha}"));
                let mut outputs = pull.outputs();
                outputs.insert("merge_commit_sha".to_owned(), sha.clone());
                Ok(StepOutcome::Succeeded {
                    output: format!(
                        "merged pull request #{number} into {} with {method} as {}{}",
                        pull.base_ref,
                        short(&sha),
                        if pull.url.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", pull.url)
                        }
                    ),
                    outputs,
                })
            }
        }
    }

    // --- repo.delete_branch -------------------------------------------------

    /// Deletes a merged pull request's branch, and only that: the pull
    /// request must be merged, `params.branch` must be its head branch in
    /// this repository (never a fork's), not the default branch, still at
    /// the merged head and not the base of another open pull request.
    /// Anything else leaves the branch in place and says why; a branch that
    /// is already gone is success too.
    pub(super) async fn delete_branch(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        bounded(self.delete_branch_inner(step, ctx)).await
    }

    async fn delete_branch_inner(&self, step: &StepInput, ctx: &RunContext) -> Decided {
        self.require_access(ctx, WRITE, WRITE_HINT)?;
        let repo = repo_of(step)?;
        let branch = branch_of(step)?;
        let number = pull_number(step)?;
        let read = |error: ApiError| refused(error, READ, READ_HINT);
        let kept = |reason: String| {
            ctx.logs
                .line(format!("leaving {branch} in place: {reason}"));
            Ok(StepOutcome::ok(format!(
                "left {branch} in place: {reason}. Nothing was deleted."
            )))
        };
        let gone = || {
            Ok(StepOutcome::ok(format!(
                "{branch} is already gone from {repo}; there was nothing to delete"
            )))
        };

        let pull = self.pull(ctx, &repo, number).await.map_err(read)?;
        if !pull.merged {
            return kept(format!("pull request #{number} is not merged"));
        }
        if pull.head_ref != branch {
            return kept(format!(
                "the head branch of pull request #{number} is {}, not {branch}",
                pull.head_ref
            ));
        }
        if pull.head_repo.is_empty() {
            return kept(format!(
                "pull request #{number} came from a repository that no longer exists, not from {repo}"
            ));
        }
        if !pull.head_repo.eq_ignore_ascii_case(&repo) {
            return kept(format!(
                "pull request #{number} came from the fork {}, and only branches of {repo} are deleted",
                pull.head_repo
            ));
        }
        let default_branch = self.default_branch(ctx, &repo).await.map_err(read)?;
        if branch == default_branch {
            return kept(format!("it is the default branch of {repo}"));
        }

        let base = format!("/repos/{repo}");
        let (status, reference) = self
            .call(
                ctx,
                Method::GET,
                &format!("{base}/git/ref/heads/{}", encode_path(branch)),
                None,
                &[StatusCode::NOT_FOUND],
            )
            .await
            .map_err(read)?;
        if status == StatusCode::NOT_FOUND {
            return gone();
        }
        let head = object_sha(&reference).to_ascii_lowercase();
        if head != pull.head_sha {
            return kept(format!(
                "it is at {} now, with commits that were not part of pull request #{number} (merged at {})",
                short(&head),
                short(&pull.head_sha)
            ));
        }
        // Deleting the base of an open pull request closes that pull request.
        let (_, dependents) = self
            .call(
                ctx,
                Method::GET,
                &format!(
                    "{base}/pulls?state=open&base={}&per_page=1",
                    encode_path(branch)
                ),
                None,
                &[],
            )
            .await
            .map_err(read)?;
        if let Some(dependent) = dependents.as_array().and_then(|pulls| pulls.first()) {
            return kept(format!(
                "pull request #{} is still open against it and would be closed with it",
                dependent.get("number").and_then(Value::as_u64).unwrap_or(0)
            ));
        }

        let (status, answer) = self
            .call(
                ctx,
                Method::DELETE,
                &format!("{base}/git/refs/heads/{}", encode_path(branch)),
                None,
                &[StatusCode::NOT_FOUND, StatusCode::UNPROCESSABLE_ENTITY],
            )
            .await
            .map_err(|error| refused(error, WRITE, WRITE_HINT))?;
        match status {
            StatusCode::NOT_FOUND => gone(),
            StatusCode::UNPROCESSABLE_ENTITY => {
                let reason = said(&answer);
                if reason.to_ascii_lowercase().contains("does not exist") {
                    gone()
                } else {
                    kept(format!("GitHub refused to delete it ({reason})"))
                }
            }
            _ => {
                ctx.logs.line(format!("deleted refs/heads/{branch}"));
                Ok(StepOutcome::ok(format!(
                    "deleted {branch} from {repo}: pull request #{number} is merged and the branch had nothing else on it"
                )))
            }
        }
    }

    // --- repo.enable_security_updates ---------------------------------------

    /// Turns on Dependabot alerts and, when asked, Dependabot security
    /// updates. Both calls are idempotent on GitHub's side (204 whether or
    /// not it was on already), so a re-run changes nothing.
    pub(super) async fn enable_security_updates(
        &self,
        step: &StepInput,
        ctx: &RunContext,
    ) -> StepOutcome {
        bounded(self.enable_security_updates_inner(step, ctx)).await
    }

    async fn enable_security_updates_inner(&self, step: &StepInput, ctx: &RunContext) -> Decided {
        self.require_access(ctx, ADMIN, ADMIN_HINT)?;
        let repo = repo_of(step)?;
        let alerts = flag(step, "alerts")?;
        let security_updates = flag(step, "security_updates")?;
        if !alerts && !security_updates {
            return Err(bad_plan(
                "params.alerts and params.security_updates are both false: there is nothing to turn on",
            ));
        }
        let alerts_path = format!("/repos/{repo}/vulnerability-alerts");

        if alerts {
            self.call(ctx, Method::PUT, &alerts_path, None, &[])
                .await
                .map_err(|error| refused(error, ADMIN, ADMIN_HINT))?;
            ctx.logs
                .line(format!("Dependabot alerts are on for {repo}"));
        }
        if security_updates
            && let Err(error) = self
                .call(
                    ctx,
                    Method::PUT,
                    &format!("/repos/{repo}/automated-security-fixes"),
                    None,
                    &[],
                )
                .await
        {
            // GitHub only turns security updates on for a repository whose
            // alerts are on. When this run was not asked to turn those on
            // and GitHub's refusal is about them, that is the answer.
            let failure = if !alerts && self.alerts_are_off(ctx, &repo, &error).await {
                StepOutcome::failed(
                    diagnosis::UNKNOWN,
                    format!(
                        "GitHub refused to turn on Dependabot security updates for {repo} because Dependabot alerts are off there, and alerts have to be on first ({}). Nothing was changed; ask again with alerts included.",
                        error.describe()
                    ),
                )
            } else {
                refused(error, ADMIN, ADMIN_HINT)
            };
            return Err(match failure {
                StepOutcome::Failed {
                    code,
                    raw_output,
                    missing_permissions,
                } if alerts => StepOutcome::Failed {
                    code,
                    raw_output: format!(
                        "Dependabot alerts are on for {repo}, but security updates could not be turned on.\n{raw_output}"
                    ),
                    missing_permissions,
                },
                other => other,
            });
        }

        let turned_on = match (alerts, security_updates) {
            (true, true) => "Dependabot alerts and Dependabot security updates are",
            (true, false) => "Dependabot alerts are",
            _ => "Dependabot security updates are",
        };
        Ok(StepOutcome::ok(format!("{turned_on} on for {repo}")))
    }

    /// Whether `refusal` of the security-updates call is explained by
    /// Dependabot alerts being off, as far as this access can tell: GitHub
    /// says alerts are disabled (404 on the read) and that 404 is not just
    /// this token being unable to see the setting. A refusal that is not
    /// about access (422) already proves the token could reach it;
    /// otherwise the repository has to say this user is an admin.
    async fn alerts_are_off(&self, ctx: &RunContext, repo: &str, refusal: &ApiError) -> bool {
        let Some(refusal) = refusal.status() else {
            return false;
        };
        let disabled = matches!(
            self.call(
                ctx,
                Method::GET,
                &format!("/repos/{repo}/vulnerability-alerts"),
                None,
                &[StatusCode::NOT_FOUND],
            )
            .await,
            Ok((StatusCode::NOT_FOUND, _))
        );
        if !disabled {
            return false;
        }
        if !matches!(
            refusal,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
        ) {
            return true;
        }
        match self
            .call(ctx, Method::GET, &format!("/repos/{repo}"), None, &[])
            .await
        {
            Ok((_, repository)) => repository
                .get("permissions")
                .and_then(|permissions| permissions.get("admin"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            Err(_) => false,
        }
    }
}

/// The five operations and the `repo.open_pull_request` additions against a
/// repository that keeps real git state (`tests::fakes::repo`): every
/// assertion on a commit, a branch or a merge reads what the adapter left
/// there.
#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::{
        adapters::{
            Adapter, COMMIT_FIXES, DELETE_BRANCH, ENABLE_SECURITY_UPDATES, GITHUB, GithubAccess,
            MERGE_PULL_REQUEST, OPEN_PULL_REQUEST, VERIFY_MERGEABLE,
        },
        tests::fakes::repo::{self as fake, FakeRepo},
    };

    const REPO: &str = "acme/api";
    const BRANCH: &str = "fix/lint";
    const BROKEN: &str = "pub fn add(a: i32, b: i32) -> i32 {\n    a - b\n}\n";
    const FIXED: &str = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";

    fn adapter() -> GithubAdapter {
        // The process-wide configuration points nowhere with a token the
        // repository rejects: only the workspace's access can work.
        GithubAdapter::new(Some("process-wide-token".into()), "http://127.0.0.1:1")
            .with_mergeable_poll(Duration::from_millis(5), Duration::from_millis(60))
    }

    fn ctx(api: &str, artifacts: &[(&str, &str)]) -> RunContext {
        RunContext {
            execution_id: Uuid::now_v7(),
            execution_number: 77,
            artifacts: artifacts
                .iter()
                .map(|(path, content)| ((*path).to_owned(), (*content).to_owned()))
                .collect(),
            github: Some(GithubAccess {
                token: fake::TOKEN.into(),
                api_base_url: api.to_owned(),
            }),
            ..Default::default()
        }
    }

    fn step(action_type: &str, params: &[(&str, &str)]) -> StepInput {
        let mut params: BTreeMap<String, String> = params
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        params
            .entry("repo".to_owned())
            .or_insert_with(|| REPO.to_owned());
        StepInput {
            key: action_type.replace('.', "-"),
            title: action_type.to_owned(),
            action_type: action_type.to_owned(),
            adapter: GITHUB.to_owned(),
            params,
            attempt: 1,
        }
    }

    #[track_caller]
    fn success(outcome: StepOutcome) -> (String, BTreeMap<String, String>) {
        match outcome {
            StepOutcome::Succeeded { output, outputs } => (output, outputs),
            other => panic!("expected a success: {other:?}"),
        }
    }

    #[track_caller]
    fn failure(outcome: StepOutcome) -> (String, String, Vec<String>) {
        match outcome {
            StepOutcome::Failed {
                code,
                raw_output,
                missing_permissions,
            } => {
                assert!(
                    !raw_output.contains(fake::TOKEN),
                    "a failure never quotes the token: {raw_output}"
                );
                assert!(
                    !raw_output.contains('?'),
                    "a failure never quotes a query string: {raw_output}"
                );
                (code, raw_output, missing_permissions)
            }
            other => panic!("expected a failure: {other:?}"),
        }
    }

    /// `acme/api` with pull request #1 from `fix/lint`, one commit ahead of
    /// `main`. Returns the repository, its API base, the pull request's
    /// number and its head.
    async fn repository() -> (FakeRepo, String, u64, String) {
        let repo = FakeRepo::new(
            REPO,
            &[
                ("README.md", "# api\n"),
                (
                    "src/lib.rs",
                    "pub fn add(a: i32, b: i32) -> i32 {\n    0\n}\n",
                ),
            ],
        );
        repo.branch(BRANCH, "main");
        let head = repo.push(BRANCH, &[("src/lib.rs", BROKEN)], "Implement add");
        let number = repo.open_pull(BRANCH, "Implement add");
        let api = repo.serve().await;
        (repo, api, number, head)
    }

    fn fixes(head: &str, number: u64) -> StepInput {
        step(
            COMMIT_FIXES,
            &[
                ("branch", BRANCH),
                ("expected_head_sha", head),
                ("message", "Apply 1 review fix"),
                ("pull_request", &number.to_string()),
            ],
        )
    }

    fn pull_step(action_type: &str, head: &str, number: u64) -> StepInput {
        step(
            action_type,
            &[
                ("pull_request", &number.to_string()),
                ("expected_head_sha", head),
                ("merge_method", "squash"),
                ("branch", BRANCH),
            ],
        )
    }

    // --- repo.commit_fixes --------------------------------------------------

    #[tokio::test]
    async fn fixes_become_one_commit_on_top_of_the_reviewed_head() {
        let (repo, api, number, head) = repository().await;
        let main = repo.head("main").unwrap();
        let ctx = ctx(
            &api,
            &[("src/lib.rs", FIXED), ("docs/fixes.md", "Use a plus.\n")],
        );
        let (output, outputs) = success(adapter().run(&fixes(&head, number), &ctx).await);

        let sha = outputs["commit_sha"].clone();
        assert_eq!(repo.head(BRANCH).as_deref(), Some(sha.as_str()));
        assert_eq!(
            outputs["commit_url"],
            format!("https://github.com/{REPO}/commit/{sha}")
        );
        assert!(output.contains("committed 2 files"), "{output}");
        assert!(output.contains(&outputs["commit_url"]), "{output}");
        // One commit, whose only parent is the head the review saw.
        let (parents, message) = repo.commit(&sha);
        assert_eq!(parents, std::slice::from_ref(&head));
        assert_eq!(message, "Apply 1 review fix");
        assert_eq!(repo.commits_since(BRANCH, &head), 1);
        // The files are there in full, and what was not touched still is.
        assert_eq!(repo.file(BRANCH, "src/lib.rs").as_deref(), Some(FIXED));
        assert_eq!(
            repo.file(BRANCH, "docs/fixes.md").as_deref(),
            Some("Use a plus.\n")
        );
        assert_eq!(repo.file(BRANCH, "README.md").as_deref(), Some("# api\n"));
        // The pull request follows its branch; the default branch did not move.
        assert_eq!(repo.pull(number).head_sha, sha);
        assert_eq!(repo.head("main"), Some(main));
        assert_eq!(repo.file("main", "docs/fixes.md"), None);
    }

    #[tokio::test]
    async fn an_executable_stays_executable_and_a_symlink_is_never_replaced() {
        let (repo, api, number, _) = repository().await;
        repo.push_with_mode(
            BRANCH,
            "scripts/run.sh",
            fake::EXECUTABLE,
            "#!/bin/sh\napi\n",
        );
        let head = repo.push_with_mode(BRANCH, "docs/latest", fake::SYMLINK, "README.md");

        let ctx_script = ctx(&api, &[("scripts/run.sh", "#!/bin/sh\nexec api\n")]);
        let (_, outputs) = success(adapter().run(&fixes(&head, number), &ctx_script).await);
        assert_eq!(
            repo.mode(BRANCH, "scripts/run.sh").as_deref(),
            Some(fake::EXECUTABLE)
        );
        assert_eq!(
            repo.file(BRANCH, "scripts/run.sh").as_deref(),
            Some("#!/bin/sh\nexec api\n")
        );

        let head = outputs["commit_sha"].clone();
        repo.forget_calls();
        let ctx_link = ctx(&api, &[("docs/latest", "# not a link any more\n")]);
        let (code, raw, _) = failure(adapter().run(&fixes(&head, number), &ctx_link).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw.contains("docs/latest is not a regular file"), "{raw}");
        assert!(raw.contains("120000"), "{raw}");
        assert!(repo.writes().is_empty(), "{:?}", repo.writes());
        assert_eq!(repo.head(BRANCH), Some(head));
    }

    #[tokio::test]
    async fn a_branch_that_moved_is_head_moved_and_nothing_is_committed() {
        let (repo, api, number, reviewed) = repository().await;
        let ctx = ctx(&api, &[("src/lib.rs", FIXED)]);

        // The author pushed twice after the review.
        repo.push(BRANCH, &[("README.md", "# api v2\n")], "Update the readme");
        let pushed = repo.push(BRANCH, &[("README.md", "# api v3\n")], "Again");
        repo.forget_calls();
        let (code, raw, _) = failure(adapter().run(&fixes(&reviewed, number), &ctx).await);
        assert_eq!(code, diagnosis::HEAD_MOVED);
        assert!(raw.contains(short(&pushed)), "{raw}");
        assert!(raw.contains(short(&reviewed)), "{raw}");
        assert!(raw.contains("Nothing was committed"), "{raw}");
        assert!(
            repo.writes().is_empty(),
            "a moved head is noticed before anything is sent: {:?}",
            repo.writes()
        );
        assert_eq!(repo.head(BRANCH), Some(pushed.clone()));
        assert_eq!(repo.file(BRANCH, "src/lib.rs").as_deref(), Some(BROKEN));

        // The branch was rewound behind the reviewed head (a force push).
        let main = repo.head("main").unwrap();
        repo.branch(BRANCH, "main");
        let (code, _, _) = failure(adapter().run(&fixes(&reviewed, number), &ctx).await);
        assert_eq!(code, diagnosis::HEAD_MOVED);
        assert_eq!(repo.head(BRANCH), Some(main));
    }

    /// A foreign commit directly on top of the reviewed head is not an
    /// interrupted attempt of ours: not when it merely sits where ours
    /// would, and not when it even carries the same message. The second
    /// case is only told apart by its tree; finding that out uploads the
    /// files and builds the tree, objects no branch points to, and still
    /// makes no commit and moves no ref.
    #[tokio::test]
    async fn somebody_elses_commit_on_the_reviewed_head_is_not_mistaken_for_ours() {
        let (repo, api, number, reviewed) = repository().await;
        let theirs = repo.push(BRANCH, &[("README.md", "# theirs\n")], "Theirs");
        repo.forget_calls();
        let ctx = ctx(&api, &[("src/lib.rs", FIXED)]);
        let (code, raw, _) = failure(adapter().run(&fixes(&reviewed, number), &ctx).await);
        assert_eq!(code, diagnosis::HEAD_MOVED);
        assert!(raw.contains(short(&theirs)), "{raw}");
        assert_eq!(repo.head(BRANCH), Some(theirs));
        assert!(repo.writes().is_empty(), "{:?}", repo.writes());

        let (repo, api, number, reviewed) = repository().await;
        let theirs = repo.push(
            BRANCH,
            &[(
                "src/lib.rs",
                "pub fn add(a: i32, b: i32) -> i32 {\n    b + a\n}\n",
            )],
            "Apply 1 review fix",
        );
        let ctx = self::ctx(&api, &[("src/lib.rs", FIXED)]);
        let (code, raw, _) = failure(adapter().run(&fixes(&reviewed, number), &ctx).await);
        assert_eq!(code, diagnosis::HEAD_MOVED);
        assert!(raw.contains(short(&theirs)), "{raw}");
        assert_eq!(repo.head(BRANCH), Some(theirs));
        assert!(
            !repo
                .writes()
                .iter()
                .any(|call| call.starts_with("PATCH") || call.ends_with("/git/commits")),
            "no commit and no ref update: {:?}",
            repo.writes()
        );
    }

    #[tokio::test]
    async fn a_push_that_lands_during_the_commit_loses_nothing() {
        let (repo, api, number, reviewed) = repository().await;
        let ctx = ctx(&api, &[("src/lib.rs", FIXED)]);

        // The branch is where it should be when it is read, and somebody
        // pushes right before the ref update arrives: not a fast-forward.
        repo.behave(|b| {
            b.push_before_ref_update = Some((BRANCH.into(), "NOTES.md".into(), "theirs\n".into()));
        });
        let (code, raw, _) = failure(adapter().run(&fixes(&reviewed, number), &ctx).await);
        assert_eq!(code, diagnosis::HEAD_MOVED);
        assert!(raw.contains("Nothing was committed"), "{raw}");
        // Their commit is the head and their file is there; the fix is not.
        let head = repo.head(BRANCH).unwrap();
        assert_eq!(repo.commit(&head).1, "Somebody else's commit");
        assert_eq!(repo.file(BRANCH, "NOTES.md").as_deref(), Some("theirs\n"));
        assert_eq!(repo.file(BRANCH, "src/lib.rs").as_deref(), Some(BROKEN));

        // A refusal that is not about the head (a rule on the branch) is
        // reported as what GitHub said, not as a moved head.
        let head = repo.head(BRANCH).unwrap();
        repo.behave(|b| {
            b.ref_update_refusal = Some("Changes must be made through a pull request.".into())
        });
        let (code, raw, _) = failure(adapter().run(&fixes(&head, number), &ctx).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(
            raw.contains("Changes must be made through a pull request"),
            "{raw}"
        );
        assert!(raw.contains("The branch is unchanged"), "{raw}");
        assert_eq!(repo.head(BRANCH), Some(head));

        // The branch is gone altogether.
        let gone = repo.head(BRANCH).unwrap();
        let api_gone = {
            let other = FakeRepo::new(REPO, &[("README.md", "# api\n")]);
            other.serve().await
        };
        let (code, raw, _) = failure(
            adapter()
                .run(&fixes(&gone, number), &self::ctx(&api_gone, &[("a", "b")]))
                .await,
        );
        assert_eq!(code, diagnosis::HEAD_MOVED);
        assert!(raw.contains("no longer exists"), "{raw}");
    }

    #[tokio::test]
    async fn a_rerun_after_a_crash_finds_its_own_commit_and_commits_nothing_again() {
        let (repo, api, number, reviewed) = repository().await;
        let ctx = ctx(&api, &[("src/lib.rs", FIXED)]);
        let (_, first) = success(adapter().run(&fixes(&reviewed, number), &ctx).await);

        // The step was not checkpointed (the service stopped): it runs
        // again with the same plan, against a branch it already moved.
        repo.forget_calls();
        let (output, second) = success(adapter().run(&fixes(&reviewed, number), &ctx).await);
        assert_eq!(second["commit_sha"], first["commit_sha"]);
        assert_eq!(second["commit_url"], first["commit_url"]);
        assert!(output.contains("already on"), "{output}");
        assert!(output.contains("nothing was committed again"), "{output}");
        assert_eq!(repo.commits_since(BRANCH, &reviewed), 1);
        assert_eq!(
            repo.head(BRANCH).as_deref(),
            Some(first["commit_sha"].as_str())
        );
        assert!(
            !repo
                .writes()
                .iter()
                .any(|call| call.starts_with("PATCH") || call.ends_with("/git/commits")),
            "no second commit and no ref update: {:?}",
            repo.writes()
        );

        // The author kept working on top of that commit before the step
        // ran again: the fixes are still on the branch, exactly once, and
        // the step says where the branch is now instead of claiming that
        // nothing was committed.
        let later = repo.push(BRANCH, &[("README.md", "# api v2\n")], "Update the readme");
        repo.forget_calls();
        let (output, third) = success(adapter().run(&fixes(&reviewed, number), &ctx).await);
        assert_eq!(third["commit_sha"], first["commit_sha"]);
        assert_eq!(third["commit_url"], first["commit_url"]);
        assert!(
            output.contains(&format!(
                "the branch has moved on to {} since",
                short(&later)
            )),
            "{output}"
        );
        assert_eq!(repo.head(BRANCH), Some(later));
        assert_eq!(repo.commits_since(BRANCH, &reviewed), 2);
        assert!(
            !repo
                .writes()
                .iter()
                .any(|call| call.starts_with("PATCH") || call.ends_with("/git/commits")),
            "{:?}",
            repo.writes()
        );
    }

    #[tokio::test]
    async fn plans_that_must_not_run_are_refused_before_anything_is_written() {
        let (repo, api, number, head) = repository().await;
        let main = repo.head("main").unwrap();
        let with_fix = ctx(&api, &[("src/lib.rs", FIXED)]);
        repo.forget_calls();

        // No files: never an empty commit.
        let (code, raw, _) = failure(adapter().run(&fixes(&head, number), &ctx(&api, &[])).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw.contains("nothing to commit"), "{raw}");
        assert!(repo.calls().is_empty(), "{:?}", repo.calls());

        // Files that change nothing: never an empty commit either.
        let (code, raw, _) = failure(
            adapter()
                .run(&fixes(&head, number), &ctx(&api, &[("src/lib.rs", BROKEN)]))
                .await,
        );
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw.contains("every file already has this content"), "{raw}");
        assert_eq!(repo.head(BRANCH), Some(head.clone()));

        // The default branch is never committed to.
        let on_main = step(
            COMMIT_FIXES,
            &[
                ("branch", "main"),
                ("expected_head_sha", &main),
                ("pull_request", "1"),
            ],
        );
        let (code, raw, _) = failure(adapter().run(&on_main, &with_fix).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw.contains("default branch"), "{raw}");
        assert_eq!(repo.head("main"), Some(main));

        // An abbreviated sha would read as a moved head forever.
        let (code, raw, _) = failure(adapter().run(&fixes(&head[..7], number), &with_fix).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw.contains("full commit sha"), "{raw}");
        for missing in ["branch", "expected_head_sha"] {
            let mut incomplete = fixes(&head, number);
            incomplete.params.remove(missing);
            let (code, raw, _) = failure(adapter().run(&incomplete, &with_fix).await);
            assert_eq!(code, diagnosis::UNKNOWN);
            assert!(
                raw.contains(&format!("params.{missing} is required")),
                "{raw}"
            );
        }
        let mut nowhere = fixes(&head, number);
        nowhere.params.remove("repo");
        let (code, raw, _) = failure(adapter().run(&nowhere, &with_fix).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw.contains("params.repo"), "{raw}");

        // Names that would address something else once they are part of a
        // request path never get that far.
        repo.forget_calls();
        for (key, value) in [
            ("branch", "../../../acme/other"),
            ("branch", "fix lint"),
            ("branch", "refs/heads/"),
            ("repo", "acme/.."),
            ("repo", "acme/api/extra"),
        ] {
            let mut crafted = fixes(&head, number);
            crafted.params.insert(key.into(), value.into());
            let (code, raw, _) = failure(adapter().run(&crafted, &with_fix).await);
            assert_eq!(code, diagnosis::UNKNOWN, "{key}={value}");
            assert!(raw.contains(&format!("params.{key}")), "{raw}");
        }
        assert!(repo.calls().is_empty(), "{:?}", repo.calls());
        // Files that are not paths inside the repository, or that would
        // turn an existing file into a directory.
        for (path, says) in [
            ("../outside.txt", "is not a path inside the repository"),
            ("src//lib.rs", "is not a path inside the repository"),
            (".git/config", "is not a path inside the repository"),
            (
                "README.md/notes.txt",
                "is inside README.md, which is a file",
            ),
            ("src", "is not a regular file (git mode 040000)"),
        ] {
            let (code, raw, _) = failure(
                adapter()
                    .run(&fixes(&head, number), &ctx(&api, &[(path, "x\n")]))
                    .await,
            );
            assert_eq!(code, diagnosis::UNKNOWN, "{path}");
            assert!(raw.contains(says), "{path}: {raw}");
            assert!(raw.contains("No commit was created"), "{raw}");
        }
        assert_eq!(repo.file(BRANCH, "README.md").as_deref(), Some("# api\n"));
        // Whatever was looked at on the way, no commit was made and no
        // branch moved.
        assert!(
            !repo
                .writes()
                .iter()
                .any(|call| !call.ends_with("/git/blobs") && !call.ends_with("/git/trees")),
            "{:?}",
            repo.writes()
        );
        assert_eq!(repo.head(BRANCH), Some(head));
    }

    #[tokio::test]
    async fn a_token_that_may_not_write_is_permission_denied_with_what_it_needs() {
        let (repo, api, number, head) = repository().await;
        repo.behave(|b| b.read_only = true);
        let with_fix = ctx(&api, &[("src/lib.rs", FIXED)]);
        let (code, raw, missing) = failure(adapter().run(&fixes(&head, number), &with_fix).await);
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(missing, [WRITE]);
        assert!(raw.contains("403"), "{raw}");
        assert!(raw.contains("contents: write"), "{raw}");
        assert_eq!(repo.head(BRANCH), Some(head.clone()));

        // No access at all: refused before a single request, and the
        // process-wide token is not a way around the workspace's.
        repo.forget_calls();
        let no_access = RunContext {
            github: None,
            ..with_fix.clone()
        };
        let nobody = GithubAdapter::new(None, &api);
        for action in [
            COMMIT_FIXES,
            VERIFY_MERGEABLE,
            MERGE_PULL_REQUEST,
            DELETE_BRANCH,
        ] {
            let (code, raw, _) = failure(
                nobody
                    .run(&pull_step(action, &head, number), &no_access)
                    .await,
            );
            assert_eq!(code, diagnosis::PERMISSION_DENIED, "{action}");
            assert!(raw.contains("no GitHub access"), "{raw}");
        }
        let (code, _, missing) = failure(
            nobody
                .run(
                    &step(ENABLE_SECURITY_UPDATES, &[("alerts", "true")]),
                    &no_access,
                )
                .await,
        );
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(missing, [ADMIN]);
        assert!(repo.calls().is_empty(), "{:?}", repo.calls());
    }

    /// GitHub Enterprise Server: the API lives under `/api/v3` of the
    /// workspace's own host, which only the run's context knows.
    #[tokio::test]
    async fn every_operation_goes_through_the_workspace_api_base() {
        let repo = FakeRepo::new(REPO, &[("src/lib.rs", BROKEN)]);
        repo.branch(BRANCH, "main");
        let head = repo.push(BRANCH, &[("README.md", "# api\n")], "Add a readme");
        let number = repo.open_pull(BRANCH, "Add a readme");
        let api = repo.serve_at("/api/v3").await;
        assert!(api.ends_with("/api/v3"));
        let ctx = ctx(&api, &[("src/lib.rs", FIXED)]);

        let (_, outputs) = success(adapter().run(&fixes(&head, number), &ctx).await);
        let head = outputs["commit_sha"].clone();
        success(
            adapter()
                .run(&pull_step(VERIFY_MERGEABLE, &head, number), &ctx)
                .await,
        );
        success(
            adapter()
                .run(&pull_step(MERGE_PULL_REQUEST, &head, number), &ctx)
                .await,
        );
        success(
            adapter()
                .run(&pull_step(DELETE_BRANCH, &head, number), &ctx)
                .await,
        );
        success(
            adapter()
                .run(
                    &step(
                        ENABLE_SECURITY_UPDATES,
                        &[("alerts", "true"), ("security_updates", "true")],
                    ),
                    &ctx,
                )
                .await,
        );
        assert!(repo.pull(number).merged);
        assert_eq!(repo.head(BRANCH), None);
        assert!(repo.alerts() && repo.security_updates());
        assert_eq!(repo.file("main", "src/lib.rs").as_deref(), Some(FIXED));
    }

    #[tokio::test]
    async fn failures_never_quote_a_token_or_a_query_string() {
        let (_, api, _, _) = repository().await;
        // A token the repository rejects, on a path with a query.
        let mut rejected = ctx(&api, &[]);
        rejected.github = Some(GithubAccess {
            token: "ghs_rejected_0123456789".into(),
            api_base_url: api.clone(),
        });
        let path = format!("/repos/{REPO}/pulls?state=open&base=secret-branch&per_page=1");
        let Err(error) = adapter()
            .call(&rejected, Method::GET, &path, None, &[])
            .await
        else {
            panic!("the repository accepted a rejected token");
        };
        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(
            error.describe(),
            format!("GET /repos/{REPO}/pulls -> 401 Unauthorized: Bad credentials")
        );

        // Nobody listening: the address is named, the query is not.
        let mut unreachable = ctx("http://127.0.0.1:1", &[]);
        unreachable.github.as_mut().unwrap().token = "ghs_unreachable_0123456789".into();
        let Err(error) = adapter()
            .call(&unreachable, Method::GET, &path, None, &[])
            .await
        else {
            panic!("nothing listens on port 1");
        };
        let text = error.describe();
        assert!(
            text.starts_with(&format!("GET http://127.0.0.1:1/repos/{REPO}/pulls failed")),
            "{text}"
        );
        assert!(
            !text.contains('?') && !text.contains("secret-branch"),
            "{text}"
        );
        assert!(!text.contains("ghs_unreachable"), "{text}");
        let outcome = refused(error, READ, READ_HINT);
        assert!(
            matches!(outcome, StepOutcome::Failed { ref code, .. } if code == diagnosis::UNKNOWN),
            "{outcome:?}"
        );
    }

    // --- repo.verify_mergeable ----------------------------------------------

    #[tokio::test]
    async fn a_clean_pull_request_with_green_checks_can_be_merged() {
        let (repo, api, number, head) = repository().await;
        repo.check_run(&head, "build", "completed", Some("success"));
        repo.check_run(&head, "docs", "completed", Some("skipped"));
        repo.check_run(&head, "coverage", "completed", Some("neutral"));
        repo.status(&head, "ci/legacy", "success");
        // A check that failed and was run again: the latest run counts.
        repo.check_run(&head, "lint", "completed", Some("failure"));
        repo.check_run(&head, "lint", "completed", Some("success"));
        repo.forget_calls();

        let (output, outputs) = success(
            adapter()
                .run(&pull_step(VERIFY_MERGEABLE, &head, number), &ctx(&api, &[]))
                .await,
        );
        assert!(output.contains("can be merged"), "{output}");
        assert!(output.contains("all 5 checks passed"), "{output}");
        assert_eq!(
            outputs["pull_request_url"],
            format!("https://github.com/{REPO}/pull/{number}")
        );
        let calls = repo.calls();
        assert!(
            calls.contains(&format!(
                "GET /repos/{REPO}/commits/{head}/check-runs?per_page=100"
            )),
            "{calls:?}"
        );
        assert!(
            calls
                .iter()
                .any(|call| call.starts_with(&format!("GET /repos/{REPO}/commits/{head}/status"))),
            "{calls:?}"
        );
        assert!(repo.writes().is_empty(), "verifying never writes");

        // No checks at all is said, not counted as "all passed".
        let (bare, api, number, head) = repository().await;
        let (output, _) = success(
            adapter()
                .run(&pull_step(VERIFY_MERGEABLE, &head, number), &ctx(&api, &[]))
                .await,
        );
        assert!(output.contains("no checks reported"), "{output}");
        assert!(bare.writes().is_empty());
    }

    #[tokio::test]
    async fn merged_closed_draft_and_moved_pull_requests_are_told_apart() {
        let (repo, api, number, head) = repository().await;
        let ctx = ctx(&api, &[]);
        let verify = |head: &str| pull_step(VERIFY_MERGEABLE, head, number);

        // Still a draft.
        repo.update_pull(number, |pull| pull.draft = true);
        let (code, raw, _) = failure(adapter().run(&verify(&head), &ctx).await);
        assert_eq!(code, diagnosis::MERGE_BLOCKED);
        assert!(raw.contains("still a draft"), "{raw}");
        repo.update_pull(number, |pull| pull.draft = false);

        // The head is no longer the one the merge was requested for.
        let pushed = repo.push(BRANCH, &[("README.md", "# more\n")], "More");
        let (code, raw, _) = failure(adapter().run(&verify(&head), &ctx).await);
        assert_eq!(code, diagnosis::HEAD_MOVED);
        assert!(
            raw.contains(short(&pushed)) && raw.contains(short(&head)),
            "{raw}"
        );

        // Closed without a merge.
        repo.update_pull(number, |pull| pull.open = false);
        let (code, raw, _) = failure(adapter().run(&verify(&pushed), &ctx).await);
        assert_eq!(code, diagnosis::MERGE_BLOCKED);
        assert!(raw.contains("closed and was not merged"), "{raw}");

        // Already merged (by a person, or by this run before a restart):
        // nothing to verify, and it says so. Before the merge GitHub keeps
        // a test merge sha in the same field, which was never taken for one.
        repo.update_pull(number, |pull| {
            pull.merged = true;
            pull.merge_commit_sha = "f".repeat(40);
        });
        let (output, outputs) = success(adapter().run(&verify(&head), &ctx).await);
        assert!(output.contains("already merged"), "{output}");
        assert!(output.contains("merge commit fffffff"), "{output}");
        assert!(outputs.contains_key("pull_request_url"));

        // A pull request that is not there (or that the token cannot see).
        let missing = pull_step(VERIFY_MERGEABLE, &head, 999);
        let (code, raw, permissions) = failure(adapter().run(&missing, &ctx).await);
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(permissions, [READ]);
        assert!(raw.contains("404"), "{raw}");
        let mut unnumbered = verify(&head);
        unnumbered
            .params
            .insert("pull_request".into(), "#12".into());
        let (code, raw, _) = failure(adapter().run(&unnumbered, &ctx).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw.contains("pull request number"), "{raw}");
        assert!(repo.writes().is_empty());
    }

    #[tokio::test]
    async fn mergeability_github_has_not_computed_is_asked_again_for_a_bounded_time() {
        let (repo, api, number, head) = repository().await;
        let ctx = ctx(&api, &[]);
        let verify = pull_step(VERIFY_MERGEABLE, &head, number);

        // Two reads say "computing", the third knows.
        repo.update_pull(number, |pull| pull.unknown_reads = 2);
        repo.forget_calls();
        success(adapter().run(&verify, &ctx).await);
        let reads = |repo: &FakeRepo| {
            repo.calls()
                .iter()
                .filter(|call| call.ends_with(&format!("/pulls/{number}")))
                .count()
        };
        assert_eq!(reads(&repo), 3);

        // It never finds out: a bounded number of reads, then MERGE_BLOCKED
        // that says why.
        repo.update_pull(number, |pull| pull.unknown_reads = usize::MAX);
        repo.forget_calls();
        let started = std::time::Instant::now();
        let (code, raw, _) = failure(adapter().run(&verify, &ctx).await);
        assert_eq!(code, diagnosis::MERGE_BLOCKED);
        assert!(raw.contains("has not finished computing"), "{raw}");
        assert!(raw.contains("retry in a moment"), "{raw}");
        assert!((2..=14).contains(&reads(&repo)), "{} reads", reads(&repo));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn conflicts_failing_checks_and_branch_protection_have_their_own_codes() {
        let (repo, api, number, head) = repository().await;
        let ctx = ctx(&api, &[]);
        let verify = pull_step(VERIFY_MERGEABLE, &head, number);

        // A conflict with the base.
        repo.update_pull(number, |pull| {
            pull.mergeable = Some(false);
            pull.mergeable_state = "dirty".into();
        });
        let (code, raw, _) = failure(adapter().run(&verify, &ctx).await);
        assert_eq!(code, diagnosis::MERGE_CONFLICT);
        assert!(raw.contains("conflicts with main"), "{raw}");
        repo.update_pull(number, |pull| {
            pull.mergeable = Some(true);
            pull.mergeable_state = "blocked".into();
        });

        // Failed and running checks win over "blocked": they explain it.
        repo.check_run(&head, "build", "completed", Some("success"));
        repo.check_run(&head, "unit", "completed", Some("failure"));
        repo.check_run(&head, "e2e", "completed", Some("timed_out"));
        repo.check_run(&head, "deploy-preview", "in_progress", None);
        repo.check_run(&head, "queue", "queued", None);
        repo.status(&head, "ci/legacy", "error");
        repo.status(&head, "ci/slow", "pending");
        let (code, raw, _) = failure(adapter().run(&verify, &ctx).await);
        assert_eq!(code, diagnosis::CHECKS_FAILING);
        assert!(
            raw.contains("3 checks failed (ci/legacy, e2e, unit)"),
            "{raw}"
        );
        assert!(
            raw.contains("3 checks are still running (ci/slow, deploy-preview, queue)"),
            "{raw}"
        );
        assert!(!raw.contains("build"), "a passed check is not named: {raw}");

        // Only running ones: the same code, and it says nothing failed.
        for name in ["unit", "e2e"] {
            repo.check_run(&head, name, "completed", Some("success"));
        }
        repo.status(&head, "ci/legacy", "success");
        let (code, raw, _) = failure(adapter().run(&verify, &ctx).await);
        assert_eq!(code, diagnosis::CHECKS_FAILING);
        assert!(!raw.contains("failed"), "{raw}");
        assert!(raw.contains("still running"), "{raw}");

        // A status and a check run under one name: the worse one counts.
        for name in ["deploy-preview", "queue"] {
            repo.check_run(&head, name, "completed", Some("success"));
        }
        repo.status(&head, "ci/slow", "success");
        repo.status(&head, "build", "failure");
        let (code, raw, _) = failure(adapter().run(&verify, &ctx).await);
        assert_eq!(code, diagnosis::CHECKS_FAILING);
        assert!(raw.contains("1 check failed (build)"), "{raw}");
        repo.status(&head, "build", "success");

        // Everything green and GitHub still says blocked: branch protection.
        let (code, raw, _) = failure(adapter().run(&verify, &ctx).await);
        assert_eq!(code, diagnosis::MERGE_BLOCKED);
        assert!(raw.contains("branch protection rule on main"), "{raw}");
        assert!(repo.writes().is_empty());
    }

    #[tokio::test]
    async fn every_page_of_checks_is_read_and_at_most_ten_are_named() {
        let (repo, api, number, head) = repository().await;
        let ctx = ctx(&api, &[]);
        let verify = pull_step(VERIFY_MERGEABLE, &head, number);

        // 150 check runs, the only failed one on the second page.
        for n in 0..150 {
            let conclusion = if n == 130 { "failure" } else { "success" };
            repo.check_run(&head, &format!("job-{n:03}"), "completed", Some(conclusion));
        }
        repo.forget_calls();
        let (code, raw, _) = failure(adapter().run(&verify, &ctx).await);
        assert_eq!(code, diagnosis::CHECKS_FAILING);
        assert!(raw.contains("1 check failed (job-130)"), "{raw}");
        assert!(
            repo.calls()
                .iter()
                .any(|call| call.ends_with("check-runs?per_page=100&page=2")),
            "{:?}",
            repo.calls()
        );

        // Twelve failed, three running: ten names in all, the failed first.
        for n in 0..12 {
            repo.check_run(&head, &format!("job-{n:03}"), "completed", Some("failure"));
        }
        for n in 20..23 {
            repo.check_run(&head, &format!("job-{n:03}"), "in_progress", None);
        }
        let (_, raw, _) = failure(adapter().run(&verify, &ctx).await);
        assert!(raw.contains("13 checks failed (job-000, "), "{raw}");
        assert!(raw.contains("job-009, 3 more)"), "{raw}");
        assert!(raw.contains("3 checks are still running (3 more)"), "{raw}");
        assert!(!raw.contains("job-010"), "{raw}");
    }

    // --- repo.merge_pull_request --------------------------------------------

    #[tokio::test]
    async fn merges_the_verified_head_and_reports_the_merge_commit() {
        let (repo, api, number, head) = repository().await;
        let ctx = ctx(&api, &[]);
        let mut merge = pull_step(MERGE_PULL_REQUEST, &head, number);
        merge
            .params
            .insert("commit_title".into(), "Implement add (#1)".into());

        let (output, outputs) = success(adapter().run(&merge, &ctx).await);
        let merged = repo.pull(number);
        assert!(merged.merged && !merged.open);
        assert_eq!(outputs["merge_commit_sha"], merged.merge_commit_sha);
        assert_eq!(
            repo.head("main").as_deref(),
            Some(merged.merge_commit_sha.as_str())
        );
        assert_eq!(
            outputs["pull_request_url"],
            format!("https://github.com/{REPO}/pull/{number}")
        );
        assert!(
            output.contains("merged pull request #1 into main with squash"),
            "{output}"
        );
        // GitHub was told which head to merge, how, and under which title.
        assert_eq!(
            repo.merges(),
            [json!({
                "sha": head,
                "merge_method": "squash",
                "commit_title": "Implement add (#1)",
            })]
        );
        assert_eq!(
            repo.commit(&merged.merge_commit_sha).1,
            "Implement add (#1)"
        );
        assert_eq!(repo.file("main", "src/lib.rs").as_deref(), Some(BROKEN));

        // The step runs again after a restart: merged is merged, with the
        // same merge commit and without a second merge request.
        let (output, again) = success(adapter().run(&merge, &ctx).await);
        assert_eq!(again["merge_commit_sha"], merged.merge_commit_sha);
        assert!(output.contains("already merged"), "{output}");
        assert!(output.contains("nothing was merged again"), "{output}");
        assert_eq!(repo.merges().len(), 1);

        // Without a title none is sent; `merge` and `rebase` are accepted.
        for method in ["merge", "rebase"] {
            let (repo, api, number, head) = repository().await;
            let mut merge = pull_step(MERGE_PULL_REQUEST, &head, number);
            merge.params.insert("merge_method".into(), method.into());
            success(adapter().run(&merge, &self::ctx(&api, &[])).await);
            assert_eq!(
                repo.merges(),
                [json!({ "sha": head, "merge_method": method })]
            );
        }
    }

    #[tokio::test]
    async fn a_merge_github_refuses_keeps_its_reason_and_merges_nothing() {
        let (repo, api, number, head) = repository().await;
        let ctx = ctx(&api, &[]);
        let merge = pull_step(MERGE_PULL_REQUEST, &head, number);
        let main = repo.head("main").unwrap();

        // The head moved between the read and the merge: GitHub answers 409.
        repo.behave(|b| {
            b.merge_refusal = Some((
                409,
                "Head branch was modified. Review and try the merge again.".into(),
            ));
        });
        let (code, raw, _) = failure(adapter().run(&merge, &ctx).await);
        assert_eq!(code, diagnosis::HEAD_MOVED);
        assert!(raw.contains("Head branch was modified"), "{raw}");
        assert!(raw.contains("Nothing was merged"), "{raw}");

        // A rule on GitHub: 405 with its own words.
        repo.behave(|b| b.merge_refusal = None);
        repo.update_pull(number, |pull| pull.mergeable_state = "blocked".into());
        let (code, raw, _) = failure(adapter().run(&merge, &ctx).await);
        assert_eq!(code, diagnosis::MERGE_BLOCKED);
        assert!(
            raw.contains("At least 1 approving review is required"),
            "{raw}"
        );
        assert!(!raw.contains(".."), "{raw}");
        repo.update_pull(number, |pull| pull.mergeable_state = "clean".into());

        // 422 is a refusal too.
        repo.behave(|b| {
            b.merge_refusal = Some((
                422,
                "Squash merges are not allowed on this repository.".into(),
            ));
        });
        let (code, raw, _) = failure(adapter().run(&merge, &ctx).await);
        assert_eq!(code, diagnosis::MERGE_BLOCKED);
        assert!(raw.contains("Squash merges are not allowed"), "{raw}");

        // 404 and 403: the token may not merge.
        for (status, message) in [
            (404, "Not Found"),
            (403, "Resource not accessible by integration"),
        ] {
            repo.behave(|b| b.merge_refusal = Some((status, message.into())));
            let (code, raw, missing) = failure(adapter().run(&merge, &ctx).await);
            assert_eq!(code, diagnosis::PERMISSION_DENIED, "{status}");
            assert_eq!(missing, [WRITE]);
            assert!(raw.contains("`repo` scope"), "{raw}");
            assert!(raw.contains("pull_requests: write"), "{raw}");
        }
        // A rate limit is not a missing permission.
        repo.behave(|b| {
            b.merge_refusal = Some((403, "You have exceeded a secondary rate limit.".into()));
        });
        let (code, _, missing) = failure(adapter().run(&merge, &ctx).await);
        assert_eq!(code, diagnosis::QUOTA_EXCEEDED);
        assert!(missing.is_empty());
        repo.behave(|b| b.merge_refusal = None);
        assert_eq!(repo.head("main"), Some(main.clone()));
        assert!(!repo.pull(number).merged);

        // A head that is already another commit, or a closed pull request,
        // never reaches the merge call.
        let asked = repo.merges().len();
        let pushed = repo.push(BRANCH, &[("README.md", "# more\n")], "More");
        let (code, raw, _) = failure(adapter().run(&merge, &ctx).await);
        assert_eq!(code, diagnosis::HEAD_MOVED);
        assert!(raw.contains(short(&pushed)), "{raw}");
        repo.update_pull(number, |pull| pull.open = false);
        let (code, _, _) = failure(
            adapter()
                .run(&pull_step(MERGE_PULL_REQUEST, &pushed, number), &ctx)
                .await,
        );
        assert_eq!(code, diagnosis::MERGE_BLOCKED);
        let mut unknown_method = pull_step(MERGE_PULL_REQUEST, &pushed, number);
        unknown_method
            .params
            .insert("merge_method".into(), "fast-forward".into());
        let (code, raw, _) = failure(adapter().run(&unknown_method, &ctx).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw.contains("merge, squash or rebase"), "{raw}");
        assert_eq!(repo.merges().len(), asked);
        assert_eq!(repo.head("main"), Some(main));
    }

    // --- repo.delete_branch -------------------------------------------------

    #[tokio::test]
    async fn a_merged_branch_is_deleted_once_and_gone_is_success() {
        let (repo, api, number, head) = repository().await;
        let ctx = ctx(&api, &[]);
        let delete = pull_step(DELETE_BRANCH, &head, number);

        // Not merged yet: the branch stays, and the step says so.
        let (output, _) = success(adapter().run(&delete, &ctx).await);
        assert!(output.contains("left fix/lint in place"), "{output}");
        assert!(output.contains("is not merged"), "{output}");
        assert!(output.contains("Nothing was deleted"), "{output}");
        assert_eq!(repo.head(BRANCH), Some(head.clone()));
        assert!(repo.writes().is_empty());

        success(
            adapter()
                .run(&pull_step(MERGE_PULL_REQUEST, &head, number), &ctx)
                .await,
        );
        let (output, _) = success(adapter().run(&delete, &ctx).await);
        assert!(output.contains("deleted fix/lint"), "{output}");
        assert_eq!(repo.head(BRANCH), None);
        assert!(repo.head("main").is_some());

        // Again after a restart: already gone, nothing to delete.
        repo.forget_calls();
        let (output, _) = success(adapter().run(&delete, &ctx).await);
        assert!(output.contains("already gone"), "{output}");
        assert!(repo.writes().is_empty(), "{:?}", repo.writes());
    }

    #[tokio::test]
    async fn branches_that_are_not_safely_ours_are_left_in_place_with_the_reason() {
        let ctx_for = |api: &str| ctx(api, &[]);
        let kept = |outcome: StepOutcome, reason: &str| {
            let (output, _) = success(outcome);
            assert!(output.contains("in place"), "{output}");
            assert!(output.contains(reason), "{reason}: {output}");
        };

        // A fork's pull request, merged, whose branch name also exists here.
        let (repo, api, _, _) = repository().await;
        repo.branch("patch-1", "main");
        let ours = repo.head("patch-1");
        let fork = repo.open_fork_pull("mallory/api", "patch-1", &"a".repeat(40));
        repo.update_pull(fork, |pull| {
            pull.open = false;
            pull.merged = true;
        });
        let delete = step(
            DELETE_BRANCH,
            &[("branch", "patch-1"), ("pull_request", &fork.to_string())],
        );
        kept(
            adapter().run(&delete, &ctx_for(&api)).await,
            "the fork mallory/api",
        );
        assert_eq!(repo.head("patch-1"), ours);
        // The fork itself is gone.
        repo.update_pull(fork, |pull| pull.head_repo.clear());
        kept(
            adapter().run(&delete, &ctx_for(&api)).await,
            "no longer exists",
        );
        assert_eq!(repo.head("patch-1"), ours);
        assert!(repo.writes().is_empty(), "{:?}", repo.writes());

        // The pull request's head is another branch than the plan names.
        let (repo, api, number, head) = repository().await;
        let merge = pull_step(MERGE_PULL_REQUEST, &head, number);
        success(adapter().run(&merge, &ctx_for(&api)).await);
        repo.branch("release", "main");
        let wrong = step(
            DELETE_BRANCH,
            &[("branch", "release"), ("pull_request", &number.to_string())],
        );
        kept(
            adapter().run(&wrong, &ctx_for(&api)).await,
            "the head branch of pull request #1 is fix/lint, not release",
        );
        assert!(repo.head("release").is_some());

        // Commits pushed after the merge would be lost.
        let delete = pull_step(DELETE_BRANCH, &head, number);
        let later = repo.push(BRANCH, &[("NEXT.md", "more work\n")], "Keep going");
        kept(
            adapter().run(&delete, &ctx_for(&api)).await,
            "commits that were not part of pull request #1",
        );
        assert_eq!(repo.head(BRANCH), Some(later));

        // Another pull request is open against the branch.
        let (repo, api, number, head) = repository().await;
        success(
            adapter()
                .run(
                    &pull_step(MERGE_PULL_REQUEST, &head, number),
                    &ctx_for(&api),
                )
                .await,
        );
        repo.branch("fix/lint-followup", BRANCH);
        repo.push("fix/lint-followup", &[("MORE.md", "stacked\n")], "Stacked");
        let stacked = repo.open_pull("fix/lint-followup", "Stacked on fix/lint");
        repo.update_pull(stacked, |pull| pull.base_ref = BRANCH.into());
        let delete = pull_step(DELETE_BRANCH, &head, number);
        kept(
            adapter().run(&delete, &ctx_for(&api)).await,
            &format!("pull request #{stacked} is still open against it"),
        );
        assert_eq!(repo.head(BRANCH), Some(head.clone()));
        repo.update_pull(stacked, |pull| pull.open = false);

        // GitHub protects it.
        repo.behave(|b| b.protected.push(BRANCH.into()));
        kept(
            adapter().run(&delete, &ctx_for(&api)).await,
            "GitHub refused to delete it (Cannot delete this protected branch)",
        );
        assert_eq!(repo.head(BRANCH), Some(head.clone()));
        repo.behave(|b| b.protected.clear());

        // The token may not delete it: that is a failure, not a skip.
        repo.behave(|b| b.read_only = true);
        let (code, raw, missing) = failure(adapter().run(&delete, &ctx_for(&api)).await);
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(missing, [WRITE]);
        assert!(raw.contains("DELETE"), "{raw}");
        assert_eq!(repo.head(BRANCH), Some(head));
        repo.behave(|b| b.read_only = false);

        // The default branch, even as the head of a merged pull request.
        repo.update_pull(number, |pull| pull.head_ref = "main".into());
        let on_main = step(
            DELETE_BRANCH,
            &[("branch", "main"), ("pull_request", &number.to_string())],
        );
        kept(
            adapter().run(&on_main, &ctx_for(&api)).await,
            "it is the default branch of acme/api",
        );
        assert!(repo.head("main").is_some());
    }

    // --- repo.enable_security_updates ---------------------------------------

    #[tokio::test]
    async fn dependabot_is_turned_on_and_turning_it_on_again_changes_nothing() {
        let (repo, api, _, _) = repository().await;
        let ctx = ctx(&api, &[]);
        let both = step(
            ENABLE_SECURITY_UPDATES,
            &[("alerts", "true"), ("security_updates", "true")],
        );
        assert!(!repo.alerts() && !repo.security_updates());
        repo.forget_calls();

        let (output, outputs) = success(adapter().run(&both, &ctx).await);
        assert_eq!(
            output,
            "Dependabot alerts and Dependabot security updates are on for acme/api"
        );
        assert!(outputs.is_empty());
        assert!(repo.alerts() && repo.security_updates());
        // Alerts first: security updates need them.
        assert_eq!(
            repo.writes(),
            [
                "PUT /repos/acme/api/vulnerability-alerts",
                "PUT /repos/acme/api/automated-security-fixes",
            ]
        );
        // The same step again (a re-run): both stay on, nothing fails.
        let (again, _) = success(adapter().run(&both, &ctx).await);
        assert_eq!(again, output);
        assert!(repo.alerts() && repo.security_updates());

        // Alerts only.
        let (repo, api, _, _) = repository().await;
        let alerts = step(
            ENABLE_SECURITY_UPDATES,
            &[("alerts", "true"), ("security_updates", "false")],
        );
        let (output, _) = success(adapter().run(&alerts, &self::ctx(&api, &[])).await);
        assert_eq!(output, "Dependabot alerts are on for acme/api");
        assert!(repo.alerts() && !repo.security_updates());
        // Security updates only, with alerts already on.
        let updates = step(ENABLE_SECURITY_UPDATES, &[("security_updates", "true")]);
        let (output, _) = success(adapter().run(&updates, &self::ctx(&api, &[])).await);
        assert_eq!(output, "Dependabot security updates are on for acme/api");
        assert!(repo.security_updates());
    }

    #[tokio::test]
    async fn security_updates_without_alerts_say_that_alerts_come_first() {
        let updates = step(
            ENABLE_SECURITY_UPDATES,
            &[("alerts", "false"), ("security_updates", "true")],
        );

        // GitHub refuses with a validation error: the token could reach the
        // setting, and alerts are off.
        let (repo, api, _, _) = repository().await;
        let (code, raw, missing) = failure(adapter().run(&updates, &ctx(&api, &[])).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(missing.is_empty());
        assert!(
            raw.contains("because Dependabot alerts are off there, and alerts have to be on first"),
            "{raw}"
        );
        assert!(raw.contains("Nothing was changed"), "{raw}");
        assert!(!repo.alerts() && !repo.security_updates());

        // The same refusal as a 403, from a user GitHub says is an admin:
        // still about the alerts.
        repo.behave(|b| {
            b.updates_refusal = Some(403);
            b.says_admin = true;
        });
        let (code, raw, _) = failure(adapter().run(&updates, &ctx(&api, &[])).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw.contains("alerts have to be on first"), "{raw}");

        // A 403 from somebody who is not an admin is a missing permission.
        repo.behave(|b| b.says_admin = false);
        let (code, _, missing) = failure(adapter().run(&updates, &ctx(&api, &[])).await);
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(missing, [ADMIN]);
        assert!(!repo.security_updates());
    }

    #[tokio::test]
    async fn settings_github_will_not_change_need_repository_admin() {
        let (repo, api, _, _) = repository().await;
        let ctx = ctx(&api, &[]);
        let both = step(
            ENABLE_SECURITY_UPDATES,
            &[("alerts", "true"), ("security_updates", "true")],
        );

        // 404: GitHub hides the setting from a token that is not an admin.
        repo.behave(|b| b.not_admin = true);
        let (code, raw, missing) = failure(adapter().run(&both, &ctx).await);
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(missing, [ADMIN]);
        assert!(raw.contains("404"), "{raw}");
        assert!(raw.contains("admin rights on the repository"), "{raw}");
        assert!(raw.contains("administration: write"), "{raw}");
        assert!(!repo.alerts());
        repo.behave(|b| b.not_admin = false);

        // 403 likewise.
        repo.behave(|b| b.read_only = true);
        let (code, _, missing) = failure(adapter().run(&both, &ctx).await);
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(missing, [ADMIN]);
        repo.behave(|b| b.read_only = false);

        // Alerts went on, security updates were refused: both are said.
        repo.behave(|b| b.updates_forbidden = true);
        let (code, raw, missing) = failure(adapter().run(&both, &ctx).await);
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(missing, [ADMIN]);
        assert!(
            raw.starts_with(
                "Dependabot alerts are on for acme/api, but security updates could not be turned on."
            ),
            "{raw}"
        );
        assert!(repo.alerts() && !repo.security_updates());

        // Nothing to turn on, or a value that is neither true nor false.
        repo.forget_calls();
        let neither = step(ENABLE_SECURITY_UPDATES, &[("alerts", "false")]);
        let (code, raw, _) = failure(adapter().run(&neither, &ctx).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw.contains("nothing to turn on"), "{raw}");
        let unclear = step(ENABLE_SECURITY_UPDATES, &[("alerts", "yes")]);
        let (code, raw, _) = failure(adapter().run(&unclear, &ctx).await);
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw.contains("must be true or false"), "{raw}");
        assert!(repo.calls().is_empty());
    }

    // --- repo.open_pull_request additions (§23.5) ---------------------------

    const DEPENDABOT: &str = ".github/dependabot.yml";
    const DEPENDABOT_YML: &str = "version: 2\nupdates:\n  - package-ecosystem: cargo\n    directory: /\n    schedule:\n      interval: weekly\n";

    fn dependabot_step() -> StepInput {
        step(
            OPEN_PULL_REQUEST,
            &[
                ("branch", "sdi/dependabot"),
                ("title", "Turn on Dependabot version updates"),
                ("body", "Adds the Dependabot configuration."),
                ("skip_if_exists", DEPENDABOT),
            ],
        )
    }

    #[tokio::test]
    async fn a_pull_request_names_its_branch_title_and_body_and_is_reused_on_a_rerun() {
        let (repo, api, _, _) = repository().await;
        // An execution without a proposal: the plan names the branch.
        let ctx = ctx(&api, &[(DEPENDABOT, DEPENDABOT_YML)]);
        assert!(ctx.proposal_id.is_empty());

        let (output, outputs) = success(adapter().run(&dependabot_step(), &ctx).await);
        let pulls = repo.pulls();
        let opened = pulls.last().unwrap();
        assert_eq!(opened.head_ref, "sdi/dependabot");
        assert_eq!(opened.base_ref, "main");
        assert_eq!(opened.title, "Turn on Dependabot version updates");
        assert!(
            opened
                .body
                .starts_with("Adds the Dependabot configuration."),
            "{}",
            opened.body
        );
        assert_eq!(outputs["branch"], "sdi/dependabot");
        assert_eq!(
            outputs["pull_request_url"],
            format!("https://github.com/{REPO}/pull/{}", opened.number)
        );
        assert!(output.contains(&outputs["pull_request_url"]), "{output}");
        assert_eq!(
            repo.file("sdi/dependabot", DEPENDABOT).as_deref(),
            Some(DEPENDABOT_YML)
        );
        assert_eq!(repo.file("main", DEPENDABOT), None);

        // The step again: the open pull request of that branch is reused,
        // not duplicated, and nothing is committed twice.
        let head = repo.head("sdi/dependabot").unwrap();
        let (output, again) = success(adapter().run(&dependabot_step(), &ctx).await);
        assert_eq!(again["pull_request_url"], outputs["pull_request_url"]);
        assert!(output.contains("already open"), "{output}");
        assert_eq!(repo.pulls().len(), pulls.len());
        assert_eq!(repo.head("sdi/dependabot"), Some(head));
    }

    #[tokio::test]
    async fn a_file_that_is_already_on_the_default_branch_skips_the_pull_request() {
        let repo = FakeRepo::new(
            REPO,
            &[
                ("README.md", "# api\n"),
                (DEPENDABOT, "version: 2\nupdates: []\n"),
            ],
        );
        let api = repo.serve().await;
        let ctx = ctx(&api, &[(DEPENDABOT, DEPENDABOT_YML)]);

        let (output, outputs) = success(adapter().run(&dependabot_step(), &ctx).await);
        assert_eq!(
            output,
            "skipped: .github/dependabot.yml already exists on main of acme/api, so no branch, commit or pull request was created"
        );
        // Honest outputs: no pull request, no branch, no commit.
        assert!(outputs.is_empty(), "{outputs:?}");
        assert!(repo.writes().is_empty(), "{:?}", repo.writes());
        assert_eq!(repo.head("sdi/dependabot"), None);
        assert!(repo.pulls().is_empty());
        // The repository's own file is untouched.
        assert_eq!(
            repo.file("main", DEPENDABOT).as_deref(),
            Some("version: 2\nupdates: []\n")
        );

        // Without the parameter the same run opens the pull request.
        let mut always = dependabot_step();
        always.params.remove("skip_if_exists");
        let (_, outputs) = success(adapter().run(&always, &ctx).await);
        assert!(outputs.contains_key("pull_request_url"));
        assert_eq!(repo.pulls().len(), 1);
    }
}
