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
    AuditEntry, GetInstallationTokenRequest, RecordAuditRequest,
    identity_service_client::IdentityServiceClient,
};
use tonic::{Request, transport::Channel};
use uuid::Uuid;

use crate::adapters::GithubAccess;

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
