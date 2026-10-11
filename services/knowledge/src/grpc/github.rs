use std::collections::HashMap;

use sdi_common::{grpc::db_status, time::opt_to_proto};
use sdi_proto::knowledge::v1::*;
use tonic::Status;

use super::{KnowledgeState, github_status};
use crate::db;

pub(super) async fn get_github_account(
    state: &KnowledgeState,
    _req: GetGithubAccountRequest,
) -> Result<GithubAccount, Status> {
    if !state.github.token_configured() {
        return Ok(GithubAccount {
            token_configured: false,
            error: "GITHUB_TOKEN is not configured".to_owned(),
            ..Default::default()
        });
    }
    // A bad token is an answer, not a failure: the UI shows it next to the account box.
    let (user, scopes) = match state.github.user().await {
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
    let private_repos = match state.github.repos().await {
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

pub(super) async fn list_github_repos(
    state: &KnowledgeState,
    req: ListGithubReposRequest,
) -> Result<ListGithubReposResponse, Status> {
    let repos = state.github.repos().await.map_err(github_status)?;
    let connected: HashMap<String, String> = db::projects::list(&state.pool)
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
