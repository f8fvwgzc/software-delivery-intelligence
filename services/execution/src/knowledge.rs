//! The calls this service makes to knowledge: closing the loop with the
//! outcome of a pattern-backed run, and naming the project an execution
//! belongs to. Every call forwards a `Principal` as metadata (CONTRACT.md
//! §18): the caller's for RPC-driven work, the workspace's system principal
//! for what the runner does in the background. Trait-abstracted so tests
//! need no peer.

#[cfg(test)]
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use async_trait::async_trait;
use sdi_common::principal::Principal;
use sdi_proto::knowledge::v1::{
    GetProjectRequest, RecordPatternOutcomeRequest,
    knowledge_service_client::KnowledgeServiceClient,
};
use tonic::{Request, transport::Channel};

#[async_trait]
pub trait PatternOutcomes: Send + Sync {
    async fn record(
        &self,
        principal: &Principal,
        pattern_id: &str,
        project_id: &str,
        succeeded: bool,
    ) -> Result<()>;

    /// Display name of a project, or empty when knowledge cannot say: the
    /// execution list and header show it next to the title, and
    /// `CreateExecutionRequest` only carries the id.
    async fn project_name(&self, _principal: &Principal, _project_id: &str) -> String {
        String::new()
    }
}

pub struct GrpcPatternOutcomes {
    channel: Channel,
}

impl GrpcPatternOutcomes {
    pub fn new(url: &str) -> Result<Self> {
        Ok(Self {
            channel: sdi_common::grpc::lazy_channel(url)?,
        })
    }
}

fn outgoing<T>(principal: &Principal, message: T) -> Request<T> {
    let mut request = Request::new(message);
    principal.apply(&mut request);
    request
}

#[async_trait]
impl PatternOutcomes for GrpcPatternOutcomes {
    async fn record(
        &self,
        principal: &Principal,
        pattern_id: &str,
        project_id: &str,
        succeeded: bool,
    ) -> Result<()> {
        let mut client = KnowledgeServiceClient::new(self.channel.clone());
        client
            .record_pattern_outcome(outgoing(
                principal,
                RecordPatternOutcomeRequest {
                    pattern_id: pattern_id.to_owned(),
                    project_id: project_id.to_owned(),
                    succeeded,
                    incident: false,
                },
            ))
            .await
            .context("RecordPatternOutcome failed")?;
        Ok(())
    }

    async fn project_name(&self, principal: &Principal, project_id: &str) -> String {
        let mut client = KnowledgeServiceClient::new(self.channel.clone());
        match client
            .get_project(outgoing(
                principal,
                GetProjectRequest {
                    id: project_id.to_owned(),
                },
            ))
            .await
        {
            Ok(project) => project.into_inner().name,
            Err(status) => {
                tracing::warn!(project_id, %status, "could not resolve the project name");
                String::new()
            }
        }
    }
}

/// Test double that remembers every call and who made it.
#[cfg(test)]
#[derive(Default, Clone)]
pub struct RecordingPatternOutcomes {
    pub calls: Arc<Mutex<Vec<(String, String, bool)>>>,
    pub callers: Arc<Mutex<Vec<Principal>>>,
}

#[cfg(test)]
#[async_trait]
impl PatternOutcomes for RecordingPatternOutcomes {
    async fn record(
        &self,
        principal: &Principal,
        pattern_id: &str,
        project_id: &str,
        succeeded: bool,
    ) -> Result<()> {
        self.callers
            .lock()
            .expect("callers lock")
            .push(principal.clone());
        self.calls.lock().expect("outcomes lock").push((
            pattern_id.to_owned(),
            project_id.to_owned(),
            succeeded,
        ));
        Ok(())
    }

    async fn project_name(&self, principal: &Principal, _project_id: &str) -> String {
        self.callers
            .lock()
            .expect("callers lock")
            .push(principal.clone());
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use sdi_common::principal::{ROLE_HEADER, Role, USER_HEADER, WORKSPACE_HEADER};

    use super::*;

    #[test]
    fn outgoing_requests_carry_the_principal_as_metadata() {
        let principal = Principal {
            user_id: "u-1".into(),
            workspace_id: "ws-1".into(),
            role: Role::Member,
        };
        let request = outgoing(&principal, GetProjectRequest { id: "p".into() });
        let meta = request.metadata();
        assert_eq!(meta.get(USER_HEADER).unwrap(), "u-1");
        assert_eq!(meta.get(WORKSPACE_HEADER).unwrap(), "ws-1");
        assert_eq!(meta.get(ROLE_HEADER).unwrap(), "member");
        let system = Principal::system("ws-1");
        let request = outgoing(&system, GetProjectRequest { id: "p".into() });
        assert_eq!(request.metadata().get(USER_HEADER).unwrap(), "system");
        assert_eq!(request.metadata().get(ROLE_HEADER).unwrap(), "owner");
    }
}
