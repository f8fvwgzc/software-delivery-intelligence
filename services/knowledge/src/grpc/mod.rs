//! tonic implementation of `KnowledgeService`. The trait must be one `impl`
//! block, so it only delegates; each RPC group lives in its own file.
//!
//! Every RPC starts by reading the [`Caller`] (the interceptor attached it,
//! or the metadata is read directly); handlers take it explicitly so no
//! query can be written without a workspace. Role checks (CONTRACT.md §18):
//! reads need VIEWER; connect/scan/answer/mode/delete need MEMBER; settings
//! and data controls need ADMIN; deleting the workspace's data needs OWNER.

mod code;
mod context;
mod data;
mod delivery;
mod github;
mod knowledge;
mod patterns;
mod projects;
mod pull_requests;
mod retrieval;
mod scans;
mod settings;

use std::{pin::Pin, sync::Arc};

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
    auth::Caller,
    controls::ControlsProvider,
    db::{self, projects::ProjectRow, settings::SettingsRow},
    events::EventBus,
    github::{Github, GithubClient, GithubError},
    identity::Identity,
    scanner::ScanInputs,
};

/// Server-streaming responses are boxed: the concrete stream types differ
/// per RPC and tonic only needs `Send + 'static`.
pub type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Clone)]
pub struct KnowledgeState {
    pub pool: PgPool,
    pub github: Arc<Github>,
    pub events: EventBus,
    pub identity: Identity,
    pub controls: Arc<ControlsProvider>,
}

impl KnowledgeState {
    pub fn new(pool: PgPool, github: Github, events: EventBus, identity: Identity) -> Self {
        Self {
            pool,
            github: Arc::new(github),
            events,
            controls: Arc::new(ControlsProvider::new(identity.clone())),
            identity,
        }
    }

    /// The project by id, in the caller's workspace only: another
    /// workspace's project is `NOT_FOUND`, never `PERMISSION_DENIED`, so an
    /// id leaks nothing about other tenants.
    async fn project(&self, caller: &Caller, id: &str) -> Result<ProjectRow, Status> {
        let id = parse_id(id, "project id")?;
        db::projects::get(&self.pool, caller.workspace_id, id)
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => Status::not_found("project not found"),
                other => db_status(other),
            })
    }

    /// The workspace's settings (seeded on first read); the defaults when
    /// the row cannot be read, so a scan still runs with sane limits.
    async fn settings(&self, caller: &Caller) -> SettingsRow {
        match db::settings::get(&self.pool, caller.workspace_id).await {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(%error, workspace = %caller.workspace_id, "settings unreadable, using defaults");
                SettingsRow::default()
            }
        }
    }

    /// A GitHub client bound to the caller's workspace token, honouring the
    /// workspace's `cache_github` switch.
    async fn github(&self, caller: &Caller) -> Result<GithubClient, Status> {
        let settings = self.settings(caller).await;
        self.github
            .client(caller, settings.cache_github)
            .await
            .map_err(github_status)
    }

    /// Everything a scan needs, resolved once per scan.
    async fn scan_inputs(&self, caller: &Caller) -> Result<ScanInputs, Status> {
        let settings = self.settings(caller).await;
        let github = self
            .github
            .client(caller, settings.cache_github)
            .await
            .map_err(github_status)?;
        let controls = self.controls.get(caller).await;
        Ok(ScanInputs {
            github,
            settings,
            controls,
        })
    }
}

/// The caller of this RPC, or `UNAUTHENTICATED`.
fn caller<T>(request: &Request<T>) -> Result<Caller, Status> {
    Caller::from_request(request)
}

/// Requests of the data-control RPCs may name the workspace for the
/// gateway's convenience; it must be the caller's own. The principal, not
/// the body, is what scopes the call.
fn own_workspace(caller: &Caller, raw: &str) -> Result<(), Status> {
    if raw.trim().is_empty() {
        return Ok(());
    }
    match Uuid::parse_str(raw.trim()) {
        Ok(id) if id == caller.workspace_id => Ok(()),
        Ok(_) => Err(Status::permission_denied(
            "workspace_id must be the caller's workspace",
        )),
        Err(_) => Err(Status::invalid_argument("workspace_id is not a valid id")),
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
        GithubError::NoToken(_) => Status::failed_precondition(error.to_string()),
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
                GithubError::NoToken(m) => GithubError::NoToken(m.clone()),
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
    type ExportWorkspaceDataStream = ResponseStream<ExportRecord>;

    async fn get_github_account(
        &self,
        request: Request<GetGithubAccountRequest>,
    ) -> Result<Response<GithubAccount>, Status> {
        let caller = caller(&request)?;
        github::get_github_account(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_github_repos(
        &self,
        request: Request<ListGithubReposRequest>,
    ) -> Result<Response<ListGithubReposResponse>, Status> {
        let caller = caller(&request)?;
        github::list_github_repos(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn connect_project(
        &self,
        request: Request<ConnectProjectRequest>,
    ) -> Result<Response<Project>, Status> {
        let caller = caller(&request)?;
        projects::connect_project(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_projects(
        &self,
        request: Request<ListProjectsRequest>,
    ) -> Result<Response<ListProjectsResponse>, Status> {
        let caller = caller(&request)?;
        projects::list_projects(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_project(
        &self,
        request: Request<GetProjectRequest>,
    ) -> Result<Response<Project>, Status> {
        let caller = caller(&request)?;
        projects::get_project(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn delete_project(
        &self,
        request: Request<DeleteProjectRequest>,
    ) -> Result<Response<DeleteProjectResponse>, Status> {
        let caller = caller(&request)?;
        projects::delete_project(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn set_project_mode(
        &self,
        request: Request<SetProjectModeRequest>,
    ) -> Result<Response<Project>, Status> {
        let caller = caller(&request)?;
        projects::set_project_mode(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn start_scan(
        &self,
        request: Request<StartScanRequest>,
    ) -> Result<Response<Scan>, Status> {
        let caller = caller(&request)?;
        scans::start_scan(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_scan(&self, request: Request<GetScanRequest>) -> Result<Response<Scan>, Status> {
        let caller = caller(&request)?;
        scans::get_scan(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_scans(
        &self,
        request: Request<ListScansRequest>,
    ) -> Result<Response<ListScansResponse>, Status> {
        let caller = caller(&request)?;
        scans::list_scans(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn watch_scan(
        &self,
        request: Request<WatchScanRequest>,
    ) -> Result<Response<Self::WatchScanStream>, Status> {
        let caller = caller(&request)?;
        let stream = scans::watch_scan(self, &caller, request.into_inner()).await?;
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
        let caller = caller(&request)?;
        knowledge::list_facts(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_questions(
        &self,
        request: Request<ListQuestionsRequest>,
    ) -> Result<Response<ListQuestionsResponse>, Status> {
        let caller = caller(&request)?;
        knowledge::list_questions(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn answer_question(
        &self,
        request: Request<AnswerQuestionRequest>,
    ) -> Result<Response<Question>, Status> {
        let caller = caller(&request)?;
        knowledge::answer_question(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_understanding(
        &self,
        request: Request<GetUnderstandingRequest>,
    ) -> Result<Response<Understanding>, Status> {
        let caller = caller(&request)?;
        knowledge::get_understanding(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_maturity(
        &self,
        request: Request<GetMaturityRequest>,
    ) -> Result<Response<MaturityScore>, Status> {
        let caller = caller(&request)?;
        knowledge::get_maturity(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_graph(
        &self,
        request: Request<GetGraphRequest>,
    ) -> Result<Response<Graph>, Status> {
        let caller = caller(&request)?;
        knowledge::get_graph(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn analyze_impact(
        &self,
        request: Request<AnalyzeImpactRequest>,
    ) -> Result<Response<ImpactAnalysis>, Status> {
        let caller = caller(&request)?;
        knowledge::analyze_impact(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_dependencies(
        &self,
        request: Request<ListDependenciesRequest>,
    ) -> Result<Response<ListDependenciesResponse>, Status> {
        let caller = caller(&request)?;
        knowledge::list_dependencies(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_repo_tree(
        &self,
        request: Request<GetRepoTreeRequest>,
    ) -> Result<Response<RepoTree>, Status> {
        let caller = caller(&request)?;
        code::get_repo_tree(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_repo_file(
        &self,
        request: Request<GetRepoFileRequest>,
    ) -> Result<Response<RepoFile>, Status> {
        let caller = caller(&request)?;
        code::get_repo_file(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_pull_requests(
        &self,
        request: Request<ListPullRequestsRequest>,
    ) -> Result<Response<ListPullRequestsResponse>, Status> {
        let caller = caller(&request)?;
        pull_requests::list_pull_requests(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_pull_request(
        &self,
        request: Request<GetPullRequestRequest>,
    ) -> Result<Response<PullRequest>, Status> {
        let caller = caller(&request)?;
        pull_requests::get_pull_request(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_pipelines(
        &self,
        request: Request<ListPipelinesRequest>,
    ) -> Result<Response<ListPipelinesResponse>, Status> {
        let caller = caller(&request)?;
        delivery::list_pipelines(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_pipeline_runs(
        &self,
        request: Request<ListPipelineRunsRequest>,
    ) -> Result<Response<ListPipelineRunsResponse>, Status> {
        let caller = caller(&request)?;
        delivery::list_pipeline_runs(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_deployments(
        &self,
        request: Request<ListDeploymentsRequest>,
    ) -> Result<Response<ListDeploymentsResponse>, Status> {
        let caller = caller(&request)?;
        delivery::list_deployments(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_patterns(
        &self,
        request: Request<ListPatternsRequest>,
    ) -> Result<Response<ListPatternsResponse>, Status> {
        let caller = caller(&request)?;
        patterns::list_patterns(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_pattern(
        &self,
        request: Request<GetPatternRequest>,
    ) -> Result<Response<Pattern>, Status> {
        let caller = caller(&request)?;
        patterns::get_pattern(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn record_pattern_outcome(
        &self,
        request: Request<RecordPatternOutcomeRequest>,
    ) -> Result<Response<Pattern>, Status> {
        let caller = caller(&request)?;
        patterns::record_pattern_outcome(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn search_knowledge(
        &self,
        request: Request<SearchKnowledgeRequest>,
    ) -> Result<Response<SearchKnowledgeResponse>, Status> {
        let caller = caller(&request)?;
        retrieval::search_knowledge(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn index_knowledge(
        &self,
        request: Request<IndexKnowledgeRequest>,
    ) -> Result<Response<IndexKnowledgeResponse>, Status> {
        let caller = caller(&request)?;
        retrieval::index_knowledge(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn list_similar_projects(
        &self,
        request: Request<ListSimilarProjectsRequest>,
    ) -> Result<Response<ListSimilarProjectsResponse>, Status> {
        let caller = caller(&request)?;
        retrieval::list_similar_projects(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_project_context(
        &self,
        request: Request<GetProjectContextRequest>,
    ) -> Result<Response<ProjectContext>, Status> {
        let caller = caller(&request)?;
        context::get_project_context(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn get_knowledge_stats(
        &self,
        request: Request<GetKnowledgeStatsRequest>,
    ) -> Result<Response<KnowledgeStats>, Status> {
        let caller = caller(&request)?;
        context::get_knowledge_stats(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_settings(
        &self,
        request: Request<GetSettingsRequest>,
    ) -> Result<Response<KnowledgeSettings>, Status> {
        let caller = caller(&request)?;
        settings::get_settings(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn update_settings(
        &self,
        request: Request<UpdateSettingsRequest>,
    ) -> Result<Response<KnowledgeSettings>, Status> {
        let caller = caller(&request)?;
        settings::update_settings(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    /// Forwards the process-wide bus, filtered to the caller's workspace.
    /// Never ends on its own: the stream closes only when the client goes
    /// away. A receiver that lags skips the missed events (the client
    /// re-reads what it shows) and logs once.
    async fn watch_events(
        &self,
        request: Request<WatchEventsRequest>,
    ) -> Result<Response<Self::WatchEventsStream>, Status> {
        let caller = caller(&request)?;
        let workspace_id = caller.workspace_id;
        let stream =
            BroadcastStream::new(self.events.subscribe()).filter_map(move |item| match item {
                Ok(event) if crate::events::belongs_to(&event, workspace_id) => Some(Ok(event)),
                Ok(_) => None,
                Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "WatchEvents subscriber lagged, skipping events");
                    None
                }
            });
        // The feed never ends on its own; shutdown must be able to end it.
        let stream = futures::StreamExt::take_until(stream, sdi_common::shutdown::cancelled());
        Ok(Response::new(Box::pin(stream)))
    }

    async fn purge_code_cache(
        &self,
        request: Request<PurgeCodeCacheRequest>,
    ) -> Result<Response<PurgeCodeCacheResponse>, Status> {
        let caller = caller(&request)?;
        data::purge_code_cache(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn delete_workspace_data(
        &self,
        request: Request<DeleteWorkspaceDataRequest>,
    ) -> Result<Response<DeleteWorkspaceDataResponse>, Status> {
        let caller = caller(&request)?;
        data::delete_workspace_data(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }
    async fn export_workspace_data(
        &self,
        request: Request<ExportWorkspaceDataRequest>,
    ) -> Result<Response<Self::ExportWorkspaceDataStream>, Status> {
        let caller = caller(&request)?;
        let stream = data::export_workspace_data(self, &caller, request.into_inner()).await?;
        Ok(Response::new(Box::pin(futures::StreamExt::take_until(
            stream,
            sdi_common::shutdown::cancelled(),
        ))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_workspace_must_match_the_principal() {
        let caller = Caller::system(Uuid::now_v7());
        assert!(own_workspace(&caller, "").is_ok());
        assert!(own_workspace(&caller, &caller.workspace_id.to_string()).is_ok());
        assert_eq!(
            own_workspace(&caller, &Uuid::now_v7().to_string())
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            own_workspace(&caller, "nope").unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn no_token_maps_to_failed_precondition() {
        let status = github_status(GithubError::NoToken("x".into()));
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        let chained =
            anyhow_status(anyhow::Error::new(GithubError::NoToken("x".into())).context("scan"));
        assert_eq!(chained.code(), tonic::Code::FailedPrecondition);
    }
}
