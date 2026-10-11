//! Projects and everything the knowledge service knows about one of them:
//! scans, facts, questions, understanding, graph, dependencies, delivery
//! history and similar projects.

use axum::{Json, http::StatusCode};
use sdi_proto::knowledge::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::knowledge::{
        AnalyzeImpactRequest, AnswerQuestionRequest, ConnectProjectRequest, DeliveryHistoryQuery,
        DependenciesResponse, DeploymentsResponse, Fact, GetGraphQuery, Graph, ImpactAnalysis,
        ListFactsQuery, ListQuestionsQuery, ListScansQuery, ListSimilarQuery, MaturityScore,
        Pipeline, PipelineRunsResponse, Project, ProjectConfig, Question, Scan,
        SetProjectModeRequest, SimilarProject, Understanding,
    },
    error::{ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Scoped},
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_projects, connect_project))
        .routes(routes!(get_project, delete_project))
        .routes(routes!(set_project_mode))
        .routes(routes!(list_scans, start_scan))
        .routes(routes!(get_scan))
        .routes(routes!(list_facts))
        .routes(routes!(list_questions))
        .routes(routes!(answer_question))
        .routes(routes!(get_understanding))
        .routes(routes!(get_maturity))
        .routes(routes!(get_config))
        .routes(routes!(get_graph))
        .routes(routes!(analyze_impact))
        .routes(routes!(list_dependencies))
        .routes(routes!(list_pipelines))
        .routes(routes!(list_pipeline_runs))
        .routes(routes!(list_deployments))
        .routes(routes!(list_similar_projects))
}

/// Every connected project.
#[utoipa::path(
    get,
    path = "/projects",
    tag = "projects",
    responses((status = 200, body = Vec<Project>), ErrorResponses),
)]
async fn list_projects(state: Scoped) -> ApiResult<Json<Vec<Project>>> {
    let response = state
        .knowledge()
        .list_projects(proto::ListProjectsRequest {})
        .await?
        .into_inner();
    Ok(Json(map_all(response.projects)))
}

/// Connects a repository and queues its first scan.
#[utoipa::path(
    post,
    path = "/projects",
    tag = "projects",
    request_body = ConnectProjectRequest,
    responses((status = 201, body = Project), ErrorResponses),
)]
async fn connect_project(
    state: Scoped,
    ApiJson(body): ApiJson<ConnectProjectRequest>,
) -> ApiResult<(StatusCode, Json<Project>)> {
    let project = state
        .knowledge()
        .connect_project(proto::ConnectProjectRequest {
            repo_full_name: body.repo_full_name,
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(project.into())))
}

/// One project.
#[utoipa::path(
    get,
    path = "/projects/{id}",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = Project), ErrorResponses),
)]
async fn get_project(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<Json<Project>> {
    let project = state
        .knowledge()
        .get_project(proto::GetProjectRequest { id })
        .await?
        .into_inner();
    Ok(Json(project.into()))
}

/// Disconnects a project and deletes everything known about it.
#[utoipa::path(
    delete,
    path = "/projects/{id}",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, description = "Deleted"), ErrorResponses),
)]
async fn delete_project(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<StatusCode> {
    state
        .knowledge()
        .delete_project(proto::DeleteProjectRequest { id })
        .await?;
    Ok(StatusCode::OK)
}

/// Switches a project between observe and execute mode.
#[utoipa::path(
    patch,
    path = "/projects/{id}/mode",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    request_body = SetProjectModeRequest,
    responses((status = 200, body = Project), ErrorResponses),
)]
async fn set_project_mode(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<SetProjectModeRequest>,
) -> ApiResult<Json<Project>> {
    let project = state
        .knowledge()
        .set_project_mode(proto::SetProjectModeRequest {
            id,
            mode: body.mode.into(),
        })
        .await?
        .into_inner();
    Ok(Json(project.into()))
}

/// Scans of a project, newest first.
#[utoipa::path(
    get,
    path = "/projects/{id}/scans",
    tag = "scans",
    params(("id" = String, Path, description = "Project id"), ListScansQuery),
    responses((status = 200, body = Vec<Scan>), ErrorResponses),
)]
async fn list_scans(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ListScansQuery>,
) -> ApiResult<Json<Vec<Scan>>> {
    let response = state
        .knowledge()
        .list_scans(proto::ListScansRequest {
            project_id: id,
            limit: query.limit.unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.scans)))
}

/// Queues a new scan of the project's default branch.
#[utoipa::path(
    post,
    path = "/projects/{id}/scans",
    tag = "scans",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 201, body = Scan), ErrorResponses),
)]
async fn start_scan(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<(StatusCode, Json<Scan>)> {
    let scan = state
        .knowledge()
        .start_scan(proto::StartScanRequest { project_id: id })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(scan.into())))
}

/// One scan with its stages.
#[utoipa::path(
    get,
    path = "/scans/{id}",
    tag = "scans",
    params(("id" = String, Path, description = "Scan id")),
    responses((status = 200, body = Scan), ErrorResponses),
)]
async fn get_scan(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<Json<Scan>> {
    let scan = state
        .knowledge()
        .get_scan(proto::GetScanRequest { id })
        .await?
        .into_inner();
    Ok(Json(scan.into()))
}

/// Facts of a project, optionally filtered by kind and category.
#[utoipa::path(
    get,
    path = "/projects/{id}/facts",
    tag = "projects",
    params(("id" = String, Path, description = "Project id"), ListFactsQuery),
    responses((status = 200, body = Vec<Fact>), ErrorResponses),
)]
async fn list_facts(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ListFactsQuery>,
) -> ApiResult<Json<Vec<Fact>>> {
    let response = state
        .knowledge()
        .list_facts(proto::ListFactsRequest {
            project_id: id,
            kind: query.kind.map(Into::into).unwrap_or(0),
            category: query.category.map(Into::into).unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.facts)))
}

/// Open questions of a project (and answered ones unless `onlyOpen`).
#[utoipa::path(
    get,
    path = "/projects/{id}/questions",
    tag = "projects",
    params(("id" = String, Path, description = "Project id"), ListQuestionsQuery),
    responses((status = 200, body = Vec<Question>), ErrorResponses),
)]
async fn list_questions(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ListQuestionsQuery>,
) -> ApiResult<Json<Vec<Question>>> {
    let response = state
        .knowledge()
        .list_questions(proto::ListQuestionsRequest {
            project_id: id,
            only_open: query.only_open.unwrap_or(false),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.questions)))
}

/// Answers a question. Asked once, remembered afterwards.
#[utoipa::path(
    post,
    path = "/questions/{id}/answer",
    tag = "projects",
    params(("id" = String, Path, description = "Question id")),
    request_body = AnswerQuestionRequest,
    responses((status = 200, body = Question), ErrorResponses),
)]
async fn answer_question(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<AnswerQuestionRequest>,
) -> ApiResult<Json<Question>> {
    let question = state
        .knowledge()
        .answer_question(proto::AnswerQuestionRequest {
            id,
            answer: body.answer,
        })
        .await?
        .into_inner();
    Ok(Json(question.into()))
}

/// TL;DR and deep-dive understanding of a project.
#[utoipa::path(
    get,
    path = "/projects/{id}/understanding",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = Understanding), ErrorResponses),
)]
async fn get_understanding(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Understanding>> {
    let understanding = state
        .knowledge()
        .get_understanding(proto::GetUnderstandingRequest { project_id: id })
        .await?
        .into_inner();
    Ok(Json(understanding.into()))
}

/// Delivery maturity: eight axes 0–100 with findings and the patterns that
/// would raise each (CONTRACT.md §13).
#[utoipa::path(
    get,
    path = "/projects/{id}/maturity",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = MaturityScore), ErrorResponses),
)]
async fn get_maturity(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<MaturityScore>> {
    let maturity = state
        .knowledge()
        .get_maturity(proto::GetMaturityRequest { project_id: id })
        .await?
        .into_inner();
    Ok(Json(maturity.into()))
}

/// What the repository declares in `.sdi.yml` (CONTRACT.md §12). A project
/// without the file answers `present: false`.
#[utoipa::path(
    get,
    path = "/projects/{id}/config",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = ProjectConfig), ErrorResponses),
)]
async fn get_config(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<Json<ProjectConfig>> {
    // The config only travels inside the project context; a service that
    // leaves it unset means "no file", not an error.
    let context = state
        .knowledge()
        .get_project_context(proto::GetProjectContextRequest { project_id: id })
        .await?
        .into_inner();
    Ok(Json(context.config.map(Into::into).unwrap_or_default()))
}

/// Knowledge graph of a project.
#[utoipa::path(
    get,
    path = "/projects/{id}/graph",
    tag = "projects",
    params(("id" = String, Path, description = "Project id"), GetGraphQuery),
    responses((status = 200, body = Graph), ErrorResponses),
)]
async fn get_graph(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<GetGraphQuery>,
) -> ApiResult<Json<Graph>> {
    let graph = state
        .knowledge()
        .get_graph(proto::GetGraphRequest {
            project_id: id,
            include_history: query.include_history.unwrap_or(false),
        })
        .await?
        .into_inner();
    Ok(Json(graph.into()))
}

/// What a change to one node or path would touch.
#[utoipa::path(
    post,
    path = "/projects/{id}/impact",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    request_body = AnalyzeImpactRequest,
    responses((status = 200, body = ImpactAnalysis), ErrorResponses),
)]
async fn analyze_impact(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<AnalyzeImpactRequest>,
) -> ApiResult<Json<ImpactAnalysis>> {
    if body.node_id.as_deref().unwrap_or_default().is_empty()
        && body.path.as_deref().unwrap_or_default().is_empty()
    {
        return Err(ApiError::bad_request("one of nodeId or path is required"));
    }
    let analysis = state
        .knowledge()
        .analyze_impact(proto::AnalyzeImpactRequest {
            project_id: id,
            node_id: body.node_id.unwrap_or_default(),
            path: body.path.unwrap_or_default(),
            max_depth: body.max_depth.unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(analysis.into()))
}

/// Declared dependencies with ecosystem counts and Dependabot status.
#[utoipa::path(
    get,
    path = "/projects/{id}/dependencies",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = DependenciesResponse), ErrorResponses),
)]
async fn list_dependencies(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<DependenciesResponse>> {
    let response = state
        .knowledge()
        .list_dependencies(proto::ListDependenciesRequest { project_id: id })
        .await?
        .into_inner();
    Ok(Json(response.into()))
}

/// Pipeline definitions found in the repository.
#[utoipa::path(
    get,
    path = "/projects/{id}/pipelines",
    tag = "delivery",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = Vec<Pipeline>), ErrorResponses),
)]
async fn list_pipelines(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Vec<Pipeline>>> {
    let response = state
        .knowledge()
        .list_pipelines(proto::ListPipelinesRequest { project_id: id })
        .await?
        .into_inner();
    Ok(Json(map_all(response.pipelines)))
}

/// Pipeline runs observed at the provider, with aggregate stats.
#[utoipa::path(
    get,
    path = "/projects/{id}/pipeline-runs",
    tag = "delivery",
    params(("id" = String, Path, description = "Project id"), DeliveryHistoryQuery),
    responses((status = 200, body = PipelineRunsResponse), ErrorResponses),
)]
async fn list_pipeline_runs(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<DeliveryHistoryQuery>,
) -> ApiResult<Json<PipelineRunsResponse>> {
    let response = state
        .knowledge()
        .list_pipeline_runs(proto::ListPipelineRunsRequest {
            project_id: id,
            refresh: query.refresh.unwrap_or(false),
            limit: query.limit.unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(response.into()))
}

/// Deployments observed at the provider, with aggregate stats.
#[utoipa::path(
    get,
    path = "/projects/{id}/deployments",
    tag = "delivery",
    params(("id" = String, Path, description = "Project id"), DeliveryHistoryQuery),
    responses((status = 200, body = DeploymentsResponse), ErrorResponses),
)]
async fn list_deployments(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<DeliveryHistoryQuery>,
) -> ApiResult<Json<DeploymentsResponse>> {
    let response = state
        .knowledge()
        .list_deployments(proto::ListDeploymentsRequest {
            project_id: id,
            refresh: query.refresh.unwrap_or(false),
            limit: query.limit.unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(response.into()))
}

/// Projects that look like this one, most similar first.
#[utoipa::path(
    get,
    path = "/projects/{id}/similar",
    tag = "projects",
    params(("id" = String, Path, description = "Project id"), ListSimilarQuery),
    responses((status = 200, body = Vec<SimilarProject>), ErrorResponses),
)]
async fn list_similar_projects(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ListSimilarQuery>,
) -> ApiResult<Json<Vec<SimilarProject>>> {
    let response = state
        .knowledge()
        .list_similar_projects(proto::ListSimilarProjectsRequest {
            project_id: id,
            limit: query.limit.unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.projects)))
}
