//! Decision memory with provenance.

use axum::{Json, extract::State, http::StatusCode};
use sdi_proto::intelligence::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::intelligence::{
        CreateDecisionRequest, Decision, ListDecisionsQuery, SupersedeDecisionRequest,
    },
    error::{ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_decisions, create_decision))
        .routes(routes!(supersede_decision))
}

/// Decisions, optionally of one project; active ones unless `includeSuperseded`.
#[utoipa::path(
    get,
    path = "/decisions",
    tag = "decisions",
    params(ListDecisionsQuery),
    responses((status = 200, body = Vec<Decision>), ErrorResponses),
)]
async fn list_decisions(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ListDecisionsQuery>,
) -> ApiResult<Json<Vec<Decision>>> {
    let response = state
        .intelligence()
        .list_decisions(proto::ListDecisionsRequest {
            project_id: query.project_id.unwrap_or_default(),
            include_superseded: query.include_superseded.unwrap_or(false),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.decisions)))
}

/// Records a decision by hand.
#[utoipa::path(
    post,
    path = "/decisions",
    tag = "decisions",
    request_body = CreateDecisionRequest,
    responses((status = 201, body = Decision), ErrorResponses),
)]
async fn create_decision(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CreateDecisionRequest>,
) -> ApiResult<(StatusCode, Json<Decision>)> {
    let decision = state
        .intelligence()
        .create_decision(proto::CreateDecisionRequest {
            project_id: body.project_id.unwrap_or_default(),
            scope: body.scope.into(),
            title: body.title,
            statement: body.statement,
            reasons: body.reasons,
            approved_by: body.approved_by.unwrap_or_default(),
            tags: body.tags,
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(decision.into())))
}

/// Marks a decision as superseded, optionally by another one.
#[utoipa::path(
    post,
    path = "/decisions/{id}/supersede",
    tag = "decisions",
    params(("id" = String, Path, description = "Decision id")),
    request_body = SupersedeDecisionRequest,
    responses((status = 200, body = Decision), ErrorResponses),
)]
async fn supersede_decision(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<SupersedeDecisionRequest>,
) -> ApiResult<Json<Decision>> {
    let decision = state
        .intelligence()
        .supersede_decision(proto::SupersedeDecisionRequest {
            id,
            superseded_by: body.superseded_by.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(decision.into()))
}
