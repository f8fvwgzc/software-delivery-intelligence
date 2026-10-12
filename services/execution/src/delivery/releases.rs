//! Cutting a release (CONTRACT.md §22): one immutable image at one commit.
//!
//! A release is created by the build webhook, by the package webhook or by
//! hand; all three end here. What the request leaves out is derived: the
//! commit is the default branch head, the tag its short SHA, the version
//! the tag, the image `<default registry>/<namespace>/<repository name>`,
//! and the notes the commit subjects since the previous release (GitHub's
//! compare API, read with the workspace's own token). Creating the same
//! (project, tag) twice returns the first release.

use std::{sync::Arc, time::Duration};

use reqwest::StatusCode;
use sdi_common::{grpc::db_status, principal::Principal};
use sdi_proto::execution::v1::CreateReleaseRequest;
use serde_json::Value;
use tonic::Status;
use uuid::Uuid;

use super::{
    db::releases::{self, NewRelease, ReleaseRow},
    image,
    model::{RELEASE_MANUAL, RELEASE_SOURCES},
    registry_for,
};
use crate::{adapters::GithubAccess, runner::Runtime};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT: &str = concat!("sdi-execution/", env!("CARGO_PKG_VERSION"));
/// Commit subjects a release's notes list before saying "and N more".
pub const MAX_NOTE_LINES: usize = 50;
const MAX_SUBJECT_CHARS: usize = 200;
const MAX_VERSION_CHARS: usize = 128;

/// A commit as a release needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub sha: String,
    /// First line of the commit message.
    pub subject: String,
}

/// Why GitHub could not answer a read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The commit or repository is not there (or not visible to the token).
    NotFound(String),
    Denied(String),
    Unavailable(String),
}

/// Reads commits for release notes. GitHub's API base is operator
/// configuration, so this client is not the SSRF-guarded one.
#[derive(Clone)]
pub struct GithubReader {
    http: reqwest::Client,
}

impl Default for GithubReader {
    fn default() -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("reqwest client"),
        }
    }
}

fn subject_of(commit: &Value) -> String {
    commit
        .get("commit")
        .and_then(|c| c.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(MAX_SUBJECT_CHARS)
        .collect()
}

impl GithubReader {
    async fn get(&self, access: &GithubAccess, path: &str) -> Result<Value, ReadError> {
        let url = format!("{}{path}", access.api_base_url.trim_end_matches('/'));
        let response = self
            .http
            .get(&url)
            .bearer_auth(&access.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| {
                ReadError::Unavailable(format!("GET {path} failed: {}", e.without_url()))
            })?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(json);
        }
        let message = json
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let detail = format!("GET {path} -> {status}: {message}");
        Err(match status {
            StatusCode::NOT_FOUND | StatusCode::UNPROCESSABLE_ENTITY | StatusCode::CONFLICT => {
                ReadError::NotFound(detail)
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ReadError::Denied(detail),
            _ => ReadError::Unavailable(detail),
        })
    }

    /// The commit `reference` (a SHA or a branch) points at.
    pub async fn commit(
        &self,
        access: &GithubAccess,
        repo: &str,
        reference: &str,
    ) -> Result<CommitInfo, ReadError> {
        let commit = self
            .get(access, &format!("/repos/{repo}/commits/{reference}"))
            .await?;
        let sha = commit
            .get("sha")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if sha.is_empty() {
            return Err(ReadError::NotFound(format!(
                "{repo} has no commit {reference}"
            )));
        }
        Ok(CommitInfo {
            subject: subject_of(&commit),
            sha,
        })
    }

    /// The commits after `base` up to `head`, oldest first, and how many
    /// there are in total (GitHub lists at most 250).
    pub async fn commits_between(
        &self,
        access: &GithubAccess,
        repo: &str,
        base: &str,
        head: &str,
    ) -> Result<(Vec<CommitInfo>, usize), ReadError> {
        let compared = self
            .get(
                access,
                &format!("/repos/{repo}/compare/{base}...{head}?per_page=100"),
            )
            .await?;
        let commits: Vec<CommitInfo> = compared
            .get("commits")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|commit| CommitInfo {
                sha: commit
                    .get("sha")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                subject: subject_of(commit),
            })
            .collect();
        let total = compared
            .get("total_commits")
            .and_then(Value::as_u64)
            .map_or(commits.len(), |n| n as usize)
            .max(commits.len());
        Ok((commits, total))
    }
}

/// Markdown notes: one bullet per commit subject, newest first. `total` is
/// how many commits there are in all; what the list leaves out (GitHub
/// listed fewer, or the list was capped) is summed up in a last line.
pub fn notes(commits: &[CommitInfo], total: usize) -> String {
    let titled: Vec<&CommitInfo> = commits
        .iter()
        .rev()
        .filter(|commit| !commit.subject.is_empty())
        .collect();
    let mut lines: Vec<String> = titled
        .iter()
        .take(MAX_NOTE_LINES)
        .map(|commit| {
            format!(
                "- {} ({})",
                escape_markdown(&commit.subject),
                image::short_sha(&commit.sha)
            )
        })
        .collect();
    let left_out = total.saturating_sub(commits.len()) + titled.len().saturating_sub(lines.len());
    if left_out > 0 && !lines.is_empty() {
        lines.push(format!("- … and {left_out} more"));
    }
    lines.join("\n")
}

/// Keeps a commit subject from starting a block or opening HTML.
fn escape_markdown(subject: &str) -> String {
    subject.replace('<', "&lt;").replace('>', "&gt;")
}

/// What a `ref` path segment may hold when it comes from another service.
fn safe_reference(reference: &str) -> bool {
    !reference.is_empty()
        && reference.len() <= 255
        && !reference.contains("..")
        && reference
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
}

fn invalid(message: impl Into<String>) -> Status {
    Status::invalid_argument(message)
}

/// Creates the release, or returns the one the project already has for
/// this tag. The boolean says whether a release was created.
pub async fn create(
    rt: &Arc<Runtime>,
    principal: &Principal,
    workspace: Uuid,
    req: CreateReleaseRequest,
) -> Result<(ReleaseRow, bool), Status> {
    let project_id = req.project_id.trim();
    if project_id.is_empty() {
        return Err(invalid("project_id is required"));
    }
    let source = match req.source.trim() {
        "" => RELEASE_MANUAL,
        source => RELEASE_SOURCES
            .iter()
            .copied()
            .find(|known| *known == source)
            .ok_or_else(|| {
                invalid(format!(
                    "source must be one of {}",
                    RELEASE_SOURCES.join(", ")
                ))
            })?,
    };
    let commit_arg = req.commit_sha.trim().to_ascii_lowercase();
    if !commit_arg.is_empty() && !image::valid_commit(&commit_arg) {
        return Err(invalid("commit_sha must be 7 to 40 hex digits"));
    }
    // An image given with its tag (`ghcr.io/acme/api:v1`) is taken apart.
    let (image_arg, tag_in_image, digest_in_image) = image::split_reference(&req.image);
    let image_arg = image_arg.to_ascii_lowercase();
    if !image_arg.is_empty() && !image::valid_image(&image_arg) {
        return Err(invalid(
            "image must be host/namespace/name in lowercase, without a tag",
        ));
    }
    let tag_arg = match req.tag.trim() {
        "" => tag_in_image.as_str(),
        tag => tag,
    };
    if !tag_arg.is_empty() && !image::valid_tag(tag_arg) {
        return Err(invalid(
            "tag may hold letters, digits, underscores, dots and dashes (at most 128 characters)",
        ));
    }
    let digest_arg = match req.digest.trim() {
        "" => digest_in_image.as_str(),
        digest => digest,
    };
    if !digest_arg.is_empty() && !image::valid_digest(digest_arg) {
        return Err(invalid("digest must look like sha256:<64 hex digits>"));
    }
    let version_arg = req.version.trim();
    if version_arg.chars().count() > MAX_VERSION_CHARS || version_arg.chars().any(char::is_control)
    {
        return Err(invalid("version must be at most 128 printable characters"));
    }

    let project = rt.delivery.projects.project(principal, project_id).await?;
    let repo = project.repo_full_name.trim();
    if repo.split('/').count() != 2 {
        return Err(Status::failed_precondition(
            "the project has no GitHub repository to cut a release from",
        ));
    }
    let access = match rt.github_access.access(workspace).await {
        Some(access) => Some(access),
        None => rt.delivery.github_fallback.clone(),
    };

    // The commit: the one named, else the default branch head.
    let reference = if commit_arg.is_empty() {
        match project.default_branch.trim() {
            "" => "HEAD".to_owned(),
            branch => branch.to_owned(),
        }
    } else {
        commit_arg.clone()
    };
    if !safe_reference(&reference) {
        return Err(Status::failed_precondition(
            "the project's default branch name cannot be used in a request",
        ));
    }
    let commit = match &access {
        Some(access) => match rt.delivery.github.commit(access, repo, &reference).await {
            Ok(commit) => Some(commit),
            // The form names the field, so the answer does too.
            Err(ReadError::NotFound(_)) if !commit_arg.is_empty() => {
                return Err(invalid(format!(
                    "commit_sha: {repo} has no commit {reference}"
                )));
            }
            Err(ReadError::NotFound(detail)) => {
                return Err(Status::failed_precondition(format!(
                    "{repo} has no commit on {reference} to release ({detail})"
                )));
            }
            // A release for a commit the caller named does not need GitHub.
            Err(error) if !commit_arg.is_empty() => {
                tracing::warn!(
                    repo,
                    ?error,
                    "commit not confirmed with GitHub; using it as given"
                );
                None
            }
            Err(ReadError::Denied(detail)) => {
                return Err(Status::failed_precondition(format!(
                    "GitHub refused to show {repo}@{reference}: {detail}"
                )));
            }
            Err(ReadError::Unavailable(detail)) => {
                return Err(Status::unavailable(format!(
                    "GitHub could not be asked for the head of {repo}@{reference}: {detail}"
                )));
            }
        },
        None if commit_arg.is_empty() => {
            return Err(Status::failed_precondition(
                "no GitHub access for this workspace: pass commitSha, or connect GitHub so the default branch head can be read",
            ));
        }
        None => None,
    };
    let sha = commit
        .as_ref()
        .map_or(commit_arg.as_str(), |c| c.sha.as_str())
        .to_ascii_lowercase();
    let tag = if tag_arg.is_empty() {
        image::short_sha(&sha)
    } else {
        tag_arg.to_owned()
    };
    let version = if version_arg.is_empty() {
        tag.clone()
    } else {
        version_arg.to_owned()
    };

    // Idempotent on (project, tag).
    if let Some(existing) = releases::by_tag(&rt.pool, workspace, project_id, &tag)
        .await
        .map_err(db_status)?
    {
        return Ok((
            learn_digest(rt, workspace, existing, digest_arg).await?,
            false,
        ));
    }

    let image = if image_arg.is_empty() {
        let registry = registry_for(rt, workspace, None).await?;
        image::default_image(&registry.host, &registry.namespace, repo)
    } else {
        image_arg
    };

    // Notes: what changed since the previous release.
    let previous = releases::latest(&rt.pool, workspace, project_id)
        .await
        .map_err(db_status)?;
    let own_subject = || {
        commit
            .as_ref()
            .map(|c| notes(std::slice::from_ref(c), 1))
            .unwrap_or_default()
    };
    let release_notes = match (&access, &previous) {
        (Some(access), Some(previous)) if previous.commit_sha != sha => {
            match rt
                .delivery
                .github
                .commits_between(access, repo, &previous.commit_sha, &sha)
                .await
            {
                Ok((commits, total)) if !commits.is_empty() => notes(&commits, total),
                Ok(_) => own_subject(),
                Err(error) => {
                    tracing::warn!(
                        repo,
                        ?error,
                        "release notes not built; the release has none"
                    );
                    own_subject()
                }
            }
        }
        _ => own_subject(),
    };

    let inserted = releases::insert(
        &rt.pool,
        NewRelease {
            id: Uuid::now_v7(),
            workspace_id: workspace,
            project_id,
            version: &version,
            commit_sha: &sha,
            image: &image,
            tag: &tag,
            digest: digest_arg,
            source,
            pipeline_run_id: req.pipeline_run_id.trim(),
            notes: &release_notes,
            created_by: &principal.user_id,
            repo_full_name: repo,
            project_name: project.name.trim(),
        },
    )
    .await
    .map_err(db_status)?;
    match inserted {
        Some(row) => Ok((row, true)),
        // Created concurrently by the other webhook of the same build.
        None => {
            let existing = releases::by_tag(&rt.pool, workspace, project_id, &tag)
                .await
                .map_err(db_status)?
                .ok_or_else(|| {
                    Status::aborted("the release was created and removed concurrently")
                })?;
            Ok((
                learn_digest(rt, workspace, existing, digest_arg).await?,
                false,
            ))
        }
    }
}

/// A later event may know the digest an earlier one did not (the package
/// webhook after the workflow webhook): fill it in, never replace it.
async fn learn_digest(
    rt: &Runtime,
    workspace: Uuid,
    existing: ReleaseRow,
    digest: &str,
) -> Result<ReleaseRow, Status> {
    if digest.is_empty() || !existing.digest.is_empty() {
        return Ok(existing);
    }
    releases::fill_digest(&rt.pool, workspace, existing.id, digest)
        .await
        .map_err(db_status)?;
    releases::get(&rt.pool, workspace, existing.id)
        .await
        .map_err(db_status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(sha: &str, subject: &str) -> CommitInfo {
        CommitInfo {
            sha: sha.into(),
            subject: subject.into(),
        }
    }

    #[test]
    fn notes_list_subjects_newest_first_with_short_shas() {
        let commits = [
            commit(
                "1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "Add health endpoint",
            ),
            commit(
                "2222222bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "Fix <script> in title",
            ),
            commit("3333333ccccccccccccccccccccccccccccccccc", ""),
        ];
        assert_eq!(
            notes(&commits, 3),
            "- Fix &lt;script&gt; in title (2222222)\n- Add health endpoint (1111111)"
        );
        assert_eq!(notes(&[], 0), "");
        let many: Vec<CommitInfo> = (0..60)
            .map(|i| {
                commit(
                    &format!("{i:07}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                    &format!("Change {i}"),
                )
            })
            .collect();
        let text = notes(&many, 300);
        assert_eq!(text.lines().count(), MAX_NOTE_LINES + 1);
        assert!(text.starts_with("- Change 59 (0000059)\n"), "{text}");
        assert!(text.ends_with("- … and 250 more"), "{text}");
    }

    #[test]
    fn only_plain_references_reach_a_request_path() {
        for good in ["main", "release/1.x", "a1b2c3d", "HEAD"] {
            assert!(safe_reference(good), "{good}");
        }
        for bad in ["", "main..evil", "a b", "x?y=1", "x#y", "x%2e"] {
            assert!(!safe_reference(bad), "{bad}");
        }
        let commit = serde_json::json!({"commit": {"message": "Subject line\n\nBody"}});
        assert_eq!(subject_of(&commit), "Subject line");
        assert_eq!(subject_of(&serde_json::json!({})), "");
    }
}
