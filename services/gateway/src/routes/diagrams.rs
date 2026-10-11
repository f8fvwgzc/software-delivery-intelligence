//! Diagrams (CONTRACT.md §16): generation, listing, editing and deletion.

use axum::{Json, http::StatusCode};
use sdi_proto::intelligence::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::diagrams::{Diagram, GenerateDiagramRequest, UpdateDiagramRequest},
    error::{ApiError, ApiJson, ApiPath, ApiResult, ErrorResponses},
    state::{AppState, Scoped},
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_diagrams, generate_diagram))
        .routes(routes!(get_diagram, update_diagram, delete_diagram))
}

/// Diagrams of a project, without their spec and Mermaid.
#[utoipa::path(
    get,
    path = "/projects/{id}/diagrams",
    tag = "diagrams",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = Vec<Diagram>), ErrorResponses),
)]
async fn list_diagrams(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Vec<Diagram>>> {
    let response = state
        .intelligence()
        .list_diagrams(proto::ListDiagramsRequest { project_id: id })
        .await?
        .into_inner();
    Ok(Json(map_all(response.diagrams)))
}

/// Builds a diagram from the knowledge graph, repository tree and key files.
/// For `custom`, the prompt is free text or Mermaid source to import.
#[utoipa::path(
    post,
    path = "/projects/{id}/diagrams",
    tag = "diagrams",
    params(("id" = String, Path, description = "Project id")),
    request_body = GenerateDiagramRequest,
    responses((status = 201, body = Diagram), ErrorResponses),
)]
async fn generate_diagram(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<GenerateDiagramRequest>,
) -> ApiResult<(StatusCode, Json<Diagram>)> {
    let diagram = state
        .intelligence()
        .generate_diagram(proto::GenerateDiagramRequest {
            project_id: id,
            kind: body.kind.into(),
            prompt: body.prompt.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(diagram.into())))
}

/// One diagram with its spec and Mermaid.
#[utoipa::path(
    get,
    path = "/diagrams/{id}",
    tag = "diagrams",
    params(("id" = String, Path, description = "Diagram id")),
    responses((status = 200, body = Diagram), ErrorResponses),
)]
async fn get_diagram(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<Json<Diagram>> {
    let diagram = state
        .intelligence()
        .get_diagram(proto::GetDiagramRequest { id })
        .await?
        .into_inner();
    Ok(Json(diagram.into()))
}

/// Edits the title, the spec (Mermaid is regenerated, `source` = `edited`) or
/// the Mermaid source (`source` = `mermaid`, the spec is kept). Bumps the revision.
#[utoipa::path(
    patch,
    path = "/diagrams/{id}",
    tag = "diagrams",
    params(("id" = String, Path, description = "Diagram id")),
    request_body = UpdateDiagramRequest,
    responses((status = 200, body = Diagram), ErrorResponses),
)]
async fn update_diagram(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<UpdateDiagramRequest>,
) -> ApiResult<Json<Diagram>> {
    if body.title.as_deref().is_none_or(|t| t.trim().is_empty())
        && body.spec.is_none()
        && body.mermaid.as_deref().is_none_or(|m| m.trim().is_empty())
    {
        return Err(ApiError::bad_request(
            "nothing to update: set title, spec or mermaid",
        ));
    }
    let diagram = state
        .intelligence()
        .update_diagram(proto::UpdateDiagramRequest {
            id,
            title: body.title.unwrap_or_default(),
            spec: body.spec.map(Into::into),
            mermaid: body.mermaid.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(diagram.into()))
}

/// Deletes a diagram.
#[utoipa::path(
    delete,
    path = "/diagrams/{id}",
    tag = "diagrams",
    params(("id" = String, Path, description = "Diagram id")),
    responses((status = 200, description = "Deleted"), ErrorResponses),
)]
async fn delete_diagram(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<StatusCode> {
    state
        .intelligence()
        .delete_diagram(proto::DeleteDiagramRequest { id })
        .await?;
    Ok(StatusCode::OK)
}
