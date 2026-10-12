//! Generated documents and intelligence counters.

use axum::{Json, http::StatusCode};
use sdi_proto::intelligence::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::common::PageQuery,
    dto::intelligence::{Document, GenerateDocumentRequest, IntelligenceStats},
    error::{ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Scoped},
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_documents, generate_document))
        .routes(routes!(get_document))
        .routes(routes!(intelligence_stats))
}

/// Documents of a project, newest first, without their markdown bodies.
#[utoipa::path(
    get,
    path = "/projects/{id}/documents",
    tag = "documents",
    params(("id" = String, Path, description = "Project id"), PageQuery),
    responses((status = 200, body = Vec<Document>), ErrorResponses),
)]
async fn list_documents(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(page): ApiQuery<PageQuery>,
) -> ApiResult<Json<Vec<Document>>> {
    let response = state
        .intelligence()
        .list_documents(proto::ListDocumentsRequest {
            project_id: id,
            limit: page.limit.unwrap_or(0),
            before: page.before.map(sdi_common::time::to_proto),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.documents)))
}

/// Generates a document from the knowledge model.
#[utoipa::path(
    post,
    path = "/projects/{id}/documents",
    tag = "documents",
    params(("id" = String, Path, description = "Project id")),
    request_body = GenerateDocumentRequest,
    responses((status = 201, body = Document), ErrorResponses),
)]
async fn generate_document(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<GenerateDocumentRequest>,
) -> ApiResult<(StatusCode, Json<Document>)> {
    let document = state
        .intelligence()
        .generate_document(proto::GenerateDocumentRequest {
            project_id: id,
            kind: body.kind.into(),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(document.into())))
}

/// One document with its markdown body.
#[utoipa::path(
    get,
    path = "/documents/{id}",
    tag = "documents",
    params(("id" = String, Path, description = "Document id")),
    responses((status = 200, body = Document), ErrorResponses),
)]
async fn get_document(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<Json<Document>> {
    let document = state
        .intelligence()
        .get_document(proto::GetDocumentRequest { id })
        .await?
        .into_inner();
    Ok(Json(document.into()))
}

/// Counters of the intelligence store.
#[utoipa::path(
    get,
    path = "/intelligence/stats",
    tag = "proposals",
    responses((status = 200, body = IntelligenceStats), ErrorResponses),
)]
async fn intelligence_stats(state: Scoped) -> ApiResult<Json<IntelligenceStats>> {
    let stats = state
        .intelligence()
        .get_intelligence_stats(proto::GetIntelligenceStatsRequest {})
        .await?
        .into_inner();
    Ok(Json(stats.into()))
}
