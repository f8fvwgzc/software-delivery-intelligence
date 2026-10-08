//! Proposals and their negotiation threads.

use axum::{Json, extract::State, http::StatusCode};
use sdi_proto::intelligence::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::intelligence::{
        AcceptProposalRequest, GenerateProposalRequest, ListProposalsQuery, Message,
        NegotiateRequest, NegotiateResponse, Proposal, RejectProposalRequest,
    },
    error::{ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_proposals, generate_proposal))
        .routes(routes!(get_proposal))
        .routes(routes!(accept_proposal))
        .routes(routes!(reject_proposal))
        .routes(routes!(list_messages, negotiate))
}

/// Proposals, optionally of one project.
#[utoipa::path(
    get,
    path = "/proposals",
    tag = "proposals",
    params(ListProposalsQuery),
    responses((status = 200, body = Vec<Proposal>), ErrorResponses),
)]
async fn list_proposals(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ListProposalsQuery>,
) -> ApiResult<Json<Vec<Proposal>>> {
    let response = state
        .intelligence()
        .list_proposals(proto::ListProposalsRequest {
            project_id: query.project_id.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.proposals)))
}

/// Generates a ranked, evidence-backed proposal for a project.
#[utoipa::path(
    post,
    path = "/proposals",
    tag = "proposals",
    request_body = GenerateProposalRequest,
    responses((status = 201, body = Proposal), ErrorResponses),
)]
async fn generate_proposal(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<GenerateProposalRequest>,
) -> ApiResult<(StatusCode, Json<Proposal>)> {
    let proposal = state
        .intelligence()
        .generate_proposal(proto::GenerateProposalRequest {
            project_id: body.project_id,
            kind: body.kind.into(),
            prompt: body.prompt.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(proposal.into())))
}

/// One proposal with its options.
#[utoipa::path(
    get,
    path = "/proposals/{id}",
    tag = "proposals",
    params(("id" = String, Path, description = "Proposal id")),
    responses((status = 200, body = Proposal), ErrorResponses),
)]
async fn get_proposal(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Proposal>> {
    let proposal = state
        .intelligence()
        .get_proposal(proto::GetProposalRequest { id })
        .await?
        .into_inner();
    Ok(Json(proposal.into()))
}

/// Accepts an option (the recommended one by default), optionally creating an execution.
#[utoipa::path(
    post,
    path = "/proposals/{id}/accept",
    tag = "proposals",
    params(("id" = String, Path, description = "Proposal id")),
    request_body = AcceptProposalRequest,
    responses((status = 200, body = Proposal), ErrorResponses),
)]
async fn accept_proposal(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<AcceptProposalRequest>,
) -> ApiResult<Json<Proposal>> {
    let proposal = state
        .intelligence()
        .accept_proposal(proto::AcceptProposalRequest {
            id,
            option_id: body.option_id.unwrap_or_default(),
            create_execution: body.create_execution.unwrap_or(false),
        })
        .await?
        .into_inner();
    Ok(Json(proposal.into()))
}

/// Rejects a proposal with a reason.
#[utoipa::path(
    post,
    path = "/proposals/{id}/reject",
    tag = "proposals",
    params(("id" = String, Path, description = "Proposal id")),
    request_body = RejectProposalRequest,
    responses((status = 200, body = Proposal), ErrorResponses),
)]
async fn reject_proposal(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<RejectProposalRequest>,
) -> ApiResult<Json<Proposal>> {
    let proposal = state
        .intelligence()
        .reject_proposal(proto::RejectProposalRequest {
            id,
            reason: body.reason,
        })
        .await?
        .into_inner();
    Ok(Json(proposal.into()))
}

/// Negotiation thread of a proposal, oldest first.
#[utoipa::path(
    get,
    path = "/proposals/{id}/messages",
    tag = "proposals",
    params(("id" = String, Path, description = "Proposal id")),
    responses((status = 200, body = Vec<Message>), ErrorResponses),
)]
async fn list_messages(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Vec<Message>>> {
    let response = state
        .intelligence()
        .list_messages(proto::ListMessagesRequest { proposal_id: id })
        .await?
        .into_inner();
    Ok(Json(map_all(response.messages)))
}

/// One negotiation turn. May re-rank the options and change the recommendation.
#[utoipa::path(
    post,
    path = "/proposals/{id}/messages",
    tag = "proposals",
    params(("id" = String, Path, description = "Proposal id")),
    request_body = NegotiateRequest,
    responses((status = 201, body = NegotiateResponse), ErrorResponses),
)]
async fn negotiate(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<NegotiateRequest>,
) -> ApiResult<(StatusCode, Json<NegotiateResponse>)> {
    let response = state
        .intelligence()
        .negotiate(proto::NegotiateRequest {
            proposal_id: id,
            content: body.content,
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(response.into())))
}
