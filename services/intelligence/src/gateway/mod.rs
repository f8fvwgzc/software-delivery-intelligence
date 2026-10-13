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
        StartExecutionRequest, execution_service_client::ExecutionServiceClient,
    },
    identity::v1::{
        AuditEntry, DataControls, GetDataControlsRequest, GetInstallationTokenRequest,
        GetReasonerKeyRequest, InstallationToken, ReasonerKey, RecordAuditRequest,
        identity_service_client::IdentityServiceClient,
    },
    knowledge::v1::{
        GetGraphRequest, GetProjectContextRequest, GetProjectRequest, GetPullRequestRequest,
        GetPullRequestStatusRequest, GetRepoFileRequest, GetRepoTreeRequest,
        GetRepositorySecurityRequest, Graph, IndexKnowledgeRequest, ListDependenciesRequest,
        ListDependenciesResponse, ListPatternsRequest, ListPipelineRunsRequest,
        ListProjectsRequest, ListPullRequestsRequest, ListSimilarProjectsRequest, Pattern,
        PipelineRun, Project, ProjectContext, PullRequest, PullRequestStatus, PullRequestSummary,
        RepoFile, RepoTree, RepositorySecurity, SimilarProject,
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

    // --- Pull request autopilot (CONTRACT.md §23) ---------------------------

    /// One file's text at `git_ref` (a pull request's head commit, a full
    /// SHA) instead of the scanned commit. Knowledge never caches it.
    async fn repo_file_at(
        &self,
        principal: &Principal,
        project_id: &str,
        path: &str,
        git_ref: &str,
    ) -> Result<RepoFile, GatewayError>;
    /// One connected project: its repository and default branch.
    async fn project(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<Project, GatewayError>;
    /// Every project of the principal's workspace.
    async fn list_projects(&self, principal: &Principal) -> Result<Vec<Project>, GatewayError>;
    /// The project's pull requests in `state`; `refresh` asks GitHub again.
    async fn list_pull_requests(
        &self,
        principal: &Principal,
        project_id: &str,
        state: &str,
        refresh: bool,
        limit: u32,
    ) -> Result<Vec<PullRequestSummary>, GatewayError>;
    /// Whether one pull request can be merged, read from GitHub now.
    async fn pull_request_status(
        &self,
        principal: &Principal,
        project_id: &str,
        number: u64,
    ) -> Result<PullRequestStatus, GatewayError>;
    /// Dependabot's state for the repository and its open alerts.
    async fn repository_security(
        &self,
        principal: &Principal,
        project_id: &str,
        refresh: bool,
    ) -> Result<RepositorySecurity, GatewayError>;
    /// The dependencies the last scan found in the manifests.
    async fn list_dependencies(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<ListDependenciesResponse, GatewayError>;
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
    /// Sets a created execution going: it runs until its first approval
    /// (or to the end when the workspace's policies ask for none).
    async fn start_execution(
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
                    r#ref: String::new(),
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

    async fn repo_file_at(
        &self,
        principal: &Principal,
        project_id: &str,
        path: &str,
        git_ref: &str,
    ) -> Result<RepoFile, GatewayError> {
        self.client
            .clone()
            .get_repo_file(outgoing(
                principal,
                GetRepoFileRequest {
                    project_id: project_id.to_owned(),
                    path: path.to_owned(),
                    r#ref: git_ref.to_owned(),
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }

    async fn project(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<Project, GatewayError> {
        self.client
            .clone()
            .get_project(outgoing(
                principal,
                GetProjectRequest {
                    id: project_id.to_owned(),
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }

    async fn list_projects(&self, principal: &Principal) -> Result<Vec<Project>, GatewayError> {
        self.client
            .clone()
            .list_projects(outgoing(principal, ListProjectsRequest {}))
            .await
            .map(|r| r.into_inner().projects)
            .map_err(knowledge_err)
    }

    async fn list_pull_requests(
        &self,
        principal: &Principal,
        project_id: &str,
        state: &str,
        refresh: bool,
        limit: u32,
    ) -> Result<Vec<PullRequestSummary>, GatewayError> {
        self.client
            .clone()
            .list_pull_requests(outgoing(
                principal,
                ListPullRequestsRequest {
                    project_id: project_id.to_owned(),
                    state: state.to_owned(),
                    refresh,
                    limit,
                },
            ))
            .await
            .map(|r| r.into_inner().pull_requests)
            .map_err(knowledge_err)
    }

    async fn pull_request_status(
        &self,
        principal: &Principal,
        project_id: &str,
        number: u64,
    ) -> Result<PullRequestStatus, GatewayError> {
        self.client
            .clone()
            .get_pull_request_status(outgoing(
                principal,
                GetPullRequestStatusRequest {
                    project_id: project_id.to_owned(),
                    number,
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }

    async fn repository_security(
        &self,
        principal: &Principal,
        project_id: &str,
        refresh: bool,
    ) -> Result<RepositorySecurity, GatewayError> {
        self.client
            .clone()
            .get_repository_security(outgoing(
                principal,
                GetRepositorySecurityRequest {
                    project_id: project_id.to_owned(),
                    refresh,
                },
            ))
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }

    async fn list_dependencies(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<ListDependenciesResponse, GatewayError> {
        self.client
            .clone()
            .list_dependencies(outgoing(
                principal,
                ListDependenciesRequest {
                    project_id: project_id.to_owned(),
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

    async fn start_execution(
        &self,
        principal: &Principal,
        id: &str,
    ) -> Result<Execution, GatewayError> {
        self.client
            .clone()
            .start_execution(outgoing(
                principal,
                StartExecutionRequest {
                    id: id.to_owned(),
                    fault: None,
                },
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
    use std::{
        collections::BTreeMap,
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use sdi_proto::{execution::v1::ExecutionStatus, knowledge::v1::RepoTreeEntry};

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
        /// Pull requests by number; `None` for every number otherwise. A
        /// test changes a head or a state between two calls through the lock.
        pub pull_requests: Mutex<BTreeMap<u64, PullRequest>>,
        /// Paths asked for through `repo_file`, in order.
        pub files_read: Mutex<Vec<String>>,
        /// The principal every call arrived with, in order.
        pub callers: Mutex<Vec<Principal>>,
        /// Contexts of further projects, by project id (`context` is tried first).
        pub contexts: Mutex<BTreeMap<String, ProjectContext>>,
        /// The projects `ListProjects` answers, per workspace id.
        pub projects: Mutex<BTreeMap<String, Vec<Project>>>,
        /// Calls that fail: `ws:<workspace id>` fails `ListProjects`,
        /// `project:<project id>` fails `ListPullRequests`, with this code.
        pub failing: Mutex<BTreeMap<String, Code>>,
        /// Every `ListPullRequests` call: `(project, state, refresh, limit)`.
        pub list_calls: Mutex<Vec<(String, String, bool, u32)>>,
        /// Files at a commit, by `(ref, path)`.
        pub files_at: Mutex<BTreeMap<(String, String), RepoFile>>,
        /// `(ref, path)` of every `repo_file_at` call, in order.
        pub refs_read: Mutex<Vec<(String, String)>>,
        /// What GitHub says about a pull request, by number. Without an
        /// entry the pull request is clean and mergeable with every method.
        pub statuses: Mutex<BTreeMap<u64, PullRequestStatus>>,
        /// Pull request numbers `GetPullRequestStatus` was asked for.
        pub status_calls: Mutex<Vec<u64>>,
        /// `None` answers `UNAVAILABLE`.
        pub security: Mutex<Option<RepositorySecurity>>,
        /// The `refresh` flag of every `GetRepositorySecurity` call.
        pub security_calls: Mutex<Vec<bool>>,
        /// `None` answers `UNAVAILABLE`.
        pub dependencies: Mutex<Option<ListDependenciesResponse>>,
    }

    impl FakeKnowledge {
        fn saw(&self, principal: &Principal) {
            self.callers.lock().unwrap().push(principal.clone());
        }

        fn failure(&self, key: &str) -> Option<GatewayError> {
            self.failing
                .lock()
                .unwrap()
                .get(key)
                .map(|code| knowledge_err(Status::new(*code, format!("{key} is failing"))))
        }

        /// Stores a file at a commit for `repo_file_at`.
        pub fn put_file(&self, git_ref: &str, path: &str, content: &str) {
            self.files_at.lock().unwrap().insert(
                (git_ref.to_owned(), path.to_owned()),
                RepoFile {
                    path: path.to_owned(),
                    commit_sha: git_ref.to_owned(),
                    content: content.to_owned(),
                    size: content.len() as u64,
                    ..Default::default()
                },
            );
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
                .or_else(|| self.contexts.lock().unwrap().get(project_id).cloned())
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
            self.pull_requests
                .lock()
                .unwrap()
                .get(&number)
                .filter(|pr| {
                    pr.summary
                        .as_ref()
                        .is_none_or(|s| s.project_id.is_empty() || s.project_id == project_id)
                })
                .cloned()
                .ok_or_else(|| {
                    knowledge_err(Status::not_found(format!(
                        "pull request #{number} not found"
                    )))
                })
        }

        async fn repo_file_at(
            &self,
            principal: &Principal,
            _project_id: &str,
            path: &str,
            git_ref: &str,
        ) -> Result<RepoFile, GatewayError> {
            self.saw(principal);
            self.refs_read
                .lock()
                .unwrap()
                .push((git_ref.to_owned(), path.to_owned()));
            self.files_at
                .lock()
                .unwrap()
                .get(&(git_ref.to_owned(), path.to_owned()))
                .cloned()
                .ok_or_else(|| {
                    knowledge_err(Status::not_found(format!("{path} not found at {git_ref}")))
                })
        }

        async fn project(
            &self,
            principal: &Principal,
            project_id: &str,
        ) -> Result<Project, GatewayError> {
            let listed = self
                .projects
                .lock()
                .unwrap()
                .get(&principal.workspace_id)
                .and_then(|projects| projects.iter().find(|p| p.id == project_id).cloned());
            match listed {
                Some(project) => {
                    self.saw(principal);
                    Ok(project)
                }
                None => self
                    .project_context(principal, project_id)
                    .await?
                    .project
                    .ok_or_else(|| knowledge_err(Status::not_found("project not found"))),
            }
        }

        async fn list_projects(&self, principal: &Principal) -> Result<Vec<Project>, GatewayError> {
            self.saw(principal);
            if let Some(error) = self.failure(&format!("ws:{}", principal.workspace_id)) {
                return Err(error);
            }
            Ok(self
                .projects
                .lock()
                .unwrap()
                .get(&principal.workspace_id)
                .cloned()
                .unwrap_or_default())
        }

        async fn list_pull_requests(
            &self,
            principal: &Principal,
            project_id: &str,
            state: &str,
            refresh: bool,
            limit: u32,
        ) -> Result<Vec<PullRequestSummary>, GatewayError> {
            self.saw(principal);
            self.list_calls.lock().unwrap().push((
                project_id.to_owned(),
                state.to_owned(),
                refresh,
                limit,
            ));
            if let Some(error) = self.failure(&format!("project:{project_id}")) {
                return Err(error);
            }
            Ok(self
                .pull_requests
                .lock()
                .unwrap()
                .values()
                .filter_map(|pr| pr.summary.clone())
                .filter(|s| s.project_id == project_id)
                // GitHub's "open" includes drafts; knowledge names them apart.
                .filter(|s| match state {
                    "" | "open" => matches!(s.state.as_str(), "open" | "draft"),
                    "all" => true,
                    other => s.state == other,
                })
                .take(if limit == 0 {
                    usize::MAX
                } else {
                    limit as usize
                })
                .collect())
        }

        async fn pull_request_status(
            &self,
            principal: &Principal,
            project_id: &str,
            number: u64,
        ) -> Result<PullRequestStatus, GatewayError> {
            self.saw(principal);
            self.status_calls.lock().unwrap().push(number);
            let summary = self
                .pull_requests
                .lock()
                .unwrap()
                .get(&number)
                .and_then(|pr| pr.summary.clone())
                .filter(|s| s.project_id == project_id);
            let scripted = self.statuses.lock().unwrap().get(&number).cloned();
            match (scripted, summary) {
                (Some(mut status), summary) => {
                    if status.summary.is_none() {
                        status.summary = summary;
                    }
                    Ok(status)
                }
                (None, Some(summary)) => Ok(PullRequestStatus {
                    summary: Some(summary),
                    mergeable: "mergeable".into(),
                    mergeable_state: "clean".into(),
                    allowed_methods: vec!["merge".into(), "squash".into(), "rebase".into()],
                    ..Default::default()
                }),
                (None, None) => Err(knowledge_err(Status::not_found(format!(
                    "pull request #{number} not found"
                )))),
            }
        }

        async fn repository_security(
            &self,
            principal: &Principal,
            project_id: &str,
            refresh: bool,
        ) -> Result<RepositorySecurity, GatewayError> {
            self.project_context(principal, project_id).await?;
            self.security_calls.lock().unwrap().push(refresh);
            self.security
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| knowledge_err(Status::unavailable("connection refused")))
        }

        async fn list_dependencies(
            &self,
            principal: &Principal,
            project_id: &str,
        ) -> Result<ListDependenciesResponse, GatewayError> {
            self.project_context(principal, project_id).await?;
            self.dependencies
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| knowledge_err(Status::unavailable("connection refused")))
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
        /// The status `GetExecution` answers per execution id; an execution
        /// without an entry is still `PLANNING` (created, not started).
        pub statuses: Mutex<BTreeMap<String, ExecutionStatus>>,
        /// Ids `GetExecution` was asked for, in order.
        pub fetched: Mutex<Vec<String>>,
        /// Ids `GetExecution` answers `NOT_FOUND` for.
        pub missing: Mutex<Vec<String>>,
        /// `CreateExecution` and `GetExecution` answer `UNAVAILABLE` while set.
        pub down: AtomicBool,
        /// Ids `StartExecution` was asked for, with the principal that asked.
        pub started: Mutex<Vec<(String, Principal)>>,
        /// `StartExecution` answers `UNAVAILABLE` while set.
        pub start_down: AtomicBool,
    }

    impl FakeExecution {
        fn delivery_up(&self) -> Result<(), GatewayError> {
            if self.delivery_down.load(std::sync::atomic::Ordering::SeqCst) {
                Err(execution_err(Status::unavailable("connection refused")))
            } else {
                Ok(())
            }
        }

        fn up(&self) -> Result<(), GatewayError> {
            if self.down.load(Ordering::SeqCst) {
                Err(execution_err(Status::unavailable("connection refused")))
            } else {
                Ok(())
            }
        }

        /// What `GetExecution` answers for `id` from now on.
        pub fn set_status(&self, id: &str, status: ExecutionStatus) {
            self.statuses.lock().unwrap().insert(id.to_owned(), status);
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
            self.up()?;
            let mut created = self.created.lock().unwrap();
            created.push(request.clone());
            Ok(Execution {
                id: format!("exec-{}", created.len()),
                number: created.len() as u64,
                project_id: request.project_id,
                proposal_id: request.proposal_id,
                pattern_id: request.pattern_id,
                title: request.title,
                status: ExecutionStatus::Planning as i32,
                ..Default::default()
            })
        }

        async fn get_execution(
            &self,
            principal: &Principal,
            id: &str,
        ) -> Result<Execution, GatewayError> {
            self.callers.lock().unwrap().push(principal.clone());
            self.up()?;
            self.fetched.lock().unwrap().push(id.to_owned());
            if self.missing.lock().unwrap().iter().any(|m| m == id) {
                return Err(execution_err(Status::not_found("not found")));
            }
            let status = self
                .statuses
                .lock()
                .unwrap()
                .get(id)
                .copied()
                .unwrap_or(ExecutionStatus::Planning);
            Ok(Execution {
                id: id.to_owned(),
                status: status as i32,
                ..Default::default()
            })
        }

        async fn start_execution(
            &self,
            principal: &Principal,
            id: &str,
        ) -> Result<Execution, GatewayError> {
            if self.start_down.load(Ordering::SeqCst) {
                return Err(execution_err(Status::unavailable("connection refused")));
            }
            self.started
                .lock()
                .unwrap()
                .push((id.to_owned(), principal.clone()));
            Ok(Execution {
                id: id.to_owned(),
                status: ExecutionStatus::WaitingForApproval as i32,
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
        /// `GetDataControls` answers `UNAVAILABLE` while set.
        pub down: AtomicBool,
    }

    impl FakeIdentity {
        pub fn set_reasoner(&self, workspace_id: &str, reasoner: &str, key: Option<&str>) {
            {
                let mut controls = self.controls.lock().unwrap();
                let entry =
                    controls
                        .entry(workspace_id.to_owned())
                        .or_insert_with(|| DataControls {
                            workspace_id: workspace_id.to_owned(),
                            ..Default::default()
                        });
                entry.reasoner = reasoner.to_owned();
                entry.reasoner_key_configured = key.is_some();
            }
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

        /// The `code_to_model` data control: "off" | "changed_lines".
        pub fn set_code_to_model(&self, workspace_id: &str, value: &str) {
            self.controls
                .lock()
                .unwrap()
                .entry(workspace_id.to_owned())
                .or_insert_with(|| DataControls {
                    workspace_id: workspace_id.to_owned(),
                    reasoner: "off".to_owned(),
                    ..Default::default()
                })
                .code_to_model = value.to_owned();
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
            if self.down.load(Ordering::SeqCst) {
                return Err(identity_err(Status::unavailable("connection refused")));
            }
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
