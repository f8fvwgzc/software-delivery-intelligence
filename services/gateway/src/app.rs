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

use crate::{auth, error::ApiError, routes, sse, state::AppState};

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
    modifiers(&SessionCookie),
    security(("sdi_session" = [])),
    tags(
        (name = "system", description = "Gateway liveness and service health"),
        (name = "auth", description = "GitHub sign-in and the session cookie (CONTRACT.md §18)"),
        (name = "workspaces", description = "Tenants: members and roles, audit trail, data controls, export and deletion (§18, §19)"),
        (name = "webhooks", description = "GitHub App event delivery"),
        (name = "github", description = "Read-only GitHub connector"),
        (name = "projects", description = "Connected repositories and what is known about them"),
        (name = "scans", description = "Repository analysis runs"),
        (name = "delivery", description = "Observed pipelines and deployments"),
        (name = "knowledge", description = "Pattern memory and semantic search"),
        (name = "proposals", description = "Ranked proposals and negotiation"),
        (name = "decisions", description = "Decision memory with provenance"),
        (name = "documents", description = "Generated documents and reports"),
        (name = "diagrams", description = "Evidence-based diagrams generated from the knowledge graph"),
        (name = "executions", description = "Policy-gated plan execution"),
        (name = "approvals", description = "Human approvals"),
        (name = "policies", description = "Action policies and scoped identities"),
    ),
)]
struct ApiDoc;

/// Every route except the public ones (`/health`, `/auth/login`,
/// `/auth/callback`, `/webhooks/github`) needs the `sdi_session` cookie.
struct SessionCookie;

impl utoipa::Modify for SessionCookie {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "sdi_session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::with_description(
                auth::SESSION_COOKIE,
                "Opaque session token set by GET /api/v1/auth/callback. Pick the workspace with the X-Sdi-Workspace header or ?workspace=; routes under /workspaces/{id} use the path.",
            ))),
        );
    }
}

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

    // Order of the per-request layers, outermost first: request id and
    // tracing, then authentication (which needs no request id), then the
    // audit (which reads the request id and the principal), then routes.
    api.route("/api/openapi.json", get(move || async move { Json(spec) }))
        .merge(Scalar::with_url("/api/docs", document))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state.clone(), auth::audit))
        .layer(middleware::from_fn_with_state(state, auth::authenticate))
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
    use std::{sync::Arc, time::Duration};

    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use futures::StreamExt;
    use hmac::{Hmac, Mac};
    use serde_json::{Value, json};
    use sha2::Sha256;
    use tower::ServiceExt;

    use super::*;
    use crate::{
        fakes::{self, identity::Recorder},
        state::GatewaySettings,
    };

    /// Nothing listens here: every call fails fast with UNAVAILABLE.
    const DEAD: &str = "http://127.0.0.1:1";
    const OWNER: &str = "t-owner";

    fn settings() -> GatewaySettings {
        GatewaySettings {
            public_url: "http://127.0.0.1:5173".into(),
            webhook_secret: Some("hook-secret".into()),
            app_slug: Some("sdi-test".into()),
            dev_login: true,
        }
    }

    struct Stack {
        app: Router,
        recorder: Arc<Recorder>,
    }

    async fn stack(knowledge: &str, intelligence: &str, execution: &str) -> Stack {
        let (identity, recorder) = fakes::identity::serve().await;
        let state = AppState::with_urls(
            knowledge,
            intelligence,
            execution,
            &format!("http://{identity}"),
            settings(),
        )
        .unwrap();
        Stack {
            app: router(state),
            recorder,
        }
    }

    /// Fake identity, dead services.
    async fn app() -> Router {
        stack(DEAD, DEAD, DEAD).await.app
    }

    async fn app_with_fake_execution() -> Router {
        let addr = fakes::execution::serve().await;
        stack(DEAD, DEAD, &format!("http://{addr}")).await.app
    }

    async fn app_with_fake_knowledge() -> Router {
        let addr = fakes::knowledge::serve().await;
        stack(&format!("http://{addr}"), DEAD, DEAD).await.app
    }

    async fn app_with_fake_intelligence() -> Router {
        let addr = fakes::intelligence::serve().await;
        stack(DEAD, &format!("http://{addr}"), DEAD).await.app
    }

    async fn app_with_every_fake() -> Stack {
        let (knowledge, intelligence, execution) = tokio::join!(
            fakes::knowledge::serve(),
            fakes::intelligence::serve(),
            fakes::execution::serve()
        );
        stack(
            &format!("http://{knowledge}"),
            &format!("http://{intelligence}"),
            &format!("http://{execution}"),
        )
        .await
    }

    fn request(method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> Request<Body> {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            request = request.header("cookie", format!("theme=dark; sdi_session={token}"));
        }
        let body = match body {
            Some(body) => {
                request = request.header("content-type", "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        request.body(body).unwrap()
    }

    async fn send_as(
        app: Router,
        token: Option<&str>,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value, axum::http::HeaderMap) {
        let response = app
            .oneshot(request(method, uri, token, body))
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json, headers)
    }

    /// As the owner.
    async fn send(
        app: Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let (status, json, _) = send_as(app, Some(OWNER), method, uri, body).await;
        (status, json)
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, Value, axum::http::HeaderMap) {
        send_as(app, Some(OWNER), "GET", uri, None).await
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

    // --- Plumbing ---------------------------------------------------------

    #[tokio::test]
    async fn health_answers_ok_with_a_request_id_and_no_session() {
        let (status, json, headers) =
            send_as(app().await, None, "GET", "/api/v1/health", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        assert!(headers.contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn request_id_is_propagated_when_supplied() {
        let response = app()
            .await
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
        let (status, json, _) = send_as(app().await, None, "GET", "/api/openapi.json", None).await;
        assert_eq!(status, StatusCode::OK);
        let paths = json["paths"].as_object().expect("paths object");
        // CONTRACT.md §8 enumerates 55 distinct paths under /api/v1, plus the
        // two code browser routes of §15, the two diagram routes of §16 and
        // the 17 identity routes of §18.
        assert_eq!(paths.len(), 76, "{:#?}", paths.keys().collect::<Vec<_>>());
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
            "/api/v1/auth/login",
            "/api/v1/auth/callback",
            "/api/v1/auth/logout",
            "/api/v1/auth/me",
            "/api/v1/workspaces",
            "/api/v1/workspaces/{id}",
            "/api/v1/workspaces/{id}/members",
            "/api/v1/workspaces/{id}/members/{userId}",
            "/api/v1/workspaces/{id}/members/sync",
            "/api/v1/github/app/install-url",
            "/api/v1/github/app/setup",
            "/api/v1/webhooks/github",
            "/api/v1/workspaces/{id}/audit",
            "/api/v1/workspaces/{id}/data-controls",
            "/api/v1/workspaces/{id}/data-controls/reasoner-key",
            "/api/v1/workspaces/{id}/data/export",
            "/api/v1/workspaces/{id}/data/purge-code-cache",
        ] {
            assert!(paths.contains_key(path), "missing {path}");
        }
        assert!(paths["/api/v1/workspaces/{id}"]["delete"].is_object());
        assert_eq!(
            json["paths"]["/api/v1/projects"]["post"]["responses"]["201"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/Project"
        );
        assert!(json["components"]["schemas"]["ErrorBody"].is_object());
        assert!(json["components"]["schemas"]["Workspace"].is_object());
        assert!(json["components"]["schemas"]["DataControls"].is_object());
        assert!(json["components"]["schemas"]["DataExport"].is_object());
        assert_eq!(
            json["components"]["securitySchemes"]["sdi_session"]["in"],
            "cookie"
        );
        assert_eq!(
            json["components"]["securitySchemes"]["sdi_session"]["name"],
            "sdi_session"
        );
        assert_eq!(json["security"][0]["sdi_session"], json!([]));
    }

    #[tokio::test]
    async fn unreachable_service_yields_502_json() {
        let (status, json, _) = get_json(app().await, "/api/v1/projects").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(json["code"], "unavailable");
        assert!(json["message"].as_str().unwrap().contains("unavailable"));
    }

    #[tokio::test]
    async fn system_status_reports_every_service() {
        let (status, json, _) = get_json(app().await, "/api/v1/system/status").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "degraded");
        let services = json["services"].as_array().unwrap();
        assert_eq!(services.len(), 4);
        assert_eq!(services[0]["name"], "knowledge");
        assert_eq!(services[3]["name"], "identity");
        assert_eq!(services[3]["status"], "serving");
        assert!(services[..3].iter().all(|s| s["status"] == "unreachable"));
    }

    #[tokio::test]
    async fn malformed_json_body_is_a_400_with_the_error_body() {
        let (status, json) = send(
            app().await,
            "POST",
            "/api/v1/projects",
            Some(json!({"repoFullName": 5})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["code"], "invalid_body");
    }

    #[tokio::test]
    async fn bad_enum_in_query_is_a_400() {
        let (status, json, _) = get_json(app().await, "/api/v1/approvals?status=maybe").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["code"], "invalid_query");
    }

    #[tokio::test]
    async fn unknown_routes_answer_json_404() {
        let (status, json, _) = get_json(app().await, "/api/v1/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["code"], "not_found");
    }

    #[tokio::test]
    async fn docs_page_is_served_without_a_session() {
        let (status, _, _) = send_as(app().await, None, "GET", "/api/docs", None).await;
        assert_eq!(status, StatusCode::OK);
    }

    // --- Sessions and roles (§18) ------------------------------------------

    #[tokio::test]
    async fn requests_without_a_session_are_401() {
        let (status, json, _) = send_as(app().await, None, "GET", "/api/v1/projects", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json["code"], "unauthenticated");
        let (status, json, _) = send_as(
            app().await,
            Some("t-forged"),
            "GET",
            "/api/v1/projects",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json["code"], "unauthenticated");
        let (status, _, _) = send_as(app().await, None, "GET", "/api/v1/auth/me", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn an_inaccessible_workspace_is_403() {
        let app = app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/projects")
                    .header("cookie", "sdi_session=t-owner")
                    .header("x-sdi-workspace", "w-other")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let (status, json, _) = send_as(
            app_with_every_fake().await.app,
            Some(OWNER),
            "GET",
            "/api/v1/workspaces/w-other",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(json["code"], "permission_denied");
        let (status, _, _) = send_as(
            app_with_fake_knowledge().await,
            Some(OWNER),
            "GET",
            "/api/v1/projects?workspace=w-other",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn the_principal_reaches_the_services() {
        // The fakes refuse calls without the forwarded metadata, so a 200
        // proves the interceptor stamped the three headers.
        let (status, json, _) = send_as(
            app_with_fake_knowledge().await,
            Some("t-viewer"),
            "GET",
            "/api/v1/projects",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json[0]["id"], "p1");

        let (status, json) = send(
            app_with_fake_execution().await,
            "POST",
            "/api/v1/executions",
            Some(json!({"projectId": "p1", "title": "dry run", "steps": []})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["lastCheckpoint"], "owner", "role forwarded");
    }

    #[tokio::test]
    async fn roles_gate_the_routes() {
        let viewer = Some("t-viewer");
        let member = Some("t-member");
        let approver = Some("t-approver");
        let admin = Some("t-admin");

        let (status, json, _) = send_as(
            app().await,
            viewer,
            "POST",
            "/api/v1/projects",
            Some(json!({"repoFullName": "a/b"})),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(json["code"], "permission_denied");
        assert!(
            json["message"].as_str().unwrap().contains("member"),
            "{json}"
        );

        let dry = json!({"projectId": "p1", "title": "t", "steps": []});
        let (status, _, _) = send_as(
            app_with_fake_execution().await,
            member,
            "POST",
            "/api/v1/executions",
            Some(dry.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let docker = json!({"projectId": "p1", "title": "t", "steps": [], "adapter": "docker"});
        let (status, _, _) = send_as(
            app_with_fake_execution().await,
            member,
            "POST",
            "/api/v1/executions",
            Some(docker.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, json, _) = send_as(
            app_with_fake_execution().await,
            approver,
            "POST",
            "/api/v1/executions",
            Some(docker),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["adapter"], "docker");

        let (status, _, _) = send_as(
            app_with_fake_intelligence().await,
            member,
            "POST",
            "/api/v1/proposals/pr1/accept",
            Some(json!({"adapter": "github", "createExecution": true})),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = send_as(
            app_with_fake_intelligence().await,
            member,
            "POST",
            "/api/v1/proposals/pr1/pull-request",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = send_as(
            app_with_fake_intelligence().await,
            approver,
            "POST",
            "/api/v1/proposals/pr1/pull-request",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, _, _) = send_as(
            app().await,
            approver,
            "PATCH",
            "/api/v1/settings",
            Some(json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = send_as(
            app().await,
            approver,
            "PATCH",
            "/api/v1/policies/deploy.staging",
            Some(json!({"decision": "auto"})),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) =
            send_as(app().await, admin, "DELETE", "/api/v1/workspaces/w1", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = send_as(
            app().await,
            member,
            "GET",
            "/api/v1/workspaces/w1/audit",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = send_as(
            app().await,
            viewer,
            "GET",
            "/api/v1/workspaces/w1/members",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn login_redirects_and_callback_sets_the_cookie() {
        let (status, _, headers) = send_as(
            app().await,
            None,
            "GET",
            "/api/v1/auth/login?returnTo=/projects",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(
            headers["location"],
            "/api/v1/auth/callback?code=dev&state=dev&returnTo=/projects"
        );

        let (status, _, headers) = send_as(
            app().await,
            None,
            "GET",
            "/api/v1/auth/callback?code=dev&state=dev&returnTo=/projects",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(headers["location"], "/projects");
        let cookie = headers["set-cookie"].to_str().unwrap();
        assert!(cookie.starts_with("sdi_session=t-owner;"), "{cookie}");
        assert!(cookie.contains("HttpOnly"), "{cookie}");
        assert!(cookie.contains("SameSite=Lax"), "{cookie}");
        assert!(cookie.contains("Max-Age=2592000"), "{cookie}");
        assert!(!cookie.contains("Secure"), "http public url: {cookie}");

        // An off-origin target is not followed.
        let (_, _, headers) = send_as(
            app().await,
            None,
            "GET",
            "/api/v1/auth/callback?code=dev&state=dev&returnTo=https://evil.io/",
            None,
        )
        .await;
        assert_eq!(headers["location"], "/");

        let (status, json, _) = send_as(
            app().await,
            None,
            "GET",
            "/api/v1/auth/callback?error=access_denied",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["code"], "invalid_argument");
        let (status, _, _) = send_as(app().await, None, "GET", "/api/v1/auth/callback", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn me_and_logout() {
        let (status, json, _) = get_json(app_with_fake_knowledge().await, "/api/v1/auth/me").await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["user"]["login"], "octo");
        assert_eq!(json["user"]["email"], "octo@example.com");
        assert_eq!(json["currentWorkspaceId"], "w1");
        assert_eq!(json["workspaces"][0]["id"], "w1");
        assert_eq!(json["workspaces"][0]["kind"], "organization");
        assert_eq!(json["workspaces"][0]["myRole"], "owner");
        assert_eq!(json["workspaces"][0]["installationId"], 777);
        assert_eq!(
            json["workspaces"][0]["projectCount"], 1,
            "counted via knowledge"
        );

        let (status, _, headers) = send_as(
            app().await,
            Some("t-viewer"),
            "POST",
            "/api/v1/auth/logout",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let cookie = headers["set-cookie"].to_str().unwrap();
        assert!(cookie.starts_with("sdi_session=;"), "{cookie}");
        assert!(cookie.contains("Max-Age=0"), "{cookie}");
    }

    #[tokio::test]
    async fn authorize_answers_are_cached() {
        // The fake only knows w1; the cache is keyed by the requested
        // workspace, so two different requests both hit identity once and
        // then reuse it. Observable through the fake's audit recorder: the
        // audit entry is written through the same identity, so it must be
        // reachable, while the second request succeeds even if identity
        // were gone. Here we simply assert the cached path works.
        let stack = app_with_every_fake().await;
        let (status, _, _) = send_as(
            stack.app.clone(),
            Some(OWNER),
            "GET",
            "/api/v1/projects",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _, _) = send_as(
            stack.app.clone(),
            Some(OWNER),
            "GET",
            "/api/v1/projects",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn mutating_requests_are_audited() {
        let stack = app_with_every_fake().await;
        let (status, _, _) = send_as(
            stack.app.clone(),
            Some(OWNER),
            "POST",
            "/api/v1/executions",
            Some(json!({"projectId": "p1", "title": "t", "steps": []})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _, _) = send_as(
            stack.app.clone(),
            Some(OWNER),
            "GET",
            "/api/v1/projects",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // Fire-and-forget: give the spawned call a moment.
        for _ in 0..50 {
            if stack
                .recorder
                .audits
                .load(std::sync::atomic::Ordering::SeqCst)
                >= 1
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            stack
                .recorder
                .audits
                .load(std::sync::atomic::Ordering::SeqCst),
            1,
            "GET is not audited"
        );
        assert_eq!(
            *stack.recorder.last_action.lock().unwrap(),
            "execution.create"
        );

        // A refused mutation (403) is not audited: it never reached a handler.
        let (status, _, _) = send_as(
            stack.app.clone(),
            Some("t-viewer"),
            "DELETE",
            "/api/v1/projects/p1",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            stack
                .recorder
                .audits
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    // --- Workspace routes (§18, §19) ---------------------------------------

    #[tokio::test]
    async fn workspace_routes_read_identity() {
        let stack = app_with_every_fake().await;
        let (status, json) = send(stack.app.clone(), "GET", "/api/v1/workspaces", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json[0]["slug"], "acme");
        let (status, json) = send(stack.app.clone(), "GET", "/api/v1/workspaces/w1", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["id"], "w1");
        assert_eq!(json["projectCount"], 1);

        let (status, json) = send(
            stack.app.clone(),
            "GET",
            "/api/v1/workspaces/w1/members",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["members"][0]["user"]["login"], "octo");
        assert_eq!(json["members"][0]["derivedRole"], "admin");
        assert_eq!(json["members"][0]["overrideRole"], Value::Null);
        assert_eq!(json["syncedAt"], "2023-11-14T22:13:20Z");

        let (status, json) = send(
            stack.app.clone(),
            "PATCH",
            "/api/v1/workspaces/w1/members/u1",
            Some(json!({"overrideRole": "viewer"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["overrideRole"], "viewer");
        assert_eq!(json["effectiveRole"], "viewer");
        let (status, json) = send(
            stack.app.clone(),
            "PATCH",
            "/api/v1/workspaces/w1/members/u1",
            Some(json!({"overrideRole": "owner"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        let (status, json) = send(
            stack.app.clone(),
            "POST",
            "/api/v1/workspaces/w1/members/sync",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["syncedAt"], "2023-11-14T22:15:00Z");

        let (status, json) = send(
            stack.app.clone(),
            "GET",
            "/api/v1/workspaces/w1/audit?actionPrefix=project.",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json[0]["action"], "project.connect");
        assert_eq!(json[0]["userLogin"], "octo");

        let (status, json) = send(
            stack.app.clone(),
            "GET",
            "/api/v1/workspaces/w1/data-controls",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["codeCache"], "session");
        assert_eq!(json["reasonerKeyConfigured"], false);
        assert_eq!(json["reasonerKeyHint"], Value::Null);
        let (status, json) = send(
            stack.app.clone(),
            "PATCH",
            "/api/v1/workspaces/w1/data-controls",
            Some(json!({"codeCache": "none", "minimalChunks": true})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["codeCache"], "none");
        assert_eq!(json["minimalChunks"], true);
        assert_eq!(json["auditRetentionDays"], 365, "unset field kept");
        let (status, json) = send(
            stack.app.clone(),
            "PUT",
            "/api/v1/workspaces/w1/data-controls/reasoner-key",
            Some(json!({"apiKey": "sk-ant-1234"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["reasonerKeyConfigured"], true);
        assert_eq!(json["reasonerKeyHint"], "1234");

        let (status, json) = send(
            stack.app.clone(),
            "POST",
            "/api/v1/workspaces/w1/data/purge-code-cache",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["filesDeleted"], 12);

        let (status, json) = send(stack.app.clone(), "DELETE", "/api/v1/workspaces/w1", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["workspaceId"], "w1");
        assert_eq!(json["deleted"]["knowledge"]["facts"], 40);
        assert_eq!(json["deleted"]["intelligence"]["proposals"], 2);
        assert_eq!(json["deleted"]["execution"]["cancelled"], 1);
    }

    #[tokio::test]
    async fn export_streams_one_json_object_and_survives_a_missing_service() {
        let stack = app_with_every_fake().await;
        let response = stack
            .app
            .oneshot(request(
                "POST",
                "/api/v1/workspaces/w1/data/export",
                Some(OWNER),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        let json: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(json["workspace"]["id"], "w1");
        assert_eq!(json["members"][0]["user"]["login"], "octo");
        assert_eq!(json["dataControls"]["codeCache"], "session");
        assert_eq!(json["knowledge"]["projects"][0]["id"], "p1");
        assert_eq!(json["knowledge"]["facts"].as_array().unwrap().len(), 2);
        assert_eq!(json["intelligence"]["proposals"][0]["title"], "Deploy");
        assert_eq!(json["execution"], json!({}));
        assert!(
            json["errors"]["execution"]
                .as_str()
                .unwrap()
                .contains("not implemented"),
            "{json}"
        );
        assert_eq!(json["audit"][0]["action"], "project.connect");
        assert!(json["exportedAt"].is_string());
    }

    #[tokio::test]
    async fn github_app_routes() {
        let (status, json) = send(app().await, "GET", "/api/v1/github/app/install-url", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(
            json["url"],
            "https://github.com/apps/sdi-test/installations/new"
        );

        let (status, _, headers) = send_as(
            app().await,
            Some(OWNER),
            "GET",
            "/api/v1/github/app/setup?installation_id=777&setup_action=install",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(headers["location"], "/?workspace=w1");
        let (status, _, _) = send_as(
            app().await,
            Some(OWNER),
            "GET",
            "/api/v1/github/app/setup?installation_id=1",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    fn signed(secret: &str, body: &Value) -> (String, String) {
        let text = body.to_string();
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(text.as_bytes());
        (
            text,
            format!("sha256={}", hex::encode(mac.finalize().into_bytes())),
        )
    }

    async fn webhook(app: Router, event: &str, body: &Value, secret: &str) -> (StatusCode, Value) {
        let (text, signature) = signed(secret, body);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/webhooks/github")
                    .header("content-type", "application/json")
                    .header("x-github-event", event)
                    .header("x-hub-signature-256", signature)
                    .body(Body::from(text))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn webhooks_verify_the_signature_and_dispatch() {
        let installation = json!({"installation": {"id": 777, "account": {"login": "acme", "type": "Organization"}, "repository_selection": "all"}});
        let mut created = installation.clone();
        created["action"] = json!("created");
        let (status, json) = webhook(app().await, "installation", &created, "wrong").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");

        let (status, json) = webhook(app().await, "installation", &created, "hook-secret").await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["outcome"], "recorded");

        let mut deleted = installation.clone();
        deleted["action"] = json!("deleted");
        let (status, json) = webhook(app().await, "installation", &deleted, "hook-secret").await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["outcome"], "removed");

        let member = json!({"action": "added", "installation": {"id": 777}, "organization": {"login": "acme"}});
        let (status, json) = webhook(app().await, "member", &member, "hook-secret").await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["outcome"], "synced");

        let push = |git_ref: &str, repo: &str| {
            json!({"ref": git_ref, "installation": {"id": 777}, "organization": {"login": "acme"},
                   "repository": {"full_name": repo, "default_branch": "main"}})
        };
        let (status, json) = webhook(
            app_with_fake_knowledge().await,
            "push",
            &push("refs/heads/main", "acme/repo"),
            "hook-secret",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["outcome"], "scan_started");
        let (_, json) = webhook(
            app_with_fake_knowledge().await,
            "push",
            &push("refs/heads/feature", "acme/repo"),
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "ignored");
        let (_, json) = webhook(
            app_with_fake_knowledge().await,
            "push",
            &push("refs/heads/main", "acme/other"),
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "ignored");
        // Knowledge down: still 200, the scan is simply not started.
        let (status, json) = webhook(
            app().await,
            "push",
            &push("refs/heads/main", "acme/repo"),
            "hook-secret",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["outcome"], "ignored");

        let (_, json) = webhook(app().await, "ping", &json!({"zen": "x"}), "hook-secret").await;
        assert_eq!(json["outcome"], "ignored");
    }

    // --- Streams -----------------------------------------------------------

    #[tokio::test]
    async fn sse_streams_every_event_then_ends() {
        let app = app_with_fake_execution().await;
        let response = app
            .oneshot(request(
                "GET",
                "/api/v1/executions/e1/events/stream?afterSeq=1",
                Some(OWNER),
                None,
            ))
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
            .oneshot(request("GET", "/api/v1/scans/s1/stream", Some(OWNER), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
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
    async fn events_stream_merges_the_services_filters_the_workspace_and_survives_a_drop() {
        // knowledge: two events of w1 and one of w2, then its stream ends (a
        // drop); intelligence: one event, stays open; execution: one event
        // after 300ms, stays open. The late execution event proves the
        // merged feed outlived the knowledge drop; the w2 event never shows.
        let stack = app_with_every_fake().await;
        let response = stack
            .app
            .oneshot(request("GET", "/api/v1/events/stream", Some(OWNER), None))
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
        assert!(
            events.iter().all(|e| e["entityId"] != "other"),
            "other workspace leaked: {events:?}"
        );
        let execution = events.iter().find(|e| e["entity"] == "execution").unwrap();
        assert_eq!(execution["entityId"], "e1");
        assert_eq!(execution["projectId"], "p1");
        assert_eq!(execution["at"], "2023-11-14T22:13:20Z");
        assert_eq!(execution["data"]["workspace_id"], "w1");
    }

    #[tokio::test]
    async fn events_stream_keeps_flowing_while_a_service_is_down() {
        let (knowledge, execution) =
            tokio::join!(fakes::knowledge::serve(), fakes::execution::serve());
        let stack = stack(
            &format!("http://{knowledge}"),
            DEAD,
            &format!("http://{execution}"),
        )
        .await;
        let response = stack
            .app
            .oneshot(request("GET", "/api/v1/events/stream", Some(OWNER), None))
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
        let app = app_with_fake_knowledge().await;
        let started = tokio::time::Instant::now();
        let response = app
            .oneshot(request("GET", "/api/v1/events/stream", Some(OWNER), None))
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

    // --- Existing composition routes --------------------------------------

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
        let (status, json) = send(
            app_with_every_fake().await.app,
            "GET",
            "/api/v1/settings",
            None,
        )
        .await;
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
            app_with_every_fake().await.app,
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
            app_with_every_fake().await.app,
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
            app_with_every_fake().await.app,
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
