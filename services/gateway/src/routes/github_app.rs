//! The SDI GitHub App: where to install it and the post-install redirect
//! (CONTRACT.md §18).

use axum::{Json, extract::State, response::Redirect};
use sdi_proto::identity::v1 as identity;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::identity::{InstallUrl, SetupQuery},
    error::{ApiError, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Auth},
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(install_url))
        .routes(routes!(setup))
}

/// Where an organisation owner installs the SDI GitHub App.
#[utoipa::path(
    get,
    path = "/github/app/install-url",
    tag = "github",
    responses((status = 200, body = InstallUrl), ErrorResponses),
)]
async fn install_url(State(state): State<AppState>, auth: Auth) -> ApiResult<Json<InstallUrl>> {
    let Some(slug) = state.settings.app_slug.as_deref() else {
        return Err(ApiError::precondition(
            "GITHUB_APP_SLUG is not configured on the gateway",
        ));
    };
    let host = if auth.user.github_host == "github.com" || auth.user.github_host.is_empty() {
        "https://github.com".to_owned()
    } else {
        format!("https://{}", auth.user.github_host)
    };
    Ok(Json(InstallUrl {
        url: format!("{host}/apps/{slug}/installations/new"),
    }))
}

/// GitHub's post-install redirect: records the installation as a workspace
/// of the signed-in user and sends them to it.
#[utoipa::path(
    get,
    path = "/github/app/setup",
    tag = "github",
    params(SetupQuery),
    responses(
        (status = 302, description = "Redirect to the new workspace"),
        ErrorResponses,
    ),
)]
async fn setup(
    State(state): State<AppState>,
    auth: Auth,
    ApiQuery(query): ApiQuery<SetupQuery>,
) -> ApiResult<Redirect> {
    if query.installation_id <= 0 {
        return Err(ApiError::bad_request("installation_id is required"));
    }
    tracing::info!(
        installation = query.installation_id,
        action = query.setup_action.as_deref().unwrap_or("install"),
        user = %auth.user.login,
        "GitHub App setup redirect"
    );
    let workspace = state
        .identity()
        .record_installation(identity::RecordInstallationRequest {
            installation_id: query.installation_id,
            github_host: auth.user.github_host.clone(),
            installed_by_user_id: auth.user.id.clone(),
            ..Default::default()
        })
        .await?
        .into_inner();
    state.forget_auth(Some(&auth.token), None);
    Ok(Redirect::to(&super::auth::absolute(
        &state,
        &format!("/?workspace={}", workspace.id),
    )))
}
