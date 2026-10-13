//! The slices of GitHub's JSON we read. Unknown fields are ignored.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_url: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub public_repos: u32,
    #[serde(default)]
    pub total_private_repos: Option<u32>,
    #[serde(default)]
    pub owned_private_repos: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Actor {
    pub login: String,
    #[serde(default)]
    pub avatar_url: String,
    /// GitHub's account type: "User" | "Bot" | "Organization". Empty when
    /// the payload did not say.
    #[serde(default, rename = "type")]
    pub kind: String,
}

impl Actor {
    /// A bot account (a GitHub App acting as itself).
    pub fn is_bot(&self) -> bool {
        self.kind == "Bot"
    }

    /// Dependabot, under either of the logins it has used.
    pub fn is_dependabot(&self) -> bool {
        is_dependabot_login(&self.login)
    }
}

/// Whether `login` is Dependabot's (`dependabot[bot]`, or the retired
/// `dependabot-preview[bot]` old pull requests still carry).
pub fn is_dependabot_login(login: &str) -> bool {
    login.eq_ignore_ascii_case("dependabot[bot]")
        || login.eq_ignore_ascii_case("dependabot-preview[bot]")
}

#[derive(Debug, Clone, Deserialize)]
pub struct Repo {
    pub id: i64,
    pub full_name: String,
    pub name: String,
    pub owner: Actor,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_branch")]
    pub default_branch: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub pushed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub stargazers_count: u32,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub fork: bool,
    #[serde(default)]
    pub topics: Vec<String>,
}

/// `/installation/repositories`: the selection an installation grants.
#[derive(Debug, Clone, Deserialize)]
pub struct InstallationRepos {
    #[serde(default)]
    pub total_count: u64,
    #[serde(default)]
    pub repositories: Vec<Repo>,
}

fn default_branch() -> String {
    "main".to_owned()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Commit {
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tree {
    pub sha: String,
    #[serde(default)]
    pub tree: Vec<TreeNode>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TreeNode {
    pub path: String,
    /// `blob` | `tree` | `commit` (a submodule).
    #[serde(rename = "type")]
    pub kind: String,
    /// Git mode: `100644`/`100755` files, `040000` trees, `120000` symlinks,
    /// `160000` submodules.
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub sha: String,
    #[serde(default)]
    pub size: Option<u64>,
}

impl TreeNode {
    pub fn is_dir(&self) -> bool {
        self.kind == "tree"
    }

    /// A symbolic link: its "content" is the link target, never a file
    /// worth reading, and following it could point outside the tree.
    pub fn is_symlink(&self) -> bool {
        self.mode == "120000"
    }

    /// A submodule: another repository, with no content here.
    pub fn is_submodule(&self) -> bool {
        self.kind == "commit" || self.mode == "160000"
    }

    /// A regular file the scanner and the code browser may read.
    pub fn is_file(&self) -> bool {
        self.kind == "blob" && !self.is_symlink()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowRuns {
    #[serde(default)]
    pub workflow_runs: Vec<WorkflowRun>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowRun {
    pub id: i64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub run_number: i64,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub head_branch: Option<String>,
    #[serde(default)]
    pub head_sha: String,
    #[serde(default)]
    pub head_commit: Option<HeadCommit>,
    #[serde(default)]
    pub actor: Option<Actor>,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub run_started_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub html_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeadCommit {
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Jobs {
    #[serde(default)]
    pub jobs: Vec<Job>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    pub name: String,
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub name: String,
    #[serde(default)]
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Deployment {
    pub id: i64,
    #[serde(default)]
    pub environment: String,
    #[serde(default, rename = "ref")]
    pub reference: String,
    #[serde(default)]
    pub sha: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub creator: Option<Actor>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentStatus {
    pub state: String,
    #[serde(default)]
    pub environment_url: Option<String>,
    #[serde(default)]
    pub log_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// The repository one end of a pull request lives in.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PullRepo {
    #[serde(default)]
    pub full_name: String,
}

/// A branch end of a pull request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PullRef {
    #[serde(default, rename = "ref")]
    pub reference: String,
    #[serde(default)]
    pub sha: String,
    /// `null` on the head of a pull request whose fork was deleted.
    #[serde(default)]
    pub repo: Option<PullRepo>,
}

/// `/pulls` and `/pulls/{n}`: the list omits `changed_files`, `additions`
/// and `deletions` (they are zero until the single-PR call fills them), and
/// `merged`, `mergeable` and `mergeable_state`, which only the single-PR
/// call computes.
///
/// Serialisable because the client keeps this slice, not GitHub's whole
/// payload, next to the ETag (see `GithubClient::get_slim`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pull {
    #[serde(default)]
    pub number: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub user: Option<Actor>,
    /// "open" | "closed"
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub merged_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub merged: bool,
    /// `null` while GitHub is still computing it (ask again shortly).
    #[serde(default)]
    pub mergeable: Option<bool>,
    /// "clean" | "dirty" | "blocked" | "behind" | "unstable" | "has_hooks" | "draft" | "unknown"
    #[serde(default)]
    pub mergeable_state: String,
    #[serde(default)]
    pub base: Option<PullRef>,
    #[serde(default)]
    pub head: Option<PullRef>,
    #[serde(default)]
    pub changed_files: u32,
    #[serde(default)]
    pub additions: u32,
    #[serde(default)]
    pub deletions: u32,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl Pull {
    /// The four states the platform shows: a draft is "draft", a merged PR
    /// "merged", a closed unmerged one "closed", anything else "open".
    pub fn platform_state(&self) -> &'static str {
        if self.merged || self.merged_at.is_some() {
            "merged"
        } else if self.state == "closed" {
            "closed"
        } else if self.draft {
            "draft"
        } else {
            "open"
        }
    }
}

/// `/pulls/{n}/files`: `patch` is absent for binary files and for diffs
/// GitHub considers too large.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PullFile {
    pub filename: String,
    #[serde(default)]
    pub previous_filename: Option<String>,
    /// "added" | "removed" | "modified" | "renamed" | "copied" | "changed" | "unchanged"
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub additions: u32,
    #[serde(default)]
    pub deletions: u32,
    #[serde(default)]
    pub patch: Option<String>,
}

// ---------------------------------------------------------------------------
// Merge readiness (CONTRACT.md §23.3)
// ---------------------------------------------------------------------------

/// The three switches of `/repos/{o}/{r}` that say how a pull request may
/// be merged. GitHub only returns them to a token with push access; absent
/// means "not told", which is different from `false`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoMergeSettings {
    #[serde(default)]
    pub allow_squash_merge: Option<bool>,
    #[serde(default)]
    pub allow_merge_commit: Option<bool>,
    #[serde(default)]
    pub allow_rebase_merge: Option<bool>,
}

/// `/commits/{sha}/check-runs`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckRuns {
    #[serde(default)]
    pub total_count: u64,
    #[serde(default)]
    pub check_runs: Vec<CheckRun>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckRun {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    /// "queued" | "in_progress" | "completed" | "waiting" | "requested" | "pending"
    #[serde(default)]
    pub status: String,
    /// Set once `status` is "completed".
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
}

/// `/commits/{sha}/status`: the latest status of every context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CombinedStatus {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub total_count: u64,
    #[serde(default)]
    pub statuses: Vec<CommitStatus>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommitStatus {
    #[serde(default)]
    pub context: String,
    /// "success" | "failure" | "error" | "pending"
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

/// `/pulls/{n}/reviews`, oldest first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Review {
    /// `null` when the reviewer's account was deleted.
    #[serde(default)]
    pub user: Option<Actor>,
    /// "APPROVED" | "CHANGES_REQUESTED" | "COMMENTED" | "DISMISSED" | "PENDING"
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub submitted_at: Option<DateTime<Utc>>,
}

/// `/branches/{branch}`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Branch {
    #[serde(default)]
    pub protected: bool,
}

/// `/compare/{base}...{head}`: only the distance, never the commits or files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Comparison {
    #[serde(default)]
    pub ahead_by: u32,
    #[serde(default)]
    pub behind_by: u32,
}

// ---------------------------------------------------------------------------
// Dependabot (CONTRACT.md §23.4)
// ---------------------------------------------------------------------------

/// `/automated-security-fixes`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SecurityFixes {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub paused: bool,
}

/// One entry of `/dependabot/alerts`. Every nested object is optional:
/// GitHub omits what it does not know and the alert is still worth showing.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Alert {
    #[serde(default)]
    pub number: u64,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub dependency: Option<AlertDependency>,
    #[serde(default)]
    pub security_advisory: Option<AlertAdvisory>,
    #[serde(default)]
    pub security_vulnerability: Option<AlertVulnerability>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AlertDependency {
    #[serde(default)]
    pub package: Option<AlertPackage>,
    #[serde(default)]
    pub manifest_path: Option<String>,
    /// "runtime" | "development"
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AlertPackage {
    #[serde(default)]
    pub ecosystem: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AlertAdvisory {
    #[serde(default)]
    pub ghsa_id: Option<String>,
    #[serde(default)]
    pub cve_id: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    /// "low" | "medium" | "high" | "critical"
    #[serde(default)]
    pub severity: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AlertVulnerability {
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub vulnerable_version_range: Option<String>,
    #[serde(default)]
    pub first_patched_version: Option<AlertPatchedVersion>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AlertPatchedVersion {
    #[serde(default)]
    pub identifier: Option<String>,
}
