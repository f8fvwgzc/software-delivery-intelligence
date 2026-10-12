//! Authentication, authorisation and audit middleware (CONTRACT.md §18).
//!
//! Every `/api/v1` request except the public routes carries the `sdi_session`
//! cookie. The cookie and the requested workspace (the `/workspaces/{id}`
//! path, the `X-Sdi-Workspace` header or `?workspace=`) are resolved through
//! `Identity.Authorize`, cached 60 s, into an [`Auth`] in the request
//! extensions. The role the route needs is checked here too, so handlers
//! never have to.

use std::net::SocketAddr;

use axum::{
    body::{Body, to_bytes},
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderName, Method, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sdi_common::principal::{Principal, Role};
use sdi_proto::identity::v1 as identity;

use crate::{
    error::ApiError,
    state::{AppState, Auth},
};

pub const SESSION_COOKIE: &str = "sdi_session";
pub const WORKSPACE_HEADER: &str = "x-sdi-workspace";
const API_PREFIX: &str = "/api/v1/";
/// Bodies peeked at for the adapter role check; anything larger is refused.
const PEEK_LIMIT: usize = 1024 * 1024;

/// Routes that work without a session.
pub fn is_public(path: &str) -> bool {
    !path.starts_with(API_PREFIX)
        || matches!(
            path,
            "/api/v1/health"
                | "/api/v1/auth/login"
                | "/api/v1/auth/callback"
                | "/api/v1/webhooks/github"
        )
}

/// The `sdi_session` cookie value, if any.
pub fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The workspace a request names: the `/workspaces/{id}` path first, then
/// the header, then the query; empty means "the user's first workspace".
pub fn requested_workspace(path: &str, query: Option<&str>, headers: &HeaderMap) -> String {
    if let Some(id) = segments(path)
        .get(1)
        .filter(|_| segments(path).first() == Some(&"workspaces"))
    {
        return (*id).to_owned();
    }
    if let Some(id) = headers
        .get(HeaderName::from_static(WORKSPACE_HEADER))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return id.to_owned();
    }
    query
        .unwrap_or_default()
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == "workspace")
        .map(|(_, value)| value.trim().to_owned())
        .unwrap_or_default()
}

/// Path segments after `/api/v1/`.
pub fn segments(path: &str) -> Vec<&str> {
    path.strip_prefix(API_PREFIX)
        .unwrap_or_default()
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

/// The role a route needs (CONTRACT.md §18). `adapter` is the execution
/// adapter named in the body of the routes that take one.
pub fn required_role(method: &Method, path: &str, adapter: Option<&str>) -> Role {
    let segments = segments(path);
    let real_adapter = matches!(adapter, Some("docker") | Some("github"));
    match (method, segments.as_slice()) {
        (&Method::DELETE, ["workspaces", _]) => Role::Owner,
        (&Method::GET, ["workspaces", _, "audit"]) => Role::Admin,
        (_, ["workspaces", _, "members", ..]) if method != Method::GET => Role::Admin,
        (_, ["workspaces", _, "data-controls", ..]) if method != Method::GET => Role::Admin,
        (_, ["workspaces", _, "data", ..]) => Role::Admin,
        // Continuous delivery (§22): secrets (even their names), registries
        // and environment edits are an admin's.
        (_, ["workspaces", _, "secrets", ..]) => Role::Admin,
        (&Method::POST | &Method::PUT | &Method::DELETE, ["registries", ..]) => Role::Admin,
        (&Method::POST, ["projects", _, "environments"]) => Role::Admin,
        (&Method::PUT | &Method::DELETE, ["environments", _]) => Role::Admin,
        (&Method::PATCH, ["policies", ..]) | (&Method::PATCH, ["settings"]) => Role::Admin,
        (&Method::GET | &Method::HEAD | &Method::OPTIONS, _) => Role::Viewer,
        (&Method::POST, ["auth", "logout"]) => Role::Viewer,
        (&Method::POST, ["approvals", _, "decide"]) => Role::Approver,
        (&Method::POST, ["identities", _, "grants"]) => Role::Approver,
        (&Method::POST, ["proposals", _, "pull-request"]) => Role::Approver,
        (&Method::POST, ["reviews", _, "publish"]) => Role::Approver,
        // Promoting and rolling back start a real deployment.
        (&Method::POST, ["environments", _, "promote" | "rollback"]) => Role::Approver,
        (&Method::POST, ["executions"]) if real_adapter => Role::Approver,
        (&Method::POST, ["proposals", _, "accept"]) if real_adapter => Role::Approver,
        _ => Role::Member,
    }
}

/// Routes whose body names an adapter, which decides between member and
/// approver.
fn peeks_adapter(method: &Method, path: &str) -> bool {
    method == Method::POST
        && matches!(
            segments(path).as_slice(),
            ["executions"] | ["proposals", _, "accept"]
        )
}

pub async fn authenticate(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    if is_public(&path) {
        return next.run(request).await;
    }
    let Some(token) = session_cookie(request.headers()) else {
        return ApiError::unauthenticated().into_response();
    };
    let workspace = requested_workspace(&path, request.uri().query(), request.headers());

    let auth = match state.cached_auth(&token, &workspace) {
        Some(auth) => auth,
        None => match authorize(&state, &token, &workspace).await {
            Ok(auth) => {
                state.remember_auth(&token, &workspace, auth.clone());
                auth
            }
            Err(error) => return error.into_response(),
        },
    };

    let (mut parts, body) = request.into_parts();
    let (adapter, body) = if peeks_adapter(&parts.method, &path) {
        match to_bytes(body, PEEK_LIMIT).await {
            Ok(bytes) => {
                let adapter = serde_json::from_slice::<serde_json::Value>(&bytes)
                    .ok()
                    .and_then(|v| v.get("adapter")?.as_str().map(str::to_owned));
                (adapter, Body::from(bytes))
            }
            Err(_) => {
                return ApiError::bad_request("body too large").into_response();
            }
        }
    } else {
        (None, body)
    };
    let needed = required_role(&parts.method, &path, adapter.as_deref());
    if auth.principal.role < needed {
        return ApiError::forbidden(format!(
            "requires the {} role in this workspace (you are {})",
            needed.as_str(),
            auth.principal.role.as_str()
        ))
        .into_response();
    }
    parts.extensions.insert(auth);
    next.run(Request::from_parts(parts, body)).await
}

async fn authorize(state: &AppState, token: &str, workspace: &str) -> Result<Auth, ApiError> {
    let principal = state
        .identity()
        .authorize(identity::AuthorizeRequest {
            session_token: token.to_owned(),
            workspace_id: workspace.to_owned(),
        })
        .await
        .map_err(|status| match status.code() {
            tonic::Code::PermissionDenied | tonic::Code::NotFound => {
                ApiError::forbidden("this workspace is not accessible with your session")
            }
            _ => ApiError::from(status),
        })?
        .into_inner();
    let user = principal.user.unwrap_or_default();
    let ws = principal.workspace.unwrap_or_default();
    let role = Role::parse(
        identity::Role::try_from(principal.role)
            .unwrap_or_default()
            .as_str_name()
            .trim_start_matches("ROLE_")
            .to_ascii_lowercase()
            .as_str(),
    )
    .ok_or_else(|| ApiError::forbidden("no role in this workspace"))?;
    Ok(Auth {
        principal: Principal {
            user_id: user.id.clone(),
            workspace_id: ws.id.clone(),
            role,
        },
        user: user.into(),
        workspace: ws.into(),
        token: token.to_owned(),
    })
}

// ---------------------------------------------------------------------------
// Audit
// ---------------------------------------------------------------------------

/// The audit action, entity and entity id of a mutating route.
pub fn audit_action(method: &Method, path: &str) -> (String, String, String) {
    let segments = segments(path);
    let owned = |s: &str| s.to_owned();
    let (action, entity, id): (String, &str, &str) = match (method, segments.as_slice()) {
        (&Method::POST, ["projects"]) => (owned("project.connect"), "project", ""),
        (&Method::DELETE, ["projects", id]) => (owned("project.disconnect"), "project", id),
        (&Method::PATCH, ["projects", id, "mode"]) => (owned("project.mode"), "project", id),
        (&Method::POST, ["projects", id, "scans"]) => (owned("scan.start"), "project", id),
        (&Method::POST, ["projects", id, "impact"]) => (owned("impact.analyze"), "project", id),
        (&Method::POST, ["projects", id, "documents"]) => {
            (owned("document.generate"), "project", id)
        }
        (&Method::POST, ["projects", id, "diagrams"]) => (owned("diagram.generate"), "project", id),
        (&Method::POST, ["questions", id, "answer"]) => (owned("question.answer"), "question", id),
        (&Method::POST, ["proposals"]) => (owned("proposal.generate"), "proposal", ""),
        (&Method::POST, ["proposals", id, "accept"]) => (owned("proposal.accept"), "proposal", id),
        (&Method::POST, ["proposals", id, "reject"]) => (owned("proposal.reject"), "proposal", id),
        (&Method::POST, ["proposals", id, "messages"]) => {
            (owned("proposal.negotiate"), "proposal", id)
        }
        (&Method::POST, ["proposals", id, "pull-request"]) => {
            (owned("proposal.pull_request"), "proposal", id)
        }
        (&Method::POST, ["decisions"]) => (owned("decision.create"), "decision", ""),
        (&Method::POST, ["decisions", id, "supersede"]) => {
            (owned("decision.supersede"), "decision", id)
        }
        (&Method::PATCH, ["diagrams", id]) => (owned("diagram.update"), "diagram", id),
        (&Method::POST, ["projects", _, "pull-requests", number, "reviews"]) => {
            (owned("review.start"), "pull_request", number)
        }
        (&Method::POST, ["findings", id, "decide"]) => (owned("finding.decide"), "finding", id),
        (&Method::POST, ["reviews", id, "publish"]) => (owned("review.publish"), "review", id),
        (&Method::DELETE, ["diagrams", id]) => (owned("diagram.delete"), "diagram", id),
        (&Method::POST, ["executions"]) => (owned("execution.create"), "execution", ""),
        (&Method::POST, ["executions", id, verb]) => (format!("execution.{verb}"), "execution", id),
        (&Method::POST, ["approvals", id, "decide"]) => (owned("approval.decide"), "approval", id),
        (&Method::PATCH, ["policies", action_type]) => {
            (owned("policy.update"), "policy", action_type)
        }
        (&Method::POST, ["identities", id, "grants"]) => (owned("identity.grant"), "identity", id),
        // Continuous delivery (§22).
        (&Method::POST, ["registries"]) => (owned("registry.upsert"), "registry", ""),
        (&Method::PUT, ["registries", id]) => (owned("registry.upsert"), "registry", id),
        (&Method::DELETE, ["registries", id]) => (owned("registry.delete"), "registry", id),
        (&Method::POST, ["projects", id, "environments"]) => {
            (owned("environment.upsert"), "project", id)
        }
        (&Method::PUT, ["environments", id]) => (owned("environment.upsert"), "environment", id),
        (&Method::DELETE, ["environments", id]) => (owned("environment.delete"), "environment", id),
        (&Method::POST, ["projects", id, "releases"]) => (owned("release.create"), "project", id),
        (&Method::POST, ["environments", id, "promote"]) => {
            (owned("deployment.promote"), "environment", id)
        }
        (&Method::POST, ["environments", id, "rollback"]) => {
            (owned("deployment.rollback"), "environment", id)
        }
        (&Method::PUT, ["workspaces", _, "secrets", name]) => (owned("secret.set"), "secret", name),
        (&Method::DELETE, ["workspaces", _, "secrets", name]) => {
            (owned("secret.delete"), "secret", name)
        }
        (&Method::PATCH, ["settings"]) => (owned("settings.update"), "settings", ""),
        (&Method::PATCH, ["workspaces", _, "members", user_id]) => {
            (owned("member.override"), "member", user_id)
        }
        (&Method::POST, ["workspaces", id, "members", "sync"]) => {
            (owned("members.sync"), "workspace", id)
        }
        (&Method::PATCH, ["workspaces", id, "data-controls"]) => {
            (owned("data_controls.update"), "workspace", id)
        }
        (&Method::PUT, ["workspaces", id, "data-controls", "reasoner-key"]) => {
            (owned("reasoner_key.set"), "workspace", id)
        }
        (&Method::POST, ["workspaces", id, "data", "export"]) => {
            (owned("data.export"), "workspace", id)
        }
        (&Method::POST, ["workspaces", id, "data", "purge-code-cache"]) => {
            (owned("data.purge"), "workspace", id)
        }
        (&Method::DELETE, ["workspaces", id]) => (owned("workspace.delete"), "workspace", id),
        (&Method::POST, ["auth", "logout"]) => (owned("auth.logout"), "session", ""),
        (_, [first, rest @ ..]) => {
            let entity = first.trim_end_matches('s');
            (
                format!("{entity}.{}", method.as_str().to_ascii_lowercase()),
                entity,
                rest.first().copied().unwrap_or_default(),
            )
        }
        (_, []) => (
            format!("api.{}", method.as_str().to_ascii_lowercase()),
            "",
            "",
        ),
    };
    (action, entity.to_owned(), id.to_owned())
}

fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .or_else(|| peer.map(|p| p.ip().to_string()))
        .unwrap_or_default()
}

/// Records every mutating authenticated request, fire-and-forget, after
/// the handler answered. Failures only say so (`detail`).
pub async fn audit(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let mutating = matches!(
        *request.method(),
        Method::POST | Method::PATCH | Method::PUT | Method::DELETE
    );
    let auth = request.extensions().get::<Auth>().cloned();
    let (Some(auth), true) = (auth, mutating) else {
        return next.run(request).await;
    };
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0);
    let ip = client_ip(request.headers(), peer);

    let response = next.run(request).await;

    let (action, entity, entity_id) = audit_action(&method, &path);
    let detail = if response.status().is_success() {
        String::new()
    } else {
        format!("http {}", response.status().as_u16())
    };
    let entry = identity::AuditEntry {
        workspace_id: auth.principal.workspace_id.clone(),
        user_id: auth.principal.user_id.clone(),
        user_login: auth.user.login.clone(),
        action,
        entity,
        entity_id,
        detail,
        request_id,
        ip,
        ..Default::default()
    };
    let mut client = state.identity();
    sdi_common::tasks::spawn("audit-entry", async move {
        if let Err(error) = client
            .record_audit(identity::RecordAuditRequest { entry: Some(entry) })
            .await
        {
            tracing::warn!(%error, "audit entry not recorded");
        }
    });
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(cookie: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, cookie.parse().unwrap());
        headers
    }

    #[test]
    fn session_cookie_is_found_among_others() {
        assert_eq!(
            session_cookie(&headers("theme=dark; sdi_session=abc123; other=1")),
            Some("abc123".into())
        );
        assert_eq!(session_cookie(&headers("theme=dark")), None);
        assert_eq!(session_cookie(&headers("sdi_session=")), None);
        assert_eq!(session_cookie(&HeaderMap::new()), None);
    }

    #[test]
    fn workspace_comes_from_path_then_header_then_query() {
        let mut with_header = HeaderMap::new();
        with_header.insert("x-sdi-workspace", "w-header".parse().unwrap());
        assert_eq!(
            requested_workspace(
                "/api/v1/workspaces/w-path/members",
                Some("workspace=w-q"),
                &with_header
            ),
            "w-path"
        );
        assert_eq!(
            requested_workspace("/api/v1/projects", Some("workspace=w-q"), &with_header),
            "w-header"
        );
        assert_eq!(
            requested_workspace(
                "/api/v1/projects",
                Some("a=1&workspace=w-q"),
                &HeaderMap::new()
            ),
            "w-q"
        );
        assert_eq!(
            requested_workspace("/api/v1/projects", None, &HeaderMap::new()),
            ""
        );
        assert_eq!(
            requested_workspace("/api/v1/workspaces", None, &HeaderMap::new()),
            ""
        );
    }

    #[test]
    fn public_routes_need_no_session() {
        for path in [
            "/api/v1/health",
            "/api/v1/auth/login",
            "/api/v1/auth/callback",
            "/api/v1/webhooks/github",
            "/api/openapi.json",
            "/api/docs",
        ] {
            assert!(is_public(path), "{path}");
        }
        for path in [
            "/api/v1/projects",
            "/api/v1/auth/me",
            "/api/v1/auth/logout",
            "/api/v1/nope",
        ] {
            assert!(!is_public(path), "{path}");
        }
    }

    #[test]
    fn roles_follow_the_contract_matrix() {
        let r = |m: Method, p: &str, a: Option<&str>| required_role(&m, p, a);
        assert_eq!(r(Method::GET, "/api/v1/projects", None), Role::Viewer);
        assert_eq!(
            r(Method::GET, "/api/v1/workspaces/w/members", None),
            Role::Viewer
        );
        assert_eq!(r(Method::POST, "/api/v1/projects", None), Role::Member);
        assert_eq!(
            r(Method::POST, "/api/v1/projects/p/scans", None),
            Role::Member
        );
        assert_eq!(
            r(Method::POST, "/api/v1/proposals/x/messages", None),
            Role::Member
        );
        assert_eq!(r(Method::POST, "/api/v1/executions", None), Role::Member);
        assert_eq!(
            r(Method::POST, "/api/v1/executions", Some("simulated")),
            Role::Member
        );
        assert_eq!(
            r(Method::POST, "/api/v1/executions", Some("docker")),
            Role::Approver
        );
        assert_eq!(
            r(Method::POST, "/api/v1/executions", Some("github")),
            Role::Approver
        );
        assert_eq!(
            r(Method::POST, "/api/v1/proposals/x/accept", None),
            Role::Member
        );
        assert_eq!(
            r(Method::POST, "/api/v1/proposals/x/accept", Some("docker")),
            Role::Approver
        );
        assert_eq!(
            r(Method::POST, "/api/v1/proposals/x/pull-request", None),
            Role::Approver
        );
        assert_eq!(
            r(Method::POST, "/api/v1/approvals/a/decide", None),
            Role::Approver
        );
        assert_eq!(
            r(Method::POST, "/api/v1/identities/i/grants", None),
            Role::Approver
        );
        assert_eq!(
            r(Method::PATCH, "/api/v1/policies/deploy.staging", None),
            Role::Admin
        );
        assert_eq!(r(Method::PATCH, "/api/v1/settings", None), Role::Admin);
        assert_eq!(
            r(Method::PATCH, "/api/v1/workspaces/w/members/u", None),
            Role::Admin
        );
        assert_eq!(
            r(Method::POST, "/api/v1/workspaces/w/members/sync", None),
            Role::Admin
        );
        assert_eq!(
            r(Method::GET, "/api/v1/workspaces/w/data-controls", None),
            Role::Viewer
        );
        assert_eq!(
            r(Method::PATCH, "/api/v1/workspaces/w/data-controls", None),
            Role::Admin
        );
        assert_eq!(
            r(
                Method::PUT,
                "/api/v1/workspaces/w/data-controls/reasoner-key",
                None
            ),
            Role::Admin
        );
        assert_eq!(
            r(Method::POST, "/api/v1/workspaces/w/data/export", None),
            Role::Admin
        );
        assert_eq!(
            r(
                Method::POST,
                "/api/v1/workspaces/w/data/purge-code-cache",
                None
            ),
            Role::Admin
        );
        assert_eq!(
            r(Method::GET, "/api/v1/workspaces/w/audit", None),
            Role::Admin
        );
        assert_eq!(r(Method::DELETE, "/api/v1/workspaces/w", None), Role::Owner);
        assert_eq!(r(Method::POST, "/api/v1/auth/logout", None), Role::Viewer);
        // Code review (§20): start and decide are member work, publishing needs an approver.
        assert_eq!(
            r(
                Method::POST,
                "/api/v1/projects/p/pull-requests/4/reviews",
                None
            ),
            Role::Member
        );
        assert_eq!(
            r(Method::POST, "/api/v1/findings/f/decide", None),
            Role::Member
        );
        assert_eq!(
            r(Method::POST, "/api/v1/reviews/r/publish", None),
            Role::Approver
        );
        assert_eq!(r(Method::GET, "/api/v1/activity", None), Role::Viewer);
        // Continuous delivery (§22): reads viewer, create release member,
        // promote/rollback approver, registries, secrets and environment
        // edits admin.
        for path in [
            "/api/v1/registries",
            "/api/v1/projects/p/environments",
            "/api/v1/projects/p/pipeline",
            "/api/v1/projects/p/releases",
            "/api/v1/releases/r",
            "/api/v1/projects/p/deployment-runs",
            "/api/v1/deployment-runs/d",
            "/api/v1/projects/p/delivery/artifacts",
        ] {
            assert_eq!(r(Method::GET, path, None), Role::Viewer, "{path}");
        }
        assert_eq!(
            r(Method::POST, "/api/v1/projects/p/releases", None),
            Role::Member
        );
        for path in [
            "/api/v1/environments/e/promote",
            "/api/v1/environments/e/rollback",
        ] {
            assert_eq!(r(Method::POST, path, None), Role::Approver, "{path}");
        }
        for (method, path) in [
            (Method::POST, "/api/v1/registries"),
            (Method::PUT, "/api/v1/registries/r"),
            (Method::DELETE, "/api/v1/registries/r"),
            (Method::POST, "/api/v1/projects/p/environments"),
            (Method::PUT, "/api/v1/environments/e"),
            (Method::DELETE, "/api/v1/environments/e"),
            (Method::GET, "/api/v1/workspaces/w/secrets"),
            (Method::PUT, "/api/v1/workspaces/w/secrets/JENKINS_TOKEN"),
            (Method::DELETE, "/api/v1/workspaces/w/secrets/JENKINS_TOKEN"),
        ] {
            assert_eq!(
                r(method.clone(), path, None),
                Role::Admin,
                "{method} {path}"
            );
        }
    }

    #[test]
    fn audit_actions_name_the_entity() {
        let a = |m: Method, p: &str| audit_action(&m, p);
        assert_eq!(
            a(Method::POST, "/api/v1/projects"),
            ("project.connect".into(), "project".into(), String::new())
        );
        assert_eq!(
            a(Method::DELETE, "/api/v1/projects/p1"),
            ("project.disconnect".into(), "project".into(), "p1".into())
        );
        assert_eq!(
            a(Method::POST, "/api/v1/executions/e1/start"),
            ("execution.start".into(), "execution".into(), "e1".into())
        );
        assert_eq!(
            a(Method::PATCH, "/api/v1/workspaces/w/members/u9"),
            ("member.override".into(), "member".into(), "u9".into())
        );
        assert_eq!(
            a(Method::PATCH, "/api/v1/policies/deploy.staging"),
            (
                "policy.update".into(),
                "policy".into(),
                "deploy.staging".into()
            )
        );
        assert_eq!(
            a(Method::DELETE, "/api/v1/workspaces/w"),
            ("workspace.delete".into(), "workspace".into(), "w".into())
        );
        assert_eq!(
            a(Method::POST, "/api/v1/projects/p/pull-requests/4/reviews"),
            ("review.start".into(), "pull_request".into(), "4".into())
        );
        assert_eq!(
            a(Method::POST, "/api/v1/findings/f1/decide"),
            ("finding.decide".into(), "finding".into(), "f1".into())
        );
        assert_eq!(
            a(Method::POST, "/api/v1/reviews/r1/publish"),
            ("review.publish".into(), "review".into(), "r1".into())
        );
        // Continuous delivery (§22).
        for (method, path, action, entity, id) in [
            (
                Method::POST,
                "/api/v1/registries",
                "registry.upsert",
                "registry",
                "",
            ),
            (
                Method::PUT,
                "/api/v1/registries/r1",
                "registry.upsert",
                "registry",
                "r1",
            ),
            (
                Method::DELETE,
                "/api/v1/registries/r1",
                "registry.delete",
                "registry",
                "r1",
            ),
            (
                Method::POST,
                "/api/v1/projects/p1/environments",
                "environment.upsert",
                "project",
                "p1",
            ),
            (
                Method::PUT,
                "/api/v1/environments/e1",
                "environment.upsert",
                "environment",
                "e1",
            ),
            (
                Method::DELETE,
                "/api/v1/environments/e1",
                "environment.delete",
                "environment",
                "e1",
            ),
            (
                Method::POST,
                "/api/v1/projects/p1/releases",
                "release.create",
                "project",
                "p1",
            ),
            (
                Method::POST,
                "/api/v1/environments/e1/promote",
                "deployment.promote",
                "environment",
                "e1",
            ),
            (
                Method::POST,
                "/api/v1/environments/e1/rollback",
                "deployment.rollback",
                "environment",
                "e1",
            ),
            (
                Method::PUT,
                "/api/v1/workspaces/w/secrets/JENKINS_TOKEN",
                "secret.set",
                "secret",
                "JENKINS_TOKEN",
            ),
            (
                Method::DELETE,
                "/api/v1/workspaces/w/secrets/JENKINS_TOKEN",
                "secret.delete",
                "secret",
                "JENKINS_TOKEN",
            ),
        ] {
            assert_eq!(
                a(method, path),
                (action.to_owned(), entity.to_owned(), id.to_owned()),
                "{path}"
            );
        }
        // Unknown mutating routes still get a sensible key.
        assert_eq!(
            a(Method::POST, "/api/v1/widgets/x/spin"),
            ("widget.post".into(), "widget".into(), "x".into())
        );
    }

    #[test]
    fn client_ip_prefers_the_proxy_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.9, 10.0.0.1".parse().unwrap());
        assert_eq!(client_ip(&headers, None), "203.0.113.9");
        let peer: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        assert_eq!(client_ip(&HeaderMap::new(), Some(peer)), "127.0.0.1");
        assert_eq!(client_ip(&HeaderMap::new(), None), "");
    }
}
