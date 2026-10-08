//! Outbound calls to the knowledge and execution services behind traits, so
//! the proposal flow is testable without either service running.

use async_trait::async_trait;
use sdi_proto::{
    execution::v1::{
        CreateExecutionRequest, DeliveryStats, Execution, GetDeliveryStatsRequest,
        GetExecutionRequest, execution_service_client::ExecutionServiceClient,
    },
    knowledge::v1::{
        GetProjectContextRequest, IndexKnowledgeRequest, ListPatternsRequest,
        ListPipelineRunsRequest, ListSimilarProjectsRequest, Pattern, PipelineRun, ProjectContext,
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
    use std::sync::Mutex;

    use super::*;

    /// In-memory knowledge: a fixed context and catalog, remembers what was indexed.
    #[derive(Default)]
    pub struct FakeKnowledge {
        pub context: Option<ProjectContext>,
        pub patterns: Vec<Pattern>,
        pub similar: Vec<SimilarProject>,
        pub runs: Vec<PipelineRun>,
        pub indexed: Mutex<Vec<IndexKnowledgeRequest>>,
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
