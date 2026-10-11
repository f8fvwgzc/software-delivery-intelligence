//! Outbound calls to the knowledge and execution services behind traits, so
//! the proposal flow is testable without either service running.

use async_trait::async_trait;
use sdi_proto::{
    execution::v1::{
        CreateExecutionRequest, DeliveryStats, Execution, GetDeliveryStatsRequest,
        GetExecutionRequest, execution_service_client::ExecutionServiceClient,
    },
    knowledge::v1::{
        GetGraphRequest, GetProjectContextRequest, GetRepoFileRequest, GetRepoTreeRequest, Graph,
        IndexKnowledgeRequest, ListPatternsRequest, ListPipelineRunsRequest,
        ListSimilarProjectsRequest, Pattern, PipelineRun, ProjectContext, RepoFile, RepoTree,
        SimilarProject, knowledge_service_client::KnowledgeServiceClient,
    },
};
use tonic::{Code, Status, transport::Channel};

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

#[async_trait]
pub trait KnowledgeGateway: Send + Sync {
    async fn project_context(&self, project_id: &str) -> Result<ProjectContext, GatewayError>;
    async fn list_patterns(&self, category: &str) -> Result<Vec<Pattern>, GatewayError>;
    async fn similar_projects(
        &self,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<SimilarProject>, GatewayError>;
    async fn pipeline_runs(
        &self,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<PipelineRun>, GatewayError>;
    async fn index_knowledge(&self, request: IndexKnowledgeRequest)
    -> Result<String, GatewayError>;
    /// The graph alone (`ProjectContext.graph` carries it too; this is the
    /// cheaper call when nothing else is needed).
    async fn graph(&self, project_id: &str, include_history: bool) -> Result<Graph, GatewayError>;
    /// Code browser (CONTRACT.md §15): the whole tree at the scanned commit.
    async fn repo_tree(&self, project_id: &str) -> Result<RepoTree, GatewayError>;
    /// One file's text at the scanned commit.
    async fn repo_file(&self, project_id: &str, path: &str) -> Result<RepoFile, GatewayError>;
}

#[async_trait]
pub trait ExecutionGateway: Send + Sync {
    async fn create_execution(
        &self,
        request: CreateExecutionRequest,
    ) -> Result<Execution, GatewayError>;
    async fn get_execution(&self, id: &str) -> Result<Execution, GatewayError>;
    async fn delivery_stats(&self, project_id: &str) -> Result<DeliveryStats, GatewayError>;
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
    async fn project_context(&self, project_id: &str) -> Result<ProjectContext, GatewayError> {
        self.client
            .clone()
            .get_project_context(GetProjectContextRequest {
                project_id: project_id.to_owned(),
            })
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }

    async fn list_patterns(&self, category: &str) -> Result<Vec<Pattern>, GatewayError> {
        self.client
            .clone()
            .list_patterns(ListPatternsRequest {
                category: category.to_owned(),
                include_deprecated: false,
            })
            .await
            .map(|r| r.into_inner().patterns)
            .map_err(knowledge_err)
    }

    async fn similar_projects(
        &self,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<SimilarProject>, GatewayError> {
        self.client
            .clone()
            .list_similar_projects(ListSimilarProjectsRequest {
                project_id: project_id.to_owned(),
                limit,
            })
            .await
            .map(|r| r.into_inner().projects)
            .map_err(knowledge_err)
    }

    async fn pipeline_runs(
        &self,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<PipelineRun>, GatewayError> {
        self.client
            .clone()
            .list_pipeline_runs(ListPipelineRunsRequest {
                project_id: project_id.to_owned(),
                refresh: false,
                limit,
            })
            .await
            .map(|r| r.into_inner().runs)
            .map_err(knowledge_err)
    }

    async fn index_knowledge(
        &self,
        request: IndexKnowledgeRequest,
    ) -> Result<String, GatewayError> {
        self.client
            .clone()
            .index_knowledge(request)
            .await
            .map(|r| r.into_inner().id)
            .map_err(knowledge_err)
    }

    async fn graph(&self, project_id: &str, include_history: bool) -> Result<Graph, GatewayError> {
        self.client
            .clone()
            .get_graph(GetGraphRequest {
                project_id: project_id.to_owned(),
                include_history,
            })
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }

    async fn repo_tree(&self, project_id: &str) -> Result<RepoTree, GatewayError> {
        self.client
            .clone()
            .get_repo_tree(GetRepoTreeRequest {
                project_id: project_id.to_owned(),
            })
            .await
            .map(|r| r.into_inner())
            .map_err(knowledge_err)
    }

    async fn repo_file(&self, project_id: &str, path: &str) -> Result<RepoFile, GatewayError> {
        self.client
            .clone()
            .get_repo_file(GetRepoFileRequest {
                project_id: project_id.to_owned(),
                path: path.to_owned(),
            })
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
        request: CreateExecutionRequest,
    ) -> Result<Execution, GatewayError> {
        self.client
            .clone()
            .create_execution(request)
            .await
            .map(|r| r.into_inner())
            .map_err(execution_err)
    }

    async fn get_execution(&self, id: &str) -> Result<Execution, GatewayError> {
        self.client
            .clone()
            .get_execution(GetExecutionRequest { id: id.to_owned() })
            .await
            .map(|r| r.into_inner())
            .map_err(execution_err)
    }

    async fn delivery_stats(&self, project_id: &str) -> Result<DeliveryStats, GatewayError> {
        self.client
            .clone()
            .get_delivery_stats(GetDeliveryStatsRequest {
                project_id: project_id.to_owned(),
            })
            .await
            .map(|r| r.into_inner())
            .map_err(execution_err)
    }
}

#[cfg(test)]
pub mod fake {
    use std::{collections::BTreeMap, sync::Mutex};

    use sdi_proto::knowledge::v1::RepoTreeEntry;

    use super::*;

    /// In-memory knowledge: a fixed context and catalog, a repository tree
    /// with file contents, and a record of what was indexed.
    #[derive(Default)]
    pub struct FakeKnowledge {
        pub context: Option<ProjectContext>,
        pub patterns: Vec<Pattern>,
        pub similar: Vec<SimilarProject>,
        pub runs: Vec<PipelineRun>,
        pub indexed: Mutex<Vec<IndexKnowledgeRequest>>,
        pub tree: Vec<RepoTreeEntry>,
        pub files: BTreeMap<String, String>,
        /// Paths asked for through `repo_file`, in order.
        pub files_read: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl KnowledgeGateway for FakeKnowledge {
        async fn project_context(&self, project_id: &str) -> Result<ProjectContext, GatewayError> {
            self.context
                .clone()
                .filter(|c| c.project.as_ref().is_some_and(|p| p.id == project_id))
                .ok_or_else(|| knowledge_err(Status::not_found("project not found")))
        }

        async fn list_patterns(&self, category: &str) -> Result<Vec<Pattern>, GatewayError> {
            Ok(self
                .patterns
                .iter()
                .filter(|p| category.is_empty() || p.category == category)
                .cloned()
                .collect())
        }

        async fn similar_projects(
            &self,
            _project_id: &str,
            limit: u32,
        ) -> Result<Vec<SimilarProject>, GatewayError> {
            Ok(self.similar.iter().take(limit as usize).cloned().collect())
        }

        async fn pipeline_runs(
            &self,
            _project_id: &str,
            limit: u32,
        ) -> Result<Vec<PipelineRun>, GatewayError> {
            Ok(self.runs.iter().take(limit as usize).cloned().collect())
        }

        async fn index_knowledge(
            &self,
            request: IndexKnowledgeRequest,
        ) -> Result<String, GatewayError> {
            let mut indexed = self.indexed.lock().unwrap();
            indexed.push(request);
            Ok(format!("chunk-{}", indexed.len()))
        }

        async fn graph(
            &self,
            project_id: &str,
            _include_history: bool,
        ) -> Result<Graph, GatewayError> {
            Ok(self
                .project_context(project_id)
                .await?
                .graph
                .unwrap_or_default())
        }

        async fn repo_tree(&self, project_id: &str) -> Result<RepoTree, GatewayError> {
            let context = self.project_context(project_id).await?;
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

        async fn repo_file(&self, project_id: &str, path: &str) -> Result<RepoFile, GatewayError> {
            self.project_context(project_id).await?;
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
    }

    #[derive(Default)]
    pub struct FakeExecution {
        pub created: Mutex<Vec<CreateExecutionRequest>>,
        pub stats: Option<DeliveryStats>,
    }

    #[async_trait]
    impl ExecutionGateway for FakeExecution {
        async fn create_execution(
            &self,
            request: CreateExecutionRequest,
        ) -> Result<Execution, GatewayError> {
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

        async fn get_execution(&self, id: &str) -> Result<Execution, GatewayError> {
            Ok(Execution {
                id: id.to_owned(),
                ..Default::default()
            })
        }

        async fn delivery_stats(&self, _project_id: &str) -> Result<DeliveryStats, GatewayError> {
            Ok(self.stats.unwrap_or_default())
        }
    }
}
