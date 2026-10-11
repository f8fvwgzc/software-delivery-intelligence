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

#[derive(Debug, Clone, Deserialize)]
pub struct Pull {
    #[serde(default)]
    pub user: Option<Actor>,
}
