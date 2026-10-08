//! Policies per action type and the scoped identities actions run as.

use axum::{Json, extract::State};
use sdi_proto::execution::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::execution::{GrantPermissionRequest, Identity, Policy, UpdatePolicyRequest},
    error::{ApiError, ApiJson, ApiPath, ApiResult, ErrorResponses},
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_policies))
        .routes(routes!(update_policy))
        .routes(routes!(list_identities))
        .routes(routes!(grant_permission))
}

/// Every policy row (CONTRACT.md §5.2).
#[utoipa::path(
    get,
    path = "/policies",
    tag = "policies",
    responses((status = 200, body = Vec<Policy>), ErrorResponses),
)]
async fn list_policies(State(state): State<AppState>) -> ApiResult<Json<Vec<Policy>>> {
    let response = state
        .execution()
        .list_policies(proto::ListPoliciesRequest {})
        .await?
        .into_inner();
    Ok(Json(map_all(response.policies)))
}

/// Changes the decision of an editable policy.
#[utoipa::path(
    patch,
    path = "/policies/{actionType}",
    tag = "policies",
    params(("actionType" = String, Path, description = "Policy key, e.g. `deploy.staging`")),
    request_body = UpdatePolicyRequest,
    responses((status = 200, body = Policy), ErrorResponses),
)]
async fn update_policy(
    State(state): State<AppState>,
    ApiPath(action_type): ApiPath<String>,
    ApiJson(body): ApiJson<UpdatePolicyRequest>,
) -> ApiResult<Json<Policy>> {
    let policy = state
        .execution()
        .update_policy(proto::UpdatePolicyRequest {
            action_type,
            decision: body.decision.into(),
        })
        .await?
        .into_inner();
    Ok(Json(policy.into()))
}

/// Every identity with its current grants.
#[utoipa::path(
    get,
    path = "/identities",
    tag = "policies",
    responses((status = 200, body = Vec<Identity>), ErrorResponses),
)]
async fn list_identities(State(state): State<AppState>) -> ApiResult<Json<Vec<Identity>>> {
    let response = state
        .execution()
        .list_identities(proto::ListIdentitiesRequest {})
        .await?
        .into_inner();
    Ok(Json(map_all(response.identities)))
}

/// Issues a short-lived, scoped grant. Nothing is ever granted permanently.
#[utoipa::path(
    post,
    path = "/identities/{id}/grants",
    tag = "policies",
    params(("id" = String, Path, description = "Identity id")),
    request_body = GrantPermissionRequest,
    responses((status = 200, body = Identity), ErrorResponses),
)]
async fn grant_permission(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<GrantPermissionRequest>,
) -> ApiResult<Json<Identity>> {
    if body.permissions.is_empty() {
        return Err(ApiError::bad_request("permissions must not be empty"));
    }
    let identity = state
        .execution()
        .grant_permission(proto::GrantPermissionRequest {
            identity_id: id,
            permissions: body.permissions,
            ttl_minutes: body.ttl_minutes.unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(identity.into()))
}
