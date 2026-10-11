//! tonic service. Each RPC group lives in its own file; this module wires
//! the trait to them and holds the shared state and helpers.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use sdi_common::{grpc::db_status, principal::Role};
use sdi_proto::identity::v1::{
    AuthorizeRequest, BeginLoginRequest, BeginLoginResponse, CompleteLoginRequest, DataControls,
    GetDataControlsRequest, GetInstallationTokenRequest, GetReasonerKeyRequest, GetSessionRequest,
    GetWorkspaceRequest, InstallationToken, ListAuditRequest, ListAuditResponse,
    ListMembersRequest, ListMembersResponse, ListWorkspacesRequest, ListWorkspacesResponse, Member,
    Principal, ReasonerKey, RecordAuditRequest, RecordAuditResponse, RecordInstallationRequest,
    RemoveInstallationRequest, RemoveInstallationResponse, RevokeSessionRequest,
    RevokeSessionResponse, Session, SetMemberRoleRequest, SetReasonerKeyRequest,
    SyncMembersRequest, UpdateDataControlsRequest, Workspace,
    identity_service_server::IdentityService,
};
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{
    config::Settings,
    crypto::Keys,
    db::{self, workspaces::WorkspaceRow},
    github::{GithubClient, GithubError},
    roles,
};

pub(crate) mod audit;
pub(crate) mod authorize;
pub(crate) mod controls;
pub(crate) mod members;
pub(crate) mod sessions;
pub(crate) mod workspaces;

/// A minted installation token, kept until shortly before it expires.
#[derive(Debug, Clone)]
pub struct CachedToken {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

pub struct IdentityState {
    pub pool: PgPool,
    pub settings: Settings,
    pub keys: Keys,
    pub github: GithubClient,
    /// Installation tokens by installation id (CONTRACT.md §19: in memory only).
    pub tokens: Mutex<HashMap<i64, CachedToken>>,
}

impl IdentityState {
    pub fn new(pool: PgPool, settings: Settings) -> Self {
        Self {
            pool,
            keys: Keys::from_secret(&settings.session_secret),
            settings,
            github: GithubClient::new(),
            tokens: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Clone)]
pub struct IdentityGrpc {
    state: Arc<IdentityState>,
}

impl IdentityGrpc {
    pub fn new(state: Arc<IdentityState>) -> Self {
        Self { state }
    }
}

pub(crate) fn parse_id(raw: &str, what: &str) -> Result<Uuid, Status> {
    Uuid::parse_str(raw.trim())
        .map_err(|_| Status::invalid_argument(format!("{what} is not a valid id")))
}

/// Maps a GitHub failure onto the status the gateway will translate: the
/// caller's credentials (`UNAUTHENTICATED`), GitHub saying no
/// (`PERMISSION_DENIED`), GitHub being away (`UNAVAILABLE`).
pub(crate) fn github_status(error: GithubError) -> Status {
    match error {
        GithubError::Unauthorized => Status::unauthenticated("GitHub rejected the token"),
        GithubError::Forbidden(message) => Status::permission_denied(format!("GitHub: {message}")),
        GithubError::NotFound(message) => Status::not_found(format!("GitHub: {message}")),
        GithubError::OAuth(message) => Status::invalid_argument(format!("GitHub OAuth: {message}")),
        GithubError::Transport(error) => {
            tracing::warn!(%error, "GitHub unreachable");
            Status::unavailable("GitHub is unreachable")
        }
        other => {
            tracing::error!(error = %other, "GitHub call failed");
            Status::unavailable(other.to_string())
        }
    }
}

/// The caller's effective role in a workspace from what is stored, without
/// touching GitHub: owner of a personal workspace, or the synced membership.
pub(crate) async fn stored_role(
    state: &IdentityState,
    workspace: &WorkspaceRow,
    user_id: Uuid,
) -> Result<Option<Role>, Status> {
    if workspace.kind == db::workspaces::KIND_PERSONAL {
        return Ok((workspace.owner_user_id == Some(user_id)).then_some(Role::Owner));
    }
    let member = db::memberships::get(&state.pool, workspace.id, user_id)
        .await
        .map_err(db_status)?;
    Ok(member.map(|m| {
        let derived = roles::from_db(&m.membership.derived_role).unwrap_or(Role::Viewer);
        let override_role = m
            .membership
            .override_role
            .as_deref()
            .and_then(roles::from_db);
        roles::effective(derived, override_role)
    }))
}

#[tonic::async_trait]
impl IdentityService for IdentityGrpc {
    async fn begin_login(
        &self,
        request: Request<BeginLoginRequest>,
    ) -> Result<Response<BeginLoginResponse>, Status> {
        sessions::begin(&self.state, request.into_inner()).map(Response::new)
    }

    async fn complete_login(
        &self,
        request: Request<CompleteLoginRequest>,
    ) -> Result<Response<Session>, Status> {
        sessions::complete(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_session(
        &self,
        request: Request<GetSessionRequest>,
    ) -> Result<Response<Session>, Status> {
        sessions::get(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn revoke_session(
        &self,
        request: Request<RevokeSessionRequest>,
    ) -> Result<Response<RevokeSessionResponse>, Status> {
        sessions::revoke(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_workspaces(
        &self,
        request: Request<ListWorkspacesRequest>,
    ) -> Result<Response<ListWorkspacesResponse>, Status> {
        workspaces::list(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_workspace(
        &self,
        request: Request<GetWorkspaceRequest>,
    ) -> Result<Response<Workspace>, Status> {
        workspaces::get(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_members(
        &self,
        request: Request<ListMembersRequest>,
    ) -> Result<Response<ListMembersResponse>, Status> {
        members::list(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn set_member_role(
        &self,
        request: Request<SetMemberRoleRequest>,
    ) -> Result<Response<Member>, Status> {
        members::set_role(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn sync_members(
        &self,
        request: Request<SyncMembersRequest>,
    ) -> Result<Response<ListMembersResponse>, Status> {
        members::sync(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn record_installation(
        &self,
        request: Request<RecordInstallationRequest>,
    ) -> Result<Response<Workspace>, Status> {
        workspaces::record_installation(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn remove_installation(
        &self,
        request: Request<RemoveInstallationRequest>,
    ) -> Result<Response<RemoveInstallationResponse>, Status> {
        workspaces::remove_installation(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_installation_token(
        &self,
        request: Request<GetInstallationTokenRequest>,
    ) -> Result<Response<InstallationToken>, Status> {
        workspaces::installation_token(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn authorize(
        &self,
        request: Request<AuthorizeRequest>,
    ) -> Result<Response<Principal>, Status> {
        authorize::authorize(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn record_audit(
        &self,
        request: Request<RecordAuditRequest>,
    ) -> Result<Response<RecordAuditResponse>, Status> {
        audit::record(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_audit(
        &self,
        request: Request<ListAuditRequest>,
    ) -> Result<Response<ListAuditResponse>, Status> {
        audit::list(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_data_controls(
        &self,
        request: Request<GetDataControlsRequest>,
    ) -> Result<Response<DataControls>, Status> {
        controls::get(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn update_data_controls(
        &self,
        request: Request<UpdateDataControlsRequest>,
    ) -> Result<Response<DataControls>, Status> {
        controls::update(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn set_reasoner_key(
        &self,
        request: Request<SetReasonerKeyRequest>,
    ) -> Result<Response<DataControls>, Status> {
        controls::set_key(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_reasoner_key(
        &self,
        request: Request<GetReasonerKeyRequest>,
    ) -> Result<Response<ReasonerKey>, Status> {
        controls::get_key(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }
}
