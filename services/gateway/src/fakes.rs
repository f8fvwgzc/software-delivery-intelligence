//! Real tonic servers in-process standing in for the three services, so the
//! SSE bridges, the gRPC status mapping, the settings composition and the
//! health check are exercised against actual streams and calls instead of a
//! closed port. Each fake implements the handful of RPCs a gateway test
//! needs for real and answers `UNIMPLEMENTED` to the rest.

use std::{net::SocketAddr, pin::Pin, time::Duration};

use futures::{Stream, StreamExt, stream};
use sdi_proto::common::v1 as common;
use tonic::{Request, Response, Status, transport::server::Router};
use tonic_health::ServingStatus;

pub type ServerStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

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

pub fn platform_event(entity: &str, entity_id: &str, action: &str) -> common::PlatformEvent {
    common::PlatformEvent {
        entity: entity.into(),
        entity_id: entity_id.into(),
        project_id: "p1".into(),
        action: action.into(),
        status: String::new(),
        at: at(1_700_000_000),
        data: Default::default(),
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

            async fn watch_execution(
                &self,
                request: Request<proto::WatchExecutionRequest>,
            ) -> Result<Response<Self::WatchExecutionStream>, Status> {
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
                let request = request.into_inner();
                Ok(Response::new(proto::Execution {
                    id: "e-new".into(),
                    project_id: request.project_id,
                    title: request.title,
                    adapter: request.adapter,
                    status: proto::ExecutionStatus::Planning as i32,
                    outputs: request.artifacts,
                    ..Default::default()
                }))
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

            async fn watch_scan(
                &self,
                request: Request<proto::WatchScanRequest>,
            ) -> Result<Response<Self::WatchScanStream>, Status> {
                if request.into_inner().id == "missing" {
                    return Err(Status::not_found("no such scan"));
                }
                let scans = vec![
                    Ok(scan(proto::ScanStatus::Running)),
                    Ok(scan(proto::ScanStatus::Succeeded)),
                ];
                Ok(Response::new(stream::iter(scans).boxed()))
            }

            /// Two events, then the stream ends: the "service dropped" case.
            async fn watch_events(
                &self,
                _: Request<common::WatchEventsRequest>,
            ) -> Result<Response<Self::WatchEventsStream>, Status> {
                let events = vec![
                    Ok(platform_event("scan", "s1", "progress")),
                    Ok(platform_event("project", "p1", "updated")),
                ];
                Ok(Response::new(stream::iter(events).boxed()))
            }

            async fn get_maturity(
                &self,
                request: Request<proto::GetMaturityRequest>,
            ) -> Result<Response<proto::MaturityScore>, Status> {
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
                _: Request<proto::GetSettingsRequest>,
            ) -> Result<Response<proto::KnowledgeSettings>, Status> {
                Ok(Response::new(settings()))
            }

            async fn update_settings(
                &self,
                request: Request<proto::UpdateSettingsRequest>,
            ) -> Result<Response<proto::KnowledgeSettings>, Status> {
                request
                    .into_inner()
                    .settings
                    .map(Response::new)
                    .ok_or_else(|| Status::invalid_argument("settings missing"))
            }
        }
        unimplemented {
            get_github_account: proto::GetGithubAccountRequest => proto::GithubAccount,
            list_github_repos: proto::ListGithubReposRequest => proto::ListGithubReposResponse,
            connect_project: proto::ConnectProjectRequest => proto::Project,
            list_projects: proto::ListProjectsRequest => proto::ListProjectsResponse,
            get_project: proto::GetProjectRequest => proto::Project,
            delete_project: proto::DeleteProjectRequest => proto::DeleteProjectResponse,
            set_project_mode: proto::SetProjectModeRequest => proto::Project,
            start_scan: proto::StartScanRequest => proto::Scan,
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
    use super::*;
    use sdi_proto::intelligence::v1::{
        self as proto,
        intelligence_service_server::{IntelligenceService, IntelligenceServiceServer},
    };

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
                _: Request<proto::GetSettingsRequest>,
            ) -> Result<Response<proto::IntelligenceSettings>, Status> {
                Ok(Response::new(settings()))
            }

            async fn update_settings(
                &self,
                request: Request<proto::UpdateSettingsRequest>,
            ) -> Result<Response<proto::IntelligenceSettings>, Status> {
                request
                    .into_inner()
                    .settings
                    .map(Response::new)
                    .ok_or_else(|| Status::invalid_argument("settings missing"))
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

    pub async fn serve() -> SocketAddr {
        super::serve(|router| router.add_service(IntelligenceServiceServer::new(Fake))).await
    }
}
