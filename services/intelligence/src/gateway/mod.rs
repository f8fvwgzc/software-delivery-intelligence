//! Outbound calls to the knowledge, execution and identity services behind
//! traits, so the proposal flow is testable without any peer running.
//! Every call forwards the caller's `Principal` as gRPC metadata
//! (CONTRACT.md §18 "service-to-service"), so the peer scopes its work to the
//! same workspace and role.

use async_trait::async_trait;
use sdi_common::principal::Principal;
use sdi_proto::{
    execution::v1::{
        CreateExecutionRequest, DeliveryStats, Environment, Execution, GetDeliveryStatsRequest,
        GetExecutionRequest, ListEnvironmentsRequest, ListRegistriesRequest, Registry,
        execution_service_client::ExecutionServiceClient,
    },
    identity::v1::{
        AuditEntry, DataControls, GetDataControlsRequest, GetInstallationTokenRequest,
        GetReasonerKeyRequest, InstallationToken, ReasonerKey, RecordAuditRequest,
        identity_service_client::IdentityServiceClient,
    },
    knowledge::v1::{
        GetGraphRequest, GetProjectContextRequest, GetPullRequestRequest, GetRepoFileRequest,
        GetRepoTreeRequest, Graph, IndexKnowledgeRequest, ListPatternsRequest,
        ListPipelineRunsRequest, ListSimilarProjectsRequest, Pattern, PipelineRun, ProjectContext,
        PullRequest, RepoFile, RepoTree, SimilarProject,
        knowledge_service_client::KnowledgeServiceClient,
    },
};
use tonic::{Code, Request, Status, transport::Channel};

#[derive(Debug, thiserror::Error)]
#[error("{service}: {status}")]
pub struct GatewayError {
    pub service: &'static str,
    pub status: Status,
}

impl GatewayError {
    /// Upstream "not found" and "invalid argument" are the caller's problem;
    /// everything else means the peer is unreachable or broken.
    pub fn into_status(self) -> Status {
        match self.status.code() {
            Code::NotFound => {
                Status::not_found(format!("{}: {}", self.service, self.status.message()))
            }
            Code::InvalidArgument => {
                Status::invalid_argument(format!("{}: {}", self.service, self.status.message()))
            }
            Code::FailedPrecondition => {
                Status::failed_precondition(format!("{}: {}", self.service, self.status.message()))
            }
            Code::PermissionDenied => {
                Status::permission_denied(format!("{}: {}", self.service, self.status.message()))
            }
            code => {
                tracing::warn!(service = self.service, %code, message = self.status.message(), "upstream call failed");
                Status::unavailable(format!(
                    "{} service unavailable: {}",
                    self.service,
                    self.status.message()
                ))
            }
        }
    }
}

/// A request carrying the caller's identity for a peer service.
pub fn outgoing<T>(principal: &Principal, message: T) -> Request<T> {
    let mut request = Request::new(message);
    principal.apply(&mut request);
    request
}

#[async_trait]
pub trait KnowledgeGateway: Send + Sync {
    async fn project_context(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<ProjectContext, GatewayError>;
    async fn list_patterns(
        &self,
        principal: &Principal,
        category: &str,
    ) -> Result<Vec<Pattern>, GatewayError>;
    async fn similar_projects(
        &self,
        principal: &Principal,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<SimilarProject>, GatewayError>;
    async fn pipeline_runs(
        &self,
        principal: &Principal,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<PipelineRun>, GatewayError>;
    async fn index_knowledge(
        &self,
        principal: &Principal,
        request: IndexKnowledgeRequest,
    ) -> Result<String, GatewayError>;
    /// The graph alone (`ProjectContext.graph` carries it too; this is the
    /// cheaper call when nothing else is needed).
    async fn graph(
        &self,
        principal: &Principal,
        project_id: &str,
        include_history: bool,
    ) -> Result<Graph, GatewayError>;
    /// Code browser (CONTRACT.md §15): the whole tree at the scanned commit.
    async fn repo_tree(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<RepoTree, GatewayError>;
    /// One file's text at the scanned commit.
    async fn repo_file(
        &self,
        principal: &Principal,
        project_id: &str,
        path: &str,
    ) -> Result<RepoFile, GatewayError>;
    /// A pull request with its bounded diff, touched facts and impacted
    /// nodes (CONTRACT.md §20).
    async fn pull_request(
        &self,
        principal: &Principal,
        project_id: &str,
        number: u64,
    ) -> Result<PullRequest, GatewayError>;
}

#[async_trait]
pub trait ExecutionGateway: Send + Sync {
    async fn create_execution(
        &self,
        principal: &Principal,
        request: CreateExecutionRequest,
    ) -> Result<Execution, GatewayError>;
    async fn get_execution(
        &self,
        principal: &Principal,
        id: &str,
    ) -> Result<Execution, GatewayError>;
    async fn delivery_stats(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<DeliveryStats, GatewayError>;
    /// Where the workspace's images live (CONTRACT.md §22).
    async fn list_registries(&self, principal: &Principal) -> Result<Vec<Registry>, GatewayError>;
    /// The project's environments, in promotion order.
    async fn list_environments(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<Vec<Environment>, GatewayError>;
}

/// The identity service: per-workspace data controls, the workspace's own
/// model key (BYOK) and the audit log.
#[async_trait]
pub trait IdentityGateway: Send + Sync {
    async fn data_controls(&self, principal: &Principal) -> Result<DataControls, GatewayError>;
    /// `Ok(None)` when the workspace stored no key.
    async fn reasoner_key(
        &self,
        principal: &Principal,
    ) -> Result<Option<ReasonerKey>, GatewayError>;
    /// Fire-and-forget: returns at once, the entry is written in the
    /// background and a failure is only logged. Audit must never slow down
    /// or fail the action it records.
    fn record_audit(&self, principal: &Principal, entry: AuditEntry);
    /// The workspace's GitHub access (installation token or the user's
    /// OAuth token, with its API base), for publishing a review.
    async fn installation_token(
        &self,
        principal: &Principal,
    ) -> Result<InstallationToken, GatewayError>;
}

fn knowledge_err(status: Status) -> GatewayError {
    GatewayError {
        service: "knowledge",
        status,
    }
}

fn execution_err(status: Status) -> GatewayError {
    GatewayError {
        service: "execution",
        status,
    }
}

fn identity_err(status: Status) -> GatewayError {
    GatewayError {
        service: "identity",
        status,
    }
}

/// Real client over a lazy channel; cloning a tonic client is cheap.
#[derive(Clone)]
pub struct GrpcKnowledge {
    client: KnowledgeServiceClient<Channel>,
}

impl GrpcKnowledge {
    pub fn new(channel: Channel) -> Self {
        Self {
            client: KnowledgeServiceClient::new(channel),
        }
    }
}

#[async_trait]
impl KnowledgeGateway for GrpcKnowledge {
    async fn project_context(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<ProjectContext, GatewayError> {
        self.client
            .clone()
            .get_project_context(outgoing(
                principal,
                GetProjectContextRequest {
                    project_id: project_id.to_owned(),
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }

    async fn list_patterns(
        &self,
        principal: &Principal,
        category: &str,
    ) -> Result<Vec<Pattern>, GatewayError> {
        self.client
            .clone()
            .list_patterns(outgoing(
                principal,
                ListPatternsRequest {
                    category: category.to_owned(),
                    include_deprecated: false,
                },
            ))
            .await
            .map(|r| r.into_inner().patterns)
            .map_err(knowledge_err)
    }

    async fn similar_projects(
        &self,
        principal: &Principal,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<SimilarProject>, GatewayError> {
        self.client
            .clone()
            .list_similar_projects(outgoing(
                principal,
                ListSimilarProjectsRequest {
                    project_id: project_id.to_owned(),
                    limit,
                },
            ))
            .await
            .map(|r| r.into_inner().projects)
            .map_err(knowledge_err)
    }

    async fn pipeline_runs(
        &self,
        principal: &Principal,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<PipelineRun>, GatewayError> {
        self.client
            .clone()
            .list_pipeline_runs(outgoing(
                principal,
                ListPipelineRunsRequest {
                    project_id: project_id.to_owned(),
                    refresh: false,
                    limit,
                },
            ))
            .await
            .map(|r| r.into_inner().runs)
            .map_err(knowledge_err)
    }

    async fn index_knowledge(
        &self,
        principal: &Principal,
        request: IndexKnowledgeRequest,
    ) -> Result<String, GatewayError> {
        self.client
            .clone()
            .index_knowledge(outgoing(principal, request))
            .await
            .map(|r| r.into_inner().id)
            .map_err(knowledge_err)
    }

    async fn graph(
        &self,
        principal: &Principal,
        project_id: &str,
        include_history: bool,
    ) -> Result<Graph, GatewayError> {
        self.client
            .clone()
            .get_graph(outgoing(
                principal,
                GetGraphRequest {
                    project_id: project_id.to_owned(),
                    include_history,
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }

    async fn repo_tree(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<RepoTree, GatewayError> {
        self.client
            .clone()
            .get_repo_tree(outgoing(
                principal,
                GetRepoTreeRequest {
                    project_id: project_id.to_owned(),
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }

    async fn repo_file(
        &self,
        principal: &Principal,
        project_id: &str,
        path: &str,
    ) -> Result<RepoFile, GatewayError> {
        self.client
            .clone()
            .get_repo_file(outgoing(
                principal,
                GetRepoFileRequest {
                    project_id: project_id.to_owned(),
                    path: path.to_owned(),
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }

    async fn pull_request(
        &self,
        principal: &Principal,
        project_id: &str,
        number: u64,
    ) -> Result<PullRequest, GatewayError> {
        self.client
            .clone()
            .get_pull_request(outgoing(
                principal,
                GetPullRequestRequest {
                    project_id: project_id.to_owned(),
                    number,
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }
}

#[derive(Clone)]
pub struct GrpcExecution {
    client: ExecutionServiceClient<Channel>,
}

impl GrpcExecution {
    pub fn new(channel: Channel) -> Self {
        Self {
            client: ExecutionServiceClient::new(channel),
        }
    }
}

#[async_trait]
impl ExecutionGateway for GrpcExecution {
    async fn create_execution(
        &self,
        principal: &Principal,
        request: CreateExecutionRequest,
    ) -> Result<Execution, GatewayError> {
        self.client
            .clone()
            .create_execution(outgoing(principal, request))
            .await
            .map(|r| r.into_inner())
            .map_err(execution_err)
    }

    async fn get_execution(
        &self,
        principal: &Principal,
        id: &str,
    ) -> Result<Execution, GatewayError> {
        self.client
            .clone()
            .get_execution(outgoing(
                principal,
                GetExecutionRequest { id: id.to_owned() },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(execution_err)
    }

    async fn delivery_stats(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<DeliveryStats, GatewayError> {
        self.client
            .clone()
            .get_delivery_stats(outgoing(
                principal,
                GetDeliveryStatsRequest {
                    project_id: project_id.to_owned(),
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(execution_err)
    }

    async fn list_registries(&self, principal: &Principal) -> Result<Vec<Registry>, GatewayError> {
        self.client
            .clone()
            .list_registries(outgoing(principal, ListRegistriesRequest {}))
            .await
            .map(|r| r.into_inner().registries)
            .map_err(execution_err)
    }

    async fn list_environments(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<Vec<Environment>, GatewayError> {
        self.client
            .clone()
            .list_environments(outgoing(
                principal,
                ListEnvironmentsRequest {
                    project_id: project_id.to_owned(),
                },
            ))
            .await
            .map(|r| r.into_inner().environments)
            .map_err(execution_err)
    }
}

#[derive(Clone)]
pub struct GrpcIdentity {
    client: IdentityServiceClient<Channel>,
}

impl GrpcIdentity {
    pub fn new(channel: Channel) -> Self {
        Self {
            client: IdentityServiceClient::new(channel),
        }
    }
}

#[async_trait]
impl IdentityGateway for GrpcIdentity {
    async fn data_controls(&self, principal: &Principal) -> Result<DataControls, GatewayError> {
        self.client
            .clone()
            .get_data_controls(outgoing(
                principal,
                GetDataControlsRequest {
                    workspace_id: principal.workspace_id.clone(),
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(identity_err)
    }

    async fn reasoner_key(
        &self,
        principal: &Principal,
    ) -> Result<Option<ReasonerKey>, GatewayError> {
        match self
            .client
            .clone()
            .get_reasoner_key(outgoing(
                principal,
                GetReasonerKeyRequest {
                    workspace_id: principal.workspace_id.clone(),
                },
            ))
            .await
        {
            Ok(key) => Ok(Some(key.into_inner())),
            Err(status) if status.code() == Code::NotFound => Ok(None),
            Err(status) => Err(identity_err(status)),
        }
    }

    fn record_audit(&self, principal: &Principal, entry: AuditEntry) {
        let mut client = self.client.clone();
        let request = outgoing(principal, RecordAuditRequest { entry: Some(entry) });
        tokio::spawn(async move {
            if let Err(status) = client.record_audit(request).await {
                tracing::warn!(%status, "could not record audit entry");
            }
        });
    }

    async fn installation_token(
        &self,
        principal: &Principal,
    ) -> Result<InstallationToken, GatewayError> {
        self.client
            .clone()
            .get_installation_token(outgoing(
                principal,
                GetInstallationTokenRequest {
                    workspace_id: principal.workspace_id.clone(),
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(identity_err)
    }
}

#[cfg(test)]
pub mod fake {
    use std::{collections::BTreeMap, sync::Mutex};

    use sdi_proto::knowledge::v1::RepoTreeEntry;

    use super::*;

    /// In-memory knowledge: a fixed context and catalog, a repository tree
    /// with file contents, and a record of what was indexed and who asked.
    #[derive(Default)]
    pub struct FakeKnowledge {
        pub context: Option<ProjectContext>,
        pub patterns: Vec<Pattern>,
        pub similar: Vec<SimilarProject>,
        pub runs: Vec<PipelineRun>,
        pub indexed: Mutex<Vec<IndexKnowledgeRequest>>,
        pub tree: Vec<RepoTreeEntry>,
        pub files: BTreeMap<String, String>,
        /// Pull requests by number; `None` for every number otherwise.
        pub pull_requests: BTreeMap<u64, PullRequest>,
        /// Paths asked for through `repo_file`, in order.
        pub files_read: Mutex<Vec<String>>,
        /// The principal every call arrived with, in order.
        pub callers: Mutex<Vec<Principal>>,
    }

    impl FakeKnowledge {
        fn saw(&self, principal: &Principal) {
            self.callers.lock().unwrap().push(principal.clone());
        }
    }

    #[async_trait]
    impl KnowledgeGateway for FakeKnowledge {
        async fn project_context(
            &self,
            principal: &Principal,
            project_id: &str,
        ) -> Result<ProjectContext, GatewayError> {
            self.saw(principal);
            self.context
                .clone()
                .filter(|c| c.project.as_ref().is_some_and(|p| p.id == project_id))
                .ok_or_else(|| knowledge_err(Status::not_found("project not found")))
        }

        async fn list_patterns(
            &self,
            principal: &Principal,
            category: &str,
        ) -> Result<Vec<Pattern>, GatewayError> {
            self.saw(principal);
            Ok(self
                .patterns
                .iter()
                .filter(|p| category.is_empty() || p.category == category)
                .cloned()
                .collect())
        }

        async fn similar_projects(
            &self,
            principal: &Principal,
            _project_id: &str,
            limit: u32,
        ) -> Result<Vec<SimilarProject>, GatewayError> {
            self.saw(principal);
            Ok(self.similar.iter().take(limit as usize).cloned().collect())
        }

        async fn pipeline_runs(
            &self,
            principal: &Principal,
            _project_id: &str,
            limit: u32,
        ) -> Result<Vec<PipelineRun>, GatewayError> {
            self.saw(principal);
            Ok(self.runs.iter().take(limit as usize).cloned().collect())
        }

        async fn index_knowledge(
            &self,
            principal: &Principal,
            request: IndexKnowledgeRequest,
        ) -> Result<String, GatewayError> {
            self.saw(principal);
            let mut indexed = self.indexed.lock().unwrap();
            indexed.push(request);
            Ok(format!("chunk-{}", indexed.len()))
        }

        async fn graph(
            &self,
            principal: &Principal,
            project_id: &str,
            _include_history: bool,
        ) -> Result<Graph, GatewayError> {
            Ok(self
                .project_context(principal, project_id)
                .await?
                .graph
                .unwrap_or_default())
        }

        async fn repo_tree(
            &self,
            principal: &Principal,
            project_id: &str,
        ) -> Result<RepoTree, GatewayError> {
            let context = self.project_context(principal, project_id).await?;
            Ok(RepoTree {
                project_id: project_id.to_owned(),
                commit_sha: context
                    .understanding
                    .map(|u| u.commit_sha)
                    .unwrap_or_default(),
                entries: self.tree.clone(),
                truncated: false,
            })
        }

        async fn repo_file(
            &self,
            principal: &Principal,
            project_id: &str,
            path: &str,
        ) -> Result<RepoFile, GatewayError> {
            self.project_context(principal, project_id).await?;
            self.files_read.lock().unwrap().push(path.to_owned());
            let content = self
                .files
                .get(path)
                .cloned()
                .ok_or_else(|| knowledge_err(Status::not_found(format!("{path} not found"))))?;
            Ok(RepoFile {
                path: path.to_owned(),
                size: content.len() as u64,
                content,
                ..Default::default()
            })
        }

        async fn pull_request(
            &self,
            principal: &Principal,
            project_id: &str,
            number: u64,
        ) -> Result<PullRequest, GatewayError> {
            self.project_context(principal, project_id).await?;
            self.pull_requests.get(&number).cloned().ok_or_else(|| {
                knowledge_err(Status::not_found(format!(
                    "pull request #{number} not found"
                )))
            })
        }
    }

    #[derive(Default)]
    pub struct FakeExecution {
        pub created: Mutex<Vec<CreateExecutionRequest>>,
        pub stats: Option<DeliveryStats>,
        pub callers: Mutex<Vec<Principal>>,
        /// The workspace's registries (CONTRACT.md §22).
        pub registries: Mutex<Vec<Registry>>,
        /// Environments of every project; `list_environments` filters.
        pub environments: Mutex<Vec<Environment>>,
        /// The delivery RPCs answer `UNAVAILABLE` while set.
        pub delivery_down: std::sync::atomic::AtomicBool,
    }

    impl FakeExecution {
        fn delivery_up(&self) -> Result<(), GatewayError> {
            if self.delivery_down.load(std::sync::atomic::Ordering::SeqCst) {
                Err(execution_err(Status::unavailable("connection refused")))
            } else {
                Ok(())
            }
        }
    }

    #[async_trait]
    impl ExecutionGateway for FakeExecution {
        async fn create_execution(
            &self,
            principal: &Principal,
            request: CreateExecutionRequest,
        ) -> Result<Execution, GatewayError> {
            self.callers.lock().unwrap().push(principal.clone());
            let mut created = self.created.lock().unwrap();
            created.push(request.clone());
            Ok(Execution {
                id: format!("exec-{}", created.len()),
                number: created.len() as u64,
                project_id: request.project_id,
                proposal_id: request.proposal_id,
                pattern_id: request.pattern_id,
                title: request.title,
                ..Default::default()
            })
        }

        async fn get_execution(
            &self,
            principal: &Principal,
            id: &str,
        ) -> Result<Execution, GatewayError> {
            self.callers.lock().unwrap().push(principal.clone());
            Ok(Execution {
                id: id.to_owned(),
                ..Default::default()
            })
        }

        async fn delivery_stats(
            &self,
            principal: &Principal,
            _project_id: &str,
        ) -> Result<DeliveryStats, GatewayError> {
            self.callers.lock().unwrap().push(principal.clone());
            Ok(self.stats.unwrap_or_default())
        }

        async fn list_registries(
            &self,
            principal: &Principal,
        ) -> Result<Vec<Registry>, GatewayError> {
            self.callers.lock().unwrap().push(principal.clone());
            self.delivery_up()?;
            Ok(self.registries.lock().unwrap().clone())
        }

        async fn list_environments(
            &self,
            principal: &Principal,
            project_id: &str,
        ) -> Result<Vec<Environment>, GatewayError> {
            self.callers.lock().unwrap().push(principal.clone());
            self.delivery_up()?;
            Ok(self
                .environments
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.project_id == project_id)
                .cloned()
                .collect())
        }
    }

    /// Per-workspace data controls and keys, plus every audit entry recorded.
    #[derive(Default)]
    pub struct FakeIdentity {
        pub controls: Mutex<BTreeMap<String, DataControls>>,
        pub keys: Mutex<BTreeMap<String, ReasonerKey>>,
        pub audit: Mutex<Vec<(Principal, AuditEntry)>>,
        /// GitHub access per workspace: `(token, api base url)`.
        pub tokens: Mutex<BTreeMap<String, (String, String)>>,
    }

    impl FakeIdentity {
        pub fn set_reasoner(&self, workspace_id: &str, reasoner: &str, key: Option<&str>) {
            self.controls.lock().unwrap().insert(
                workspace_id.to_owned(),
                DataControls {
                    workspace_id: workspace_id.to_owned(),
                    reasoner: reasoner.to_owned(),
                    reasoner_key_configured: key.is_some(),
                    ..Default::default()
                },
            );
            let mut keys = self.keys.lock().unwrap();
            match key {
                Some(key) => {
                    keys.insert(
                        workspace_id.to_owned(),
                        ReasonerKey {
                            api_key: key.to_owned(),
                            model: String::new(),
                        },
                    );
                }
                None => {
                    keys.remove(workspace_id);
                }
            }
        }

        pub fn actions(&self) -> Vec<String> {
            self.audit
                .lock()
                .unwrap()
                .iter()
                .map(|(_, e)| e.action.clone())
                .collect()
        }
    }

    #[async_trait]
    impl IdentityGateway for FakeIdentity {
        async fn data_controls(&self, principal: &Principal) -> Result<DataControls, GatewayError> {
            Ok(self
                .controls
                .lock()
                .unwrap()
                .get(&principal.workspace_id)
                .cloned()
                .unwrap_or_else(|| DataControls {
                    workspace_id: principal.workspace_id.clone(),
                    reasoner: "off".to_owned(),
                    ..Default::default()
                }))
        }

        async fn reasoner_key(
            &self,
            principal: &Principal,
        ) -> Result<Option<ReasonerKey>, GatewayError> {
            Ok(self
                .keys
                .lock()
                .unwrap()
                .get(&principal.workspace_id)
                .cloned())
        }

        fn record_audit(&self, principal: &Principal, entry: AuditEntry) {
            self.audit.lock().unwrap().push((principal.clone(), entry));
        }

        async fn installation_token(
            &self,
            principal: &Principal,
        ) -> Result<InstallationToken, GatewayError> {
            self.tokens
                .lock()
                .unwrap()
                .get(&principal.workspace_id)
                .cloned()
                .map(|(token, api_base_url)| InstallationToken {
                    token,
                    api_base_url,
                    expires_at: None,
                })
                .ok_or_else(|| {
                    identity_err(Status::not_found("no GitHub access for this workspace"))
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use sdi_common::principal::{ROLE_HEADER, Role, USER_HEADER, WORKSPACE_HEADER};

    use super::*;

    #[test]
    fn outgoing_requests_carry_the_callers_metadata() {
        let principal = Principal {
            user_id: "u-1".into(),
            workspace_id: "ws-1".into(),
            role: Role::Member,
        };
        let request = outgoing(&principal, GetExecutionRequest { id: "x".into() });
        let meta = request.metadata();
        assert_eq!(meta.get(USER_HEADER).unwrap(), "u-1");
        assert_eq!(meta.get(WORKSPACE_HEADER).unwrap(), "ws-1");
        assert_eq!(meta.get(ROLE_HEADER).unwrap(), "member");
        assert_eq!(Principal::from_request(&request).unwrap(), principal);
    }
}
