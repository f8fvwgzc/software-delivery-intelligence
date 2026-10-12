//! Proposals, their negotiation threads, the generated files behind each
//! option and the shortcut that turns an option into a pull request.

use axum::{Json, http::StatusCode};
use sdi_proto::intelligence::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::{
        common::Adapter,
        intelligence::{
            AcceptProposalRequest, Artifact, GenerateProposalRequest, ListProposalsQuery, Message,
            NegotiateRequest, NegotiateResponse, OpenPullRequestRequest, Proposal,
            RejectProposalRequest,
        },
    },
    error::{ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Scoped},
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_proposals, generate_proposal))
        .routes(routes!(get_proposal))
        .routes(routes!(accept_proposal))
        .routes(routes!(reject_proposal))
        .routes(routes!(list_messages, negotiate))
        .routes(routes!(list_option_artifacts))
        .routes(routes!(open_pull_request))
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
    state: Scoped,
    ApiQuery(query): ApiQuery<ListProposalsQuery>,
) -> ApiResult<Json<Vec<Proposal>>> {
    let response = state
        .intelligence()
        .list_proposals(proto::ListProposalsRequest {
            project_id: query.project_id.unwrap_or_default(),
            limit: query.limit.unwrap_or(0),
            before: query.before.map(sdi_common::time::to_proto),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.proposals)))
}

/// Generates a ranked, evidence-backed proposal for a project. A
/// `budgetMonthlyUsd` becomes a constraint the options are marked against.
#[utoipa::path(
    post,
    path = "/proposals",
    tag = "proposals",
    request_body = GenerateProposalRequest,
    responses((status = 201, body = Proposal), ErrorResponses),
)]
async fn generate_proposal(
    state: Scoped,
    ApiJson(body): ApiJson<GenerateProposalRequest>,
) -> ApiResult<(StatusCode, Json<Proposal>)> {
    let budget = body.budget_monthly_usd.unwrap_or(0.0);
    if budget < 0.0 || !budget.is_finite() {
        return Err(ApiError::bad_request(
            "budgetMonthlyUsd must be a non-negative number",
        ));
    }
    let proposal = state
        .intelligence()
        .generate_proposal(proto::GenerateProposalRequest {
            project_id: body.project_id,
            kind: body.kind.into(),
            prompt: body.prompt.unwrap_or_default(),
            budget_monthly_usd: budget,
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(proposal.into())))
}

/// One proposal with its options. Artifacts are summarised as
/// `artifactCount`; fetch them per option.
#[utoipa::path(
    get,
    path = "/proposals/{id}",
    tag = "proposals",
    params(("id" = String, Path, description = "Proposal id")),
    responses((status = 200, body = Proposal), ErrorResponses),
)]
async fn get_proposal(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<Json<Proposal>> {
    let proposal = state
        .intelligence()
        .get_proposal(proto::GetProposalRequest { id })
        .await?
        .into_inner();
    Ok(Json(proposal.into()))
}

/// Accepts an option (the recommended one by default), optionally creating
/// an execution on the chosen adapter.
#[utoipa::path(
    post,
    path = "/proposals/{id}/accept",
    tag = "proposals",
    params(("id" = String, Path, description = "Proposal id")),
    request_body = AcceptProposalRequest,
    responses((status = 200, body = Proposal), ErrorResponses),
)]
async fn accept_proposal(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<AcceptProposalRequest>,
) -> ApiResult<Json<Proposal>> {
    let proposal = state
        .intelligence()
        .accept_proposal(proto::AcceptProposalRequest {
            id,
            option_id: body.option_id.unwrap_or_default(),
            create_execution: body.create_execution.unwrap_or(false),
            adapter: body.adapter.unwrap_or_default().as_str().to_owned(),
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
    state: Scoped,
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
    state: Scoped,
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
    state: Scoped,
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

/// The generated files of one option, complete and ready to commit. Kept
/// off the proposal body because they can be large.
#[utoipa::path(
    get,
    path = "/proposals/{id}/options/{optionId}/artifacts",
    tag = "proposals",
    params(
        ("id" = String, Path, description = "Proposal id"),
        ("optionId" = String, Path, description = "Option id"),
    ),
    responses((status = 200, body = Vec<Artifact>), ErrorResponses),
)]
async fn list_option_artifacts(
    state: Scoped,
    ApiPath((id, option_id)): ApiPath<(String, String)>,
) -> ApiResult<Json<Vec<Artifact>>> {
    let proposal = state
        .intelligence()
        .get_proposal(proto::GetProposalRequest { id })
        .await?
        .into_inner();
    let option = proposal
        .options
        .into_iter()
        .find(|option| option.id == option_id)
        .ok_or_else(|| ApiError::not_found(format!("no option {option_id} in this proposal")))?;
    Ok(Json(map_all(option.artifacts)))
}

/// Accepts an option and opens a pull request with its generated files:
/// an execution on the `github` adapter (`repo.checkout` →
/// `repo.open_pull_request`). The returned proposal carries the
/// `executionId`; the execution's `outputs.pull_request_url` appears once
/// the step succeeds.
#[utoipa::path(
    post,
    path = "/proposals/{id}/pull-request",
    tag = "proposals",
    params(("id" = String, Path, description = "Proposal id")),
    request_body = OpenPullRequestRequest,
    responses((status = 201, body = Proposal), ErrorResponses),
)]
async fn open_pull_request(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    body: Option<ApiJson<OpenPullRequestRequest>>,
) -> ApiResult<(StatusCode, Json<Proposal>)> {
    let option_id = body.and_then(|ApiJson(body)| body.option_id);
    let proposal = state
        .intelligence()
        .accept_proposal(proto::AcceptProposalRequest {
            id,
            option_id: option_id.unwrap_or_default(),
            create_execution: true,
            adapter: Adapter::Github.as_str().to_owned(),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(proposal.into())))
}
