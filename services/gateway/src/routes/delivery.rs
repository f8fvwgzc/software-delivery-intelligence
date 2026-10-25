//! Continuous delivery (CONTRACT.md §22): registries, environments,
//! releases, the pipeline view, promotions and rollbacks, and the setup
//! files. Workspace secrets live with the other workspace routes.
//!
//! Roles (checked by the auth middleware): reads VIEWER; creating a
//! release MEMBER; promote and rollback APPROVER; registries and
//! environment edits ADMIN.

use axum::{Json, http::StatusCode};
use sdi_proto::{execution::v1 as proto, intelligence::v1 as intelligence};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{map_all, require_id};
use crate::{
    dto::delivery::{
        CreateReleaseRequest, DeliveryArtifacts, DeliveryPipeline, DeploymentRun, Environment,
        ListDeploymentRunsQuery, ListReleasesQuery, PromoteRequest, Registry, Release,
        RollbackRequest, UpsertEnvironmentRequest, UpsertRegistryRequest,
    },
    error::{ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Scoped},
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_registries, create_registry))
        .routes(routes!(update_registry, delete_registry))
        .routes(routes!(list_environments, create_environment))
        .routes(routes!(update_environment, delete_environment))
        .routes(routes!(get_pipeline))
        .routes(routes!(list_releases, create_release))
        .routes(routes!(get_release))
        .routes(routes!(promote))
        .routes(routes!(rollback))
        .routes(routes!(list_deployment_runs))
        .routes(routes!(get_deployment_run))
        .routes(routes!(delivery_artifacts))
}

// --- registries -------------------------------------------------------------

/// The workspace's image registries, the default first. A workspace that
/// never configured one has `ghcr.io/<its GitHub login>`.
#[utoipa::path(
    get,
    path = "/registries",
    tag = "delivery",
    responses((status = 200, body = Vec<Registry>), ErrorResponses),
)]
async fn list_registries(state: Scoped) -> ApiResult<Json<Vec<Registry>>> {
    let response = state
        .execution()
        .list_registries(proto::ListRegistriesRequest {})
        .await?
        .into_inner();
    Ok(Json(map_all(response.registries)))
}

/// Adds a registry (admin). Credentials are named workspace secrets.
#[utoipa::path(
    post,
    path = "/registries",
    tag = "delivery",
    request_body = UpsertRegistryRequest,
    responses((status = 201, body = Registry), ErrorResponses),
)]
async fn create_registry(
    state: Scoped,
    ApiJson(body): ApiJson<UpsertRegistryRequest>,
) -> ApiResult<(StatusCode, Json<Registry>)> {
    let registry = state
        .execution()
        .upsert_registry(proto::UpsertRegistryRequest {
            registry: Some(body.into_proto(String::new())),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(registry.into())))
}

/// Replaces a registry's settings (admin).
#[utoipa::path(
    put,
    path = "/registries/{id}",
    tag = "delivery",
    params(("id" = String, Path, description = "Registry id")),
    request_body = UpsertRegistryRequest,
    responses((status = 200, body = Registry), ErrorResponses),
)]
async fn update_registry(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<UpsertRegistryRequest>,
) -> ApiResult<Json<Registry>> {
    let registry = state
        .execution()
        .upsert_registry(proto::UpsertRegistryRequest {
            registry: Some(body.into_proto(require_id(id)?)),
        })
        .await?
        .into_inner();
    Ok(Json(registry.into()))
}

/// Deletes a registry (admin). The default and registries in use stay.
#[utoipa::path(
    delete,
    path = "/registries/{id}",
    tag = "delivery",
    params(("id" = String, Path, description = "Registry id")),
    responses((status = 200, description = "Deleted"), ErrorResponses),
)]
async fn delete_registry(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<StatusCode> {
    state
        .execution()
        .delete_registry(proto::DeleteRegistryRequest { id })
        .await?;
    Ok(StatusCode::OK)
}

// --- environments -----------------------------------------------------------

/// A project's environments in promotion order.
#[utoipa::path(
    get,
    path = "/projects/{id}/environments",
    tag = "delivery",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = Vec<Environment>), ErrorResponses),
)]
async fn list_environments(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Vec<Environment>>> {
    let response = state
        .execution()
        .list_environments(proto::ListEnvironmentsRequest { project_id: id })
        .await?
        .into_inner();
    Ok(Json(map_all(response.environments)))
}

/// Adds an environment to a project (admin). A refused setting is named
/// in the message (`config.path is required for gitops_flux environments`).
#[utoipa::path(
    post,
    path = "/projects/{id}/environments",
    tag = "delivery",
    params(("id" = String, Path, description = "Project id")),
    request_body = UpsertEnvironmentRequest,
    responses((status = 201, body = Environment), ErrorResponses),
)]
async fn create_environment(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<UpsertEnvironmentRequest>,
) -> ApiResult<(StatusCode, Json<Environment>)> {
    let environment = state
        .execution()
        .upsert_environment(proto::UpsertEnvironmentRequest {
            environment: Some(body.into_proto(String::new(), id)),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(environment.into())))
}

/// Replaces an environment's settings (admin). Environments declared in
/// `.sdi.yml` answer 412: they are edited in the file.
#[utoipa::path(
    put,
    path = "/environments/{id}",
    tag = "delivery",
    params(("id" = String, Path, description = "Environment id")),
    request_body = UpsertEnvironmentRequest,
    responses((status = 200, body = Environment), ErrorResponses),
)]
async fn update_environment(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<UpsertEnvironmentRequest>,
) -> ApiResult<Json<Environment>> {
    let environment = state
        .execution()
        .upsert_environment(proto::UpsertEnvironmentRequest {
            environment: Some(body.into_proto(require_id(id)?, String::new())),
        })
        .await?
        .into_inner();
    Ok(Json(environment.into()))
}

/// Deletes an environment (admin) unless a deployment to it is still open.
#[utoipa::path(
    delete,
    path = "/environments/{id}",
    tag = "delivery",
    params(("id" = String, Path, description = "Environment id")),
    responses((status = 200, description = "Deleted"), ErrorResponses),
)]
async fn delete_environment(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<StatusCode> {
    state
        .execution()
        .delete_environment(proto::DeleteEnvironmentRequest { id })
        .await?;
    Ok(StatusCode::OK)
}

// --- pipeline ---------------------------------------------------------------

/// The pipeline of a project: its stages in order with the current
/// release, the last run, live health and the promotable candidate, plus
/// the 30-day success rate, deployment count and median lead time.
#[utoipa::path(
    get,
    path = "/projects/{id}/pipeline",
    tag = "delivery",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = DeliveryPipeline), ErrorResponses),
)]
async fn get_pipeline(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<DeliveryPipeline>> {
    let pipeline = state
        .execution()
        .get_pipeline(proto::GetPipelineRequest { project_id: id })
        .await?
        .into_inner();
    Ok(Json(pipeline.into()))
}

// --- releases ---------------------------------------------------------------

/// A project's releases, newest first. `before` pages backwards.
#[utoipa::path(
    get,
    path = "/projects/{id}/releases",
    tag = "delivery",
    params(("id" = String, Path, description = "Project id"), ListReleasesQuery),
    responses((status = 200, body = Vec<Release>), ErrorResponses),
)]
async fn list_releases(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ListReleasesQuery>,
) -> ApiResult<Json<Vec<Release>>> {
    let response = state
        .execution()
        .list_releases(proto::ListReleasesRequest {
            project_id: id,
            limit: query.limit.unwrap_or(0),
            before: query.before.map(sdi_common::time::to_proto),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.releases)))
}

/// Cuts a release by hand (member): by default the default branch head,
/// tagged with its short SHA. Creating the same tag twice returns the
/// release that exists.
#[utoipa::path(
    post,
    path = "/projects/{id}/releases",
    tag = "delivery",
    params(("id" = String, Path, description = "Project id")),
    request_body = CreateReleaseRequest,
    responses((status = 201, body = Release), ErrorResponses),
)]
async fn create_release(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    body: Option<ApiJson<CreateReleaseRequest>>,
) -> ApiResult<(StatusCode, Json<Release>)> {
    let body = body.map(|ApiJson(body)| body).unwrap_or_default();
    let release = state
        .execution()
        .create_release(proto::CreateReleaseRequest {
            project_id: id,
            version: body.version.unwrap_or_default(),
            commit_sha: body.commit_sha.unwrap_or_default(),
            image: body.image.unwrap_or_default(),
            tag: body.tag.unwrap_or_default(),
            digest: body.digest.unwrap_or_default(),
            source: "manual".to_owned(),
            pipeline_run_id: String::new(),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(release.into())))
}

/// One release with its notes.
#[utoipa::path(
    get,
    path = "/releases/{id}",
    tag = "delivery",
    params(("id" = String, Path, description = "Release id")),
    responses((status = 200, body = Release), ErrorResponses),
)]
async fn get_release(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<Json<Release>> {
    let release = state
        .execution()
        .get_release(proto::GetReleaseRequest { id })
        .await?
        .into_inner();
    Ok(Json(release.into()))
}

// --- deployment runs --------------------------------------------------------

/// Deploys a release to an environment (approver): creates a gated
/// execution and starts it. Without `releaseId` the stage's candidate is
/// promoted. Answers the run; `executionId` is where its steps, approvals
/// and logs are.
#[utoipa::path(
    post,
    path = "/environments/{id}/promote",
    tag = "delivery",
    params(("id" = String, Path, description = "Environment id")),
    request_body = PromoteRequest,
    responses((status = 201, body = DeploymentRun), ErrorResponses),
)]
async fn promote(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    body: Option<ApiJson<PromoteRequest>>,
) -> ApiResult<(StatusCode, Json<DeploymentRun>)> {
    let body = body.map(|ApiJson(body)| body).unwrap_or_default();
    let run = state
        .execution()
        .promote(proto::PromoteRequest {
            environment_id: id,
            release_id: body.release_id.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(run.into())))
}

/// Redeploys an earlier release of an environment (approver): the same
/// plan under the `deploy.rollback` policy. Without `releaseId` it is the
/// previous successful release.
#[utoipa::path(
    post,
    path = "/environments/{id}/rollback",
    tag = "delivery",
    params(("id" = String, Path, description = "Environment id")),
    request_body = RollbackRequest,
    responses((status = 201, body = DeploymentRun), ErrorResponses),
)]
async fn rollback(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    body: Option<ApiJson<RollbackRequest>>,
) -> ApiResult<(StatusCode, Json<DeploymentRun>)> {
    let body = body.map(|ApiJson(body)| body).unwrap_or_default();
    let run = state
        .execution()
        .rollback(proto::RollbackRequest {
            environment_id: id,
            release_id: body.release_id.unwrap_or_default(),
            reason: body.reason.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(run.into())))
}

/// A project's promotions and rollbacks, newest first, optionally of one
/// environment. `before` pages backwards.
#[utoipa::path(
    get,
    path = "/projects/{id}/deployment-runs",
    tag = "delivery",
    params(("id" = String, Path, description = "Project id"), ListDeploymentRunsQuery),
    responses((status = 200, body = Vec<DeploymentRun>), ErrorResponses),
)]
async fn list_deployment_runs(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ListDeploymentRunsQuery>,
) -> ApiResult<Json<Vec<DeploymentRun>>> {
    let response = state
        .execution()
        .list_deployment_runs(proto::ListDeploymentRunsRequest {
            project_id: id,
            environment_id: query.environment_id.unwrap_or_default(),
            limit: query.limit.unwrap_or(0),
            before: query.before.map(sdi_common::time::to_proto),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.runs)))
}

/// One deployment run.
#[utoipa::path(
    get,
    path = "/deployment-runs/{id}",
    tag = "delivery",
    params(("id" = String, Path, description = "Deployment run id")),
    responses((status = 200, body = DeploymentRun), ErrorResponses),
)]
async fn get_deployment_run(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<DeploymentRun>> {
    let run = state
        .execution()
        .get_deployment_run(proto::GetDeploymentRunRequest { id })
        .await?
        .into_inner();
    Ok(Json(run.into()))
}

// --- setup files ------------------------------------------------------------

/// The files that wire the project's CI to its registry and its
/// environments' deployers (build and deploy workflows, Flux or Argo CD
/// manifests, `Jenkinsfile`, `azure-pipelines.yml`), generated from facts
/// for the deployer kinds the project uses. Answers 501 while the
/// intelligence service does not generate them.
#[utoipa::path(
    get,
    path = "/projects/{id}/delivery/artifacts",
    tag = "delivery",
    params(("id" = String, Path, description = "Project id")),
    responses(
        (status = 200, body = DeliveryArtifacts),
        (status = 501, description = "Not generated by this deployment yet", body = crate::dto::common::ErrorBody),
        ErrorResponses,
    ),
)]
async fn delivery_artifacts(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<DeliveryArtifacts>> {
    let artifacts = state
        .intelligence()
        .generate_delivery_artifacts(intelligence::GenerateDeliveryArtifactsRequest {
            project_id: id,
        })
        .await
        .map_err(|status| match status.code() {
            // Said as it is, instead of the opaque 500 other unimplemented
            // RPCs get: the page can explain that the files are not there yet.
            tonic::Code::Unimplemented => ApiError::new(
                StatusCode::NOT_IMPLEMENTED,
                "unimplemented",
                "the intelligence service does not generate delivery setup files yet",
            ),
            _ => ApiError::from(status),
        })?
        .into_inner();
    Ok(Json(artifacts.into()))
}
