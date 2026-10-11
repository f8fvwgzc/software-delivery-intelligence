//! Code browser (CONTRACT.md §15): the repository tree and single files at
//! the scanned commit, both served by the knowledge service.

use axum::{Json, extract::State};
use sdi_proto::knowledge::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::code::{GetRepoFileQuery, RepoFile, RepoTree},
    error::{ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_repo_tree))
        .routes(routes!(get_repo_file))
}

/// Every file and directory of the repository at the scanned commit.
///
/// 412 when the project has not been scanned yet.
#[utoipa::path(
    get,
    path = "/projects/{id}/tree",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = RepoTree), ErrorResponses),
)]
async fn get_repo_tree(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<RepoTree>> {
    let tree = state
        .knowledge()
        .get_repo_tree(proto::GetRepoTreeRequest { project_id: id })
        .await?
        .into_inner();
    Ok(Json(tree.into()))
}

/// One file's text at the scanned commit, with the facts detected from it.
///
/// 400 for a path that is not repository-relative, 403 for a secret-looking
/// file (`.env*`, `*.pem`, `*.key`, `id_rsa*`, …), 404 when the path is not
/// a file at that commit, 412 when the project has not been scanned yet.
#[utoipa::path(
    get,
    path = "/projects/{id}/file",
    tag = "projects",
    params(("id" = String, Path, description = "Project id"), GetRepoFileQuery),
    responses(
        (status = 200, body = RepoFile),
        (status = 403, description = "The file looks like a secret and is never displayed", body = crate::dto::common::ErrorBody),
        ErrorResponses,
    ),
)]
async fn get_repo_file(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<GetRepoFileQuery>,
) -> ApiResult<Json<RepoFile>> {
    let file = state
        .knowledge()
        .get_repo_file(proto::GetRepoFileRequest {
            project_id: id,
            path: query.path,
        })
        .await?
        .into_inner();
    Ok(Json(file.into()))
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::{app, fakes, state::AppState};

    /// A port nothing listens on, for the services a test does not need.
    const DEAD: &str = "http://127.0.0.1:1";

    async fn app_with_fake_knowledge() -> Router {
        let addr = fakes::knowledge::serve().await;
        app::router(AppState::with_urls(&format!("http://{addr}"), DEAD, DEAD).unwrap())
    }

    async fn get(uri: &str) -> (StatusCode, Value) {
        let response = app_with_fake_knowledge()
            .await
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    #[tokio::test]
    async fn tree_route_reads_the_knowledge_service() {
        let (status, json) = get("/api/v1/projects/p1/tree").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["projectId"], "p1");
        assert_eq!(json["commitSha"], "abc");
        assert_eq!(json["truncated"], false);
        assert_eq!(json["entries"][0]["path"], "src");
        assert_eq!(json["entries"][0]["isDir"], true);
        assert_eq!(json["entries"][1]["path"], "src/main.rs");
        assert_eq!(json["entries"][1]["name"], "main.rs");
        assert_eq!(json["entries"][1]["language"], "rust");
        assert_eq!(json["entries"][1]["size"], 120);

        let (status, json) = get("/api/v1/projects/missing/tree").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["code"], "not_found");

        let (status, json) = get("/api/v1/projects/unscanned/tree").await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(json["code"], "failed_precondition");
    }

    #[tokio::test]
    async fn file_route_maps_every_outcome() {
        let (status, json) = get("/api/v1/projects/p1/file?path=src/main.rs").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["path"], "src/main.rs");
        assert_eq!(json["commitSha"], "abc");
        assert_eq!(json["content"], "fn main() {}\n");
        assert_eq!(json["binary"], false);
        assert_eq!(json["truncated"], false);
        assert_eq!(json["size"], 13);
        assert_eq!(json["language"], "rust");
        assert_eq!(
            json["htmlUrl"],
            "https://github.com/o/r/blob/abc/src/main.rs"
        );
        assert_eq!(json["factIds"][0], "f1");

        // Secret-looking paths are PERMISSION_DENIED upstream → 403.
        let (status, json) = get("/api/v1/projects/p1/file?path=.env").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(json["code"], "permission_denied");

        let (status, json) = get("/api/v1/projects/p1/file?path=nope.rs").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["code"], "not_found");

        let (status, json) = get("/api/v1/projects/p1/file?path=../x").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["code"], "invalid_argument");

        // The query parameter is required.
        let (status, json) = get("/api/v1/projects/p1/file").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["code"], "invalid_query");
    }

    #[tokio::test]
    async fn openapi_documents_both_routes() {
        let json = serde_json::to_value(app::openapi()).unwrap();
        let paths = json["paths"].as_object().expect("paths object");
        assert!(paths.contains_key("/api/v1/projects/{id}/tree"));
        assert!(paths.contains_key("/api/v1/projects/{id}/file"));
        assert!(json["components"]["schemas"]["RepoTree"].is_object());
        assert!(json["components"]["schemas"]["RepoFile"].is_object());
        let params = &json["paths"]["/api/v1/projects/{id}/file"]["get"]["parameters"];
        assert!(
            params
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "path" && p["in"] == "query" && p["required"] == true),
            "{params:#}"
        );
    }
}
