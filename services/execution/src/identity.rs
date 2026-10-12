//! The audit log (CONTRACT.md §19) lives in the identity service. Entries
//! are fire-and-forget: the RPC returns at once, the write happens in the
//! background and a failure is only logged, because audit must never slow
//! down or fail the action it records. Trait-abstracted so tests need no
//! peer.

#[cfg(test)]
use std::sync::{Arc, Mutex};
use std::{sync::Mutex as SyncMutex, time::Duration};

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use sdi_common::{cache::TtlCache, grpc::MAX_MESSAGE_BYTES, principal::Principal};
use sdi_proto::identity::v1::{
    AuditEntry, GetInstallationTokenRequest, GetWorkspaceRequest, ListSecretsRequest,
    RecordAuditRequest, ResolveSecretRequest, identity_service_client::IdentityServiceClient,
};
use tonic::{Code, Request, Status, transport::Channel};
use uuid::Uuid;

use crate::adapters::{GithubAccess, SecretError, SecretSource};

/// Identity answers from memory or one row; a slower answer is an outage.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// Workspaces whose GitHub access is held at once.
const ACCESS_CACHE_CAPACITY: usize = 1024;
/// Longest a minted token is reused (installation tokens live an hour;
/// the refresh margin keeps a step from starting on one about to expire).
const ACCESS_CACHE_TTL: Duration = Duration::from_secs(50 * 60);
const REFRESH_MARGIN: chrono::Duration = chrono::Duration::minutes(5);

pub trait AuditSink: Send + Sync {
    fn record(&self, principal: &Principal, entry: AuditEntry);
}

/// Builds the entry for `principal` doing `action` on one entity.
pub fn entry(
    principal: &Principal,
    action: &str,
    entity: &str,
    entity_id: &str,
    detail: String,
) -> AuditEntry {
    AuditEntry {
        workspace_id: principal.workspace_id.clone(),
        user_id: principal.user_id.clone(),
        action: action.to_owned(),
        entity: entity.to_owned(),
        entity_id: entity_id.to_owned(),
        detail,
        at: Some(sdi_common::time::to_proto(Utc::now())),
        ..Default::default()
    }
}

pub struct GrpcAudit {
    client: IdentityServiceClient<Channel>,
}

impl GrpcAudit {
    pub fn new(url: &str) -> Result<Self> {
        Ok(Self {
            client: IdentityServiceClient::new(sdi_common::grpc::lazy_channel(url)?),
        })
    }
}

impl AuditSink for GrpcAudit {
    fn record(&self, principal: &Principal, entry: AuditEntry) {
        let mut client = self.client.clone();
        let mut request = Request::new(RecordAuditRequest { entry: Some(entry) });
        principal.apply(&mut request);
        request.set_timeout(CALL_TIMEOUT);
        sdi_common::tasks::spawn("audit-entry", async move {
            if let Err(status) = client.record_audit(request).await {
                tracing::warn!(%status, "could not record audit entry");
            }
        });
    }
}

/// The GitHub access a run's steps use (CONTRACT.md §18): the workspace's
/// installation or OAuth token with its host's API base, from identity.
/// `None` means identity could not say (unreachable, no installation), and
/// the adapters fall back to the process-wide dev configuration.
#[async_trait]
pub trait GithubAccessSource: Send + Sync {
    async fn access(&self, workspace_id: Uuid) -> Option<GithubAccess>;
}

/// Never answers: the process-wide `GITHUB_TOKEN` applies (tests).
#[cfg(test)]
pub struct NoGithubAccess;

#[cfg(test)]
#[async_trait]
impl GithubAccessSource for NoGithubAccess {
    async fn access(&self, _: Uuid) -> Option<GithubAccess> {
        None
    }
}

/// Always the same access: a fake GitHub for every workspace (tests).
#[cfg(test)]
pub struct StaticGithubAccess(pub GithubAccess);

#[cfg(test)]
#[async_trait]
impl GithubAccessSource for StaticGithubAccess {
    async fn access(&self, _: Uuid) -> Option<GithubAccess> {
        Some(self.0.clone())
    }
}

struct CachedAccess {
    access: GithubAccess,
    expires_at: chrono::DateTime<Utc>,
}

/// `Identity.GetInstallationToken` per workspace, cached until shortly
/// before the token expires (bounded; CONTRACT.md §19: tokens live in
/// memory only).
pub struct GrpcGithubAccess {
    client: IdentityServiceClient<Channel>,
    cache: SyncMutex<TtlCache<Uuid, CachedAccess>>,
}

impl GrpcGithubAccess {
    pub fn new(url: &str) -> Result<Self> {
        Ok(Self {
            client: IdentityServiceClient::new(sdi_common::grpc::lazy_channel(url)?)
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES),
            cache: SyncMutex::new(TtlCache::new(ACCESS_CACHE_CAPACITY, ACCESS_CACHE_TTL)),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TtlCache<Uuid, CachedAccess>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[async_trait]
impl GithubAccessSource for GrpcGithubAccess {
    async fn access(&self, workspace_id: Uuid) -> Option<GithubAccess> {
        let now = Utc::now();
        if let Some(cached) = self
            .lock()
            .get_if(&workspace_id, |c| c.expires_at - REFRESH_MARGIN > now)
        {
            return Some(cached.access.clone());
        }
        let principal = Principal::system(workspace_id.to_string());
        let mut request = Request::new(GetInstallationTokenRequest {
            workspace_id: workspace_id.to_string(),
        });
        principal.apply(&mut request);
        request.set_timeout(CALL_TIMEOUT);
        let mut client = self.client.clone();
        let token = match tokio::time::timeout(CALL_TIMEOUT, client.get_installation_token(request))
            .await
        {
            Ok(Ok(response)) => response.into_inner(),
            Ok(Err(status)) => {
                tracing::warn!(workspace = %workspace_id, %status, "no workspace GitHub access; using GITHUB_TOKEN if set");
                return None;
            }
            Err(_) => {
                tracing::warn!(workspace = %workspace_id, "identity timed out minting GitHub access; using GITHUB_TOKEN if set");
                return None;
            }
        };
        if token.token.trim().is_empty() {
            return None;
        }
        let access = GithubAccess {
            token: token.token,
            api_base_url: if token.api_base_url.trim().is_empty() {
                crate::adapters::github::DEFAULT_API.to_owned()
            } else {
                token.api_base_url.trim().trim_end_matches('/').to_owned()
            },
        };
        let expires_at = token
            .expires_at
            .as_ref()
            .and_then(sdi_common::time::from_proto)
            .unwrap_or(now + chrono::Duration::minutes(55));
        self.lock().insert(
            workspace_id,
            CachedAccess {
                access: access.clone(),
                expires_at,
            },
        );
        Some(access)
    }
}

/// What the delivery module asks identity about a workspace (CONTRACT.md
/// §22): its GitHub login, which is the namespace of the seeded GHCR
/// registry, and the names of its secrets, to validate what an environment
/// or a registry references. Values are never listed.
#[async_trait]
pub trait WorkspaceDirectory: Send + Sync {
    /// The GitHub login of the organisation or user behind the workspace.
    async fn slug(&self, workspace_id: Uuid) -> Result<String, Status>;
    /// The workspace's secret names, asked as `principal` (an admin, or the
    /// workspace's system principal).
    async fn secret_names(&self, principal: &Principal) -> Result<Vec<String>, Status>;
}

/// Identity over gRPC: the workspace directory and the secret resolver of
/// deployment runs.
pub struct GrpcIdentity {
    client: IdentityServiceClient<Channel>,
}

impl GrpcIdentity {
    pub fn new(url: &str) -> Result<Self> {
        Ok(Self {
            client: IdentityServiceClient::new(sdi_common::grpc::lazy_channel(url)?)
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES),
        })
    }
}

fn outgoing<T>(principal: &Principal, message: T) -> Request<T> {
    let mut request = Request::new(message);
    principal.apply(&mut request);
    request.set_timeout(CALL_TIMEOUT);
    request
}

#[async_trait]
impl WorkspaceDirectory for GrpcIdentity {
    async fn slug(&self, workspace_id: Uuid) -> Result<String, Status> {
        let principal = Principal::system(workspace_id.to_string());
        let workspace = self
            .client
            .clone()
            .get_workspace(outgoing(
                &principal,
                GetWorkspaceRequest {
                    id: workspace_id.to_string(),
                },
            ))
            .await
            .map_err(|status| match status.code() {
                Code::NotFound => Status::failed_precondition(
                    "the identity service does not know this workspace, so its registry namespace cannot be derived",
                ),
                _ => Status::unavailable(format!(
                    "the identity service could not describe the workspace: {}",
                    status.message()
                )),
            })?
            .into_inner();
        Ok(workspace.slug)
    }

    async fn secret_names(&self, principal: &Principal) -> Result<Vec<String>, Status> {
        let listed = self
            .client
            .clone()
            .list_secrets(outgoing(
                principal,
                ListSecretsRequest {
                    workspace_id: principal.workspace_id.clone(),
                },
            ))
            .await
            .map_err(|status| match status.code() {
                Code::PermissionDenied => status,
                _ => Status::unavailable(format!(
                    "the identity service could not list the workspace's secrets: {}",
                    status.message()
                )),
            })?
            .into_inner();
        Ok(listed.secrets.into_iter().map(|s| s.name).collect())
    }
}

/// `Identity.ResolveSecret` as the workspace's system principal: the one
/// caller identity hands a value to. The value lives in the step that
/// asked for it and nowhere else.
#[async_trait]
impl SecretSource for GrpcIdentity {
    async fn resolve(&self, workspace_id: Uuid, name: &str) -> Result<String, SecretError> {
        let principal = Principal::system(workspace_id.to_string());
        let request = outgoing(
            &principal,
            ResolveSecretRequest {
                workspace_id: workspace_id.to_string(),
                name: name.to_owned(),
            },
        );
        let mut client = self.client.clone();
        match tokio::time::timeout(CALL_TIMEOUT, client.resolve_secret(request)).await {
            Ok(Ok(resolved)) => Ok(resolved.into_inner().value),
            Ok(Err(status)) if status.code() == Code::NotFound => {
                Err(SecretError::NotFound(name.to_owned()))
            }
            Ok(Err(status)) => {
                tracing::warn!(workspace = %workspace_id, secret = name, %status, "secret not resolved");
                Err(SecretError::Unavailable(format!(
                    "the identity service answered {:?}",
                    status.code()
                )))
            }
            Err(_) => Err(SecretError::Unavailable(
                "the identity service did not answer in time".to_owned(),
            )),
        }
    }
}

/// A workspace directory that knows nothing: a service without an identity
/// service (tests of the pre-delivery RPCs).
#[cfg(test)]
pub struct NoWorkspaceDirectory;

#[cfg(test)]
#[async_trait]
impl WorkspaceDirectory for NoWorkspaceDirectory {
    async fn slug(&self, _: Uuid) -> Result<String, Status> {
        Err(Status::unavailable(
            "the identity service is not configured on this execution service",
        ))
    }

    async fn secret_names(&self, _: &Principal) -> Result<Vec<String>, Status> {
        Err(Status::unavailable(
            "the identity service is not configured on this execution service",
        ))
    }
}

/// Test double: one slug for every workspace and a shared, editable set of
/// secrets, which also serves as the runs' secret source.
#[cfg(test)]
#[derive(Clone)]
pub struct StaticWorkspaces {
    pub slug: String,
    pub secrets: crate::adapters::StaticSecrets,
    /// Principals that asked for the secret names.
    pub listed_by: Arc<Mutex<Vec<Principal>>>,
}

#[cfg(test)]
impl StaticWorkspaces {
    pub fn new(slug: &str, secrets: &[(&str, &str)]) -> Self {
        Self {
            slug: slug.to_owned(),
            secrets: crate::adapters::StaticSecrets::with(secrets),
            listed_by: Arc::default(),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl WorkspaceDirectory for StaticWorkspaces {
    async fn slug(&self, _: Uuid) -> Result<String, Status> {
        Ok(self.slug.clone())
    }

    async fn secret_names(&self, principal: &Principal) -> Result<Vec<String>, Status> {
        principal.require(sdi_common::principal::Role::Admin)?;
        self.listed_by
            .lock()
            .expect("listed lock")
            .push(principal.clone());
        Ok(self
            .secrets
            .0
            .lock()
            .expect("secrets lock")
            .keys()
            .cloned()
            .collect())
    }
}

/// Test double that remembers every entry.
#[cfg(test)]
#[derive(Default, Clone)]
pub struct RecordingAudit {
    pub entries: Arc<Mutex<Vec<(Principal, AuditEntry)>>>,
}

#[cfg(test)]
impl RecordingAudit {
    pub fn actions(&self) -> Vec<String> {
        self.entries
            .lock()
            .expect("audit lock")
            .iter()
            .map(|(_, e)| e.action.clone())
            .collect()
    }
}

#[cfg(test)]
impl AuditSink for RecordingAudit {
    fn record(&self, principal: &Principal, entry: AuditEntry) {
        self.entries
            .lock()
            .expect("audit lock")
            .push((principal.clone(), entry));
    }
}
