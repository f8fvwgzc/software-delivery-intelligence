//! Assembles the router and the OpenAPI document from the same route
//! registrations, then wraps it in the middleware stack.

use std::{any::Any, time::Duration};

use axum::{
    Json, Router,
    body::Body,
    extract::Request,
    http::{HeaderName, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use tower::ServiceBuilder;
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    cors::CorsLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable};

use crate::{error::ApiError, routes, sse, state::AppState};

/// Upper bound for one request/response round trip. The SSE route is
/// registered outside it because a watch is meant to stay open.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Software Delivery Intelligence API",
        description = "Public REST API of the SDI platform. JSON with camelCase fields, lowercase enums, RFC 3339 timestamps and `{code, message}` error bodies (CONTRACT.md §8).",
        license(name = "MIT"),
    ),
    servers((url = "/", description = "Same origin")),
    // Referenced by every error response but never a handler body, so the
    // automatic collection misses it.
    components(schemas(crate::dto::common::ErrorBody)),
    tags(
        (name = "system", description = "Gateway liveness and service health"),
        (name = "github", description = "Read-only GitHub connector"),
        (name = "projects", description = "Connected repositories and what is known about them"),
        (name = "scans", description = "Repository analysis runs"),
        (name = "delivery", description = "Observed pipelines and deployments"),
        (name = "knowledge", description = "Pattern memory and semantic search"),
        (name = "proposals", description = "Ranked proposals and negotiation"),
        (name = "decisions", description = "Decision memory with provenance"),
        (name = "documents", description = "Generated documents and reports"),
        (name = "executions", description = "Policy-gated plan execution"),
        (name = "approvals", description = "Human approvals"),
        (name = "policies", description = "Action policies and scoped identities"),
    ),
)]
struct ApiDoc;

/// The `/api/v1` router (stateless) and the document describing it.
fn parts() -> (Router<AppState>, utoipa::openapi::OpenApi) {
    let v1 = routes::rest()
        .route_layer(middleware::from_fn(request_timeout))
        .merge(sse::router());
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/api/v1", v1)
        .split_for_parts()
}

/// The OpenAPI document, without connecting anything (`--print-openapi`).
pub fn openapi() -> utoipa::openapi::OpenApi {
    parts().1
}

/// The complete application: API routes, the OpenAPI document, Scalar docs,
/// JSON fallbacks and the middleware stack.
pub fn router(state: AppState) -> Router {
    let (api, document) = parts();
    let spec = document.clone();

    let middleware = ServiceBuilder::new()
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request<Body>| {
                let request_id = request
                    .headers()
                    .get(HeaderName::from_static("x-request-id"))
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("-");
                tracing::info_span!(
                    "request",
                    method = %request.method(),
                    path = request.uri().path(),
                    request_id,
                )
            }),
        )
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(CatchPanicLayer::custom(panic_response))
        .layer(CorsLayer::permissive())
        .layer(CompressionLayer::new());

    api.route("/api/openapi.json", get(move || async move { Json(spec) }))
        .merge(Scalar::with_url("/api/docs", document))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state)
        .layer(middleware)
}

/// `tokio::time::timeout` instead of `tower_http::timeout` so the 504 carries
/// the JSON error body like every other failure.
async fn request_timeout(request: Request, next: Next) -> Response {
    match tokio::time::timeout(REQUEST_TIMEOUT, next.run(request)).await {
        Ok(response) => response,
        Err(_) => ApiError::timeout(format!(
            "the request did not complete within {}s",
            REQUEST_TIMEOUT.as_secs()
        ))
        .into_response(),
    }
}

async fn not_found() -> Response {
    ApiError::not_found("no such route").into_response()
}

async fn method_not_allowed() -> Response {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "this route does not support that method",
    )
    .into_response()
}

fn panic_response(panic: Box<dyn Any + Send + 'static>) -> Response {
    let detail = panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "unknown panic".to_owned());
    tracing::error!(panic = %detail, "handler panicked");
    let mut response = ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        "internal error",
    )
    .into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    response
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;

    /// Ports nothing listens on: every gRPC call fails fast with UNAVAILABLE.
    fn app() -> Router {
        let state = AppState::with_urls(
            "http://127.0.0.1:1",
            "http://127.0.0.1:1",
            "http://127.0.0.1:1",
        )
        .unwrap();
        router(state)
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, Value, axum::http::HeaderMap) {
        let response = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json, headers)
    }

    #[tokio::test]
    async fn health_answers_ok_with_a_request_id() {
        let (status, json, headers) = get_json(app(), "/api/v1/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        assert!(headers.contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn request_id_is_propagated_when_supplied() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .header("x-request-id", "abc-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()["x-request-id"], "abc-123");
    }

    #[tokio::test]
    async fn openapi_document_lists_every_contract_route() {
        let (status, json, _) = get_json(app(), "/api/openapi.json").await;
        assert_eq!(status, StatusCode::OK);
        let paths = json["paths"].as_object().expect("paths object");
        // CONTRACT.md §8 enumerates 48 distinct paths under /api/v1.
        assert_eq!(paths.len(), 48, "{:#?}", paths.keys().collect::<Vec<_>>());
        for path in [
            "/api/v1/health",
            "/api/v1/system/status",
            "/api/v1/projects",
            "/api/v1/projects/{id}/mode",
            "/api/v1/executions/{id}/events/stream",
            "/api/v1/policies/{actionType}",
        ] {
            assert!(paths.contains_key(path), "missing {path}");
        }
        assert_eq!(
            json["paths"]["/api/v1/projects"]["post"]["responses"]["201"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/Project"
        );
        assert!(json["components"]["schemas"]["ErrorBody"].is_object());
    }

    #[tokio::test]
    async fn unreachable_service_yields_502_json() {
        let (status, json, _) = get_json(app(), "/api/v1/projects").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(json["code"], "unavailable");
        assert!(json["message"].as_str().unwrap().contains("unavailable"));
    }

    #[tokio::test]
    async fn system_status_reports_every_service_unreachable() {
        let (status, json, _) = get_json(app(), "/api/v1/system/status").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "degraded");
        let services = json["services"].as_array().unwrap();
        assert_eq!(services.len(), 3);
        assert!(services.iter().all(|s| s["status"] == "unreachable"));
        assert_eq!(services[0]["name"], "knowledge");
    }

    #[tokio::test]
    async fn malformed_json_body_is_a_400_with_the_error_body() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from("{\"repoFullName\": 5"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["code"], "invalid_body");
    }

    #[tokio::test]
    async fn bad_enum_in_query_is_a_400() {
        let (status, json, _) = get_json(app(), "/api/v1/approvals?status=maybe").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["code"], "invalid_query");
    }

    #[tokio::test]
    async fn unknown_routes_answer_json_404() {
        let (status, json, _) = get_json(app(), "/api/v1/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["code"], "not_found");
    }

    #[tokio::test]
    async fn docs_page_is_served() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/api/docs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}

/// A real tonic server in-process so the SSE bridge, the gRPC status mapping
/// and the health check are exercised against an actual stream, not a
/// closed port.
#[cfg(test)]
mod fake_execution {
    use std::{net::SocketAddr, pin::Pin};

    use futures::{Stream, StreamExt, stream};
    use sdi_proto::execution::v1::{
        self as proto,
        execution_service_server::{ExecutionService, ExecutionServiceServer},
    };
    use tonic::{Request, Response, Status};
    use tonic_health::ServingStatus;

    pub struct Fake;

    type EventStream = Pin<Box<dyn Stream<Item = Result<proto::ExecutionEvent, Status>> + Send>>;

    fn watch(request: proto::WatchExecutionRequest) -> Result<EventStream, Status> {
        if request.execution_id == "missing" {
            return Err(Status::not_found("no such execution"));
        }
        let events = (1..=2)
            .filter(move |seq| *seq > request.after_seq)
            .map(|seq| {
                Ok(proto::ExecutionEvent {
                    seq,
                    execution_id: "e1".into(),
                    r#type: proto::EventType::StateChanged as i32,
                    step_key: String::new(),
                    message: format!("event {seq}"),
                    status: proto::ExecutionStatus::Executing as i32,
                    data: Default::default(),
                    at: Some(prost_types::Timestamp {
                        seconds: 1_700_000_000,
                        nanos: 0,
                    }),
                })
            })
            .collect::<Vec<_>>();
        Ok(stream::iter(events).boxed())
    }

    /// `#[async_trait]` cannot see into a nested macro, so the macro emits
    /// the whole impl: one real streaming method, the rest unimplemented.
    macro_rules! fake_service {
        ($($name:ident: $req:ty => $resp:ty),* $(,)?) => {
            #[tonic::async_trait]
            impl ExecutionService for Fake {
                type WatchExecutionStream = EventStream;

                async fn watch_execution(
                    &self,
                    request: Request<proto::WatchExecutionRequest>,
                ) -> Result<Response<Self::WatchExecutionStream>, Status> {
                    watch(request.into_inner()).map(Response::new)
                }

                $(async fn $name(&self, _: Request<$req>) -> Result<Response<$resp>, Status> {
                    Err(Status::unimplemented(stringify!($name)))
                })*
            }
        };
    }

    fake_service! {
        create_execution: proto::CreateExecutionRequest => proto::Execution,
        list_executions: proto::ListExecutionsRequest => proto::ListExecutionsResponse,
        get_execution: proto::GetExecutionRequest => proto::Execution,
        start_execution: proto::StartExecutionRequest => proto::Execution,
        resolve_failure: proto::ResolveFailureRequest => proto::Execution,
        cancel_execution: proto::CancelExecutionRequest => proto::Execution,
        list_events: proto::ListEventsRequest => proto::ListEventsResponse,
        list_approvals: proto::ListApprovalsRequest => proto::ListApprovalsResponse,
        decide_approval: proto::DecideApprovalRequest => proto::Approval,
        list_policies: proto::ListPoliciesRequest => proto::ListPoliciesResponse,
        update_policy: proto::UpdatePolicyRequest => proto::Policy,
        list_identities: proto::ListIdentitiesRequest => proto::ListIdentitiesResponse,
        grant_permission: proto::GrantPermissionRequest => proto::Identity,
        get_delivery_stats: proto::GetDeliveryStatsRequest => proto::DeliveryStats,
    }

    /// Serves the fake plus `grpc.health.v1.Health` on a random port.
    pub async fn serve() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (reporter, health) = tonic_health::server::health_reporter();
        reporter
            .set_service_status("", ServingStatus::Serving)
            .await;
        let incoming = stream::unfold(listener, |listener| async move {
            let (socket, _) = listener.accept().await.ok()?;
            Some((Ok::<_, std::io::Error>(socket), listener))
        });
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(health)
                .add_service(ExecutionServiceServer::new(Fake))
                .serve_with_incoming(incoming)
                .await
                .unwrap();
        });
        addr
    }
}

#[cfg(test)]
mod integration_tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;

    async fn app_with_fake_execution() -> Router {
        let addr = fake_execution::serve().await;
        let url = format!("http://{addr}");
        router(AppState::with_urls("http://127.0.0.1:1", "http://127.0.0.1:1", &url).unwrap())
    }

    #[tokio::test]
    async fn sse_streams_every_event_then_ends() {
        let app = app_with_fake_execution().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/executions/e1/events/stream?afterSeq=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/event-stream")
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();

        let data_lines: Vec<&str> = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .filter(|line| !line.trim().is_empty())
            .collect();
        assert_eq!(data_lines.len(), 1, "{text}");
        let event: Value = serde_json::from_str(data_lines[0].trim()).unwrap();
        assert_eq!(event["seq"], 2);
        assert_eq!(event["type"], "state_changed");
        assert_eq!(event["status"], "executing");
        assert_eq!(event["at"], "2023-11-14T22:13:20Z");
        assert!(text.contains("id: 2\n"), "{text}");
        assert!(text.trim_end().ends_with("event: end"), "{text}");
    }

    #[tokio::test]
    async fn sse_maps_not_found_before_streaming() {
        let app = app_with_fake_execution().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/executions/missing/events/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "not_found");
        assert_eq!(json["message"], "no such execution");
    }

    #[tokio::test]
    async fn unimplemented_rpc_is_a_500_without_detail() {
        let app = app_with_fake_execution().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/policies")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "internal");
        assert_eq!(json["message"], "internal error");
    }

    #[tokio::test]
    async fn system_status_sees_the_serving_service() {
        let app = app_with_fake_execution().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/system/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "degraded");
        let services = json["services"].as_array().unwrap();
        assert_eq!(services[0]["status"], "unreachable");
        assert_eq!(services[2]["name"], "execution");
        assert_eq!(services[2]["status"], "serving");
        assert_eq!(services[2]["detail"], Value::Null);
    }
}
