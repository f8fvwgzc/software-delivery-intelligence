//! Pull request autopilot (CONTRACT.md §23): drafting a fix for a finding,
//! committing a review's accepted fixes, merge readiness and the merge,
//! the project's vulnerable dependencies and turning Dependabot on.
//!
//! The three routes that change the repository answer 201 with the
//! execution that will do it: permission, policy and approval are the
//! execution's (§7), so nothing here commits or merges by itself.
//!
//! Roles: reads VIEWER; drafting and applying fixes MEMBER; merging
//! APPROVER and configuring Dependabot ADMIN (both also checked by the auth
//! middleware). The intelligence service enforces all of them from the
//! forwarded principal.

use axum::{Json, http::StatusCode};
use sdi_proto::intelligence::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::{
        autopilot::{
            ConfigureDependabotRequest, ConfigureDependabotResult, MergePullRequestRequest,
            MergeReadiness, MergeResult, SecurityOverview, SecurityQuery,
        },
        reviews::{Finding, Review},
    },
    error::{ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Scoped},
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(draft_fix))
        .routes(routes!(apply_fixes))
        .routes(routes!(merge_readiness))
        .routes(routes!(merge_pull_request))
        .routes(routes!(security_overview))
        .routes(routes!(configure_dependabot))
}

// --- fixes ------------------------------------------------------------------

/// Asks the workspace's model to write the replacement for the lines the
/// finding points at (member). The answer is the finding with the draft as
/// its `suggestion` and `suggestionSource` = `model`; its decision does not
/// change, a person still accepts or dismisses it.
///
/// 412 when the data controls do not allow it (`reasoner` off, or
/// `codeToModel` not `changed_lines`), when the finding cannot be drafted
/// (`canDraftFix` is false: it already has a suggestion, has no line, is
/// about a secret, ...), when the model's reply was unusable or when the
/// daily token cap is spent. Nothing is stored in those cases.
#[utoipa::path(
    post,
    path = "/findings/{id}/draft-fix",
    tag = "autopilot",
    params(("id" = String, Path, description = "Finding id")),
    responses((status = 200, body = Finding), ErrorResponses),
)]
async fn draft_fix(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<Json<Finding>> {
    let finding = state
        .intelligence()
        .draft_fix(proto::DraftFixRequest { finding_id: id })
        .await?
        .into_inner();
    Ok(Json(finding.into()))
}

/// Commits the suggestion of every accepted finding that no fix execution
/// carries yet to the pull request branch (approver, like every request that
/// writes to the repository): one commit on top of
/// the reviewed head, through an execution that waits for approval. The
/// answer is the review, whose `fixExecutionId` is that execution.
///
/// 412 when no accepted finding has a suggestion left to commit, when the
/// pull request is not open, when its head moved since the review, when it
/// comes from a fork, or while a fix execution of this review is still open.
#[utoipa::path(
    post,
    path = "/reviews/{id}/apply-fixes",
    tag = "autopilot",
    params(("id" = String, Path, description = "Review id")),
    responses((status = 201, body = Review), ErrorResponses),
)]
async fn apply_fixes(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<(StatusCode, Json<Review>)> {
    let review = state
        .intelligence()
        .apply_fixes(proto::ApplyFixesRequest { review_id: id })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(review.into())))
}

// --- merge ------------------------------------------------------------------

/// Whether the pull request can be merged now and what stands in the way:
/// GitHub read at this moment (state, conflicts, checks, reviews, branch
/// protection) plus SDI's own gate (its review of the head commit).
#[utoipa::path(
    get,
    path = "/projects/{id}/pull-requests/{number}/merge-readiness",
    tag = "autopilot",
    params(
        ("id" = String, Path, description = "Project id"),
        ("number" = u64, Path, description = "Pull request number"),
    ),
    responses((status = 200, body = MergeReadiness), ErrorResponses),
)]
async fn merge_readiness(
    state: Scoped,
    ApiPath((id, number)): ApiPath<(String, u64)>,
) -> ApiResult<Json<MergeReadiness>> {
    let readiness = state
        .intelligence()
        .get_merge_readiness(proto::GetMergeReadinessRequest {
            project_id: id,
            pr_number: number,
        })
        .await?
        .into_inner();
    Ok(Json(readiness.into()))
}

/// Asks for the pull request to be merged (approver): creates an execution
/// that verifies the head again, merges exactly that commit and, when
/// asked, deletes the branch. The merge step waits for approval unless the
/// workspace set the `repo.merge_pull_request` policy to `auto`.
///
/// 412 with the first blocker's sentence unless the pull request is ready
/// (`GET .../merge-readiness`). The body may be left out.
#[utoipa::path(
    post,
    path = "/projects/{id}/pull-requests/{number}/merge",
    tag = "autopilot",
    params(
        ("id" = String, Path, description = "Project id"),
        ("number" = u64, Path, description = "Pull request number"),
    ),
    request_body = MergePullRequestRequest,
    responses((status = 201, body = MergeResult), ErrorResponses),
)]
async fn merge_pull_request(
    state: Scoped,
    ApiPath((id, number)): ApiPath<(String, u64)>,
    body: Option<ApiJson<MergePullRequestRequest>>,
) -> ApiResult<(StatusCode, Json<MergeResult>)> {
    let body = body.map(|ApiJson(body)| body).unwrap_or_default();
    let response = state
        .intelligence()
        .merge_pull_request(proto::MergePullRequestRequest {
            project_id: id,
            pr_number: number,
            method: body
                .method
                .map(|method| method.as_str().to_owned())
                .unwrap_or_default(),
            delete_branch: body.delete_branch.unwrap_or(false),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(response.into())))
}

// --- vulnerabilities and Dependabot -----------------------------------------

/// The project's vulnerable dependencies: Dependabot's open alerts when
/// GitHub serves them, else the manifest versions looked up at OSV
/// (`source` says which, `sourceNote` why), with the state of Dependabot on
/// the repository. Cached 5 minutes; `refresh=true` asks again.
#[utoipa::path(
    get,
    path = "/projects/{id}/security",
    tag = "autopilot",
    params(("id" = String, Path, description = "Project id"), SecurityQuery),
    responses((status = 200, body = SecurityOverview), ErrorResponses),
)]
async fn security_overview(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<SecurityQuery>,
) -> ApiResult<Json<SecurityOverview>> {
    let overview = state
        .intelligence()
        .get_security_overview(proto::GetSecurityOverviewRequest {
            project_id: id,
            refresh: query.refresh.unwrap_or(false),
        })
        .await?
        .into_inner();
    Ok(Json(overview.into()))
}

/// Turns Dependabot on for the repository (admin): alerts, security
/// updates and/or a pull request that adds `.github/dependabot.yml`,
/// through an execution. Changing repository settings needs a timed
/// `repository:admin` grant, so its first run pauses for access.
///
/// 400 unless at least one of the three is true.
#[utoipa::path(
    post,
    path = "/projects/{id}/security/dependabot",
    tag = "autopilot",
    params(("id" = String, Path, description = "Project id")),
    request_body = ConfigureDependabotRequest,
    responses((status = 201, body = ConfigureDependabotResult), ErrorResponses),
)]
async fn configure_dependabot(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    body: Option<ApiJson<ConfigureDependabotRequest>>,
) -> ApiResult<(StatusCode, Json<ConfigureDependabotResult>)> {
    let body = body.map(|ApiJson(body)| body).unwrap_or_default();
    let response = state
        .intelligence()
        .configure_dependabot(proto::ConfigureDependabotRequest {
            project_id: id,
            alerts: body.alerts.unwrap_or(false),
            security_updates: body.security_updates.unwrap_or(false),
            config_file: body.config_file.unwrap_or(false),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(response.into())))
}
