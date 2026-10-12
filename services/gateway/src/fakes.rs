//! Real tonic servers in-process standing in for the four services, so the
//! auth middleware, the SSE bridges, the gRPC status mapping, the settings
//! composition and the health check are exercised against actual streams
//! and calls instead of a closed port. Each fake implements the handful of
//! RPCs a gateway test needs for real and answers `UNIMPLEMENTED` to the rest.

use std::{
    net::SocketAddr,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures::{Stream, StreamExt, stream};
use sdi_common::principal::Principal;
use sdi_proto::common::v1 as common;
use tonic::{Request, Response, Status, transport::server::Router};
use tonic_health::ServingStatus;

pub type ServerStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

/// The workspace every fake stamps on its events and expects in metadata.
pub const WORKSPACE: &str = "w1";

/// `#[async_trait]` cannot see into a nested macro, so the macro emits the
/// whole impl: the real methods verbatim, the rest unimplemented.
macro_rules! fake_service {
    (
        $trait:ident for $ty:ident {
            $($real:item)*
        }
        unimplemented {
            $($name:ident: $req:ty => $resp:ty),* $(,)?
        }
    ) => {
        #[tonic::async_trait]
        impl $trait for $ty {
            $($real)*

            $(async fn $name(&self, _: Request<$req>) -> Result<Response<$resp>, Status> {
                Err(Status::unimplemented(stringify!($name)))
            })*
        }
    };
}

/// Serves `grpc.health.v1.Health` plus whatever `add` registers on a random
/// port and returns the address.
pub async fn serve(add: impl FnOnce(Router) -> Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (reporter, health) = tonic_health::server::health_reporter();
    reporter
        .set_service_status("", ServingStatus::Serving)
        .await;
    let incoming = stream::unfold(listener, |listener| async move {
        let (socket, _) = listener.accept().await.ok()?;
        Some((Ok::<_, std::io::Error>(socket), listener))
    });
    let router = add(tonic::transport::Server::builder().add_service(health));
    tokio::spawn(async move {
        router.serve_with_incoming(incoming).await.unwrap();
    });
    addr
}

fn at(seconds: i64) -> Option<prost_types::Timestamp> {
    Some(prost_types::Timestamp { seconds, nanos: 0 })
}

/// The principal the gateway forwarded, or `UNAUTHENTICATED`: every fake
/// that serves data checks it, which is how the tests prove the metadata
/// travels.
pub fn principal<T>(request: &Request<T>) -> Result<Principal, Status> {
    let principal = Principal::from_request(request)?;
    if principal.workspace_id != WORKSPACE {
        return Err(Status::permission_denied("wrong workspace"));
    }
    Ok(principal)
}

pub fn platform_event(entity: &str, entity_id: &str, action: &str) -> common::PlatformEvent {
    platform_event_in(WORKSPACE, entity, entity_id, action)
}

pub fn platform_event_in(
    workspace: &str,
    entity: &str,
    entity_id: &str,
    action: &str,
) -> common::PlatformEvent {
    common::PlatformEvent {
        entity: entity.into(),
        entity_id: entity_id.into(),
        project_id: "p1".into(),
        action: action.into(),
        status: String::new(),
        at: at(1_700_000_000),
        data: [("workspace_id".to_owned(), workspace.to_owned())].into(),
    }
}

pub mod execution {
    use super::*;
    use sdi_proto::execution::v1::{
        self as proto,
        execution_service_server::{ExecutionService, ExecutionServiceServer},
    };

    pub struct Fake;

    fn watch(
        request: proto::WatchExecutionRequest,
    ) -> Result<ServerStream<proto::ExecutionEvent>, Status> {
        if request.execution_id == "missing" {
            return Err(Status::not_found("no such execution"));
        }
        let events = (1..=2)
            .filter(move |seq| *seq > request.after_seq)
            .map(|seq| {
                Ok(proto::ExecutionEvent {
                    seq,
                    execution_id: "e1".into(),
                    r#type: proto::EventType::StateChanged as i32,
                    step_key: String::new(),
                    message: format!("event {seq}"),
                    status: proto::ExecutionStatus::Executing as i32,
                    data: Default::default(),
                    at: at(1_700_000_000),
                })
            })
            .collect::<Vec<_>>();
        Ok(stream::iter(events).boxed())
    }

    fake_service! {
        ExecutionService for Fake {
            type WatchExecutionStream = ServerStream<proto::ExecutionEvent>;
            type WatchEventsStream = ServerStream<common::PlatformEvent>;
            type ExportWorkspaceDataStream = ServerStream<proto::WorkspaceDataRecord>;

            async fn watch_execution(
                &self,
                request: Request<proto::WatchExecutionRequest>,
            ) -> Result<Response<Self::WatchExecutionStream>, Status> {
                principal(&request)?;
                watch(request.into_inner()).map(Response::new)
            }

            /// One event after a short delay, then open for ever: proves the
            /// merged feed still delivers after another service dropped.
            async fn watch_events(
                &self,
                _: Request<common::WatchEventsRequest>,
            ) -> Result<Response<Self::WatchEventsStream>, Status> {
                let late = stream::once(async {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    Ok(platform_event("execution", "e1", "updated"))
                });
                Ok(Response::new(late.chain(stream::pending()).boxed()))
            }

            async fn create_execution(
                &self,
                request: Request<proto::CreateExecutionRequest>,
            ) -> Result<Response<proto::Execution>, Status> {
                let caller = principal(&request)?;
                let request = request.into_inner();
                Ok(Response::new(proto::Execution {
                    id: "e-new".into(),
                    project_id: request.project_id,
                    title: request.title,
                    adapter: request.adapter,
                    status: proto::ExecutionStatus::Planning as i32,
                    outputs: request.artifacts,
                    // The caller's role, so a test can see what was forwarded.
                    last_checkpoint: caller.role.as_str().into(),
                    ..Default::default()
                }))
            }

            async fn delete_workspace_data(
                &self,
                request: Request<proto::DeleteWorkspaceDataRequest>,
            ) -> Result<Response<proto::DeleteWorkspaceDataResponse>, Status> {
                principal(&request)?;
                Ok(Response::new(proto::DeleteWorkspaceDataResponse {
                    deleted: [("execution".to_owned(), 3u64)].into(),
                    cancelled: 1,
                }))
            }

            /// Not implemented here on purpose: the export must still finish.
            async fn export_workspace_data(
                &self,
                _: Request<proto::ExportWorkspaceDataRequest>,
            ) -> Result<Response<Self::ExportWorkspaceDataStream>, Status> {
                Err(Status::unimplemented("export_workspace_data"))
            }
        }
        unimplemented {
            list_executions: proto::ListExecutionsRequest => proto::ListExecutionsResponse,
            get_execution: proto::GetExecutionRequest => proto::Execution,
            start_execution: proto::StartExecutionRequest => proto::Execution,
            resolve_failure: proto::ResolveFailureRequest => proto::Execution,
            cancel_execution: proto::CancelExecutionRequest => proto::Execution,
            list_events: proto::ListEventsRequest => proto::ListEventsResponse,
            list_approvals: proto::ListApprovalsRequest => proto::ListApprovalsResponse,
            decide_approval: proto::DecideApprovalRequest => proto::Approval,
            list_policies: proto::ListPoliciesRequest => proto::ListPoliciesResponse,
            update_policy: proto::UpdatePolicyRequest => proto::Policy,
            list_identities: proto::ListIdentitiesRequest => proto::ListIdentitiesResponse,
            grant_permission: proto::GrantPermissionRequest => proto::Identity,
            get_delivery_stats: proto::GetDeliveryStatsRequest => proto::DeliveryStats,
        }
    }

    pub async fn serve() -> SocketAddr {
        super::serve(|router| router.add_service(ExecutionServiceServer::new(Fake))).await
    }
}

pub mod knowledge {
    use super::*;
    use sdi_proto::knowledge::v1::{
        self as proto,
        knowledge_service_server::{KnowledgeService, KnowledgeServiceServer},
    };

    pub struct Fake;

    pub fn pull_request_summary(
        project_id: &str,
        number: u64,
        state: &str,
    ) -> proto::PullRequestSummary {
        proto::PullRequestSummary {
            project_id: project_id.to_owned(),
            number,
            title: format!("PR {number}"),
            author: "ana".into(),
            state: state.to_owned(),
            base_ref: "main".into(),
            head_ref: format!("feat/{number}"),
            head_sha: format!("sha{number}"),
            changed_files: 1,
            additions: 2,
            html_url: format!("https://github.com/acme/repo/pull/{number}"),
            created_at: at(1_700_000_000),
            updated_at: at(1_700_000_000),
            ..Default::default()
        }
    }

    pub fn settings() -> proto::KnowledgeSettings {
        proto::KnowledgeSettings {
            scan_max_files: 2000,
            scan_max_file_bytes: 65536,
            scan_concurrency: 8,
            pipeline_runs_limit: 50,
            cache_github: true,
        }
    }

    fn scan(status: proto::ScanStatus) -> proto::Scan {
        proto::Scan {
            id: "s1".into(),
            project_id: "p1".into(),
            status: status as i32,
            commit_sha: "abc".into(),
            started_at: at(1_700_000_000),
            ..Default::default()
        }
    }

    fake_service! {
        KnowledgeService for Fake {
            type WatchScanStream = ServerStream<proto::Scan>;
            type WatchEventsStream = ServerStream<common::PlatformEvent>;
            type ExportWorkspaceDataStream = ServerStream<proto::ExportRecord>;

            async fn watch_scan(
                &self,
                request: Request<proto::WatchScanRequest>,
            ) -> Result<Response<Self::WatchScanStream>, Status> {
                principal(&request)?;
                if request.into_inner().id == "missing" {
                    return Err(Status::not_found("no such scan"));
                }
                let scans = vec![
                    Ok(scan(proto::ScanStatus::Running)),
                    Ok(scan(proto::ScanStatus::Succeeded)),
                ];
                Ok(Response::new(stream::iter(scans).boxed()))
            }

            /// Two events of the workspace, one of another workspace (which
            /// the gateway must drop), then the stream ends: the "service
            /// dropped" case.
            async fn watch_events(
                &self,
                _: Request<common::WatchEventsRequest>,
            ) -> Result<Response<Self::WatchEventsStream>, Status> {
                let events = vec![
                    Ok(platform_event("scan", "s1", "progress")),
                    Ok(platform_event_in("w2", "project", "other", "updated")),
                    Ok(platform_event("project", "p1", "updated")),
                ];
                Ok(Response::new(stream::iter(events).boxed()))
            }

            async fn list_projects(
                &self,
                request: Request<proto::ListProjectsRequest>,
            ) -> Result<Response<proto::ListProjectsResponse>, Status> {
                principal(&request)?;
                Ok(Response::new(proto::ListProjectsResponse {
                    projects: vec![proto::Project {
                        id: "p1".into(),
                        name: "repo".into(),
                        repo_full_name: "acme/repo".into(),
                        ..Default::default()
                    }],
                }))
            }

            async fn start_scan(
                &self,
                request: Request<proto::StartScanRequest>,
            ) -> Result<Response<proto::Scan>, Status> {
                let caller = principal(&request)?;
                if caller.user_id != sdi_common::principal::SYSTEM_USER {
                    return Err(Status::permission_denied("fake only scans for system"));
                }
                Ok(Response::new(scan(proto::ScanStatus::Queued)))
            }

            async fn get_maturity(
                &self,
                request: Request<proto::GetMaturityRequest>,
            ) -> Result<Response<proto::MaturityScore>, Status> {
                principal(&request)?;
                let project_id = request.into_inner().project_id;
                if project_id == "missing" {
                    return Err(Status::not_found("no such project"));
                }
                Ok(Response::new(proto::MaturityScore {
                    project_id,
                    overall: 42,
                    stage: "transitional".into(),
                    axes: vec![proto::MaturityAxis {
                        key: "ci".into(),
                        label: "CI".into(),
                        score: 10,
                        summary: "No workflows".into(),
                        findings: vec!["No workflows".into()],
                        recommended_patterns: vec!["add-ci-baseline".into()],
                    }],
                    computed_at: at(1_700_000_000),
                }))
            }

            async fn get_project_context(
                &self,
                request: Request<proto::GetProjectContextRequest>,
            ) -> Result<Response<proto::ProjectContext>, Status> {
                principal(&request)?;
                let project_id = request.into_inner().project_id;
                let config = (project_id == "configured").then(|| proto::ProjectConfig {
                    present: true,
                    path: ".sdi.yml".into(),
                    deploy_target: "azure".into(),
                    environments: vec!["staging".into(), "production".into()],
                    budget_monthly_usd: 20.0,
                    port: 8080,
                    ..Default::default()
                });
                Ok(Response::new(proto::ProjectContext {
                    config,
                    ..Default::default()
                }))
            }

            async fn get_repo_tree(
                &self,
                request: Request<proto::GetRepoTreeRequest>,
            ) -> Result<Response<proto::RepoTree>, Status> {
                principal(&request)?;
                let project_id = request.into_inner().project_id;
                match project_id.as_str() {
                    "missing" => return Err(Status::not_found("no such project")),
                    "unscanned" => return Err(Status::failed_precondition("not scanned yet")),
                    _ => {}
                }
                Ok(Response::new(proto::RepoTree {
                    project_id,
                    commit_sha: "abc".into(),
                    entries: vec![
                        proto::RepoTreeEntry {
                            path: "src".into(),
                            name: "src".into(),
                            is_dir: true,
                            size: 0,
                            language: String::new(),
                        },
                        proto::RepoTreeEntry {
                            path: "src/main.rs".into(),
                            name: "main.rs".into(),
                            is_dir: false,
                            size: 120,
                            language: "rust".into(),
                        },
                    ],
                    truncated: false,
                }))
            }

            async fn get_repo_file(
                &self,
                request: Request<proto::GetRepoFileRequest>,
            ) -> Result<Response<proto::RepoFile>, Status> {
                principal(&request)?;
                let request = request.into_inner();
                if request.path.starts_with(".env") {
                    return Err(Status::permission_denied("secret-looking file"));
                }
                if request.path.starts_with("..") {
                    return Err(Status::invalid_argument("bad path"));
                }
                if request.path != "src/main.rs" {
                    return Err(Status::not_found("no such file"));
                }
                Ok(Response::new(proto::RepoFile {
                    path: request.path,
                    commit_sha: "abc".into(),
                    content: "fn main() {}\n".into(),
                    binary: false,
                    truncated: false,
                    size: 13,
                    language: "rust".into(),
                    html_url: "https://github.com/o/r/blob/abc/src/main.rs".into(),
                    fact_ids: vec!["f1".into()],
                }))
            }

            async fn get_settings(
                &self,
                request: Request<proto::GetSettingsRequest>,
            ) -> Result<Response<proto::KnowledgeSettings>, Status> {
                principal(&request)?;
                Ok(Response::new(settings()))
            }

            async fn update_settings(
                &self,
                request: Request<proto::UpdateSettingsRequest>,
            ) -> Result<Response<proto::KnowledgeSettings>, Status> {
                principal(&request)?;
                request
                    .into_inner()
                    .settings
                    .map(Response::new)
                    .ok_or_else(|| Status::invalid_argument("settings missing"))
            }

            /// Two pull requests of `p1`; `refresh` is echoed in `synced_at`.
            async fn list_pull_requests(
                &self,
                request: Request<proto::ListPullRequestsRequest>,
            ) -> Result<Response<proto::ListPullRequestsResponse>, Status> {
                principal(&request)?;
                let request = request.into_inner();
                if request.project_id == "missing" {
                    return Err(Status::not_found("no such project"));
                }
                let state = if request.state.is_empty() { "open".to_owned() } else { request.state };
                Ok(Response::new(proto::ListPullRequestsResponse {
                    pull_requests: vec![
                        pull_request_summary(&request.project_id, 1, &state),
                        pull_request_summary(&request.project_id, 2, &state),
                    ],
                    synced_at: at(if request.refresh { 1_700_000_100 } else { 1_700_000_000 }),
                }))
            }

            async fn get_pull_request(
                &self,
                request: Request<proto::GetPullRequestRequest>,
            ) -> Result<Response<proto::PullRequest>, Status> {
                principal(&request)?;
                let request = request.into_inner();
                if request.number == 0 {
                    return Err(Status::invalid_argument("number is required"));
                }
                if request.number > 2 {
                    return Err(Status::not_found("no such pull request"));
                }
                Ok(Response::new(proto::PullRequest {
                    summary: Some(pull_request_summary(&request.project_id, request.number, "open")),
                    body: "Adds a Dockerfile.".into(),
                    files: vec![proto::ChangedFile {
                        path: "Dockerfile".into(),
                        status: "added".into(),
                        additions: 2,
                        hunks: vec![proto::DiffHunk {
                            new_start: 1,
                            new_lines: 2,
                            lines: vec!["+FROM node:latest".into(), "+CMD [\"x\"]".into()],
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    touched_fact_ids: vec!["f1".into()],
                    impacted_node_ids: vec!["n1".into()],
                }))
            }

            async fn purge_code_cache(
                &self,
                request: Request<proto::PurgeCodeCacheRequest>,
            ) -> Result<Response<proto::PurgeCodeCacheResponse>, Status> {
                principal(&request)?;
                Ok(Response::new(proto::PurgeCodeCacheResponse {
                    files_deleted: 12,
                    trees_deleted: 1,
                }))
            }

            async fn delete_workspace_data(
                &self,
                request: Request<proto::DeleteWorkspaceDataRequest>,
            ) -> Result<Response<proto::DeleteWorkspaceDataResponse>, Status> {
                let caller = principal(&request)?;
                caller.require(sdi_common::principal::Role::Owner)?;
                Ok(Response::new(proto::DeleteWorkspaceDataResponse {
                    deleted: [("projects".to_owned(), 1u64), ("facts".to_owned(), 40u64)].into(),
                }))
            }

            async fn export_workspace_data(
                &self,
                request: Request<proto::ExportWorkspaceDataRequest>,
            ) -> Result<Response<Self::ExportWorkspaceDataStream>, Status> {
                principal(&request)?;
                let records = vec![
                    Ok(proto::ExportRecord { entity: "projects".into(), record: r#"{"id":"p1"}"#.into() }),
                    Ok(proto::ExportRecord { entity: "facts".into(), record: r#"{"id":"f1"}"#.into() }),
                    Ok(proto::ExportRecord { entity: "facts".into(), record: r#"{"id":"f2"}"#.into() }),
                ];
                Ok(Response::new(stream::iter(records).boxed()))
            }
        }
        unimplemented {
            get_github_account: proto::GetGithubAccountRequest => proto::GithubAccount,
            list_github_repos: proto::ListGithubReposRequest => proto::ListGithubReposResponse,
            connect_project: proto::ConnectProjectRequest => proto::Project,
            get_project: proto::GetProjectRequest => proto::Project,
            delete_project: proto::DeleteProjectRequest => proto::DeleteProjectResponse,
            set_project_mode: proto::SetProjectModeRequest => proto::Project,
            get_scan: proto::GetScanRequest => proto::Scan,
            list_scans: proto::ListScansRequest => proto::ListScansResponse,
            list_facts: proto::ListFactsRequest => proto::ListFactsResponse,
            list_questions: proto::ListQuestionsRequest => proto::ListQuestionsResponse,
            answer_question: proto::AnswerQuestionRequest => proto::Question,
            get_understanding: proto::GetUnderstandingRequest => proto::Understanding,
            get_graph: proto::GetGraphRequest => proto::Graph,
            analyze_impact: proto::AnalyzeImpactRequest => proto::ImpactAnalysis,
            list_dependencies: proto::ListDependenciesRequest => proto::ListDependenciesResponse,
            list_pipelines: proto::ListPipelinesRequest => proto::ListPipelinesResponse,
            list_pipeline_runs: proto::ListPipelineRunsRequest => proto::ListPipelineRunsResponse,
            list_deployments: proto::ListDeploymentsRequest => proto::ListDeploymentsResponse,
            list_patterns: proto::ListPatternsRequest => proto::ListPatternsResponse,
            get_pattern: proto::GetPatternRequest => proto::Pattern,
            record_pattern_outcome: proto::RecordPatternOutcomeRequest => proto::Pattern,
            search_knowledge: proto::SearchKnowledgeRequest => proto::SearchKnowledgeResponse,
            index_knowledge: proto::IndexKnowledgeRequest => proto::IndexKnowledgeResponse,
            list_similar_projects: proto::ListSimilarProjectsRequest => proto::ListSimilarProjectsResponse,
            get_knowledge_stats: proto::GetKnowledgeStatsRequest => proto::KnowledgeStats,
        }
    }

    pub async fn serve() -> SocketAddr {
        super::serve(|router| router.add_service(KnowledgeServiceServer::new(Fake))).await
    }
}

pub mod intelligence {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use sdi_proto::intelligence::v1::{
        self as proto,
        intelligence_service_server::{IntelligenceService, IntelligenceServiceServer},
    };

    /// What `get_settings` reports as `auto_review` (webhook tests flip it).
    pub static AUTO_REVIEW: AtomicBool = AtomicBool::new(false);

    pub struct Fake;

    pub fn settings() -> proto::IntelligenceSettings {
        proto::IntelligenceSettings {
            default_weights: Some(proto::ScoreWeights {
                semantic_similarity: 0.2,
                policy: 0.2,
                recency: 0.1,
                success: 0.15,
                architecture_fit: 0.15,
                cost: 0.1,
                security: 0.1,
            }),
            reasoner: "heuristic".into(),
            model: "claude-sonnet-5-5".into(),
            api_key_configured: false,
            cost_ceiling_usd: 500.0,
            auto_review: false,
        }
    }

    fn proposal(id: String) -> proto::Proposal {
        proto::Proposal {
            id,
            project_id: "p1".into(),
            kind: proto::ProposalKind::Deployment as i32,
            status: proto::ProposalStatus::Proposed as i32,
            title: "Deploy".into(),
            recommended_option_id: "opt-a".into(),
            options: vec![
                proto::ProposalOption {
                    id: "opt-a".into(),
                    title: "A".into(),
                    rank: 1,
                    artifacts: vec![
                        proto::Artifact {
                            path: "Dockerfile".into(),
                            content: "FROM scratch\n".into(),
                            language: "dockerfile".into(),
                            purpose: "Build the image".into(),
                            replaces_existing: false,
                        },
                        proto::Artifact {
                            path: ".dockerignore".into(),
                            content: "target\n".into(),
                            language: "text".into(),
                            purpose: "Keep the context small".into(),
                            replaces_existing: true,
                        },
                    ],
                    ..Default::default()
                },
                proto::ProposalOption {
                    id: "opt-b".into(),
                    title: "B".into(),
                    rank: 2,
                    ..Default::default()
                },
            ],
            created_at: at(1_700_000_000),
            updated_at: at(1_700_000_000),
            ..Default::default()
        }
    }

    fake_service! {
        IntelligenceService for Fake {
            type WatchEventsStream = ServerStream<common::PlatformEvent>;
            type ExportWorkspaceDataStream = ServerStream<proto::WorkspaceDataRecord>;

            /// One event, then open for ever.
            async fn watch_events(
                &self,
                _: Request<common::WatchEventsRequest>,
            ) -> Result<Response<Self::WatchEventsStream>, Status> {
                let first = stream::iter(vec![Ok(platform_event("proposal", "pr1", "created"))]);
                Ok(Response::new(first.chain(stream::pending()).boxed()))
            }

            async fn get_proposal(
                &self,
                request: Request<proto::GetProposalRequest>,
            ) -> Result<Response<proto::Proposal>, Status> {
                principal(&request)?;
                let id = request.into_inner().id;
                if id == "missing" {
                    return Err(Status::not_found("no such proposal"));
                }
                Ok(Response::new(proposal(id)))
            }

            /// Echoes what was asked in the ids so a test can see the
            /// adapter and the create flag the gateway sent.
            async fn accept_proposal(
                &self,
                request: Request<proto::AcceptProposalRequest>,
            ) -> Result<Response<proto::Proposal>, Status> {
                principal(&request)?;
                let request = request.into_inner();
                let mut accepted = proposal(request.id);
                accepted.status = proto::ProposalStatus::Accepted as i32;
                accepted.accepted_option_id = if request.option_id.is_empty() {
                    accepted.recommended_option_id.clone()
                } else {
                    request.option_id
                };
                accepted.execution_id = if request.create_execution {
                    format!("exec-{}", request.adapter)
                } else {
                    String::new()
                };
                Ok(Response::new(accepted))
            }

            async fn get_settings(
                &self,
                request: Request<proto::GetSettingsRequest>,
            ) -> Result<Response<proto::IntelligenceSettings>, Status> {
                principal(&request)?;
                let mut settings = settings();
                settings.auto_review = AUTO_REVIEW.load(Ordering::SeqCst);
                Ok(Response::new(settings))
            }

            async fn update_settings(
                &self,
                request: Request<proto::UpdateSettingsRequest>,
            ) -> Result<Response<proto::IntelligenceSettings>, Status> {
                principal(&request)?;
                request
                    .into_inner()
                    .settings
                    .map(Response::new)
                    .ok_or_else(|| Status::invalid_argument("settings missing"))
            }

            async fn delete_workspace_data(
                &self,
                request: Request<proto::DeleteWorkspaceDataRequest>,
            ) -> Result<Response<proto::DeleteWorkspaceDataResponse>, Status> {
                principal(&request)?;
                Ok(Response::new(proto::DeleteWorkspaceDataResponse {
                    deleted: [("proposals".to_owned(), 2u64)].into(),
                }))
            }

            /// A review that is already proposed; `pr_number` 9 has none.
            async fn start_review(
                &self,
                request: Request<proto::StartReviewRequest>,
            ) -> Result<Response<proto::Review>, Status> {
                let caller = principal(&request)?;
                let request = request.into_inner();
                if request.pr_number == 0 {
                    return Err(Status::invalid_argument("pr_number is required"));
                }
                let mut review = review("r1", &request.project_id, request.pr_number);
                review.requested_by = caller.user_id;
                review.status = proto::ReviewStatus::Context as i32;
                review.summary.clear();
                review.findings.clear();
                Ok(Response::new(review))
            }

            /// Two reviews of PR 1 (newest first) and one of PR 2.
            async fn list_reviews(
                &self,
                request: Request<proto::ListReviewsRequest>,
            ) -> Result<Response<proto::ListReviewsResponse>, Status> {
                principal(&request)?;
                let project_id = request.into_inner().project_id;
                let strip = |mut r: proto::Review| {
                    r.summary.clear();
                    r.findings.clear();
                    r
                };
                Ok(Response::new(proto::ListReviewsResponse {
                    reviews: vec![
                        strip(review("r-new", &project_id, 1)),
                        strip(review("r-old", &project_id, 1)),
                        strip(review("r-2", &project_id, 2)),
                    ],
                }))
            }

            async fn get_review(
                &self,
                request: Request<proto::GetReviewRequest>,
            ) -> Result<Response<proto::Review>, Status> {
                principal(&request)?;
                let id = request.into_inner().id;
                if id == "missing" {
                    return Err(Status::not_found("no such review"));
                }
                Ok(Response::new(review(&id, "p1", 1)))
            }

            async fn decide_finding(
                &self,
                request: Request<proto::DecideFindingRequest>,
            ) -> Result<Response<proto::Finding>, Status> {
                let caller = principal(&request)?;
                let request = request.into_inner();
                let mut finding = finding(&request.finding_id);
                finding.decision = request.decision;
                finding.decision_note = request.note;
                finding.decided_by = caller.user_id;
                finding.decided_at = at(1_700_000_200);
                Ok(Response::new(finding))
            }

            async fn publish_review(
                &self,
                request: Request<proto::PublishReviewRequest>,
            ) -> Result<Response<proto::Review>, Status> {
                let caller = principal(&request)?;
                caller.require(sdi_common::principal::Role::Approver)?;
                let request = request.into_inner();
                let mut review = review(&request.review_id, "p1", 1);
                review.status = proto::ReviewStatus::Published as i32;
                review.published_url = format!(
                    "https://github.com/acme/repo/pull/1#pullrequestreview-{}",
                    if request.event.is_empty() { "comment".to_owned() } else { request.event }
                );
                Ok(Response::new(review))
            }

            async fn list_review_activity(
                &self,
                request: Request<proto::ListReviewActivityRequest>,
            ) -> Result<Response<proto::ListReviewActivityResponse>, Status> {
                principal(&request)?;
                let request = request.into_inner();
                let mut activity = vec![
                    proto::ReviewActivity {
                        id: "a2".into(),
                        review_id: "r1".into(),
                        project_id: "p1".into(),
                        project_name: "repo".into(),
                        pr_number: 1,
                        action: "review.proposed".into(),
                        actor: "system".into(),
                        detail: "2 findings".into(),
                        at: at(1_700_000_050),
                    },
                    proto::ReviewActivity {
                        id: "a1".into(),
                        review_id: "r1".into(),
                        project_id: "p1".into(),
                        project_name: "repo".into(),
                        pr_number: 1,
                        action: "review.started".into(),
                        actor: "u-member".into(),
                        detail: "PR 1".into(),
                        at: at(1_700_000_000),
                    },
                ];
                if let Some(before) = request.before {
                    activity.retain(|a| a.at.as_ref().is_some_and(|t| t.seconds < before.seconds));
                }
                Ok(Response::new(proto::ListReviewActivityResponse { activity }))
            }

            async fn export_workspace_data(
                &self,
                request: Request<proto::ExportWorkspaceDataRequest>,
            ) -> Result<Response<Self::ExportWorkspaceDataStream>, Status> {
                principal(&request)?;
                let records = vec![Ok(proto::WorkspaceDataRecord {
                    entity: "proposals".into(),
                    json: r#"{"id":"pr1","title":"Deploy"}"#.into(),
                })];
                Ok(Response::new(stream::iter(records).boxed()))
            }
        }
        unimplemented {
            generate_proposal: proto::GenerateProposalRequest => proto::Proposal,
            list_proposals: proto::ListProposalsRequest => proto::ListProposalsResponse,
            reject_proposal: proto::RejectProposalRequest => proto::Proposal,
            negotiate: proto::NegotiateRequest => proto::NegotiateResponse,
            list_messages: proto::ListMessagesRequest => proto::ListMessagesResponse,
            list_decisions: proto::ListDecisionsRequest => proto::ListDecisionsResponse,
            create_decision: proto::CreateDecisionRequest => proto::Decision,
            supersede_decision: proto::SupersedeDecisionRequest => proto::Decision,
            generate_document: proto::GenerateDocumentRequest => proto::Document,
            list_documents: proto::ListDocumentsRequest => proto::ListDocumentsResponse,
            get_document: proto::GetDocumentRequest => proto::Document,
            get_intelligence_stats: proto::GetIntelligenceStatsRequest => proto::IntelligenceStats,
            generate_diagram: proto::GenerateDiagramRequest => proto::Diagram,
            list_diagrams: proto::ListDiagramsRequest => proto::ListDiagramsResponse,
            get_diagram: proto::GetDiagramRequest => proto::Diagram,
            update_diagram: proto::UpdateDiagramRequest => proto::Diagram,
            delete_diagram: proto::DeleteDiagramRequest => proto::DeleteDiagramResponse,
        }
    }

    pub fn finding(id: &str) -> proto::Finding {
        proto::Finding {
            id: id.to_owned(),
            review_id: "r1".into(),
            category: "docker".into(),
            severity: proto::FindingSeverity::Medium as i32,
            title: "Base image is not pinned".into(),
            detail: "Line 1 uses `latest`.".into(),
            path: "Dockerfile".into(),
            line: 1,
            end_line: 1,
            suggestion: "FROM node:22".into(),
            rule: "docker.latest_tag".into(),
            decision: proto::FindingDecision::Pending as i32,
            ..Default::default()
        }
    }

    pub fn review(id: &str, project_id: &str, pr_number: u64) -> proto::Review {
        proto::Review {
            id: id.to_owned(),
            project_id: project_id.to_owned(),
            project_name: "repo".into(),
            pr_number,
            pr_title: format!("PR {pr_number}"),
            pr_author: "ana".into(),
            head_sha: format!("sha{pr_number}"),
            status: proto::ReviewStatus::Proposed as i32,
            steps: ["context", "analyze", "findings", "summary", "publish"]
                .iter()
                .map(|key| proto::ReviewStep {
                    key: (*key).to_owned(),
                    label: key.to_uppercase(),
                    status: if *key == "publish" { "pending" } else { "done" }.to_owned(),
                    detail: String::new(),
                    duration_ms: 3,
                })
                .collect(),
            summary: "## Intent\n\nAdds a Dockerfile.".into(),
            findings: vec![finding("f1"), finding("f2")],
            files_reviewed: 1,
            findings_total: 2,
            findings_accepted: 0,
            reasoner: "heuristic".into(),
            requested_by: "u-member".into(),
            created_at: at(1_700_000_000),
            updated_at: at(1_700_000_050),
            ..Default::default()
        }
    }

    pub async fn serve() -> SocketAddr {
        super::serve(|router| router.add_service(IntelligenceServiceServer::new(Fake))).await
    }
}

pub mod identity {
    use super::*;
    use sdi_proto::identity::v1::{
        self as proto,
        identity_service_server::{IdentityService, IdentityServiceServer},
    };

    /// Session tokens the fake knows, and the role each carries in `w1`.
    pub const TOKENS: [(&str, proto::Role); 5] = [
        ("t-owner", proto::Role::Owner),
        ("t-admin", proto::Role::Admin),
        ("t-approver", proto::Role::Approver),
        ("t-member", proto::Role::Member),
        ("t-viewer", proto::Role::Viewer),
    ];

    /// Counts `RecordAudit` calls and remembers the last action.
    #[derive(Default)]
    pub struct Recorder {
        pub audits: AtomicUsize,
        pub last_action: std::sync::Mutex<String>,
    }

    pub struct Fake {
        pub recorder: Arc<Recorder>,
    }

    pub fn user() -> proto::User {
        proto::User {
            id: "u1".into(),
            github_id: 1,
            login: "octo".into(),
            name: "Octo Cat".into(),
            avatar_url: "https://avatars/octo".into(),
            email: "octo@example.com".into(),
            github_host: "github.com".into(),
            created_at: at(1_700_000_000),
        }
    }

    pub fn workspace(role: proto::Role) -> proto::Workspace {
        proto::Workspace {
            id: WORKSPACE.into(),
            kind: proto::WorkspaceKind::Organization as i32,
            slug: "acme".into(),
            name: "Acme".into(),
            avatar_url: "https://avatars/acme".into(),
            github_host: "github.com".into(),
            installation_id: 777,
            repository_selection: "all".into(),
            my_role: role as i32,
            member_count: 2,
            project_count: 0,
            created_at: at(1_700_000_000),
        }
    }

    fn member(role: proto::Role) -> proto::Member {
        proto::Member {
            user: Some(user()),
            github_role: "member".into(),
            teams: vec!["sdi-admins".into()],
            derived_role: proto::Role::Admin as i32,
            override_role: if role == proto::Role::Admin {
                0
            } else {
                role as i32
            },
            effective_role: role as i32,
            synced_at: at(1_700_000_000),
        }
    }

    fn controls() -> proto::DataControls {
        proto::DataControls {
            workspace_id: WORKSPACE.into(),
            code_cache: "session".into(),
            code_cache_ttl_hours: 24,
            minimal_chunks: false,
            reasoner: "off".into(),
            reasoner_key_configured: false,
            reasoner_key_hint: String::new(),
            audit_retention_days: 365,
            updated_at: at(1_700_000_000),
        }
    }

    fake_service! {
        IdentityService for Fake {
            async fn authorize(
                &self,
                request: Request<proto::AuthorizeRequest>,
            ) -> Result<Response<proto::Principal>, Status> {
                let request = request.into_inner();
                let Some((_, role)) = TOKENS.iter().find(|(t, _)| *t == request.session_token) else {
                    return Err(Status::unauthenticated("session expired or revoked"));
                };
                if !request.workspace_id.is_empty() && request.workspace_id != WORKSPACE {
                    return Err(Status::permission_denied("no such workspace"));
                }
                Ok(Response::new(proto::Principal {
                    user: Some(user()),
                    workspace: Some(workspace(*role)),
                    role: *role as i32,
                }))
            }

            async fn begin_login(
                &self,
                request: Request<proto::BeginLoginRequest>,
            ) -> Result<Response<proto::BeginLoginResponse>, Status> {
                let return_to = request.into_inner().return_to;
                Ok(Response::new(proto::BeginLoginResponse {
                    authorize_url: format!("/api/v1/auth/callback?code=dev&state=dev&returnTo={return_to}"),
                }))
            }

            async fn complete_login(
                &self,
                request: Request<proto::CompleteLoginRequest>,
            ) -> Result<Response<proto::Session>, Status> {
                let request = request.into_inner();
                if request.code != "dev" {
                    return Err(Status::invalid_argument("bad code"));
                }
                Ok(Response::new(proto::Session {
                    token: "t-owner".into(),
                    user: Some(user()),
                    expires_at: at(1_900_000_000),
                    return_to: "/".into(),
                }))
            }

            async fn revoke_session(
                &self,
                _: Request<proto::RevokeSessionRequest>,
            ) -> Result<Response<proto::RevokeSessionResponse>, Status> {
                Ok(Response::new(proto::RevokeSessionResponse {}))
            }

            async fn list_workspaces(
                &self,
                request: Request<proto::ListWorkspacesRequest>,
            ) -> Result<Response<proto::ListWorkspacesResponse>, Status> {
                if request.into_inner().user_id != "u1" {
                    return Err(Status::invalid_argument("unknown user"));
                }
                Ok(Response::new(proto::ListWorkspacesResponse {
                    workspaces: vec![workspace(proto::Role::Owner)],
                }))
            }

            async fn list_members(
                &self,
                _: Request<proto::ListMembersRequest>,
            ) -> Result<Response<proto::ListMembersResponse>, Status> {
                Ok(Response::new(proto::ListMembersResponse {
                    members: vec![member(proto::Role::Admin)],
                    synced_at: at(1_700_000_000),
                }))
            }

            async fn set_member_role(
                &self,
                request: Request<proto::SetMemberRoleRequest>,
            ) -> Result<Response<proto::Member>, Status> {
                let request = request.into_inner();
                let role = proto::Role::try_from(request.override_role).unwrap_or_default();
                if role >= proto::Role::Admin {
                    return Err(Status::invalid_argument("override is not lower than the derived role"));
                }
                Ok(Response::new(member(if role == proto::Role::Unspecified { proto::Role::Admin } else { role })))
            }

            async fn sync_members(
                &self,
                _: Request<proto::SyncMembersRequest>,
            ) -> Result<Response<proto::ListMembersResponse>, Status> {
                Ok(Response::new(proto::ListMembersResponse {
                    members: vec![member(proto::Role::Admin)],
                    synced_at: at(1_700_000_100),
                }))
            }

            async fn record_installation(
                &self,
                request: Request<proto::RecordInstallationRequest>,
            ) -> Result<Response<proto::Workspace>, Status> {
                let request = request.into_inner();
                if request.installation_id != 777 {
                    return Err(Status::not_found("unknown installation"));
                }
                Ok(Response::new(workspace(proto::Role::Owner)))
            }

            async fn remove_installation(
                &self,
                _: Request<proto::RemoveInstallationRequest>,
            ) -> Result<Response<proto::RemoveInstallationResponse>, Status> {
                Ok(Response::new(proto::RemoveInstallationResponse {}))
            }

            async fn record_audit(
                &self,
                request: Request<proto::RecordAuditRequest>,
            ) -> Result<Response<proto::RecordAuditResponse>, Status> {
                let entry = request.into_inner().entry.unwrap_or_default();
                if entry.workspace_id != WORKSPACE {
                    return Err(Status::invalid_argument("wrong workspace"));
                }
                *self.recorder.last_action.lock().unwrap() = entry.action;
                self.recorder.audits.fetch_add(1, Ordering::SeqCst);
                Ok(Response::new(proto::RecordAuditResponse {}))
            }

            async fn list_audit(
                &self,
                request: Request<proto::ListAuditRequest>,
            ) -> Result<Response<proto::ListAuditResponse>, Status> {
                let request = request.into_inner();
                // One page, then nothing: lets the export's pagination end.
                if request.before.is_some() {
                    return Ok(Response::new(proto::ListAuditResponse::default()));
                }
                Ok(Response::new(proto::ListAuditResponse {
                    entries: vec![proto::AuditEntry {
                        id: "a1".into(),
                        workspace_id: WORKSPACE.into(),
                        user_id: "u1".into(),
                        user_login: "octo".into(),
                        action: "project.connect".into(),
                        entity: "project".into(),
                        entity_id: "p1".into(),
                        at: at(1_700_000_000),
                        ..Default::default()
                    }],
                }))
            }

            async fn get_data_controls(
                &self,
                _: Request<proto::GetDataControlsRequest>,
            ) -> Result<Response<proto::DataControls>, Status> {
                Ok(Response::new(controls()))
            }

            async fn update_data_controls(
                &self,
                request: Request<proto::UpdateDataControlsRequest>,
            ) -> Result<Response<proto::DataControls>, Status> {
                request
                    .into_inner()
                    .controls
                    .map(Response::new)
                    .ok_or_else(|| Status::invalid_argument("controls missing"))
            }

            async fn set_reasoner_key(
                &self,
                request: Request<proto::SetReasonerKeyRequest>,
            ) -> Result<Response<proto::DataControls>, Status> {
                let key = request.into_inner().api_key;
                Ok(Response::new(proto::DataControls {
                    reasoner_key_configured: !key.is_empty(),
                    reasoner_key_hint: key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect(),
                    ..controls()
                }))
            }
        }
        unimplemented {
            get_session: proto::GetSessionRequest => proto::Session,
            get_workspace: proto::GetWorkspaceRequest => proto::Workspace,
            get_installation_token: proto::GetInstallationTokenRequest => proto::InstallationToken,
            get_reasoner_key: proto::GetReasonerKeyRequest => proto::ReasonerKey,
        }
    }

    pub async fn serve() -> (SocketAddr, Arc<Recorder>) {
        let recorder = Arc::new(Recorder::default());
        let fake = Fake {
            recorder: Arc::clone(&recorder),
        };
        let addr =
            super::serve(|router| router.add_service(IdentityServiceServer::new(fake))).await;
        (addr, recorder)
    }
}
