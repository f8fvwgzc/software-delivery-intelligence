//! The slices of GitHub's JSON we read. Unknown fields are ignored.

use chrono::{DateTime, Utc};
use serde::Deserialize;

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

#[derive(Debug, Clone, Deserialize)]
pub struct Actor {
    pub login: String,
    #[serde(default)]
    pub avatar_url: String,
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
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
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

/// A branch end of a pull request.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PullRef {
    #[serde(default, rename = "ref")]
    pub reference: String,
    #[serde(default)]
    pub sha: String,
}

/// `/pulls` and `/pulls/{n}`: the list omits `changed_files`, `additions`
/// and `deletions` (they are zero until the single-PR call fills them).
#[derive(Debug, Clone, Default, Deserialize)]
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
        if self.merged_at.is_some() {
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
