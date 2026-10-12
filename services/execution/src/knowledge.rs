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
use tonic::{Request, Status, transport::Channel};

/// What a release needs to know about the project it is cut from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    /// `owner/name` on GitHub.
    pub repo_full_name: String,
    pub default_branch: String,
}

/// The project behind a `project_id` (`Knowledge.GetProject`), asked as the
/// caller: a project of another workspace is NOT_FOUND there too.
#[async_trait]
pub trait ProjectSource: Send + Sync {
    async fn project(&self, principal: &Principal, project_id: &str)
    -> Result<ProjectInfo, Status>;
}

/// No knowledge service: every lookup is UNAVAILABLE (tests).
#[cfg(test)]
pub struct NoProjects;

#[cfg(test)]
#[async_trait]
impl ProjectSource for NoProjects {
    async fn project(&self, _: &Principal, _: &str) -> Result<ProjectInfo, Status> {
        Err(Status::unavailable(
            "the knowledge service is not configured on this execution service",
        ))
    }
}

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

#[async_trait]
impl ProjectSource for GrpcPatternOutcomes {
    async fn project(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<ProjectInfo, Status> {
        let mut client = KnowledgeServiceClient::new(self.channel.clone());
        let project = client
            .get_project(outgoing(
                principal,
                GetProjectRequest {
                    id: project_id.to_owned(),
                },
            ))
            .await
            .map_err(|status| match status.code() {
                tonic::Code::NotFound
                | tonic::Code::InvalidArgument
                | tonic::Code::PermissionDenied => status,
                _ => Status::unavailable(format!(
                    "the knowledge service could not describe the project: {}",
                    status.message()
                )),
            })?
            .into_inner();
        Ok(ProjectInfo {
            id: project.id,
            name: project.name,
            repo_full_name: project.repo_full_name,
            default_branch: project.default_branch,
        })
    }
}

/// Test double: the projects it was given, per workspace.
#[cfg(test)]
#[derive(Default, Clone)]
pub struct StaticProjects(pub Arc<Mutex<Vec<(String, ProjectInfo)>>>);

#[cfg(test)]
impl StaticProjects {
    pub fn add(&self, workspace_id: &str, project: ProjectInfo) {
        self.0
            .lock()
            .expect("projects lock")
            .push((workspace_id.to_owned(), project));
    }
}

#[cfg(test)]
#[async_trait]
impl ProjectSource for StaticProjects {
    async fn project(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<ProjectInfo, Status> {
        self.0
            .lock()
            .expect("projects lock")
            .iter()
            .find(|(workspace, project)| {
                *workspace == principal.workspace_id && project.id == project_id
            })
            .map(|(_, project)| project.clone())
            .ok_or_else(|| Status::not_found("no such project"))
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
