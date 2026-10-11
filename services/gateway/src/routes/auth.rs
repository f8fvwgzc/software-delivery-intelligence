//! GitHub OAuth sign-in and the session cookie (CONTRACT.md §18).

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use sdi_proto::identity::v1 as identity;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    auth::SESSION_COOKIE,
    dto::identity::{CallbackQuery, LoginQuery, Me},
    error::{ApiError, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Auth, Scoped},
};

/// 30 days, like the session (§18).
const COOKIE_MAX_AGE: u64 = 30 * 24 * 60 * 60;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(login))
        .routes(routes!(callback))
        .routes(routes!(logout))
        .routes(routes!(me))
}

fn session_cookie(state: &AppState, token: &str, max_age: u64) -> HeaderValue {
    let secure = if state.settings.public_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    format!("{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}")
        .parse()
        .expect("cookie header value")
}

/// A same-origin target: a path, or a URL on the public origin.
fn same_origin(state: &AppState, target: &str) -> bool {
    let is_path =
        target.starts_with('/') && !target.starts_with("//") && !target.starts_with("/\\");
    is_path
        || target
            .strip_prefix(state.settings.public_url.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/') || rest.starts_with('?'))
}

/// Starts sign-in: redirects to GitHub (or, in dev mode, straight to the
/// callback). `returnTo` must be a path or a URL on the app origin.
#[utoipa::path(
    get,
    path = "/auth/login",
    tag = "auth",
    params(LoginQuery),
    responses(
        (status = 302, description = "Redirect to GitHub's authorize page"),
        ErrorResponses,
    ),
)]
async fn login(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<LoginQuery>,
) -> ApiResult<Redirect> {
    let response = state
        .identity()
        .begin_login(identity::BeginLoginRequest {
            return_to: query.return_to.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Redirect::to(&response.authorize_url))
}

/// Where GitHub sends the user back. Exchanges the code for a session,
/// sets the `sdi_session` cookie and redirects to where sign-in started.
#[utoipa::path(
    get,
    path = "/auth/callback",
    tag = "auth",
    params(CallbackQuery),
    responses(
        (status = 302, description = "Session cookie set; redirect to `returnTo`"),
        ErrorResponses,
    ),
)]
async fn callback(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<CallbackQuery>,
) -> ApiResult<Response> {
    if let Some(error) = query.error.filter(|e| !e.is_empty()) {
        return Err(ApiError::bad_request(format!(
            "GitHub refused sign-in: {error}"
        )));
    }
    let (Some(code), Some(login_state)) = (query.code, query.state) else {
        return Err(ApiError::bad_request("code and state are required"));
    };
    let session = state
        .identity()
        .complete_login(identity::CompleteLoginRequest {
            code,
            state: login_state.clone(),
        })
        .await?
        .into_inner();
    // Dev mode has no signed state to carry the target, so it travels in
    // the query; it is still held to the same origin rule.
    let target = match query.return_to {
        Some(target)
            if state.settings.dev_login && login_state == "dev" && same_origin(&state, &target) =>
        {
            target
        }
        _ if !session.return_to.is_empty() => session.return_to,
        _ => "/".to_owned(),
    };
    let mut response = Redirect::to(&target).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&state, &session.token, COOKIE_MAX_AGE),
    );
    Ok(response)
}

/// Revokes the session and clears the cookie.
#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "auth",
    responses((status = 204, description = "Signed out"), ErrorResponses),
)]
async fn logout(State(state): State<AppState>, auth: Auth) -> ApiResult<Response> {
    state
        .identity()
        .revoke_session(identity::RevokeSessionRequest {
            token: auth.token.clone(),
        })
        .await?;
    state.forget_auth(Some(&auth.token), None);
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, session_cookie(&state, "", 0));
    Ok(response)
}

/// The signed-in user and their workspaces, or 401.
#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "auth",
    responses((status = 200, body = Me), ErrorResponses),
)]
async fn me(scoped: Scoped, auth: Auth) -> ApiResult<Json<Me>> {
    let workspaces = super::workspaces::list_for(&scoped, &auth.user.id).await?;
    Ok(Json(Me {
        user: auth.user,
        current_workspace_id: auth.principal.workspace_id,
        workspaces,
    }))
}
