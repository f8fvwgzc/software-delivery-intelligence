//! Read-only GitHub connector.

use axum::Json;
use sdi_proto::knowledge::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::github::{GithubAccount, GithubRepo, ListReposQuery},
    error::{ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Scoped},
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_account))
        .routes(routes!(list_repos))
}

/// The account behind the configured GitHub token.
#[utoipa::path(
    get,
    path = "/github/account",
    tag = "github",
    responses((status = 200, body = GithubAccount), ErrorResponses),
)]
async fn get_account(state: Scoped) -> ApiResult<Json<GithubAccount>> {
    let account = state
        .knowledge()
        .get_github_account(proto::GetGithubAccountRequest {})
        .await?
        .into_inner();
    Ok(Json(account.into()))
}

/// Repositories the token can see, optionally filtered by name.
#[utoipa::path(
    get,
    path = "/github/repos",
    tag = "github",
    params(ListReposQuery),
    responses((status = 200, body = Vec<GithubRepo>), ErrorResponses),
)]
async fn list_repos(
    state: Scoped,
    ApiQuery(query): ApiQuery<ListReposQuery>,
) -> ApiResult<Json<Vec<GithubRepo>>> {
    let response = state
        .knowledge()
        .list_github_repos(proto::ListGithubReposRequest {
            query: query.query.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.repos)))
}
