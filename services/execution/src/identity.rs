//! The audit log (CONTRACT.md §19) lives in the identity service. Entries
//! are fire-and-forget: the RPC returns at once, the write happens in the
//! background and a failure is only logged, because audit must never slow
//! down or fail the action it records. Trait-abstracted so tests need no
//! peer.

#[cfg(test)]
use std::sync::{Arc, Mutex};

use anyhow::Result;
use chrono::Utc;
use sdi_common::principal::Principal;
use sdi_proto::identity::v1::{
    AuditEntry, RecordAuditRequest, identity_service_client::IdentityServiceClient,
};
use tonic::{Request, transport::Channel};

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
        tokio::spawn(async move {
            if let Err(status) = client.record_audit(request).await {
                tracing::warn!(%status, "could not record audit entry");
            }
        });
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
