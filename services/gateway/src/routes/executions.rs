//! Executions, their event log and delivery counters. The live event stream
//! is in `crate::sse`.

use axum::{Json, extract::State, http::StatusCode};
use sdi_proto::execution::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::execution::{
        CancelExecutionRequest, CreateExecutionRequest, DeliveryStats, DeliveryStatsQuery,
        Execution, ExecutionEvent, ListEventsQuery, ListExecutionsQuery, ResolveFailureRequest,
        StartExecutionRequest,
    },
    error::{ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_executions, create_execution))
        .routes(routes!(get_execution))
        .routes(routes!(start_execution))
        .routes(routes!(resolve_failure))
        .routes(routes!(cancel_execution))
        .routes(routes!(list_events))
        .routes(routes!(delivery_stats))
}

/// Executions, newest first, optionally of one project.
#[utoipa::path(
    get,
    path = "/executions",
    tag = "executions",
    params(ListExecutionsQuery),
    responses((status = 200, body = Vec<Execution>), ErrorResponses),
)]
async fn list_executions(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ListExecutionsQuery>,
) -> ApiResult<Json<Vec<Execution>>> {
    let response = state
        .execution()
        .list_executions(proto::ListExecutionsRequest {
            project_id: query.project_id.unwrap_or_default(),
            limit: query.limit.unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.executions)))
}

/// Creates an execution from a plan. It stays in `planning` until started.
#[utoipa::path(
    post,
    path = "/executions",
    tag = "executions",
    request_body = CreateExecutionRequest,
    responses((status = 201, body = Execution), ErrorResponses),
)]
async fn create_execution(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CreateExecutionRequest>,
) -> ApiResult<(StatusCode, Json<Execution>)> {
    let execution = state
        .execution()
        .create_execution(proto::CreateExecutionRequest {
            project_id: body.project_id,
            proposal_id: body.proposal_id.unwrap_or_default(),
            pattern_id: body.pattern_id.unwrap_or_default(),
            title: body.title,
            steps: body.steps.into_iter().map(Into::into).collect(),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(execution.into())))
}

/// One execution with its steps and, while paused, its failure.
#[utoipa::path(
    get,
    path = "/executions/{id}",
    tag = "executions",
    params(("id" = String, Path, description = "Execution id")),
    responses((status = 200, body = Execution), ErrorResponses),
)]
async fn get_execution(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Execution>> {
    let execution = state
        .execution()
        .get_execution(proto::GetExecutionRequest { id })
        .await?
        .into_inner();
    Ok(Json(execution.into()))
}

/// Runs the policy check and moves the plan to `waiting_for_approval` or `authorized`.
#[utoipa::path(
    post,
    path = "/executions/{id}/start",
    tag = "executions",
    params(("id" = String, Path, description = "Execution id")),
    request_body = StartExecutionRequest,
    responses((status = 200, body = Execution), ErrorResponses),
)]
async fn start_execution(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    body: Option<ApiJson<StartExecutionRequest>>,
) -> ApiResult<Json<Execution>> {
    let fault = body.and_then(|ApiJson(body)| body.fault);
    let execution = state
        .execution()
        .start_execution(proto::StartExecutionRequest {
            id,
            fault: fault.map(Into::into),
        })
        .await?
        .into_inner();
    Ok(Json(execution.into()))
}

/// Answers a paused execution: picks a remediation or gives an instruction, then replans.
#[utoipa::path(
    post,
    path = "/executions/{id}/resolve",
    tag = "executions",
    params(("id" = String, Path, description = "Execution id")),
    request_body = ResolveFailureRequest,
    responses((status = 200, body = Execution), ErrorResponses),
)]
async fn resolve_failure(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<ResolveFailureRequest>,
) -> ApiResult<Json<Execution>> {
    if body.option_id.as_deref().unwrap_or_default().is_empty()
        && body.instruction.as_deref().unwrap_or_default().is_empty()
    {
        return Err(ApiError::bad_request(
            "one of optionId or instruction is required",
        ));
    }
    let execution = state
        .execution()
        .resolve_failure(proto::ResolveFailureRequest {
            execution_id: id,
            option_id: body.option_id.unwrap_or_default(),
            instruction: body.instruction.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(execution.into()))
}

/// Cancels an execution. Pending approvals expire.
#[utoipa::path(
    post,
    path = "/executions/{id}/cancel",
    tag = "executions",
    params(("id" = String, Path, description = "Execution id")),
    request_body = CancelExecutionRequest,
    responses((status = 200, body = Execution), ErrorResponses),
)]
async fn cancel_execution(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<CancelExecutionRequest>,
) -> ApiResult<Json<Execution>> {
    let execution = state
        .execution()
        .cancel_execution(proto::CancelExecutionRequest {
            id,
            reason: body.reason,
        })
        .await?
        .into_inner();
    Ok(Json(execution.into()))
}

/// Event log of an execution, in sequence order.
#[utoipa::path(
    get,
    path = "/executions/{id}/events",
    tag = "executions",
    params(("id" = String, Path, description = "Execution id"), ListEventsQuery),
    responses((status = 200, body = Vec<ExecutionEvent>), ErrorResponses),
)]
async fn list_events(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ListEventsQuery>,
) -> ApiResult<Json<Vec<ExecutionEvent>>> {
    let response = state
        .execution()
        .list_events(proto::ListEventsRequest {
            execution_id: id,
            after_seq: query.after_seq.unwrap_or(0),
            limit: query.limit.unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.events)))
}

/// Delivery counters, optionally of one project.
#[utoipa::path(
    get,
    path = "/delivery/stats",
    tag = "executions",
    params(DeliveryStatsQuery),
    responses((status = 200, body = DeliveryStats), ErrorResponses),
)]
async fn delivery_stats(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<DeliveryStatsQuery>,
) -> ApiResult<Json<DeliveryStats>> {
    let stats = state
        .execution()
        .get_delivery_stats(proto::GetDeliveryStatsRequest {
            project_id: query.project_id.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(stats.into()))
}
