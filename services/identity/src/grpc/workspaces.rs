//! Workspaces, GitHub App installations and the installation tokens the
//! knowledge/execution services read GitHub with.

use chrono::{Duration, Utc};
use sdi_common::{grpc::db_status, time};
use sdi_proto::identity::v1::{
    GetInstallationTokenRequest, GetWorkspaceRequest, InstallationToken, ListWorkspacesRequest,
    ListWorkspacesResponse, RecordInstallationRequest, RemoveInstallationRequest,
    RemoveInstallationResponse, Workspace,
};
use tonic::Status;
use uuid::Uuid;

use super::{CachedToken, IdentityState, github_status, parse_id, stored_role};
use crate::{
    config, convert,
    db::{
        self,
        workspaces::{KIND_ORGANIZATION, KIND_PERSONAL, NewInstallation, WorkspaceRow},
    },
    github,
};

/// A cached installation token is reused until this close to its expiry.
const TOKEN_MARGIN: Duration = Duration::minutes(5);

pub async fn list(
    state: &IdentityState,
    req: ListWorkspacesRequest,
) -> Result<ListWorkspacesResponse, Status> {
    let user_id = parse_id(&req.user_id, "user_id")?;
    let rows = db::workspaces::list_for_user(&state.pool, user_id)
        .await
        .map_err(db_status)?;
    let mut workspaces = Vec::with_capacity(rows.len());
    for row in rows {
        let role = stored_role(state, &row, user_id).await?;
        workspaces.push(convert::workspace_to_proto(row, role));
    }
    Ok(ListWorkspacesResponse { workspaces })
}

pub async fn get(state: &IdentityState, req: GetWorkspaceRequest) -> Result<Workspace, Status> {
    let id = parse_id(&req.id, "id")?;
    let row = db::workspaces::get(&state.pool, id)
        .await
        .map_err(db_status)?;
    Ok(convert::workspace_to_proto(row, None))
}

/// The API base of the host an installation lives on: the configured
/// base for the host this deployment signs in with, github.com's public
/// API, or `https://<host>/api/v3` for any other GitHub Enterprise Server.
pub(crate) fn api_base_for(state: &IdentityState, host: &str) -> String {
    api_base_of(&state.settings, host)
}

pub(crate) fn api_base_of(settings: &config::Settings, host: &str) -> String {
    if host.is_empty() || host == settings.github_host {
        settings.github_api_url.clone()
    } else if host == config::PUBLIC_HOST {
        config::DEFAULT_GITHUB_API.to_owned()
    } else {
        format!("https://{host}/api/v3")
    }
}

pub async fn record_installation(
    state: &IdentityState,
    mut req: RecordInstallationRequest,
) -> Result<Workspace, Status> {
    if req.installation_id <= 0 {
        return Err(Status::invalid_argument("installation_id is required"));
    }
    let host = if req.github_host.trim().is_empty() {
        state.settings.github_host.clone()
    } else {
        req.github_host.trim().to_owned()
    };
    let api_base = api_base_for(state, &host);

    // The post-install redirect only carries the id: ask GitHub for the rest.
    if req.account_login.trim().is_empty() {
        let Some(app) = &state.settings.app else {
            return Err(Status::failed_precondition(
                "installation details are missing and no GitHub App is configured to look them up",
            ));
        };
        let jwt = github::app_jwt(&app.app_id, &app.private_key_pem).map_err(github_status)?;
        let info = state
            .github
            .installation(&api_base, &jwt, req.installation_id)
            .await
            .map_err(github_status)?;
        req.account_login = info.account.login;
        req.account_type = info.account.kind.unwrap_or_default();
        req.account_avatar_url = info.account.avatar_url.unwrap_or_default();
        if req.repository_selection.is_empty() {
            req.repository_selection = info.repository_selection.unwrap_or_default();
        }
    }

    let installed_by = Uuid::parse_str(req.installed_by_user_id.trim()).ok();
    let on_user_account = req.account_type.eq_ignore_ascii_case("User");
    let kind = if on_user_account {
        KIND_PERSONAL
    } else {
        KIND_ORGANIZATION
    };
    let mut tx = state.pool.begin().await.map_err(db_status)?;
    let workspace_id = db::workspaces::upsert_installation(
        &mut tx,
        NewInstallation {
            installation_id: req.installation_id,
            kind,
            github_host: &host,
            api_base_url: &api_base,
            account_login: req.account_login.trim(),
            account_type: if on_user_account {
                "User"
            } else {
                "Organization"
            },
            avatar_url: req.account_avatar_url.trim(),
            repository_selection: req.repository_selection.trim(),
            installed_by_user_id: installed_by,
            owner_user_id: installed_by.filter(|_| on_user_account),
        },
    )
    .await
    .map_err(db_status)?;
    db::workspaces::ensure_controls(&mut *tx, workspace_id)
        .await
        .map_err(db_status)?;
    tx.commit().await.map_err(db_status)?;

    let mut row = db::workspaces::get(&state.pool, workspace_id)
        .await
        .map_err(db_status)?;
    // Best effort: the first membership snapshot. Failures are logged; the
    // lazy sync on first access covers the gap.
    if row.kind == KIND_ORGANIZATION {
        match super::members::sync_from_github(state, &row).await {
            Ok(()) => {
                row = db::workspaces::get(&state.pool, workspace_id)
                    .await
                    .map_err(db_status)?;
            }
            Err(error) => {
                tracing::warn!(workspace = %row.slug, %error, "initial member sync failed");
            }
        }
    }
    let my_role = match installed_by {
        Some(user_id) => stored_role(state, &row, user_id).await?,
        None => None,
    };
    tracing::info!(installation = req.installation_id, workspace = %row.slug, "installation recorded");
    Ok(convert::workspace_to_proto(row, my_role))
}

pub async fn remove_installation(
    state: &IdentityState,
    req: RemoveInstallationRequest,
) -> Result<RemoveInstallationResponse, Status> {
    let removed = db::workspaces::remove_installation(&state.pool, req.installation_id)
        .await
        .map_err(db_status)?;
    if removed {
        state
            .tokens
            .lock()
            .expect("token cache lock")
            .remove(&req.installation_id);
        tracing::info!(installation = req.installation_id, "installation removed");
    }
    Ok(RemoveInstallationResponse {})
}

pub async fn installation_token(
    state: &IdentityState,
    req: GetInstallationTokenRequest,
) -> Result<InstallationToken, Status> {
    let id = parse_id(&req.workspace_id, "workspace_id")?;
    let row = db::workspaces::get(&state.pool, id)
        .await
        .map_err(db_status)?;
    token_for(state, &row).await
}

/// An installation's app token (cached), or, for a personal workspace
/// without an installation, the owner's OAuth token from their newest
/// live session.
pub(crate) async fn token_for(
    state: &IdentityState,
    row: &WorkspaceRow,
) -> Result<InstallationToken, Status> {
    match (row.installation_id, row.installation_removed_at) {
        (Some(installation_id), None) => {
            let api_base = row
                .api_base_url
                .clone()
                .unwrap_or_else(|| api_base_for(state, &row.github_host));
            let token = app_token(state, &api_base, installation_id).await?;
            Ok(InstallationToken {
                token: token.token,
                expires_at: Some(time::to_proto(token.expires_at)),
                api_base_url: api_base,
            })
        }
        (Some(_), Some(_)) if row.kind == KIND_ORGANIZATION => Err(Status::failed_precondition(
            "the SDI GitHub App was uninstalled from this organisation",
        )),
        _ if row.kind == KIND_PERSONAL => {
            let owner = row
                .owner_user_id
                .ok_or_else(|| Status::failed_precondition("personal workspace has no owner"))?;
            let session = db::sessions::latest_live_for_user(&state.pool, owner)
                .await
                .map_err(db_status)?
                .ok_or_else(|| {
                    Status::failed_precondition("the workspace owner has no live session")
                })?;
            let token = state.keys.open_str(&session.oauth_token).map_err(|error| {
                tracing::error!(%error, "stored OAuth token does not open");
                Status::internal("stored token is unreadable")
            })?;
            if token.is_empty() {
                return Err(Status::failed_precondition(
                    "this workspace has no GitHub access: it belongs to the local developer \
                     (dev login without GITHUB_TOKEN); set GITHUB_TOKEN to sign in as a GitHub account",
                ));
            }
            Ok(InstallationToken {
                token,
                expires_at: Some(time::to_proto(session.expires_at)),
                api_base_url: api_base_for(state, &row.github_host),
            })
        }
        _ => Err(Status::failed_precondition(
            "this workspace has no GitHub App installation",
        )),
    }
}

async fn app_token(
    state: &IdentityState,
    api_base: &str,
    installation_id: i64,
) -> Result<CachedToken, Status> {
    let now = Utc::now();
    if let Some(cached) = state
        .tokens
        .lock()
        .expect("token cache lock")
        .get_if(&installation_id, |t| t.expires_at - TOKEN_MARGIN > now)
        .cloned()
    {
        return Ok(cached);
    }
    let Some(app) = &state.settings.app else {
        return Err(Status::failed_precondition(
            "GITHUB_APP_ID and GITHUB_APP_PRIVATE_KEY are not configured",
        ));
    };
    let jwt = github::app_jwt(&app.app_id, &app.private_key_pem).map_err(github_status)?;
    let minted = state
        .github
        .installation_token(api_base, &jwt, installation_id)
        .await
        .map_err(github_status)?;
    let token = CachedToken {
        token: minted.token,
        expires_at: minted.expires_at,
    };
    state
        .tokens
        .lock()
        .expect("token cache lock")
        .insert(installation_id, token.clone());
    tracing::debug!(
        installation_id,
        cached = state.cached_tokens(),
        "installation token minted"
    );
    Ok(token)
}
