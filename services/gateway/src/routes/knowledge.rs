//! Pattern memory, semantic search and knowledge counters.

use axum::{Json, extract::State};
use sdi_proto::knowledge::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::knowledge::{
        KnowledgeHit, KnowledgeStats, ListPatternsQuery, Pattern, SearchKnowledgeQuery,
    },
    error::{ApiError, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_patterns))
        .routes(routes!(get_pattern))
        .routes(routes!(search_knowledge))
        .routes(routes!(knowledge_stats))
}

/// Catalog and learned patterns.
#[utoipa::path(
    get,
    path = "/patterns",
    tag = "knowledge",
    params(ListPatternsQuery),
    responses((status = 200, body = Vec<Pattern>), ErrorResponses),
)]
async fn list_patterns(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ListPatternsQuery>,
) -> ApiResult<Json<Vec<Pattern>>> {
    let response = state
        .knowledge()
        .list_patterns(proto::ListPatternsRequest {
            category: query.category.unwrap_or_default(),
            include_deprecated: query.include_deprecated.unwrap_or(false),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.patterns)))
}

/// One pattern with its plan template.
#[utoipa::path(
    get,
    path = "/patterns/{id}",
    tag = "knowledge",
    params(("id" = String, Path, description = "Pattern id")),
    responses((status = 200, body = Pattern), ErrorResponses),
)]
async fn get_pattern(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Pattern>> {
    let pattern = state
        .knowledge()
        .get_pattern(proto::GetPatternRequest { id })
        .await?
        .into_inner();
    Ok(Json(pattern.into()))
}

/// Semantic search over everything indexed, best match first.
#[utoipa::path(
    get,
    path = "/knowledge/search",
    tag = "knowledge",
    params(SearchKnowledgeQuery),
    responses((status = 200, body = Vec<KnowledgeHit>), ErrorResponses),
)]
async fn search_knowledge(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<SearchKnowledgeQuery>,
) -> ApiResult<Json<Vec<KnowledgeHit>>> {
    let kinds = query.kinds().map_err(ApiError::bad_request)?;
    let response = state
        .knowledge()
        .search_knowledge(proto::SearchKnowledgeRequest {
            query: query.q,
            project_id: query.project_id.unwrap_or_default(),
            kinds: kinds.into_iter().map(Into::into).collect(),
            limit: query.limit.unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.hits)))
}

/// Counters of the knowledge store.
#[utoipa::path(
    get,
    path = "/knowledge/stats",
    tag = "knowledge",
    responses((status = 200, body = KnowledgeStats), ErrorResponses),
)]
async fn knowledge_stats(State(state): State<AppState>) -> ApiResult<Json<KnowledgeStats>> {
    let stats = state
        .knowledge()
        .get_knowledge_stats(proto::GetKnowledgeStatsRequest {})
        .await?
        .into_inner();
    Ok(Json(stats.into()))
}
