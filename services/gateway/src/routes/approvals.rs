//! Human approvals requested before a step runs.

use axum::Json;
use sdi_proto::execution::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::execution::{Approval, DecideApprovalRequest, ListApprovalsQuery},
    error::{ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Scoped},
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_approvals))
        .routes(routes!(decide_approval))
}

/// Approvals, optionally filtered by status and execution.
#[utoipa::path(
    get,
    path = "/approvals",
    tag = "approvals",
    params(ListApprovalsQuery),
    responses((status = 200, body = Vec<Approval>), ErrorResponses),
)]
async fn list_approvals(
    state: Scoped,
    ApiQuery(query): ApiQuery<ListApprovalsQuery>,
) -> ApiResult<Json<Vec<Approval>>> {
    let response = state
        .execution()
        .list_approvals(proto::ListApprovalsRequest {
            status: query.status.map(Into::into).unwrap_or(0),
            execution_id: query.execution_id.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.approvals)))
}

/// Approves or rejects a pending approval.
#[utoipa::path(
    post,
    path = "/approvals/{id}/decide",
    tag = "approvals",
    params(("id" = String, Path, description = "Approval id")),
    request_body = DecideApprovalRequest,
    responses((status = 200, body = Approval), ErrorResponses),
)]
async fn decide_approval(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<DecideApprovalRequest>,
) -> ApiResult<Json<Approval>> {
    let approval = state
        .execution()
        .decide_approval(proto::DecideApprovalRequest {
            id,
            approve: body.approve,
            note: body.note.unwrap_or_default(),
            decided_by: body.decided_by.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(approval.into()))
}
