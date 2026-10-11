//! tonic implementation of `KnowledgeService`. The trait must be one `impl`
//! block, so it only delegates; each RPC group lives in its own file.

mod code;
mod context;
mod delivery;
mod github;
mod knowledge;
mod patterns;
mod projects;
mod retrieval;
mod scans;
mod settings;

use std::pin::Pin;

use futures::Stream;
use sdi_common::grpc::db_status;
use sdi_proto::{
    common::v1::{PlatformEvent, WatchEventsRequest},
    knowledge::v1::{knowledge_service_server::KnowledgeService, *},
};
use sqlx::PgPool;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{
    db::{self, projects::ProjectRow},
    events::EventBus,
    github::{GithubClient, GithubError},
};

/// Server-streaming responses are boxed: the concrete stream types differ
/// per RPC and tonic only needs `Send + 'static`.
pub type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Clone)]
pub struct KnowledgeState {
    pub pool: PgPool,
    pub github: GithubClient,
    pub events: EventBus,
}

impl KnowledgeState {
    pub fn new(pool: PgPool, github: GithubClient, events: EventBus) -> Self {
        Self {
            pool,
            github,
            events,
        }
    }

    async fn project(&self, id: &str) -> Result<ProjectRow, Status> {
        let id = parse_id(id, "project id")?;
        db::projects::get(&self.pool, id)
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => Status::not_found("project not found"),
                other => db_status(other),
            })
    }
}

fn parse_id(raw: &str, what: &str) -> Result<Uuid, Status> {
    if raw.trim().is_empty() {
        return Err(Status::invalid_argument(format!("{what} is required")));
    }
    Uuid::parse_str(raw.trim())
        .map_err(|_| Status::invalid_argument(format!("{what} is not a valid id")))
}

fn opt_id(raw: &str, what: &str) -> Result<Option<Uuid>, Status> {
    if raw.trim().is_empty() {
        Ok(None)
    } else {
        parse_id(raw, what).map(Some)
    }
}

/// Positive limit with a default and a ceiling.
fn limit(raw: u32, default: u32, max: u32) -> i64 {
    i64::from(if raw == 0 { default } else { raw.min(max) })
}

fn github_status(error: GithubError) -> Status {
    match error {
        GithubError::NoToken => Status::failed_precondition(error.to_string()),
        GithubError::Unauthorized => Status::unauthenticated(error.to_string()),
        GithubError::Forbidden(_) => Status::permission_denied(error.to_string()),
        GithubError::NotFound(_) => Status::not_found(error.to_string()),
        GithubError::RateLimited { .. } => Status::resource_exhausted(error.to_string()),
        GithubError::Api { .. } | GithubError::Transport(_) | GithubError::Decode(_) => {
            tracing::warn!(%error, "GitHub call failed");
            Status::unavailable(error.to_string())
        }
    }
}

/// Scan and refresh errors are `anyhow` with a `GithubError` somewhere in the chain.
fn anyhow_status(error: anyhow::Error) -> Status {
    for cause in error.chain() {
        if let Some(github) = cause.downcast_ref::<GithubError>() {
            return github_status(match github {
                GithubError::NoToken => GithubError::NoToken,
                GithubError::Unauthorized => GithubError::Unauthorized,
                GithubError::Forbidden(m) => GithubError::Forbidden(m.clone()),
                GithubError::NotFound(m) => GithubError::NotFound(m.clone()),
                GithubError::RateLimited { retry_after } => GithubError::RateLimited {
                    retry_after: *retry_after,
                },
                other => GithubError::Decode(other.to_string()),
            });
        }
        if let Some(db) = cause.downcast_ref::<sqlx::Error>() {
            tracing::error!(error = %db, "database error");
            return Status::internal("database error");
        }
    }
    sdi_common::grpc::internal(format!("{error:#}"))
}

#[tonic::async_trait]
impl KnowledgeService for KnowledgeState {
    type WatchScanStream = ResponseStream<Scan>;
    type WatchEventsStream = ResponseStream<PlatformEvent>;

    async fn get_github_account(
        &self,
        request: Request<GetGithubAccountRequest>,
    ) -> Result<Response<GithubAccount>, Status> {
        github::get_github_account(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_github_repos(
        &self,
        request: Request<ListGithubReposRequest>,
    ) -> Result<Response<ListGithubReposResponse>, Status> {
        github::list_github_repos(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn connect_project(
        &self,
        request: Request<ConnectProjectRequest>,
    ) -> Result<Response<Project>, Status> {
        projects::connect_project(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_projects(
        &self,
        request: Request<ListProjectsRequest>,
    ) -> Result<Response<ListProjectsResponse>, Status> {
        projects::list_projects(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_project(
        &self,
        request: Request<GetProjectRequest>,
    ) -> Result<Response<Project>, Status> {
        projects::get_project(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn delete_project(
        &self,
        request: Request<DeleteProjectRequest>,
    ) -> Result<Response<DeleteProjectResponse>, Status> {
        projects::delete_project(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn set_project_mode(
        &self,
        request: Request<SetProjectModeRequest>,
    ) -> Result<Response<Project>, Status> {
        projects::set_project_mode(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn start_scan(
        &self,
        request: Request<StartScanRequest>,
    ) -> Result<Response<Scan>, Status> {
        scans::start_scan(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_scan(&self, request: Request<GetScanRequest>) -> Result<Response<Scan>, Status> {
        scans::get_scan(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_scans(
        &self,
        request: Request<ListScansRequest>,
    ) -> Result<Response<ListScansResponse>, Status> {
        scans::list_scans(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn watch_scan(
        &self,
        request: Request<WatchScanRequest>,
    ) -> Result<Response<Self::WatchScanStream>, Status> {
        let stream = scans::watch_scan(self, request.into_inner()).await?;
        // A scan watch ends with the scan, which a shutdown must not wait for.
        Ok(Response::new(Box::pin(futures::StreamExt::take_until(
            stream,
            sdi_common::shutdown::cancelled(),
        ))))
    }

    async fn list_facts(
        &self,
        request: Request<ListFactsRequest>,
    ) -> Result<Response<ListFactsResponse>, Status> {
        knowledge::list_facts(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_questions(
        &self,
        request: Request<ListQuestionsRequest>,
    ) -> Result<Response<ListQuestionsResponse>, Status> {
        knowledge::list_questions(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn answer_question(
        &self,
        request: Request<AnswerQuestionRequest>,
    ) -> Result<Response<Question>, Status> {
        knowledge::answer_question(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_understanding(
        &self,
        request: Request<GetUnderstandingRequest>,
    ) -> Result<Response<Understanding>, Status> {
        knowledge::get_understanding(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_maturity(
        &self,
        request: Request<GetMaturityRequest>,
    ) -> Result<Response<MaturityScore>, Status> {
        knowledge::get_maturity(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_graph(
        &self,
        request: Request<GetGraphRequest>,
    ) -> Result<Response<Graph>, Status> {
        knowledge::get_graph(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn analyze_impact(
        &self,
        request: Request<AnalyzeImpactRequest>,
    ) -> Result<Response<ImpactAnalysis>, Status> {
        knowledge::analyze_impact(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_dependencies(
        &self,
        request: Request<ListDependenciesRequest>,
    ) -> Result<Response<ListDependenciesResponse>, Status> {
        knowledge::list_dependencies(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_repo_tree(
        &self,
        request: Request<GetRepoTreeRequest>,
    ) -> Result<Response<RepoTree>, Status> {
        code::get_repo_tree(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_repo_file(
        &self,
        request: Request<GetRepoFileRequest>,
    ) -> Result<Response<RepoFile>, Status> {
        code::get_repo_file(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_pipelines(
        &self,
        request: Request<ListPipelinesRequest>,
    ) -> Result<Response<ListPipelinesResponse>, Status> {
        delivery::list_pipelines(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_pipeline_runs(
        &self,
        request: Request<ListPipelineRunsRequest>,
    ) -> Result<Response<ListPipelineRunsResponse>, Status> {
        delivery::list_pipeline_runs(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_deployments(
        &self,
        request: Request<ListDeploymentsRequest>,
    ) -> Result<Response<ListDeploymentsResponse>, Status> {
        delivery::list_deployments(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_patterns(
        &self,
        request: Request<ListPatternsRequest>,
    ) -> Result<Response<ListPatternsResponse>, Status> {
        patterns::list_patterns(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_pattern(
        &self,
        request: Request<GetPatternRequest>,
    ) -> Result<Response<Pattern>, Status> {
        patterns::get_pattern(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn record_pattern_outcome(
        &self,
        request: Request<RecordPatternOutcomeRequest>,
    ) -> Result<Response<Pattern>, Status> {
        patterns::record_pattern_outcome(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn search_knowledge(
        &self,
        request: Request<SearchKnowledgeRequest>,
    ) -> Result<Response<SearchKnowledgeResponse>, Status> {
        retrieval::search_knowledge(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn index_knowledge(
        &self,
        request: Request<IndexKnowledgeRequest>,
    ) -> Result<Response<IndexKnowledgeResponse>, Status> {
        retrieval::index_knowledge(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_similar_projects(
        &self,
        request: Request<ListSimilarProjectsRequest>,
    ) -> Result<Response<ListSimilarProjectsResponse>, Status> {
        retrieval::list_similar_projects(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_project_context(
        &self,
        request: Request<GetProjectContextRequest>,
    ) -> Result<Response<ProjectContext>, Status> {
        context::get_project_context(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_knowledge_stats(
        &self,
        request: Request<GetKnowledgeStatsRequest>,
    ) -> Result<Response<KnowledgeStats>, Status> {
        context::get_knowledge_stats(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_settings(
        &self,
        request: Request<GetSettingsRequest>,
    ) -> Result<Response<KnowledgeSettings>, Status> {
        settings::get_settings(self, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn update_settings(
        &self,
        request: Request<UpdateSettingsRequest>,
    ) -> Result<Response<KnowledgeSettings>, Status> {
        settings::update_settings(self, request.into_inner())
            .await
            .map(Response::new)
    }

    /// Forwards the process-wide bus. Never ends on its own: the stream
    /// closes only when the client goes away. A receiver that lags skips the
    /// missed events (the client re-reads what it shows) and logs once.
    async fn watch_events(
        &self,
        _request: Request<WatchEventsRequest>,
    ) -> Result<Response<Self::WatchEventsStream>, Status> {
        let stream = BroadcastStream::new(self.events.subscribe()).filter_map(|item| match item {
            Ok(event) => Some(Ok(event)),
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(missed)) => {
                tracing::warn!(missed, "WatchEvents subscriber lagged, skipping events");
                None
            }
        });
        // The feed never ends on its own; shutdown must be able to end it.
        let stream = futures::StreamExt::take_until(stream, sdi_common::shutdown::cancelled());
        Ok(Response::new(Box::pin(stream)))
    }
}
