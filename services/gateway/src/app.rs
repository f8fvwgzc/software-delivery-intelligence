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
        // CONTRACT.md §8 enumerates 55 distinct paths under /api/v1.
        assert_eq!(paths.len(), 55, "{:#?}", paths.keys().collect::<Vec<_>>());
        for path in [
            "/api/v1/health",
            "/api/v1/system/status",
            "/api/v1/projects",
            "/api/v1/projects/{id}/mode",
            "/api/v1/executions/{id}/events/stream",
            "/api/v1/policies/{actionType}",
            "/api/v1/scans/{id}/stream",
            "/api/v1/projects/{id}/maturity",
            "/api/v1/projects/{id}/config",
            "/api/v1/events/stream",
            "/api/v1/settings",
            "/api/v1/proposals/{id}/options/{optionId}/artifacts",
            "/api/v1/proposals/{id}/pull-request",
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

/// Against the in-process fakes (`crate::fakes`): the SSE bridges, the
/// status mapping, the settings composition and the proposal shortcuts are
/// exercised over real gRPC streams and calls.
#[cfg(test)]
mod integration_tests {
    use std::time::Duration;

    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use futures::StreamExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;
    use crate::fakes;

    /// Nothing listens here: every call fails fast with UNAVAILABLE.
    const DEAD: &str = "http://127.0.0.1:1";

    fn app_with(knowledge: &str, intelligence: &str, execution: &str) -> Router {
        router(AppState::with_urls(knowledge, intelligence, execution).unwrap())
    }

    async fn app_with_fake_execution() -> Router {
        let addr = fakes::execution::serve().await;
        app_with(DEAD, DEAD, &format!("http://{addr}"))
    }

    async fn app_with_fake_knowledge() -> Router {
        let addr = fakes::knowledge::serve().await;
        app_with(&format!("http://{addr}"), DEAD, DEAD)
    }

    async fn app_with_fake_intelligence() -> Router {
        let addr = fakes::intelligence::serve().await;
        app_with(DEAD, &format!("http://{addr}"), DEAD)
    }

    async fn app_with_every_fake() -> Router {
        let (knowledge, intelligence, execution) = tokio::join!(
            fakes::knowledge::serve(),
            fakes::intelligence::serve(),
            fakes::execution::serve()
        );
        app_with(
            &format!("http://{knowledge}"),
            &format!("http://{intelligence}"),
            &format!("http://{execution}"),
        )
    }

    async fn send(
        app: Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(body) => {
                request = request.header("content-type", "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = app.oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    /// The `data:` payloads of an SSE body, in order.
    fn data_lines(text: &str) -> Vec<Value> {
        text.lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line.trim()).unwrap())
            .collect()
    }

    /// Reads an SSE body that never ends until `done` is satisfied by what
    /// has arrived so far, or fails after `limit`.
    async fn read_until(
        body: Body,
        limit: Duration,
        done: impl Fn(&[Value]) -> bool,
    ) -> Vec<Value> {
        let mut stream = body.into_data_stream();
        let mut text = String::new();
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            let seen = data_lines(&text);
            if done(&seen) {
                return seen;
            }
            let chunk = tokio::time::timeout_at(deadline, stream.next())
                .await
                .unwrap_or_else(|_| panic!("timed out; received so far:\n{text}"))
                .expect("stream ended")
                .unwrap();
            text.push_str(std::str::from_utf8(&chunk).unwrap());
        }
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

        let events = data_lines(&text);
        assert_eq!(events.len(), 1, "{text}");
        assert_eq!(events[0]["seq"], 2);
        assert_eq!(events[0]["type"], "state_changed");
        assert_eq!(events[0]["status"], "executing");
        assert_eq!(events[0]["at"], "2023-11-14T22:13:20Z");
        assert!(text.contains("id: 2\n"), "{text}");
        assert!(text.trim_end().ends_with("event: end"), "{text}");
    }

    #[tokio::test]
    async fn sse_maps_not_found_before_streaming() {
        let (status, json) = send(
            app_with_fake_execution().await,
            "GET",
            "/api/v1/executions/missing/events/stream",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["code"], "not_found");
        assert_eq!(json["message"], "no such execution");
    }

    #[tokio::test]
    async fn scan_stream_has_the_execution_stream_shape() {
        let app = app_with_fake_knowledge().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/scans/s1/stream")
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
        let scans = data_lines(&text);
        assert_eq!(scans.len(), 2, "{text}");
        assert_eq!(scans[0]["status"], "running");
        assert_eq!(scans[1]["status"], "succeeded");
        assert_eq!(scans[1]["projectId"], "p1");
        assert!(text.trim_end().ends_with("event: end"), "{text}");

        let (status, json) = send(
            app_with_fake_knowledge().await,
            "GET",
            "/api/v1/scans/missing/stream",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["message"], "no such scan");
    }

    #[tokio::test]
    async fn events_stream_merges_the_services_and_survives_a_drop() {
        // knowledge: two events then its stream ends (a drop);
        // intelligence: one event, stays open; execution: one event after
        // 300ms, stays open. The late execution event proves the merged
        // feed outlived the knowledge drop.
        let app = app_with_every_fake().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/stream")
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
        let has = |seen: &[Value], entity: &str| seen.iter().any(|e| e["entity"] == entity);
        let events = read_until(response.into_body(), Duration::from_secs(10), |seen| {
            has(seen, "scan") && has(seen, "proposal") && has(seen, "execution")
        })
        .await;
        assert!(has(&events, "project"), "{events:?}");
        let execution = events.iter().find(|e| e["entity"] == "execution").unwrap();
        assert_eq!(execution["entityId"], "e1");
        assert_eq!(execution["projectId"], "p1");
        assert_eq!(execution["at"], "2023-11-14T22:13:20Z");
    }

    #[tokio::test]
    async fn events_stream_keeps_flowing_while_a_service_is_down() {
        // intelligence is a dead port: its watch fails on connect and is
        // retried with backoff in the background; the others still deliver.
        let (knowledge, execution) =
            tokio::join!(fakes::knowledge::serve(), fakes::execution::serve());
        let app = app_with(
            &format!("http://{knowledge}"),
            DEAD,
            &format!("http://{execution}"),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let events = read_until(response.into_body(), Duration::from_secs(10), |seen| {
            seen.iter().any(|e| e["entity"] == "execution")
        })
        .await;
        assert!(events.iter().any(|e| e["entity"] == "scan"), "{events:?}");
    }

    #[tokio::test]
    async fn events_stream_reconnects_a_dropped_upstream() {
        // The knowledge fake ends its stream after two events; after the
        // 1s backoff the gateway reopens it and the same two arrive again.
        let app = app_with_fake_knowledge().await;
        let started = tokio::time::Instant::now();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let events = read_until(response.into_body(), Duration::from_secs(10), |seen| {
            seen.iter().filter(|e| e["entity"] == "project").count() >= 2
        })
        .await;
        assert_eq!(
            events
                .iter()
                .map(|e| e["entity"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["scan", "project", "scan", "project"]
        );
        assert!(
            started.elapsed() >= Duration::from_secs(1),
            "reconnect waited"
        );
    }

    #[tokio::test]
    async fn maturity_and_config_routes_read_the_knowledge_service() {
        let (status, json) = send(
            app_with_fake_knowledge().await,
            "GET",
            "/api/v1/projects/p1/maturity",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["overall"], 42);
        assert_eq!(json["stage"], "transitional");
        assert_eq!(json["axes"][0]["key"], "ci");
        assert_eq!(json["axes"][0]["recommendedPatterns"][0], "add-ci-baseline");
        assert_eq!(json["computedAt"], "2023-11-14T22:13:20Z");

        let (status, json) = send(
            app_with_fake_knowledge().await,
            "GET",
            "/api/v1/projects/missing/maturity",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["code"], "not_found");

        let (status, json) = send(
            app_with_fake_knowledge().await,
            "GET",
            "/api/v1/projects/configured/config",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["present"], true);
        assert_eq!(json["path"], ".sdi.yml");
        assert_eq!(json["deployTarget"], "azure");
        assert_eq!(json["budgetMonthlyUsd"], 20.0);
        assert_eq!(json["port"], 8080);
        assert_eq!(json["region"], Value::Null);

        let (status, json) = send(
            app_with_fake_knowledge().await,
            "GET",
            "/api/v1/projects/plain/config",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["present"], false);
        assert_eq!(json["path"], Value::Null);
    }

    #[tokio::test]
    async fn settings_are_composed_from_both_services() {
        let (status, json) =
            send(app_with_every_fake().await, "GET", "/api/v1/settings", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["knowledge"]["scanMaxFiles"], 2000);
        assert_eq!(json["knowledge"]["cacheGithub"], true);
        assert_eq!(json["intelligence"]["reasoner"], "heuristic");
        assert_eq!(json["intelligence"]["defaultWeights"]["policy"], 0.2);
        assert_eq!(json["intelligence"]["apiKeyConfigured"], false);

        // One service down: the composition cannot be answered honestly.
        let (status, json) = send(
            app_with_fake_knowledge().await,
            "GET",
            "/api/v1/settings",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(json["code"], "unavailable");
    }

    #[tokio::test]
    async fn settings_patch_accepts_either_part_and_merges_fields() {
        let (status, json) = send(
            app_with_every_fake().await,
            "PATCH",
            "/api/v1/settings",
            Some(json!({"knowledge": {"scanMaxFiles": 10}})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["knowledge"]["scanMaxFiles"], 10);
        assert_eq!(json["knowledge"]["scanConcurrency"], 8, "unset field kept");
        assert_eq!(
            json["intelligence"]["reasoner"], "heuristic",
            "untouched part read back"
        );

        let (status, json) = send(
            app_with_every_fake().await,
            "PATCH",
            "/api/v1/settings",
            Some(json!({"intelligence": {"reasoner": "claude", "costCeilingUsd": 50}})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["intelligence"]["reasoner"], "claude");
        assert_eq!(json["intelligence"]["costCeilingUsd"], 50.0);
        assert_eq!(json["intelligence"]["model"], "claude-sonnet-5-5");
        assert_eq!(json["intelligence"]["defaultWeights"]["cost"], 0.1);
        assert_eq!(json["knowledge"]["scanMaxFiles"], 2000);

        let (status, json) = send(
            app_with_every_fake().await,
            "PATCH",
            "/api/v1/settings",
            Some(json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["knowledge"]["scanMaxFiles"], 2000);
    }

    #[tokio::test]
    async fn proposal_hides_artifacts_and_the_artifacts_route_returns_them() {
        let (status, json) = send(
            app_with_fake_intelligence().await,
            "GET",
            "/api/v1/proposals/pr1",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["options"][0]["artifactCount"], 2);
        assert_eq!(json["options"][1]["artifactCount"], 0);
        assert!(json["options"][0].get("artifacts").is_none(), "{json}");

        let (status, json) = send(
            app_with_fake_intelligence().await,
            "GET",
            "/api/v1/proposals/pr1/options/opt-a/artifacts",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let artifacts = json.as_array().unwrap();
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0]["path"], "Dockerfile");
        assert_eq!(artifacts[0]["content"], "FROM scratch\n");
        assert_eq!(artifacts[0]["language"], "dockerfile");
        assert_eq!(artifacts[1]["replacesExisting"], true);

        let (status, json) = send(
            app_with_fake_intelligence().await,
            "GET",
            "/api/v1/proposals/pr1/options/opt-b/artifacts",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json, json!([]));

        let (status, json) = send(
            app_with_fake_intelligence().await,
            "GET",
            "/api/v1/proposals/pr1/options/nope/artifacts",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["code"], "not_found");

        let (status, _) = send(
            app_with_fake_intelligence().await,
            "GET",
            "/api/v1/proposals/missing/options/opt-a/artifacts",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn pull_request_route_accepts_on_the_github_adapter() {
        let (status, json) = send(
            app_with_fake_intelligence().await,
            "POST",
            "/api/v1/proposals/pr1/pull-request",
            Some(json!({"optionId": "opt-b"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["status"], "accepted");
        assert_eq!(json["acceptedOptionId"], "opt-b");
        assert_eq!(json["executionId"], "exec-github");

        // No body at all: the recommended option.
        let (status, json) = send(
            app_with_fake_intelligence().await,
            "POST",
            "/api/v1/proposals/pr1/pull-request",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["acceptedOptionId"], "opt-a");
        assert_eq!(json["executionId"], "exec-github");
    }

    #[tokio::test]
    async fn accept_forwards_the_optional_adapter() {
        let (status, json) = send(
            app_with_fake_intelligence().await,
            "POST",
            "/api/v1/proposals/pr1/accept",
            Some(json!({"createExecution": true, "adapter": "docker"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["executionId"], "exec-docker");

        let (status, json) = send(
            app_with_fake_intelligence().await,
            "POST",
            "/api/v1/proposals/pr1/accept",
            Some(json!({"createExecution": true})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["executionId"], "exec-simulated");

        let (status, json) = send(
            app_with_fake_intelligence().await,
            "POST",
            "/api/v1/proposals/pr1/accept",
            Some(json!({"adapter": "cloud"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["code"], "invalid_body");
    }

    #[tokio::test]
    async fn create_execution_forwards_adapter_and_artifacts() {
        let (status, json) = send(
            app_with_fake_execution().await,
            "POST",
            "/api/v1/executions",
            Some(json!({
                "projectId": "p1",
                "title": "PR",
                "steps": [],
                "adapter": "github",
                "artifacts": {"Dockerfile": "FROM scratch\n"}
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["adapter"], "github");
        // The fake echoes the artifacts through `outputs`.
        assert_eq!(json["outputs"]["Dockerfile"], "FROM scratch\n");

        let (status, json) = send(
            app_with_fake_execution().await,
            "POST",
            "/api/v1/executions",
            Some(json!({"projectId": "p1", "title": "dry run", "steps": []})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["adapter"], "simulated");
        assert_eq!(json["outputs"], json!({}));
    }

    #[tokio::test]
    async fn unimplemented_rpc_is_a_500_without_detail() {
        let (status, json) = send(
            app_with_fake_execution().await,
            "GET",
            "/api/v1/policies",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(json["code"], "internal");
        assert_eq!(json["message"], "internal error");
    }

    #[tokio::test]
    async fn system_status_sees_the_serving_service() {
        let (_, json) = send(
            app_with_fake_execution().await,
            "GET",
            "/api/v1/system/status",
            None,
        )
        .await;
        assert_eq!(json["status"], "degraded");
        let services = json["services"].as_array().unwrap();
        assert_eq!(services[0]["status"], "unreachable");
        assert_eq!(services[2]["name"], "execution");
        assert_eq!(services[2]["status"], "serving");
        assert_eq!(services[2]["detail"], Value::Null);
    }
}
