use std::collections::HashMap;

use sdi_common::{grpc::db_status, time::opt_to_proto};
use sdi_proto::knowledge::v1::*;
use tonic::Status;

use super::{KnowledgeState, github_status};
use crate::{
    auth::Caller,
    db,
    github::{AccessKind, GithubClient, token::PUBLIC_API},
};

/// The account behind the workspace's token. A personal workspace shows the
/// user; an organisation shows the installation (its slug and what the
/// installation grants), because an installation token cannot call `/user`.
pub(super) async fn get_github_account(
    state: &KnowledgeState,
    caller: &Caller,
    _req: GetGithubAccountRequest,
) -> Result<GithubAccount, Status> {
    let github = match state.github(caller).await {
        Ok(github) => github,
        Err(status) => {
            return Ok(GithubAccount {
                token_configured: false,
                error: status.message().to_owned(),
                ..Default::default()
            });
        }
    };
    match github.access().kind {
        AccessKind::Installation => installation_account(&github).await,
        AccessKind::User => user_account(&github).await,
    }
}

async fn user_account(github: &GithubClient) -> Result<GithubAccount, Status> {
    // A bad token is an answer, not a failure: the UI shows it next to the account box.
    let (user, scopes) = match github.user().await {
        Ok(user) => user,
        Err(error) => {
            return Ok(GithubAccount {
                token_configured: true,
                error: error.to_string(),
                ..Default::default()
            });
        }
    };
    // `/user` only reports private repositories for classic tokens with the
    // `user` scope; counting the list the token can actually see is right
    // for fine-grained tokens too, and the list is ETag-cached.
    let private_repos = match github.repos().await {
        Ok(repos) => repos.iter().filter(|r| r.private).count() as u32,
        Err(error) => {
            tracing::warn!(%error, "could not list repositories for the private count");
            user.total_private_repos
                .or(user.owned_private_repos)
                .unwrap_or(0)
        }
    };
    Ok(GithubAccount {
        token_configured: true,
        login: user.login,
        name: user.name.unwrap_or_default(),
        avatar_url: user.avatar_url,
        html_url: user.html_url,
        public_repos: user.public_repos,
        private_repos,
        scopes,
        error: String::new(),
    })
}

async fn installation_account(github: &GithubClient) -> Result<GithubAccount, Status> {
    let access = github.access();
    let repos = match github.installation_repos().await {
        Ok(repos) => repos,
        Err(error) => {
            return Ok(GithubAccount {
                token_configured: true,
                login: access.slug.clone(),
                error: error.to_string(),
                ..Default::default()
            });
        }
    };
    let private_repos = repos.iter().filter(|r| r.private).count() as u32;
    Ok(GithubAccount {
        token_configured: true,
        login: access.slug.clone(),
        name: access.slug.clone(),
        avatar_url: repos
            .first()
            .map(|r| format!("{}.png", org_url(&access.api_base_url, &r.owner.login)))
            .unwrap_or_default(),
        html_url: org_url(&access.api_base_url, &access.slug),
        public_repos: repos.len() as u32 - private_repos,
        private_repos,
        scopes: vec!["installation".to_owned()],
        error: String::new(),
    })
}

/// `https://github.com/<slug>` or the GHES host behind an `/api/v3` base.
fn org_url(api_base_url: &str, slug: &str) -> String {
    let host = if api_base_url == PUBLIC_API {
        "https://github.com".to_owned()
    } else {
        api_base_url
            .trim_end_matches('/')
            .trim_end_matches("/api/v3")
            .to_owned()
    };
    format!("{host}/{slug}")
}

/// The repositories the workspace's token grants: the installation's
/// selection for an organisation, the user's own for a personal workspace.
pub(super) async fn list_github_repos(
    state: &KnowledgeState,
    caller: &Caller,
    req: ListGithubReposRequest,
) -> Result<ListGithubReposResponse, Status> {
    let github = state.github(caller).await?;
    let repos = github.accessible_repos().await.map_err(github_status)?;
    let connected: HashMap<String, String> = db::projects::list(&state.pool, caller.workspace_id)
        .await
        .map_err(db_status)?
        .into_iter()
        .map(|p| (p.repo_full_name.to_lowercase(), p.id.to_string()))
        .collect();
    let query = req.query.trim().to_lowercase();

    let repos = repos
        .into_iter()
        .filter(|r| query.is_empty() || r.full_name.to_lowercase().contains(&query))
        .map(|r| GithubRepo {
            project_id: connected
                .get(&r.full_name.to_lowercase())
                .cloned()
                .unwrap_or_default(),
            id: r.id,
            full_name: r.full_name,
            name: r.name,
            owner: r.owner.login,
            is_private: r.private,
            description: r.description.unwrap_or_default(),
            default_branch: r.default_branch,
            language: r.language.unwrap_or_default(),
            html_url: r.html_url,
            pushed_at: opt_to_proto(r.pushed_at),
            stars: r.stargazers_count,
            archived: r.archived,
            fork: r.fork,
        })
        .collect();
    Ok(ListGithubReposResponse { repos })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn org_urls_follow_the_api_base() {
        assert_eq!(org_url(PUBLIC_API, "acme"), "https://github.com/acme");
        assert_eq!(
            org_url("https://ghe.example.com/api/v3", "acme"),
            "https://ghe.example.com/acme"
        );
    }
}
