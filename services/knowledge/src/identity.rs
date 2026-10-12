//! The identity service as the knowledge service sees it: GitHub access per
//! workspace, data controls, and the audit log. Every call forwards the
//! caller's principal as metadata and is bounded by a short timeout, so an
//! identity outage degrades a request instead of hanging it.

use std::time::Duration;

use sdi_proto::identity::v1::{
    AuditEntry, DataControls, GetDataControlsRequest, GetInstallationTokenRequest,
    GetWorkspaceRequest, InstallationToken, RecordAuditRequest, Workspace,
    identity_service_client::IdentityServiceClient,
};
use tonic::{Request, Status, transport::Channel};

use crate::auth::Caller;

/// Identity answers from memory or one database row; anything slower is an
/// outage the caller should learn about quickly.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Identity {
    client: Option<IdentityServiceClient<Channel>>,
}

impl Identity {
    /// A lazily connected client: the service starts without identity and
    /// finds it once it is up.
    pub fn connect(url: &str) -> anyhow::Result<Self> {
        let channel = sdi_common::grpc::lazy_channel(url)?;
        Ok(Self {
            client: Some(IdentityServiceClient::new(channel)),
        })
    }

    /// No identity service at all: every call answers `UNAVAILABLE`. Tests
    /// and tools that only touch the database use this.
    pub fn disabled() -> Self {
        Self { client: None }
    }

    pub fn configured(&self) -> bool {
        self.client.is_some()
    }

    fn client(&self) -> Result<IdentityServiceClient<Channel>, Status> {
        self.client
            .clone()
            .ok_or_else(|| Status::unavailable("identity service not configured"))
    }

    fn request<T>(caller: &Caller, message: T) -> Request<T> {
        let mut request = Request::new(message);
        caller.principal().apply(&mut request);
        request.set_timeout(CALL_TIMEOUT);
        request
    }

    /// The workspace's GitHub access: an installation token for an
    /// organisation, the user's OAuth token for a personal workspace.
    pub async fn installation_token(&self, caller: &Caller) -> Result<InstallationToken, Status> {
        let mut client = self.client()?;
        let request = Self::request(
            caller,
            GetInstallationTokenRequest {
                workspace_id: caller.workspace_id.to_string(),
            },
        );
        Ok(timed(client.get_installation_token(request))
            .await?
            .into_inner())
    }

    pub async fn workspace(&self, caller: &Caller) -> Result<Workspace, Status> {
        let mut client = self.client()?;
        let request = Self::request(
            caller,
            GetWorkspaceRequest {
                id: caller.workspace_id.to_string(),
            },
        );
        Ok(timed(client.get_workspace(request)).await?.into_inner())
    }

    pub async fn data_controls(&self, caller: &Caller) -> Result<DataControls, Status> {
        let mut client = self.client()?;
        let request = Self::request(
            caller,
            GetDataControlsRequest {
                workspace_id: caller.workspace_id.to_string(),
            },
        );
        Ok(timed(client.get_data_controls(request)).await?.into_inner())
    }

    /// Fire-and-forget audit entry: never slows or fails the request it
    /// records, and a lost entry is logged.
    pub fn audit(
        &self,
        caller: &Caller,
        action: &str,
        entity: &str,
        entity_id: &str,
        detail: &str,
    ) {
        let Some(client) = self.client.clone() else {
            tracing::debug!(action, entity_id, "audit skipped: identity not configured");
            return;
        };
        let entry = AuditEntry {
            workspace_id: caller.workspace_id.to_string(),
            user_id: caller.user_id.clone(),
            action: action.to_owned(),
            entity: entity.to_owned(),
            entity_id: entity_id.to_owned(),
            detail: detail.to_owned(),
            at: Some(sdi_common::time::to_proto(chrono::Utc::now())),
            ..Default::default()
        };
        let request = Self::request(caller, RecordAuditRequest { entry: Some(entry) });
        let (action, entity_id) = (action.to_owned(), entity_id.to_owned());
        let mut client = client;
        sdi_common::tasks::spawn("audit-entry", async move {
            if let Err(error) = timed(client.record_audit(request)).await {
                tracing::warn!(%error, action, entity_id, "audit entry not recorded");
            }
        });
    }

    /// `file.read` for the code browser (CONTRACT.md §19).
    pub fn audit_file_read(
        &self,
        caller: &Caller,
        project: &str,
        repo: &str,
        commit: &str,
        path: &str,
    ) {
        self.audit(
            caller,
            "file.read",
            "file",
            path,
            &format!("{repo}@{commit} (project {project})"),
        );
    }
}

async fn timed<T>(call: impl Future<Output = Result<T, Status>>) -> Result<T, Status> {
    match tokio::time::timeout(CALL_TIMEOUT, call).await {
        Ok(result) => result,
        Err(_) => Err(Status::deadline_exceeded("identity service timed out")),
    }
}

/// Whether a status means identity could not be reached at all (as opposed
/// to identity answering that something is wrong), which is when dev-mode
/// fallbacks apply.
pub fn unreachable(status: &Status) -> bool {
    matches!(
        status.code(),
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded | tonic::Code::Unimplemented
    )
}
