use axum::{Json, Router};
use serde::Serialize;
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa_scalar::{Scalar, Servable};

#[derive(OpenApi)]
#[openapi(info(title = "Software Delivery Intelligence API", version = "0.1.0"))]
struct ApiDoc;

#[derive(Serialize, ToSchema)]
struct Health {
    status: String,
}

#[utoipa::path(get, path = "/api/v1/health", tag = "system", responses((status = 200, body = Health)))]
async fn health() -> Json<Health> {
    Json(Health { status: "ok".into() })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    sdi_common::telemetry::init("gateway");
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health))
        .split_for_parts();
    let spec = api.clone();
    let app: Router = router
        .route("/api/openapi.json", axum::routing::get(move || async move { Json(spec) }))
        .merge(Scalar::with_url("/api/docs", api));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;
    axum::serve(listener, app).with_graceful_shutdown(sdi_common::shutdown::signal()).await?;
    Ok(())
}
