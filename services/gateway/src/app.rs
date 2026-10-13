//! Assembles the router and the OpenAPI document from the same route
//! registrations, then wraps it in the middleware stack.

use std::{any::Any, time::Duration};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderName, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
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
        (name = "delivery", description = "Observed pipelines and deployments, and continuous delivery: registries, environments, releases, promotions and rollbacks (§22)"),
        (name = "knowledge", description = "Pattern memory and semantic search"),
        (name = "proposals", description = "Ranked proposals and negotiation"),
        (name = "decisions", description = "Decision memory with provenance"),
        (name = "documents", description = "Generated documents and reports"),
        (name = "diagrams", description = "Evidence-based diagrams generated from the knowledge graph"),
        (name = "reviews", description = "Pull request reviews: evidence first, a human decides, then SDI posts (§20)"),
        (name = "autopilot", description = "Pull request autopilot: drafted fixes, fix commits, merge readiness and merges, vulnerabilities and Dependabot; everything that changes the repository is an execution (§23)"),
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
    // The OpenAPI document and the Scalar page are rendered once and shared:
    // axum clones every route's service per connection, and a handler that
    // captured the document itself (a tree of maps) cost about 9 MB of heap
    // per open connection, which fifty SSE tabs turned into half a gigabyte.
    let spec: Bytes = document
        .to_json()
        .expect("the OpenAPI document serialises")
        .into();
    let docs: Bytes = Scalar::with_url("/api/docs", document).to_html().into();

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
    api.route(
        "/api/openapi.json",
        get(move || {
            let spec = spec.clone();
            async move {
                (
                    [(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/json"),
                    )],
                    spec,
                )
            }
        }),
    )
    .route(
        "/api/docs",
        get(move || {
            let docs = docs.clone();
            async move { Html(docs) }
        }),
    )
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

    const PUBLIC_URL: &str = "http://127.0.0.1:5173";

    fn settings() -> GatewaySettings {
        GatewaySettings {
            public_url: PUBLIC_URL.into(),
            webhook_secret: Some("hook-secret".into()),
            app_slug: Some("sdi-test".into()),
            dev_login: true,
            // Small on purpose so the cap tests stay quick.
            sse_max_per_workspace: 3,
            auth_cache_max: 4,
            webhook_queue_max: 8,
        }
    }

    struct Stack {
        app: Router,
        recorder: Arc<Recorder>,
        state: AppState,
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
            app: router(state.clone()),
            recorder,
            state,
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

    /// The audit entries the fake identity recorded, once at least `count`
    /// arrived (they are written fire-and-forget after the response), in the
    /// order they were recorded.
    async fn audited(
        recorder: &Recorder,
        count: usize,
    ) -> Vec<sdi_proto::identity::v1::AuditEntry> {
        for _ in 0..100 {
            if recorder.entries.lock().unwrap().len() >= count {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // Let a straggler that should not exist show up before the count is asserted.
        tokio::time::sleep(Duration::from_millis(50)).await;
        recorder.entries.lock().unwrap().clone()
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
        // two code browser routes of §15, the two diagram routes of §16, the
        // 18 identity routes of §18 (with `/people`), the 8 review routes of
        // §20, the 14 continuous delivery paths of §22, the 6 pull request
        // autopilot paths of §23 and the reasoner test of §19.
        assert_eq!(paths.len(), 106, "{:#?}", paths.keys().collect::<Vec<_>>());
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
            "/api/v1/people",
            "/api/v1/settings/reasoner/test",
            "/api/v1/github/app/install-url",
            "/api/v1/github/app/setup",
            "/api/v1/webhooks/github",
            "/api/v1/workspaces/{id}/audit",
            "/api/v1/workspaces/{id}/data-controls",
            "/api/v1/workspaces/{id}/data-controls/reasoner-key",
            "/api/v1/workspaces/{id}/data/export",
            "/api/v1/projects/{id}/pull-requests",
            "/api/v1/projects/{id}/pull-requests/{number}",
            "/api/v1/projects/{id}/pull-requests/{number}/reviews",
            "/api/v1/reviews",
            "/api/v1/reviews/{id}",
            "/api/v1/findings/{id}/decide",
            "/api/v1/reviews/{id}/publish",
            "/api/v1/activity",
            "/api/v1/workspaces/{id}/data/purge-code-cache",
            // §22
            "/api/v1/registries",
            "/api/v1/registries/{id}",
            "/api/v1/projects/{id}/environments",
            "/api/v1/environments/{id}",
            "/api/v1/projects/{id}/pipeline",
            "/api/v1/projects/{id}/releases",
            "/api/v1/releases/{id}",
            "/api/v1/environments/{id}/promote",
            "/api/v1/environments/{id}/rollback",
            "/api/v1/projects/{id}/deployment-runs",
            "/api/v1/deployment-runs/{id}",
            "/api/v1/projects/{id}/delivery/artifacts",
            "/api/v1/workspaces/{id}/secrets",
            "/api/v1/workspaces/{id}/secrets/{name}",
            // §23
            "/api/v1/findings/{id}/draft-fix",
            "/api/v1/reviews/{id}/apply-fixes",
            "/api/v1/projects/{id}/pull-requests/{number}/merge-readiness",
            "/api/v1/projects/{id}/pull-requests/{number}/merge",
            "/api/v1/projects/{id}/security",
            "/api/v1/projects/{id}/security/dependabot",
        ] {
            assert!(paths.contains_key(path), "missing {path}");
        }
        // §23: one method per path, and the three requests that create an
        // execution answer 201.
        for (path, method, status, body) in [
            ("/api/v1/findings/{id}/draft-fix", "post", "200", "Finding"),
            ("/api/v1/reviews/{id}/apply-fixes", "post", "201", "Review"),
            (
                "/api/v1/projects/{id}/pull-requests/{number}/merge-readiness",
                "get",
                "200",
                "MergeReadiness",
            ),
            (
                "/api/v1/projects/{id}/pull-requests/{number}/merge",
                "post",
                "201",
                "MergeResult",
            ),
            (
                "/api/v1/projects/{id}/security",
                "get",
                "200",
                "SecurityOverview",
            ),
            (
                "/api/v1/projects/{id}/security/dependabot",
                "post",
                "201",
                "ConfigureDependabotResult",
            ),
        ] {
            let operations = paths[path].as_object().unwrap();
            assert_eq!(
                operations.keys().map(String::as_str).collect::<Vec<_>>(),
                [method],
                "{path}"
            );
            assert_eq!(
                operations[method]["responses"][status]["content"]["application/json"]["schema"]["$ref"],
                format!("#/components/schemas/{body}"),
                "{path}"
            );
            assert!(
                operations[method]["responses"]["412"].is_object(),
                "{path} documents its refusal"
            );
        }
        assert_eq!(
            paths["/api/v1/projects/{id}/security"]["get"]["parameters"][1]["name"],
            "refresh"
        );
        for (path, body) in [
            (
                "/api/v1/projects/{id}/pull-requests/{number}/merge",
                "MergePullRequestRequest",
            ),
            (
                "/api/v1/projects/{id}/security/dependabot",
                "ConfigureDependabotRequest",
            ),
        ] {
            assert_eq!(
                paths[path]["post"]["requestBody"]["content"]["application/json"]["schema"]["$ref"],
                format!("#/components/schemas/{body}"),
                "{path}"
            );
        }
        // Every method §22 lists is there, with flat request bodies.
        for (path, methods) in [
            ("/api/v1/registries", vec!["get", "post"]),
            ("/api/v1/registries/{id}", vec!["put", "delete"]),
            ("/api/v1/projects/{id}/environments", vec!["get", "post"]),
            ("/api/v1/environments/{id}", vec!["put", "delete"]),
            ("/api/v1/projects/{id}/releases", vec!["get", "post"]),
            (
                "/api/v1/workspaces/{id}/secrets/{name}",
                vec!["put", "delete"],
            ),
        ] {
            let operations = paths[path].as_object().unwrap();
            assert_eq!(
                operations.keys().map(String::as_str).collect::<Vec<_>>(),
                {
                    let mut sorted = methods.clone();
                    sorted.sort_unstable();
                    sorted
                },
                "{path}"
            );
        }
        let schemas = &json["components"]["schemas"];
        for schema in [
            "Registry",
            "UpsertRegistryRequest",
            "Environment",
            "UpsertEnvironmentRequest",
            "Release",
            "CreateReleaseRequest",
            "DeploymentRun",
            "DeploymentRunStatus",
            "PipelineStage",
            "DeliveryPipeline",
            "PromoteRequest",
            "RollbackRequest",
            "DeliveryArtifacts",
            "SecretInfo",
            "SetSecretRequest",
        ] {
            assert!(schemas[schema].is_object(), "missing schema {schema}");
        }
        let pipeline = &schemas["DeliveryPipeline"]["properties"];
        for field in [
            "successRate",
            "deployments30d",
            "leadTimeSeconds",
            "stages",
            "registry",
        ] {
            assert!(pipeline[field].is_object(), "DeliveryPipeline.{field}");
        }
        assert!(
            schemas["UpsertEnvironmentRequest"]["properties"]["healthPath"].is_object(),
            "bodies are flat, camelCase"
        );
        assert!(schemas["SecretInfo"]["properties"]["value"].is_null());
        // §23: the new shapes, and the fields the feature adds to old ones.
        for schema in [
            "MergeReadiness",
            "MergeBlocker",
            "MergeMethod",
            "MergePullRequestRequest",
            "MergeResult",
            "SecurityOverview",
            "Vulnerability",
            "SeverityCount",
            "ConfigureDependabotRequest",
            "ConfigureDependabotResult",
        ] {
            assert!(schemas[schema].is_object(), "missing schema {schema}");
        }
        for (schema, fields) in [
            (
                "PullRequestSummary",
                vec!["headRepo", "isFork", "isBot", "isDependabot"],
            ),
            (
                "Finding",
                vec!["suggestionSource", "fixExecutionId", "canDraftFix"],
            ),
            (
                "Review",
                vec!["fixExecutionId", "mergeExecutionId", "fixesReady"],
            ),
            (
                "IntelligenceSettings",
                vec![
                    "autoDraftFixes",
                    "autoMerge",
                    "mergeRequiresReview",
                    "prWatchIntervalSeconds",
                    "prWatchLastRunAt",
                    "prWatchLastError",
                ],
            ),
            (
                "IntelligenceSettingsPatch",
                vec!["autoDraftFixes", "autoMerge", "mergeRequiresReview"],
            ),
            ("DataControls", vec!["codeToModel"]),
            ("UpdateDataControlsRequest", vec!["codeToModel"]),
            (
                "MergeReadiness",
                vec![
                    "blockers",
                    "canMerge",
                    "checksTotal",
                    "checksPassed",
                    "checksFailed",
                    "checksPending",
                    "failedChecks",
                    "pendingChecks",
                    "allowedMethods",
                    "reviewId",
                    "reviewStatus",
                    "undecidedBlockingFindings",
                    "mergeExecutionId",
                    "checkedAt",
                ],
            ),
            ("MergeResult", vec!["executionId", "readiness"]),
            (
                "SecurityOverview",
                vec![
                    "source",
                    "sourceNote",
                    "vulnerabilities",
                    "bySeverity",
                    "truncated",
                    "configPresent",
                    "configPath",
                    "configEcosystems",
                    "alerts",
                    "securityUpdates",
                    "openDependabotPullRequests",
                    "configureExecutionId",
                    "checkedAt",
                ],
            ),
            (
                "ConfigureDependabotRequest",
                vec!["alerts", "securityUpdates", "configFile"],
            ),
        ] {
            for field in fields {
                let property = &schemas[schema]["properties"][field];
                assert!(property.is_object(), "{schema}.{field}");
                assert!(
                    property["description"].is_string() || property["$ref"].is_string(),
                    "{schema}.{field} is documented"
                );
            }
        }
        // The read-only watcher fields are not part of the patch.
        for field in [
            "prWatchIntervalSeconds",
            "prWatchLastRunAt",
            "prWatchLastError",
        ] {
            assert!(
                schemas["IntelligenceSettingsPatch"]["properties"][field].is_null(),
                "{field} is read-only"
            );
        }
        // Nullable where the contract says "if any".
        for (schema, field) in [
            ("Finding", "suggestionSource"),
            ("Finding", "fixExecutionId"),
            ("Review", "fixExecutionId"),
            ("Review", "mergeExecutionId"),
            ("IntelligenceSettings", "prWatchLastRunAt"),
            ("IntelligenceSettings", "prWatchLastError"),
            ("MergeReadiness", "mergeExecutionId"),
            ("MergeReadiness", "reviewId"),
            ("SecurityOverview", "sourceNote"),
            ("SecurityOverview", "configureExecutionId"),
            ("Vulnerability", "fixedVersion"),
        ] {
            assert_eq!(
                schemas[schema]["properties"][field]["type"],
                json!(["string", "null"]),
                "{schema}.{field}"
            );
        }
        assert_eq!(
            schemas["Vulnerability"]["properties"]["fixPullRequest"]["type"],
            json!(["integer", "null"])
        );
        assert_eq!(
            schemas["MergeMethod"]["enum"],
            json!(["merge", "squash", "rebase"])
        );
        assert_eq!(
            schemas["MergeBlocker"]["required"],
            json!(["code", "message"])
        );
        // The knowledge service's observed deployments keep their own enum.
        assert_eq!(
            schemas["DeploymentStatus"]["enum"],
            json!([
                "unspecified",
                "pending",
                "in_progress",
                "succeeded",
                "failed",
                "inactive"
            ])
        );
        assert_eq!(
            schemas["DeploymentRunStatus"]["enum"],
            json!([
                "unspecified",
                "pending",
                "awaiting_approval",
                "deploying",
                "verifying",
                "succeeded",
                "failed",
                "cancelled"
            ])
        );
        assert_eq!(
            json["paths"]["/api/v1/registries"]["get"]["responses"]["200"]["content"]["application/json"]
                ["schema"]["type"],
            "array",
            "lists are bare arrays"
        );
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
        // Redirects are absolute on the public origin (CONTRACT.md §18), so
        // a replica behind a proxy never advertises its own host.
        assert_eq!(
            headers["location"],
            format!("{PUBLIC_URL}/api/v1/auth/callback?code=dev&state=dev&returnTo=/projects")
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
        assert_eq!(headers["location"], format!("{PUBLIC_URL}/projects"));
        let cookie = headers["set-cookie"].to_str().unwrap();
        assert!(cookie.starts_with("sdi_session=t-owner;"), "{cookie}");
        assert!(cookie.contains("HttpOnly"), "{cookie}");
        assert!(cookie.contains("SameSite=Lax"), "{cookie}");
        assert!(cookie.contains("Max-Age=2592000"), "{cookie}");
        assert!(!cookie.contains("Secure"), "http public url: {cookie}");

        // Behind a TLS-terminating edge the cookie is Secure even though
        // the replica itself speaks plain HTTP (X-Forwarded-Proto).
        let response = app()
            .await
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/callback?code=dev&state=dev&returnTo=/projects")
                    .header("x-forwarded-proto", "https")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let cookie = response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(cookie.contains("; Secure"), "forwarded https: {cookie}");

        // An off-origin target is not followed.
        let (_, _, headers) = send_as(
            app().await,
            None,
            "GET",
            "/api/v1/auth/callback?code=dev&state=dev&returnTo=https://evil.io/",
            None,
        )
        .await;
        assert_eq!(headers["location"], format!("{PUBLIC_URL}/"));

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

        // Names for stored actor ids: scoped to the caller's workspace, the
        // email never leaves, and ids it cannot name are absent.
        let (status, json) = send(
            stack.app.clone(),
            "GET",
            "/api/v1/people?ids=u1,%20u2,,ops@example.test",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json.as_array().unwrap().len(), 1, "{json}");
        assert_eq!(json[0]["id"], "u1");
        assert_eq!(json[0]["login"], "octo@w1");
        assert_eq!(json[0]["avatarUrl"], "https://avatars.example/u1");
        assert_eq!(json[0]["member"], true);
        assert!(json[0].get("email").is_none(), "{json}");
        let (status, json) = send(stack.app.clone(), "GET", "/api/v1/people?ids=", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json, json!([]));
        let many = vec!["u1"; 101].join(",");
        let (status, json) = send(
            stack.app.clone(),
            "GET",
            &format!("/api/v1/people?ids={many}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        let (status, json) = send(stack.app.clone(), "GET", "/api/v1/people", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");

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
        assert_eq!(json["codeToModel"], "off", "nothing leaves by default");
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
        assert_eq!(json["codeToModel"], "off", "unset field kept");
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
        assert_eq!(json["deleted"]["identity"]["secret"], 0);
    }

    /// `codeToModel` (CONTRACT.md §19, §23): read and patched through the
    /// data-controls routes, admin only, audited like every data control.
    #[tokio::test]
    async fn code_to_model_is_read_patched_refused_and_audited() {
        let stack = app_with_every_fake().await;
        let app = stack.app.clone();
        let path = "/api/v1/workspaces/w1/data-controls";

        let (status, json, _) = send_as(app.clone(), Some("t-viewer"), "GET", path, None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["codeToModel"], "off");

        let on = json!({"codeToModel": "changed_lines"});
        let (status, json) = send(app.clone(), "PATCH", path, Some(on.clone())).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["codeToModel"], "changed_lines");
        assert_eq!(json["codeCache"], "session", "nothing else moved");
        assert_eq!(json["reasoner"], "off");
        assert_eq!(json["auditRetentionDays"], 365);

        // The service's refusal passes through with the allowed values.
        let (status, json) = send(
            app.clone(),
            "PATCH",
            path,
            Some(json!({"codeToModel": "whole_file"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        assert_eq!(json["code"], "invalid_argument");
        assert_eq!(
            json["message"],
            "code_to_model must be one of off, changed_lines"
        );
        // Null is "not said", like every other field of the patch.
        let (status, json) = send(
            app.clone(),
            "PATCH",
            path,
            Some(json!({"codeToModel": null, "minimalChunks": true})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["codeToModel"], "off");
        assert_eq!(json["minimalChunks"], true);

        // Below admin the patch never reaches identity.
        let (status, _, _) = send_as(app, Some("t-approver"), "PATCH", path, Some(on)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let entries = audited(&stack.recorder, 3).await;
        assert_eq!(entries.len(), 3, "the refused 403 is not audited");
        for entry in &entries {
            assert_eq!(entry.action, "data_controls.update");
            assert_eq!(entry.entity, "workspace");
            assert_eq!(entry.entity_id, "w1");
            assert_eq!(entry.user_login, "octo");
        }
        // Two changes and the refused value; the value itself is not kept.
        let mut details: Vec<&str> = entries.iter().map(|e| e.detail.as_str()).collect();
        details.sort_unstable();
        assert_eq!(details, ["", "", "http 400"]);
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
        assert_eq!(
            json["dataControls"]["codeToModel"], "off",
            "the export says whether source lines may reach the model"
        );
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
        assert_eq!(json["secrets"], json!([]), "names only, and there are none");
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
        assert_eq!(headers["location"], format!("{PUBLIC_URL}/?workspace=w1"));
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

    /// Posts a delivery and returns the HTTP answer. The handler only
    /// classifies: the answer is `202 queued` or `202 ignored`.
    async fn post_webhook(
        app: Router,
        delivery_id: &str,
        event: &str,
        body: &Value,
        secret: &str,
    ) -> (StatusCode, Value) {
        let (text, signature) = signed(secret, body);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/webhooks/github")
                    .header("content-type", "application/json")
                    .header("x-github-event", event)
                    .header("x-github-delivery", delivery_id)
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

    /// Posts a delivery and, when it was queued, waits for the worker's
    /// outcome, so a test reads what the delivery ended as.
    async fn webhook(stack: Stack, event: &str, body: &Value, secret: &str) -> (StatusCode, Value) {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let delivery_id = format!(
            "d-{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        );
        let (status, mut json) = post_webhook(stack.app, &delivery_id, event, body, secret).await;
        if status != StatusCode::ACCEPTED {
            return (status, json);
        }
        if json["outcome"] == "queued" {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(outcome) = stack.state.webhook_outcome(&delivery_id) {
                    json["outcome"] = Value::String(outcome);
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "delivery {delivery_id} never processed"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        (status, json)
    }

    #[tokio::test]
    async fn webhooks_verify_the_signature_and_dispatch() {
        let installation = json!({"installation": {"id": 777, "account": {"login": "acme", "type": "Organization"}, "repository_selection": "all"}});
        let mut created = installation.clone();
        created["action"] = json!("created");
        let dead = || stack(DEAD, DEAD, DEAD);
        let (status, json) = webhook(dead().await, "installation", &created, "wrong").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");

        // The handler answers 202 at once; the recorded outcome arrives
        // from the worker (the helper waits for it).
        let (status, json) = post_webhook(
            app().await,
            "d-raw",
            "installation",
            &created,
            "hook-secret",
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["outcome"], "queued");

        let (status, json) = webhook(dead().await, "installation", &created, "hook-secret").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["outcome"], "recorded");

        let mut deleted = installation.clone();
        deleted["action"] = json!("deleted");
        let (status, json) = webhook(dead().await, "installation", &deleted, "hook-secret").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["outcome"], "removed");

        let member = json!({"action": "added", "installation": {"id": 777}, "organization": {"login": "acme"}});
        let (status, json) = webhook(dead().await, "member", &member, "hook-secret").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["outcome"], "synced");

        let push = |git_ref: &str, repo: &str| {
            json!({"ref": git_ref, "installation": {"id": 777}, "organization": {"login": "acme"},
                   "repository": {"full_name": repo, "default_branch": "main"}})
        };
        let with_knowledge = || async {
            let addr = fakes::knowledge::serve().await;
            stack(&format!("http://{addr}"), DEAD, DEAD).await
        };
        let (status, json) = webhook(
            with_knowledge().await,
            "push",
            &push("refs/heads/main", "acme/repo"),
            "hook-secret",
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["outcome"], "scan_started");
        let (_, json) = webhook(
            with_knowledge().await,
            "push",
            &push("refs/heads/feature", "acme/repo"),
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "ignored");
        let (_, json) = webhook(
            with_knowledge().await,
            "push",
            &push("refs/heads/main", "acme/other"),
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "ignored");
        // Knowledge down: still accepted, the worker records the failure.
        let (status, json) = webhook(
            dead().await,
            "push",
            &push("refs/heads/main", "acme/repo"),
            "hook-secret",
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["outcome"], "failed");

        let (_, json) = webhook(dead().await, "ping", &json!({"zen": "x"}), "hook-secret").await;
        assert_eq!(json["outcome"], "ignored");
    }

    /// The queue is bounded: once `WEBHOOK_QUEUE_MAX` deliveries wait,
    /// the handler answers 503 with Retry-After so GitHub redelivers
    /// instead of the replica growing a backlog.
    #[tokio::test]
    async fn webhook_queue_is_bounded_and_overflow_asks_for_redelivery() {
        // Every service dead: each delivery waits on the identity connect
        // timeout, so the queue fills up before the worker drains it.
        let (identity, _) = fakes::identity::serve().await;
        let state = AppState::with_urls(
            DEAD,
            DEAD,
            DEAD,
            &format!("http://{identity}"),
            GatewaySettings {
                webhook_queue_max: 2,
                ..settings()
            },
        )
        .unwrap();
        let app = router(state.clone());
        let member = json!({"action": "added", "installation": {"id": 777}, "organization": {"login": "acme"}});
        let mut seen = Vec::new();
        for i in 0..40 {
            let (status, _) = post_webhook(
                app.clone(),
                &format!("q-{i}"),
                "member",
                &member,
                "hook-secret",
            )
            .await;
            seen.push(status);
            if status == StatusCode::SERVICE_UNAVAILABLE {
                break;
            }
        }
        assert!(
            seen.contains(&StatusCode::SERVICE_UNAVAILABLE),
            "never overflowed: {seen:?}"
        );
        assert!(seen.iter().filter(|s| **s == StatusCode::ACCEPTED).count() <= 3);
    }

    // --- Code review (§20) --------------------------------------------------

    #[tokio::test]
    async fn pull_requests_carry_their_latest_review_and_the_detail_has_the_diff() {
        let app = app_with_every_fake().await.app;
        let (status, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/p1/pull-requests?state=all&refresh=true&limit=10",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let prs = json["pullRequests"].as_array().unwrap();
        assert_eq!(prs.len(), 3);
        assert_eq!(prs[0]["number"], 1);
        assert_eq!(prs[0]["state"], "all");
        // The newest review of PR 1 wins; PR 2 has its own.
        assert_eq!(prs[0]["latestReviewId"], "r-new");
        assert_eq!(prs[1]["latestReviewId"], "r-2");
        assert_eq!(json["syncedAt"], "2023-11-14T22:15:00Z");
        assert_eq!(prs[2]["latestReviewId"], Value::Null, "never reviewed");
        // §23: where the head lives and who wrote it. PR 1 is a person's
        // branch, PR 2 is Dependabot's, PR 3 comes from a fork.
        for (pr, head_repo, fork, bot, dependabot) in [
            (&prs[0], "acme/repo", false, false, false),
            (&prs[1], "acme/repo", false, true, true),
            (&prs[2], "eve/repo", true, false, false),
        ] {
            assert_eq!(pr["headRepo"], head_repo, "{pr}");
            assert_eq!(pr["isFork"], fork, "{pr}");
            assert_eq!(pr["isBot"], bot, "{pr}");
            assert_eq!(pr["isDependabot"], dependabot, "{pr}");
        }

        let (status, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/p1/pull-requests/2",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["summary"]["number"], 2);
        assert_eq!(json["summary"]["headRepo"], "acme/repo");
        assert_eq!(json["summary"]["isFork"], false);
        assert_eq!(json["summary"]["isBot"], true);
        assert_eq!(json["summary"]["isDependabot"], true);
        assert_eq!(json["body"], "Adds a Dockerfile.");
        assert_eq!(json["files"][0]["path"], "Dockerfile");
        assert_eq!(
            json["files"][0]["hunks"][0]["lines"][0],
            "+FROM node:latest"
        );
        assert_eq!(json["files"][0]["previousPath"], Value::Null);
        assert_eq!(json["touchedFactIds"][0], "f1");
        let (status, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/p1/pull-requests/3",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["summary"]["headRepo"], "eve/repo");
        assert_eq!(json["summary"]["isFork"], true);
        assert_eq!(json["summary"]["isDependabot"], false);

        let (status, json) = send(app, "GET", "/api/v1/projects/p1/pull-requests/9", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    }

    #[tokio::test]
    async fn reviews_follow_the_role_matrix_and_publish_needs_an_approver() {
        let stack = app_with_every_fake().await;
        // A viewer cannot start a review.
        let (status, _, _) = send_as(
            stack.app.clone(),
            Some("t-viewer"),
            "POST",
            "/api/v1/projects/p1/pull-requests/1/reviews",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // A member can; the answer is the review in its first state.
        let (status, json, _) = send_as(
            stack.app.clone(),
            Some("t-member"),
            "POST",
            "/api/v1/projects/p1/pull-requests/1/reviews",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["status"], "context");
        assert_eq!(json["requestedBy"], "u1");
        assert_eq!(json["summary"], Value::Null);
        assert_eq!(json["steps"].as_array().unwrap().len(), 5);

        let (status, json) = send(
            stack.app.clone(),
            "GET",
            "/api/v1/reviews?projectId=p1",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let listed = json.as_array().unwrap();
        assert_eq!(listed.len(), 3);
        assert!(listed[0]["findings"].as_array().unwrap().is_empty());
        assert_eq!(listed[0]["summary"], Value::Null);
        // §23: the list says which executions exist without their content.
        assert_eq!(listed[0]["fixExecutionId"], Value::Null);
        assert_eq!(listed[0]["mergeExecutionId"], Value::Null);
        assert_eq!(listed[0]["fixesReady"], 0);

        let (status, json) = send(stack.app.clone(), "GET", "/api/v1/reviews/r1", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["status"], "proposed");
        assert_eq!(json["findings"].as_array().unwrap().len(), 2);
        assert_eq!(json["findings"][0]["severity"], "medium");
        assert_eq!(json["findings"][0]["decision"], "pending");
        assert_eq!(json["findings"][0]["suggestion"], "FROM node:22");
        // §23: the analyzer wrote that one; the other has no suggestion yet
        // and the model may be asked for one.
        assert_eq!(json["findings"][0]["suggestionSource"], "rule");
        assert_eq!(json["findings"][0]["canDraftFix"], false);
        assert_eq!(json["findings"][0]["fixExecutionId"], Value::Null);
        assert_eq!(json["findings"][1]["suggestion"], Value::Null);
        assert_eq!(json["findings"][1]["suggestionSource"], Value::Null);
        assert_eq!(json["findings"][1]["canDraftFix"], true);
        assert_eq!(json["fixExecutionId"], Value::Null);
        assert_eq!(json["mergeExecutionId"], Value::Null);
        assert_eq!(json["fixesReady"], 0);
        let (status, json) = send(stack.app.clone(), "GET", "/api/v1/reviews/r-ready", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["findingsAccepted"], 1);
        assert_eq!(json["fixesReady"], 1, "an accepted suggestion waits");

        let (status, json, _) = send_as(
            stack.app.clone(),
            Some("t-member"),
            "POST",
            "/api/v1/findings/f1/decide",
            Some(json!({"decision": "accepted", "note": "yes"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["decision"], "accepted");
        assert_eq!(json["decisionNote"], "yes");
        assert_eq!(json["decidedBy"], "u1");

        // Publishing is approver work.
        let (status, _, _) = send_as(
            stack.app.clone(),
            Some("t-member"),
            "POST",
            "/api/v1/reviews/r1/publish",
            Some(json!({"event": "approve"})),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, json, _) = send_as(
            stack.app.clone(),
            Some("t-approver"),
            "POST",
            "/api/v1/reviews/r1/publish",
            Some(json!({"event": "approve"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["status"], "published");
        assert!(json["publishedUrl"].as_str().unwrap().ends_with("approve"));
        // Without a body the event defaults to a comment.
        let (status, json, _) = send_as(
            stack.app.clone(),
            Some("t-approver"),
            "POST",
            "/api/v1/reviews/r1/publish",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert!(json["publishedUrl"].as_str().unwrap().ends_with("comment"));
        for _ in 0..50 {
            if *stack.recorder.last_action.lock().unwrap() == "review.publish" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            *stack.recorder.last_action.lock().unwrap(),
            "review.publish"
        );

        let (status, json) = send(
            stack.app.clone(),
            "GET",
            "/api/v1/activity?projectId=p1&before=2023-11-14T22:14:00Z",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let activity = json.as_array().unwrap();
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0]["action"], "review.started");
        assert_eq!(activity[0]["actor"], "u-member");
    }

    // --- Pull request autopilot (§23) ----------------------------------------

    /// Drafting a fix answers the finding (200) with the model's suggestion;
    /// applying fixes answers the review (201) with its fix execution. What
    /// the service refuses arrives as 412 with its sentence.
    #[tokio::test]
    async fn fixes_are_drafted_by_the_model_and_applied_through_an_execution() {
        let app = app_with_every_fake().await.app;
        let member = Some("t-member");

        // A viewer may not ask the model for anything.
        let (status, json, _) = send_as(
            app.clone(),
            Some("t-viewer"),
            "POST",
            "/api/v1/findings/f2/draft-fix",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        let (status, json, _) = send_as(
            app.clone(),
            member,
            "POST",
            "/api/v1/findings/f2/draft-fix",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["id"], "f2");
        assert_eq!(json["suggestion"], "FROM node:22-slim");
        assert_eq!(json["suggestionSource"], "model");
        assert_eq!(json["canDraftFix"], false, "it has its suggestion now");
        assert_eq!(json["fixExecutionId"], Value::Null);
        assert_eq!(json["decision"], "pending", "a draft decides nothing");
        assert_eq!(json["decidedBy"], Value::Null);
        assert_eq!(json["decidedAt"], Value::Null);
        assert_eq!(json["severity"], "medium");
        // A body is not part of the contract, and one that is sent is ignored.
        let (status, json, _) = send_as(
            app.clone(),
            member,
            "POST",
            "/api/v1/findings/f3/draft-fix",
            Some(json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["id"], "f3");

        // Refusals: the data controls keep source lines from the model, or
        // the finding cannot be drafted. Both are 412 with the sentence.
        let (status, json, _) = send_as(
            app.clone(),
            member,
            "POST",
            "/api/v1/findings/f-off/draft-fix",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{json}");
        assert_eq!(json["code"], "failed_precondition");
        let message = json["message"].as_str().unwrap();
        assert!(
            message.contains("reasoner") && message.contains("code_to_model"),
            "names the two switches: {message}"
        );
        let (status, json, _) = send_as(
            app.clone(),
            member,
            "POST",
            "/api/v1/findings/f1/draft-fix",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{json}");
        assert_eq!(json["message"], "this finding already has a suggestion");
        let (status, json, _) = send_as(
            app.clone(),
            member,
            "POST",
            "/api/v1/findings/missing/draft-fix",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{json}");

        // Applying the accepted fixes: 201, the review with its execution.
        let (status, json, _) = send_as(
            app.clone(),
            Some("t-viewer"),
            "POST",
            "/api/v1/reviews/r1/apply-fixes",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        // Committing to the repository needs an approver, like opening a
        // pull request does (§18).
        let (status, json, _) = send_as(
            app.clone(),
            member,
            "POST",
            "/api/v1/reviews/r1/apply-fixes",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        let approver = Some("t-approver");
        let (status, json, _) = send_as(
            app.clone(),
            approver,
            "POST",
            "/api/v1/reviews/r1/apply-fixes",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["id"], "r1");
        assert_eq!(json["fixExecutionId"], "x-fix-r1");
        assert_eq!(json["mergeExecutionId"], Value::Null);
        assert_eq!(json["fixesReady"], 0, "nothing is left to commit");
        assert_eq!(json["findingsAccepted"], 1);
        assert_eq!(json["status"], "proposed");
        assert_eq!(json["findings"][0]["decision"], "accepted");
        assert_eq!(json["findings"][0]["fixExecutionId"], "x-fix-r1");
        assert_eq!(json["findings"][0]["suggestionSource"], "rule");
        assert_eq!(json["findings"][1]["fixExecutionId"], Value::Null);
        let (status, json, _) = send_as(
            app.clone(),
            approver,
            "POST",
            "/api/v1/reviews/r-none/apply-fixes",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{json}");
        assert_eq!(json["code"], "failed_precondition");
        assert_eq!(
            json["message"],
            "no accepted finding has a suggestion left to commit"
        );
        let (status, json, _) = send_as(
            app,
            approver,
            "POST",
            "/api/v1/reviews/missing/apply-fixes",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    }

    /// Readiness is a read; the merge is an approver's, answers 201 with
    /// the execution and the readiness it was requested on, and a blocked
    /// pull request is a 412 carrying the first blocker's sentence.
    #[tokio::test]
    async fn merge_readiness_is_read_by_anyone_and_a_merge_needs_an_approver_and_no_blocker() {
        let app = app_with_every_fake().await.app;
        let (viewer, member, approver) = (Some("t-viewer"), Some("t-member"), Some("t-approver"));

        let (status, json, _) = send_as(
            app.clone(),
            viewer,
            "GET",
            "/api/v1/projects/p1/pull-requests/1/merge-readiness",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["projectId"], "p1");
        assert_eq!(json["prNumber"], 1);
        assert_eq!(json["title"], "PR 1");
        assert_eq!(json["state"], "open");
        assert_eq!(json["headSha"], "sha1");
        assert_eq!(json["headRef"], "feat/1");
        assert_eq!(json["baseRef"], "main");
        assert_eq!(json["isFork"], false);
        assert_eq!(json["isDependabot"], false);
        assert_eq!(json["mergeable"], "mergeable");
        assert_eq!(json["mergeableState"], "clean");
        assert_eq!(json["checksTotal"], 3);
        assert_eq!(json["checksPassed"], 3);
        assert_eq!(json["checksFailed"], 0);
        assert_eq!(json["checksPending"], 0);
        assert_eq!(json["failedChecks"], json!([]));
        assert_eq!(json["pendingChecks"], json!([]));
        assert_eq!(json["approvals"], 1);
        assert_eq!(json["changesRequested"], 0);
        assert_eq!(json["allowedMethods"], json!(["squash", "merge"]));
        assert_eq!(json["baseProtected"], true);
        assert_eq!(json["behindBy"], 0);
        assert_eq!(json["reviewId"], "r-new");
        assert_eq!(json["reviewStatus"], "proposed");
        assert_eq!(json["undecidedBlockingFindings"], 0);
        assert_eq!(json["blockers"], json!([]));
        assert_eq!(json["canMerge"], true);
        assert_eq!(json["mergeExecutionId"], Value::Null);
        assert_eq!(json["checkedAt"], "2023-11-14T22:18:20Z");

        // Blocked: the reasons in order, each with a code and a sentence,
        // and nulls where there is no review.
        let (status, json, _) = send_as(
            app.clone(),
            viewer,
            "GET",
            "/api/v1/projects/p1/pull-requests/2/merge-readiness",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["canMerge"], false);
        assert_eq!(json["isDependabot"], true);
        assert_eq!(json["mergeableState"], "unstable");
        assert_eq!(json["checksFailed"], 1);
        assert_eq!(json["checksPending"], 1);
        assert_eq!(json["failedChecks"], json!(["ci / test"]));
        assert_eq!(json["pendingChecks"], json!(["ci / lint"]));
        assert_eq!(json["behindBy"], 2);
        assert_eq!(json["reviewId"], Value::Null);
        assert_eq!(json["reviewStatus"], Value::Null);
        let blockers = json["blockers"].as_array().unwrap();
        assert_eq!(blockers.len(), 2);
        assert_eq!(blockers[0]["code"], "checks_failing");
        assert!(
            blockers[0]["message"]
                .as_str()
                .unwrap()
                .starts_with("1 check failed")
        );
        assert_eq!(blockers[1]["code"], "no_review");
        assert_eq!(
            blockers[0].as_object().unwrap().len(),
            2,
            "a blocker is {{code, message}}"
        );
        let (status, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/p1/pull-requests/9/merge-readiness",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
        let (status, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/p1/pull-requests/one/merge-readiness",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        assert_eq!(json["code"], "invalid_path");

        // Merging: a member is refused before anything is asked of GitHub.
        let merge = "/api/v1/projects/p1/pull-requests/1/merge";
        let body = json!({"method": "squash", "deleteBranch": true});
        let (status, json, _) =
            send_as(app.clone(), member, "POST", merge, Some(body.clone())).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        assert_eq!(
            json["message"], "requires the approver role in this workspace (you are member)",
            "refused by the gateway, like publishing a review"
        );
        let (status, json, _) = send_as(app.clone(), approver, "POST", merge, Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(
            json["executionId"], "x-merge-1-squash-delete",
            "method and branch handling reached the service"
        );
        assert_eq!(json["readiness"]["prNumber"], 1);
        assert_eq!(json["readiness"]["canMerge"], true);
        assert_eq!(json["readiness"]["headSha"], "sha1");
        assert_eq!(
            json["readiness"]["mergeExecutionId"],
            "x-merge-1-squash-delete"
        );
        assert_eq!(
            json.as_object().unwrap().len(),
            2,
            "the answer is {{executionId, readiness}}"
        );
        // Both fields are optional, and so is the body: the service picks
        // the method and the branch stays.
        let (status, json, _) = send_as(
            app.clone(),
            approver,
            "POST",
            merge,
            Some(json!({"method": null})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["executionId"], "x-merge-1-default-keep");
        let (status, json, _) = send_as(app.clone(), approver, "POST", merge, None).await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["executionId"], "x-merge-1-default-keep");
        let (status, json, _) = send_as(
            app.clone(),
            approver,
            "POST",
            merge,
            Some(json!({"method": "rebase", "deleteBranch": false})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["executionId"], "x-merge-1-rebase-keep");
        // A method GitHub does not have never leaves the gateway.
        let (status, json, _) = send_as(
            app.clone(),
            approver,
            "POST",
            merge,
            Some(json!({"method": "fast-forward"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        assert_eq!(json["code"], "invalid_body");

        // A blocked pull request: 412 with the first blocker's sentence.
        let (status, json, _) = send_as(
            app.clone(),
            approver,
            "POST",
            "/api/v1/projects/p1/pull-requests/2/merge",
            Some(json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{json}");
        assert_eq!(json["code"], "failed_precondition");
        assert_eq!(
            json["message"],
            "1 check failed (ci / test): fix it or re-run it, then ask again"
        );
        let (status, json, _) = send_as(
            app,
            approver,
            "POST",
            "/api/v1/projects/p1/pull-requests/9/merge",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    }

    /// The security overview says where its list came from; turning
    /// Dependabot on is an admin's and answers 201 with the execution.
    #[tokio::test]
    async fn security_overview_names_its_source_and_dependabot_is_configured_by_an_admin() {
        let app = app_with_every_fake().await.app;

        let (status, json, _) = send_as(
            app.clone(),
            Some("t-viewer"),
            "GET",
            "/api/v1/projects/p1/security",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["projectId"], "p1");
        assert_eq!(json["source"], "dependabot");
        assert_eq!(json["sourceNote"], Value::Null);
        assert_eq!(json["truncated"], false);
        assert_eq!(json["configPresent"], true);
        assert_eq!(json["configPath"], ".github/dependabot.yml");
        assert_eq!(json["configEcosystems"], json!(["npm"]));
        assert_eq!(json["alerts"], "enabled");
        assert_eq!(json["securityUpdates"], "paused");
        assert_eq!(json["openDependabotPullRequests"], 1);
        assert_eq!(json["configureExecutionId"], Value::Null);
        assert_eq!(json["checkedAt"], "2023-11-14T22:13:20Z", "from the cache");
        assert_eq!(
            json["bySeverity"],
            json!([
                {"severity": "critical", "count": 1},
                {"severity": "medium", "count": 1},
            ])
        );
        let vulnerabilities = json["vulnerabilities"].as_array().unwrap();
        assert_eq!(vulnerabilities.len(), 2);
        let first = &vulnerabilities[0];
        assert_eq!(first["source"], "dependabot");
        assert_eq!(first["id"], "GHSA-35jh-r3h4-6jhm");
        assert_eq!(first["cveId"], "CVE-2021-23337");
        assert_eq!(first["ecosystem"], "npm");
        assert_eq!(first["package"], "lodash");
        assert_eq!(first["currentVersion"], Value::Null);
        assert_eq!(first["vulnerableRange"], "< 4.17.21");
        assert_eq!(first["fixedVersion"], "4.17.21");
        assert_eq!(first["severity"], "critical");
        assert_eq!(first["summary"], "lodash is vulnerable");
        assert_eq!(first["manifestPath"], "package.json");
        assert_eq!(first["scope"], "runtime");
        assert_eq!(
            first["url"],
            "https://github.com/acme/repo/security/dependabot/7"
        );
        assert_eq!(first["fixPullRequest"], 2);
        assert_eq!(first["detectedAt"], "2023-11-03T08:26:40Z");
        // No fix yet: no version, no pull request.
        assert_eq!(vulnerabilities[1]["cveId"], Value::Null);
        assert_eq!(vulnerabilities[1]["fixedVersion"], Value::Null);
        assert_eq!(vulnerabilities[1]["fixPullRequest"], Value::Null);
        assert_eq!(vulnerabilities[1]["scope"], "development");

        // `refresh` reaches the service.
        let (status, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/p1/security?refresh=true",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["checkedAt"], "2023-11-14T22:15:00Z");
        let (status, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/p1/security?refresh=maybe",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        assert_eq!(json["code"], "invalid_query");

        // Alerts are off: the list is OSV's and the note says why.
        let (status, json) =
            send(app.clone(), "GET", "/api/v1/projects/p-osv/security", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["source"], "osv");
        assert!(json["sourceNote"].as_str().unwrap().contains("OSV"));
        assert_eq!(json["alerts"], "disabled");
        assert_eq!(json["configPresent"], false);
        assert_eq!(json["configPath"], Value::Null);
        assert_eq!(json["configEcosystems"], json!([]));
        let osv = &json["vulnerabilities"][0];
        assert_eq!(osv["source"], "osv");
        assert_eq!(osv["currentVersion"], "4.17.20");
        for field in [
            "cveId",
            "vulnerableRange",
            "fixedVersion",
            "scope",
            "url",
            "fixPullRequest",
            "detectedAt",
        ] {
            assert_eq!(osv[field], Value::Null, "{field}");
        }
        let (status, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/missing/security",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{json}");

        // Turning Dependabot on changes repository settings: admin.
        let path = "/api/v1/projects/p1/security/dependabot";
        let body = json!({"alerts": true, "securityUpdates": true, "configFile": true});
        let (status, json, _) = send_as(
            app.clone(),
            Some("t-approver"),
            "POST",
            path,
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        assert_eq!(
            json["message"], "requires the admin role in this workspace (you are approver)",
            "refused by the gateway, like editing an environment"
        );
        let (status, json, _) =
            send_as(app.clone(), Some("t-admin"), "POST", path, Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(
            json,
            json!({"executionId": "x-dependabot-p1-alerts+updates+config"})
        );
        // Each switch is optional and travels on its own.
        let (status, json) = send(
            app.clone(),
            "POST",
            path,
            Some(json!({"configFile": true, "alerts": null})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["executionId"], "x-dependabot-p1-config");
        let (status, json) = send(
            app.clone(),
            "POST",
            path,
            Some(json!({"securityUpdates": true})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["executionId"], "x-dependabot-p1-updates");
        // Asking for nothing is the service's 400, body or no body.
        for nothing in [Some(json!({})), Some(json!({"alerts": false})), None] {
            let (status, json) = send(app.clone(), "POST", path, nothing).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
            assert_eq!(json["code"], "invalid_argument");
            assert!(json["message"].as_str().unwrap().contains("at least one"));
        }
    }

    /// CONTRACT.md §23.6: the four requests are audited under their own
    /// actions with the entity they name, and nothing of what was drafted
    /// or committed is in the entry.
    #[tokio::test]
    async fn autopilot_requests_are_audited_with_their_entity() {
        let stack = app_with_every_fake().await;
        let app = stack.app.clone();
        for (path, body, expected) in [
            ("/api/v1/findings/f2/draft-fix", None, StatusCode::OK),
            ("/api/v1/reviews/r1/apply-fixes", None, StatusCode::CREATED),
            (
                "/api/v1/projects/p1/pull-requests/1/merge",
                Some(json!({"method": "squash"})),
                StatusCode::CREATED,
            ),
            (
                "/api/v1/projects/p1/security/dependabot",
                Some(json!({"alerts": true})),
                StatusCode::CREATED,
            ),
            // Refused requests are part of the trail too.
            (
                "/api/v1/projects/p1/pull-requests/2/merge",
                None,
                StatusCode::PRECONDITION_FAILED,
            ),
            (
                "/api/v1/findings/f-off/draft-fix",
                None,
                StatusCode::PRECONDITION_FAILED,
            ),
        ] {
            let (status, json) = send(app.clone(), "POST", path, body).await;
            assert_eq!(status, expected, "{path}: {json}");
        }
        // Reads leave no entry.
        for path in [
            "/api/v1/projects/p1/pull-requests/1/merge-readiness",
            "/api/v1/projects/p1/security",
        ] {
            let (status, json) = send(app.clone(), "GET", path, None).await;
            assert_eq!(status, StatusCode::OK, "{path}: {json}");
        }

        let entries = audited(&stack.recorder, 6).await;
        let mut seen: Vec<(&str, &str, &str, &str)> = entries
            .iter()
            .map(|e| {
                (
                    e.action.as_str(),
                    e.entity.as_str(),
                    e.entity_id.as_str(),
                    e.detail.as_str(),
                )
            })
            .collect();
        seen.sort_unstable();
        assert_eq!(
            seen,
            [
                ("pull_request.merge_requested", "pull_request", "1", ""),
                (
                    "pull_request.merge_requested",
                    "pull_request",
                    "2",
                    "http 412"
                ),
                ("review.fix_drafted", "finding", "f-off", "http 412"),
                ("review.fix_drafted", "finding", "f2", ""),
                ("review.fixes_requested", "review", "r1", ""),
                ("security.dependabot_requested", "project", "p1", ""),
            ]
        );
        for entry in &entries {
            assert_eq!(entry.workspace_id, "w1");
            assert_eq!(entry.user_id, "u1");
            assert_eq!(entry.user_login, "octo");
            // Neither the drafted lines nor a refusal's sentence is kept.
            assert!(!entry.detail.contains("node"), "{entry:?}");
        }
    }

    #[tokio::test]
    async fn webhook_pull_request_starts_a_review_only_with_auto_review_on() {
        let opened = json!({
            "action": "opened",
            "installation": {"id": 777},
            "organization": {"login": "acme"},
            "repository": {"full_name": "acme/repo", "default_branch": "main"},
            "pull_request": {"number": 1, "draft": false}
        });
        fakes::intelligence::AUTO_REVIEW.store(false, std::sync::atomic::Ordering::SeqCst);
        let (status, json) = webhook(
            app_with_every_fake().await,
            "pull_request",
            &opened,
            "hook-secret",
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["outcome"], "ignored");

        fakes::intelligence::AUTO_REVIEW.store(true, std::sync::atomic::Ordering::SeqCst);
        let (status, json) = webhook(
            app_with_every_fake().await,
            "pull_request",
            &opened,
            "hook-secret",
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["outcome"], "review_started");

        // Drafts, closes and unknown repositories never start one.
        let mut draft = opened.clone();
        draft["pull_request"]["draft"] = json!(true);
        let (_, json) = webhook(
            app_with_every_fake().await,
            "pull_request",
            &draft,
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "ignored");
        let mut closed = opened.clone();
        closed["action"] = json!("closed");
        let (_, json) = webhook(
            app_with_every_fake().await,
            "pull_request",
            &closed,
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "ignored");
        let mut other = opened.clone();
        other["repository"]["full_name"] = json!("acme/other");
        let (_, json) = webhook(
            app_with_every_fake().await,
            "pull_request",
            &other,
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "ignored");
        fakes::intelligence::AUTO_REVIEW.store(false, std::sync::atomic::Ordering::SeqCst);
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
        // The first bytes are a comment, so a proxy that waits for a body
        // byte lets the response through before the first event exists.
        assert!(text.starts_with(": open\n\n"), "{text}");
    }

    /// A stream with nothing to say yet still answers at once: the opening
    /// comment arrives while every upstream is silent (here: unreachable),
    /// so the browser's EventSource leaves CONNECTING without waiting for
    /// the first keep-alive.
    #[tokio::test]
    async fn event_streams_open_with_a_comment_before_any_event() {
        let stack = stack(DEAD, DEAD, DEAD).await;
        let response = stack
            .app
            .oneshot(request("GET", "/api/v1/events/stream", Some(OWNER), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut chunks = response.into_body().into_data_stream();
        let first = tokio::time::timeout(Duration::from_secs(2), chunks.next())
            .await
            .expect("an opening chunk within 2s")
            .expect("the stream stays open")
            .expect("no body error");
        assert_eq!(&first[..], b": open\n\n");
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
            has(seen, "scan")
                && has(seen, "proposal")
                && has(seen, "execution")
                && has(seen, "environment")
        })
        .await;
        // The delivery entities of CONTRACT.md §22 pass like any other.
        let deployment = events.iter().find(|e| e["entity"] == "deployment").unwrap();
        assert_eq!(deployment["entityId"], "d1");
        assert_eq!(deployment["status"], "awaiting_approval");
        assert_eq!(deployment["data"]["workspace_id"], "w1");
        let release = events.iter().find(|e| e["entity"] == "release").unwrap();
        assert_eq!(release["action"], "created");
        assert!(has(&events, "environment"), "{events:?}");
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

    /// `SSE_MAX_PER_WORKSPACE` open streams per replica; the next one is
    /// 429 until one closes, and closing one frees its slot.
    #[tokio::test]
    async fn events_stream_is_capped_per_workspace_and_slots_are_released() {
        let stack = stack(DEAD, DEAD, DEAD).await;
        let open = |app: Router| async move {
            app.oneshot(request("GET", "/api/v1/events/stream", Some(OWNER), None))
                .await
                .unwrap()
        };
        let mut held = Vec::new();
        for _ in 0..3 {
            let response = open(stack.app.clone()).await;
            assert_eq!(response.status(), StatusCode::OK);
            held.push(response);
        }
        assert_eq!(stack.state.open_streams(fakes::WORKSPACE), 3);

        let response = open(stack.app.clone()).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["code"], "resource_exhausted");

        // A scan stream counts against the same cap.
        let (status, _, _) = send_as(
            stack.app.clone(),
            Some(OWNER),
            "GET",
            "/api/v1/scans/s1/stream",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

        // Dropping a response drops its stream and frees the slot.
        drop(held.pop());
        tokio::task::yield_now().await;
        assert_eq!(stack.state.open_streams(fakes::WORKSPACE), 2);
        let response = open(stack.app.clone()).await;
        assert_eq!(response.status(), StatusCode::OK);
        held.push(response);
        drop(held);
        tokio::task::yield_now().await;
        assert_eq!(stack.state.open_streams(fakes::WORKSPACE), 0);
    }

    /// The authorize cache never grows past `GATEWAY_AUTH_CACHE_MAX`
    /// entries, however many sessions a replica sees.
    #[tokio::test]
    async fn authorize_cache_is_bounded() {
        let stack = stack(DEAD, DEAD, DEAD).await;
        for token in ["t-owner", "t-admin", "t-approver", "t-member", "t-viewer"] {
            for workspace in ["", "w1"] {
                let uri = if workspace.is_empty() {
                    "/api/v1/auth/me".to_owned()
                } else {
                    format!("/api/v1/auth/me?workspace={workspace}")
                };
                // Knowledge is dead, so /auth/me fails after authorising;
                // the cache entry is written on the way in regardless.
                let _ = send_as(stack.app.clone(), Some(token), "GET", &uri, None).await;
            }
        }
        assert!(
            stack.state.cached_auths() <= 4,
            "cache holds {} entries",
            stack.state.cached_auths()
        );
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
        // Pull request autopilot (§23.6): the defaults and the watcher.
        assert_eq!(json["intelligence"]["autoDraftFixes"], false);
        assert_eq!(json["intelligence"]["autoMerge"], "off");
        assert_eq!(json["intelligence"]["mergeRequiresReview"], true);
        assert_eq!(json["intelligence"]["prWatchIntervalSeconds"], 60);
        assert_eq!(
            json["intelligence"]["prWatchLastRunAt"],
            "2023-11-14T22:13:20Z"
        );
        assert_eq!(json["intelligence"]["prWatchLastError"], Value::Null);

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

    /// The reasoner test is an admin's question, and a misconfiguration is
    /// an answer (200 with a status and a sentence), not an error.
    #[tokio::test]
    async fn reasoner_test_reports_what_the_model_call_did() {
        let app = app_with_every_fake().await.app;
        let (status, json, _) = send_as(
            app.clone(),
            Some("t-approver"),
            "POST",
            "/api/v1/settings/reasoner/test",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        let (status, json) = send(app, "POST", "/api/v1/settings/reasoner/test", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["provider"], "openai_compatible");
        assert_eq!(json["model"], "test-model");
        assert_eq!(json["endpoint"], "https://api.openai.com");
        assert_eq!(json["latencyMs"], 412);
        assert_eq!(json["inputTokens"], 21);
        assert_eq!(json["outputTokens"], 3);
        assert!(json["message"].as_str().unwrap().contains("answered"));
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
        // The fake echoes what it was sent: an unpatched model travels as
        // empty ("keep what is stored"), never as the effective value read.
        assert_eq!(json["intelligence"]["model"], "");
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

    /// CONTRACT.md §23.6: the three autopilot switches go through the
    /// existing settings route with its partial-update semantics; what the
    /// watcher reports about itself cannot be written.
    #[tokio::test]
    async fn autopilot_settings_round_trip_and_the_watcher_fields_are_read_only() {
        let app = app_with_every_fake().await.app;
        let (status, json) = send(
            app.clone(),
            "PATCH",
            "/api/v1/settings",
            Some(json!({"intelligence": {"autoMerge": "dependabot"}})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let intelligence = &json["intelligence"];
        assert_eq!(intelligence["autoMerge"], "dependabot");
        assert_eq!(intelligence["autoDraftFixes"], false, "unset field kept");
        assert_eq!(
            intelligence["mergeRequiresReview"], true,
            "an unset switch does not travel as false"
        );
        assert_eq!(intelligence["reasoner"], "heuristic");
        assert_eq!(json["knowledge"]["scanMaxFiles"], 2000);

        let (status, json) = send(
            app.clone(),
            "PATCH",
            "/api/v1/settings",
            Some(json!({"intelligence": {
                "autoDraftFixes": true,
                "mergeRequiresReview": false,
                "prWatchIntervalSeconds": 5,
                "prWatchLastRunAt": null,
                "prWatchLastError": "edited",
            }})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let intelligence = &json["intelligence"];
        assert_eq!(intelligence["autoDraftFixes"], true);
        assert_eq!(intelligence["mergeRequiresReview"], false);
        assert_eq!(
            intelligence["autoMerge"], "off",
            "each patch starts from what is stored"
        );
        // The fake echoes what the gateway sent: the read-only fields
        // travelled as they were read, not as the body tried to set them.
        assert_eq!(intelligence["prWatchIntervalSeconds"], 60);
        assert_eq!(intelligence["prWatchLastRunAt"], "2023-11-14T22:13:20Z");
        assert_eq!(intelligence["prWatchLastError"], Value::Null);

        // A switch with the wrong type is a 400, not a silent false.
        let (status, json) = send(
            app.clone(),
            "PATCH",
            "/api/v1/settings",
            Some(json!({"intelligence": {"mergeRequiresReview": "no"}})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        assert_eq!(json["code"], "invalid_body");

        // Settings are an admin's.
        let (status, _, _) = send_as(
            app,
            Some("t-approver"),
            "PATCH",
            "/api/v1/settings",
            Some(json!({"intelligence": {"autoMerge": "dependabot"}})),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
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

    // --- Continuous delivery (CONTRACT.md §22) -------------------------------

    /// The shapes the web app is written against: flat bodies, bare
    /// arrays, camelCase, `null` for what is not set.
    #[tokio::test]
    async fn delivery_routes_speak_flat_bodies_and_bare_arrays() {
        let app = app_with_fake_execution().await;

        let (status, json) = send(app.clone(), "GET", "/api/v1/registries", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let registries = json.as_array().expect("a bare array");
        assert_eq!(registries[0]["host"], "ghcr.io");
        assert_eq!(registries[0]["isDefault"], true);
        assert_eq!(registries[0]["usernameSecret"], Value::Null);
        assert_eq!(registries[0]["createdAt"], "2023-11-14T22:13:20Z");

        let body = json!({
            "name": "acr", "kind": "acr", "host": "acme.azurecr.io", "namespace": "shop",
            "auth": "secret", "usernameSecret": "ACR_USER", "passwordSecret": "ACR_PASSWORD",
            "isDefault": false,
        });
        let (status, json) = send(
            app.clone(),
            "POST",
            "/api/v1/registries",
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["id"], "r-new");
        assert_eq!(json["passwordSecret"], "ACR_PASSWORD");
        let (status, json) = send(
            app.clone(),
            "PUT",
            "/api/v1/registries/r7",
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["id"], "r7", "the path names what is updated");
        // A wrapped body is not the contract.
        let (status, json) = send(
            app.clone(),
            "POST",
            "/api/v1/registries",
            Some(json!({"registry": body})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        assert_eq!(json["code"], "invalid_body");
        // The service's refusals pass through with their message.
        let mut taken = body.clone();
        taken["name"] = json!("taken");
        let (status, json) = send(app.clone(), "POST", "/api/v1/registries", Some(taken)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{json}");
        assert!(json["message"].as_str().unwrap().starts_with("name: "));
        let (status, json) = send(app.clone(), "DELETE", "/api/v1/registries/r1", None).await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{json}");
        let (status, _) = send(app.clone(), "DELETE", "/api/v1/registries/r7", None).await;
        assert_eq!(status, StatusCode::OK);

        // Environments.
        let (status, json) =
            send(app.clone(), "GET", "/api/v1/projects/p1/environments", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let environments = json.as_array().expect("a bare array");
        assert_eq!(environments[0]["projectId"], "p1");
        assert_eq!(environments[0]["healthPath"], "/health");
        assert_eq!(environments[0]["autoPromote"], false);
        assert_eq!(environments[0]["registryId"], Value::Null);
        assert_eq!(environments[0]["currentReleaseId"], "rel1");
        assert_eq!(environments[0]["config"]["workflow"], "deploy.yml");
        let environment = json!({
            "name": "production", "kind": "webhook",
            "config": {"url": "https://deploy.example.com/hook", "secret": "HOOK_KEY"},
            "url": "https://api.example.com", "healthPath": "/health", "registryId": null,
            "protected": true, "autoPromote": true, "position": 1,
        });
        let (status, json) = send(
            app.clone(),
            "POST",
            "/api/v1/projects/p9/environments",
            Some(environment.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["id"], "e-new");
        assert_eq!(json["projectId"], "p9", "the path names the project");
        assert_eq!(json["position"], 1);
        assert_eq!(json["protected"], true);
        assert_eq!(json["config"]["secret"], "HOOK_KEY");
        assert_eq!(json["source"], "ui");
        // An edit that does not say where the stage goes leaves that to the
        // service (which keeps it in place): the position travels as unset.
        let mut edit = environment.clone();
        edit.as_object_mut().unwrap().remove("position");
        let (status, json) = send(app.clone(), "PUT", "/api/v1/environments/e7", Some(edit)).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["id"], "e7");
        assert_eq!(json["position"], u32::MAX, "{json}");
        // A refused setting names its key in the message the editor shows.
        let (status, json) = send(
            app.clone(),
            "POST",
            "/api/v1/projects/p1/environments",
            Some(json!({"name": "staging", "kind": "gitops_flux", "config": {}})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        assert_eq!(json["code"], "invalid_argument");
        assert_eq!(
            json["message"],
            "config.path is required for gitops_flux environments"
        );
        let (status, json) = send(
            app.clone(),
            "PUT",
            "/api/v1/environments/from-config",
            Some(environment.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{json}");
        assert!(json["message"].as_str().unwrap().contains(".sdi.yml"));
        let (status, _) = send(app.clone(), "DELETE", "/api/v1/environments/e7", None).await;
        assert_eq!(status, StatusCode::OK);
        let (status, json) =
            send(app.clone(), "DELETE", "/api/v1/environments/missing", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{json}");

        // The pipeline, with the names the board reads.
        let (status, json) = send(app.clone(), "GET", "/api/v1/projects/p1/pipeline", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["projectId"], "p1");
        assert_eq!(json["successRate"], 0.75);
        assert_eq!(json["deployments30d"], 8);
        assert_eq!(json["leadTimeSeconds"], 5400);
        assert_eq!(json["registry"]["name"], "ghcr");
        let stages = json["stages"].as_array().unwrap();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0]["environment"]["name"], "staging");
        assert_eq!(stages[0]["current"]["id"], "rel1");
        assert_eq!(stages[0]["current"]["deployedTo"], json!(["staging"]));
        assert_eq!(stages[0]["lastRun"]["status"], "awaiting_approval");
        assert_eq!(stages[0]["lastRun"]["executionId"], "x1");
        assert_eq!(stages[0]["candidate"]["id"], "rel2");
        assert_eq!(stages[0]["health"], "healthy");
        assert_eq!(stages[1]["environment"]["source"], "config");
        assert_eq!(stages[1]["environment"]["url"], Value::Null);
        assert_eq!(stages[1]["current"], Value::Null);
        assert_eq!(stages[1]["lastRun"], Value::Null);
        assert_eq!(stages[1]["candidate"], Value::Null);
        assert_eq!(stages[1]["health"], "unknown");

        // Releases.
        let (status, json) = send(app.clone(), "GET", "/api/v1/projects/p1/releases", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let releases = json.as_array().expect("a bare array");
        assert_eq!(releases.len(), 2);
        assert_eq!(
            releases[0]["commitSha"],
            "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678"
        );
        assert_eq!(releases[0]["digest"], Value::Null);
        assert_eq!(releases[0]["pipelineRunId"], Value::Null);
        assert_eq!(releases[0]["verified"], false);
        let (_, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/p1/releases?limit=1",
            None,
        )
        .await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        let (_, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/p1/releases?before=2023-11-14T22:13:20Z",
            None,
        )
        .await;
        assert_eq!(json, json!([]), "the cursor reached the service");
        let (status, json) = send(app.clone(), "GET", "/api/v1/releases/rel1", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["notes"], "- Add health endpoint (a1b2c3d)");
        let (status, _) = send(app.clone(), "GET", "/api/v1/releases/missing", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // A manual release: both fields optional, `null` is "not said".
        let (status, json) = send(
            app.clone(),
            "POST",
            "/api/v1/projects/p1/releases",
            Some(json!({"commitSha": null, "version": "v1.4.2"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["version"], "v1.4.2");
        assert_eq!(json["source"], "manual");
        assert_eq!(json["createdBy"], "u1");
        let (status, json) = send(app.clone(), "POST", "/api/v1/projects/p1/releases", None).await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "no body is the default branch head: {json}"
        );
        let (status, json) = send(
            app.clone(),
            "POST",
            "/api/v1/projects/p1/releases",
            Some(json!({"commitSha": "0000000"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        assert!(json["message"].as_str().unwrap().starts_with("commit_sha"));

        // Promote and rollback answer the run, with its execution.
        let (status, json) = send(
            app.clone(),
            "POST",
            "/api/v1/environments/e1/promote",
            Some(json!({"releaseId": null})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["kind"], "promote");
        assert_eq!(json["environmentId"], "e1");
        assert_eq!(
            json["releaseId"], "candidate",
            "no release: the stage's candidate"
        );
        assert_eq!(json["executionId"], "x1");
        assert_eq!(json["status"], "awaiting_approval");
        assert_eq!(json["externalUrl"], Value::Null);
        assert_eq!(json["finishedAt"], Value::Null);
        let (status, json) = send(
            app.clone(),
            "POST",
            "/api/v1/environments/e1/promote",
            Some(json!({"releaseId": "rel2"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["releaseId"], "rel2");
        let (status, json) =
            send(app.clone(), "POST", "/api/v1/environments/e1/promote", None).await;
        assert_eq!(status, StatusCode::CREATED, "a body is optional: {json}");
        let (status, json) = send(
            app.clone(),
            "POST",
            "/api/v1/environments/busy/promote",
            Some(json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{json}");
        assert!(json["message"].as_str().unwrap().contains("still open"));
        let (status, json) = send(
            app.clone(),
            "POST",
            "/api/v1/environments/e1/rollback",
            Some(json!({"releaseId": null, "reason": "login is broken"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["kind"], "rollback");
        assert_eq!(json["releaseId"], "previous");
        assert_eq!(json["previousReleaseId"], "rel2");
        assert_eq!(json["executionId"], "x1");
        assert_eq!(
            json["externalUrl"], "login is broken",
            "the reason reached the service"
        );

        // Deployment runs.
        let (status, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/p1/deployment-runs",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json.as_array().expect("a bare array").len(), 2);
        let (_, json) = send(
            app.clone(),
            "GET",
            "/api/v1/projects/p1/deployment-runs?environmentId=e9&limit=1",
            None,
        )
        .await;
        assert_eq!(json, json!([]), "the filter reached the service");
        let (status, json) = send(app.clone(), "GET", "/api/v1/deployment-runs/d1", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["status"], "succeeded");
        assert_eq!(json["durationSeconds"], 90);
        assert_eq!(json["finishedAt"], "2023-11-14T22:14:50Z");
        assert_eq!(
            json["externalUrl"],
            "https://github.com/acme/repo/actions/runs/7"
        );
        let (status, _) = send(app, "GET", "/api/v1/deployment-runs/missing", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    /// Reads viewer, create release member, promote and rollback approver,
    /// registries and environment edits admin; the role travels on.
    #[tokio::test]
    async fn delivery_routes_follow_the_role_matrix() {
        let app = app_with_fake_execution().await;
        let (viewer, member, approver, admin) = (
            Some("t-viewer"),
            Some("t-member"),
            Some("t-approver"),
            Some("t-admin"),
        );
        for path in [
            "/api/v1/registries",
            "/api/v1/projects/p1/environments",
            "/api/v1/projects/p1/pipeline",
            "/api/v1/projects/p1/releases",
            "/api/v1/releases/rel1",
            "/api/v1/projects/p1/deployment-runs",
            "/api/v1/deployment-runs/d1",
        ] {
            let (status, json, _) = send_as(app.clone(), viewer, "GET", path, None).await;
            assert_eq!(status, StatusCode::OK, "{path}: {json}");
        }
        let release = Some(json!({}));
        let (status, _, _) = send_as(
            app.clone(),
            viewer,
            "POST",
            "/api/v1/projects/p1/releases",
            release.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, json, _) = send_as(
            app.clone(),
            member,
            "POST",
            "/api/v1/projects/p1/releases",
            release,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");

        for path in [
            "/api/v1/environments/e1/promote",
            "/api/v1/environments/e1/rollback",
        ] {
            let (status, json, _) =
                send_as(app.clone(), member, "POST", path, Some(json!({}))).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {json}");
            assert!(json["message"].as_str().unwrap().contains("approver"));
            let (status, json, _) =
                send_as(app.clone(), approver, "POST", path, Some(json!({}))).await;
            assert_eq!(status, StatusCode::CREATED, "{path}: {json}");
            assert_eq!(json["executionId"], "x1");
        }

        let registry = json!({"name": "acr", "kind": "acr"});
        let environment = json!({"name": "staging", "kind": "github_actions"});
        for (method, path, body) in [
            ("POST", "/api/v1/registries", Some(registry.clone())),
            ("PUT", "/api/v1/registries/r7", Some(registry.clone())),
            ("DELETE", "/api/v1/registries/r7", None),
            (
                "POST",
                "/api/v1/projects/p1/environments",
                Some(environment.clone()),
            ),
            ("PUT", "/api/v1/environments/e7", Some(environment.clone())),
            ("DELETE", "/api/v1/environments/e7", None),
        ] {
            let (status, json, _) =
                send_as(app.clone(), approver, method, path, body.clone()).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}: {json}");
            let (status, json, _) = send_as(app.clone(), admin, method, path, body).await;
            assert!(status.is_success(), "{method} {path}: {status} {json}");
        }
    }

    /// The setup files come from the intelligence service; while it does
    /// not generate them the route says 501, not an opaque 500.
    #[tokio::test]
    async fn delivery_artifacts_pass_an_unimplemented_generator_through_as_501() {
        let (status, json) = send(
            app_with_fake_intelligence().await,
            "GET",
            "/api/v1/projects/p1/delivery/artifacts",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{json}");
        assert_eq!(json["code"], "unimplemented");
        assert!(json["message"].as_str().unwrap().contains("setup files"));
        // Other failures keep their mapping.
        let (status, json) = send(
            app().await,
            "GET",
            "/api/v1/projects/p1/delivery/artifacts",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{json}");
    }

    /// Workspace secrets: an admin stores, lists and deletes them; a value
    /// goes in and never comes out; the export names them; deleting the
    /// workspace removes them.
    #[tokio::test]
    async fn workspace_secrets_are_write_only_exported_by_name_and_deleted_with_the_workspace() {
        let stack = app_with_every_fake().await;
        let app = stack.app.clone();
        let value = "jenkins-api-token-0123456789-wxyz";

        let (status, json) = send(app.clone(), "GET", "/api/v1/workspaces/w1/secrets", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json, json!([]), "a bare array");

        let (status, json) = send(
            app.clone(),
            "PUT",
            "/api/v1/workspaces/w1/secrets/JENKINS_TOKEN",
            Some(json!({"value": value, "description": "CI trigger token"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["name"], "JENKINS_TOKEN");
        assert_eq!(json["hint"], "wxyz");
        assert_eq!(json["description"], "CI trigger token");
        assert_eq!(json["updatedBy"], "octo");
        assert!(json.get("value").is_none());
        assert!(!json.to_string().contains(value));
        // The value reached identity (and only its length was kept there).
        assert_eq!(
            *stack.recorder.secret_value_lengths.lock().unwrap(),
            [value.len()]
        );
        // `description` is optional.
        let (status, json) = send(
            app.clone(),
            "PUT",
            "/api/v1/workspaces/w1/secrets/DEPLOY_HOOK_SECRET",
            Some(json!({"value": "whsec_abcdef0123456789"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["description"], Value::Null);
        // Identity's refusals name the field.
        let (status, json) = send(
            app.clone(),
            "PUT",
            "/api/v1/workspaces/w1/secrets/jenkins",
            Some(json!({"value": value})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .starts_with("name must be")
        );
        let (status, json) = send(
            app.clone(),
            "PUT",
            "/api/v1/workspaces/w1/secrets/EMPTY_ONE",
            Some(json!({"value": ""})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        assert!(json["message"].as_str().unwrap().starts_with("value must"));
        let (status, json) = send(
            app.clone(),
            "PUT",
            "/api/v1/workspaces/w1/secrets/NO_VALUE",
            Some(json!({"description": "forgot the value"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");

        let (status, json) = send(app.clone(), "GET", "/api/v1/workspaces/w1/secrets", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let names: Vec<&str> = json
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["DEPLOY_HOOK_SECRET", "JENKINS_TOKEN"]);
        assert!(!json.to_string().contains(value));

        // Admins only, also to read the names; another workspace's are 403.
        for (method, path, body) in [
            ("GET", "/api/v1/workspaces/w1/secrets", None),
            (
                "PUT",
                "/api/v1/workspaces/w1/secrets/JENKINS_TOKEN",
                Some(json!({"value": value})),
            ),
            (
                "DELETE",
                "/api/v1/workspaces/w1/secrets/JENKINS_TOKEN",
                None,
            ),
        ] {
            let (status, json, _) =
                send_as(app.clone(), Some("t-approver"), method, path, body.clone()).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}: {json}");
            let (status, _, _) = send_as(app.clone(), None, method, path, body).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
        }
        let (status, _) = send(app.clone(), "GET", "/api/v1/workspaces/w2/secrets", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, json, _) = send_as(
            app.clone(),
            Some("t-admin"),
            "GET",
            "/api/v1/workspaces/w1/secrets",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");

        // The export lists the names, never a value.
        let response = app
            .clone()
            .oneshot(request(
                "POST",
                "/api/v1/workspaces/w1/data/export",
                Some(OWNER),
                None,
            ))
            .await
            .unwrap();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        let export: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(
            export["secrets"],
            json!(["DEPLOY_HOOK_SECRET", "JENKINS_TOKEN"])
        );
        assert!(!text.contains(value));
        assert!(!text.contains("wxyz"), "not even the hint");

        // Deleting one, then the workspace: the rest goes with it.
        let (status, json) = send(
            app.clone(),
            "DELETE",
            "/api/v1/workspaces/w1/secrets/JENKINS_TOKEN",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let (status, json) = send(
            app.clone(),
            "DELETE",
            "/api/v1/workspaces/w1/secrets/JENKINS_TOKEN",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
        let (status, json) = send(app.clone(), "DELETE", "/api/v1/workspaces/w1", None).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["deleted"]["identity"]["secret"], 1);
        assert!(stack.recorder.secrets.lock().unwrap().is_empty());
    }

    fn workflow_run(repo: &str, workflow: &str, sha: &str, run_id: u64) -> Value {
        json!({
            "action": "completed",
            "installation": {"id": 777},
            "organization": {"login": "acme"},
            "repository": {"full_name": repo, "default_branch": "main"},
            "workflow_run": {
                "id": run_id,
                "name": workflow.trim_end_matches(".yml"),
                "path": format!(".github/workflows/{workflow}"),
                "head_branch": "main",
                "head_sha": sha,
                "event": "push",
                "status": "completed",
                "conclusion": "success",
            }
        })
    }

    /// What the fake execution was asked to create for `marker` (a commit
    /// SHA or tag only this test uses), with who asked.
    fn created_releases(
        marker: &str,
    ) -> Vec<(String, sdi_proto::execution::v1::CreateReleaseRequest)> {
        fakes::execution::RELEASES
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, request)| request.commit_sha == marker || request.tag == marker)
            .cloned()
            .collect()
    }

    /// A build workflow succeeding on the default branch cuts a release as
    /// `system`: the workflow `.sdi.yml` names, else one the scan found to
    /// build an image. Promoting it is the execution service's business.
    #[tokio::test]
    async fn webhook_build_success_creates_a_release_for_the_build_workflow_only() {
        let sha = "1111111111111111111111111111111111111111";
        let (status, json) = webhook(
            app_with_every_fake().await,
            "workflow_run",
            &workflow_run("acme/repo", "ci.yml", sha, 8841),
            "hook-secret",
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["outcome"], "release_created");
        let created = created_releases(sha);
        assert_eq!(created.len(), 1);
        let (user, request) = &created[0];
        assert_eq!(user, "system");
        assert_eq!(request.project_id, "p1");
        assert_eq!(request.source, "pipeline");
        assert_eq!(request.pipeline_run_id, "8841");
        assert!(
            request.tag.is_empty() && request.image.is_empty(),
            "the service derives what the event does not say"
        );

        // The deploy workflow SDI dispatched itself builds nothing.
        let other = "2222222222222222222222222222222222222222";
        let (_, json) = webhook(
            app_with_every_fake().await,
            "workflow_run",
            &workflow_run("acme/repo", "deploy.yml", other, 8842),
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "ignored");
        assert!(created_releases(other).is_empty());

        // `.sdi.yml` names release.yml: ci.yml no longer counts, even
        // though the scan saw it build an image.
        let configured = "3333333333333333333333333333333333333333";
        let (_, json) = webhook(
            app_with_every_fake().await,
            "workflow_run",
            &workflow_run("acme/configured", "ci.yml", configured, 8843),
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "ignored");
        let (_, json) = webhook(
            app_with_every_fake().await,
            "workflow_run",
            &workflow_run("acme/configured", "release.yml", configured, 8844),
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "release_created");
        let created = created_releases(configured);
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].1.project_id, "p-configured");
        assert_eq!(created[0].1.pipeline_run_id, "8844");

        // Not the default branch, not a success, not a connected repository.
        let mut feature = workflow_run("acme/repo", "ci.yml", sha, 1);
        feature["workflow_run"]["head_branch"] = json!("feature/x");
        let mut failed = workflow_run("acme/repo", "ci.yml", sha, 2);
        failed["workflow_run"]["conclusion"] = json!("failure");
        for body in [
            feature,
            failed,
            workflow_run("acme/unknown", "ci.yml", sha, 3),
        ] {
            let (status, json) = webhook(
                app_with_every_fake().await,
                "workflow_run",
                &body,
                "hook-secret",
            )
            .await;
            assert_eq!(status, StatusCode::ACCEPTED, "{json}");
            assert_eq!(json["outcome"], "ignored", "{body}");
        }
        assert_eq!(created_releases(sha).len(), 1, "nothing more was created");
    }

    /// A container image published to the registry is a release with the
    /// tag and digest the event carries.
    #[tokio::test]
    async fn webhook_published_package_creates_a_release_with_its_tag_and_digest() {
        let digest = format!("sha256:{}", "c".repeat(64));
        let tag = "pkg-7f3a9c1";
        let event = |key: &str| {
            json!({
                "action": "published",
                "installation": {"id": 777},
                "organization": {"login": "acme"},
                "repository": {"full_name": "acme/repo"},
                key: {
                    "name": "repo",
                    "namespace": "acme",
                    "package_type": "CONTAINER",
                    "package_version": {
                        "version": digest,
                        "target_oid": "4444444444444444444444444444444444444444",
                        "package_url": format!("ghcr.io/acme/repo:{tag}"),
                        "container_metadata": {"tag": {"name": tag, "digest": digest}},
                    }
                }
            })
        };
        let (status, json) = webhook(
            app_with_every_fake().await,
            "registry_package",
            &event("registry_package"),
            "hook-secret",
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["outcome"], "release_created");
        let (_, json) = webhook(
            app_with_every_fake().await,
            "package",
            &event("package"),
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "release_created");
        let created = created_releases(tag);
        assert_eq!(
            created.len(),
            2,
            "both events are forwarded; the service is idempotent"
        );
        for (user, request) in &created {
            assert_eq!(user, "system");
            assert_eq!(request.project_id, "p1");
            assert_eq!(request.source, "package");
            assert_eq!(request.image, "ghcr.io/acme/repo");
            assert_eq!(request.tag, tag);
            assert_eq!(request.digest, digest);
            assert_eq!(
                request.commit_sha,
                "4444444444444444444444444444444444444444"
            );
        }

        // An untagged manifest and an unknown repository are nothing.
        let mut untagged = event("registry_package");
        untagged["registry_package"]["package_version"]["container_metadata"]["tag"]["name"] =
            json!("");
        let (_, json) = webhook(
            app_with_every_fake().await,
            "registry_package",
            &untagged,
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "ignored");
        let mut elsewhere = event("registry_package");
        elsewhere["repository"]["full_name"] = json!("acme/unknown");
        let (_, json) = webhook(
            app_with_every_fake().await,
            "registry_package",
            &elsewhere,
            "hook-secret",
        )
        .await;
        assert_eq!(json["outcome"], "ignored");
        // The execution service down: accepted, the failure is recorded.
        let knowledge = fakes::knowledge::serve().await;
        let (status, json) = webhook(
            stack(&format!("http://{knowledge}"), DEAD, DEAD).await,
            "registry_package",
            &event("registry_package"),
            "hook-secret",
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{json}");
        assert_eq!(json["outcome"], "failed");
    }

    /// A call the service behind the route does not have (an older build
    /// than the gateway) says so, with the remedy and without the service's
    /// own words.
    #[tokio::test]
    async fn unimplemented_rpc_is_a_501_that_names_the_remedy() {
        let (status, json) = send(
            app_with_fake_execution().await,
            "GET",
            "/api/v1/policies",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(json["code"], "unimplemented");
        let message = json["message"].as_str().unwrap();
        assert!(message.contains("older version"), "{message}");
        assert!(message.contains("same version"), "{message}");
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
