//! GitHub connector shapes (read-only).

use chrono::{DateTime, Utc};
use sdi_proto::knowledge::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{opt_string, opt_timestamp};

/// The GitHub account behind the configured token.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GithubAccount {
    /// False when no token is configured; the other fields are then empty.
    pub token_configured: bool,
    /// GitHub login.
    pub login: String,
    /// Display name, when set on the profile.
    pub name: String,
    /// Avatar image URL.
    pub avatar_url: String,
    /// Profile page URL.
    pub html_url: String,
    /// Number of public repositories.
    pub public_repos: u32,
    /// Number of private repositories visible to the token.
    pub private_repos: u32,
    /// OAuth scopes granted to the token.
    pub scopes: Vec<String>,
    /// Set when the token is missing or GitHub rejected it.
    pub error: Option<String>,
}

impl From<proto::GithubAccount> for GithubAccount {
    fn from(account: proto::GithubAccount) -> Self {
        Self {
            token_configured: account.token_configured,
            login: account.login,
            name: account.name,
            avatar_url: account.avatar_url,
            html_url: account.html_url,
            public_repos: account.public_repos,
            private_repos: account.private_repos,
            scopes: account.scopes,
            error: opt_string(account.error),
        }
    }
}

/// A repository the token can see.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GithubRepo {
    /// GitHub numeric id.
    pub id: i64,
    /// `owner/name`.
    pub full_name: String,
    /// Repository name without the owner.
    pub name: String,
    /// Owner login.
    pub owner: String,
    /// True for private repositories.
    pub is_private: bool,
    /// Repository description.
    pub description: String,
    /// Default branch, e.g. `main`.
    pub default_branch: String,
    /// Primary language reported by GitHub.
    pub language: String,
    /// Repository page URL.
    pub html_url: String,
    /// Last push time.
    pub pushed_at: Option<DateTime<Utc>>,
    /// Star count.
    pub stars: u32,
    /// True when archived.
    pub archived: bool,
    /// True when the repository is a fork.
    pub fork: bool,
    /// Project id when this repository is already connected.
    pub project_id: Option<String>,
}

impl From<proto::GithubRepo> for GithubRepo {
    fn from(repo: proto::GithubRepo) -> Self {
        Self {
            id: repo.id,
            full_name: repo.full_name,
            name: repo.name,
            owner: repo.owner,
            is_private: repo.is_private,
            description: repo.description,
            default_branch: repo.default_branch,
            language: repo.language,
            html_url: repo.html_url,
            pushed_at: opt_timestamp(repo.pushed_at.as_ref()),
            stars: repo.stars,
            archived: repo.archived,
            fork: repo.fork,
            project_id: opt_string(repo.project_id),
        }
    }
}

/// Query of `GET /github/repos`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListReposQuery {
    /// Case-insensitive filter on the repository name. Empty lists everything.
    pub query: Option<String>,
}
