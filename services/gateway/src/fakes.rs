//! Real tonic servers in-process standing in for the five services, so the
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

    /// Every `CreateRelease` the fake answered, with the `x-sdi-user` that
    /// asked: webhook tests read here what the gateway sent, and as whom.
    pub static RELEASES: std::sync::Mutex<Vec<(String, proto::CreateReleaseRequest)>> =
        std::sync::Mutex::new(Vec::new());

    /// Every `gitops` execution the fake created: its id, the role that
    /// asked and the request, so a test reads the plan the gateway built.
    pub static GITOPS_EXECUTIONS: std::sync::Mutex<
        Vec<(String, String, proto::CreateExecutionRequest)>,
    > = std::sync::Mutex::new(Vec::new());

    /// Ids of the executions the fake was asked to start, in order.
    pub static STARTED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    static NEXT_GITOPS: AtomicUsize = AtomicUsize::new(1);

    pub fn registry(id: &str) -> proto::Registry {
        proto::Registry {
            id: id.to_owned(),
            name: "ghcr".into(),
            kind: "ghcr".into(),
            host: "ghcr.io".into(),
            namespace: "acme".into(),
            auth: "github_token".into(),
            is_default: true,
            created_at: at(1_700_000_000),
            updated_at: at(1_700_000_000),
            ..Default::default()
        }
    }

    pub fn environment(id: &str, project_id: &str) -> proto::Environment {
        proto::Environment {
            id: id.to_owned(),
            project_id: project_id.to_owned(),
            name: "staging".into(),
            position: 0,
            kind: "github_actions".into(),
            config: [("workflow".to_owned(), "deploy.yml".to_owned())].into(),
            url: "https://staging.example.com".into(),
            health_path: "/health".into(),
            source: "ui".into(),
            current_release_id: "rel1".into(),
            created_at: at(1_700_000_000),
            updated_at: at(1_700_000_000),
            ..Default::default()
        }
    }

    pub fn release(id: &str, project_id: &str) -> proto::Release {
        proto::Release {
            id: id.to_owned(),
            project_id: project_id.to_owned(),
            version: "a1b2c3d".into(),
            commit_sha: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".into(),
            image: "ghcr.io/acme/repo".into(),
            tag: "a1b2c3d".into(),
            source: "manual".into(),
            notes: "- Add health endpoint (a1b2c3d)".into(),
            created_by: "u1".into(),
            created_at: at(1_700_000_000),
            deployed_to: vec!["staging".into()],
            ..Default::default()
        }
    }

    pub fn run(id: &str, kind: &str) -> proto::DeploymentRun {
        proto::DeploymentRun {
            id: id.to_owned(),
            project_id: "p1".into(),
            environment_id: "e1".into(),
            environment_name: "staging".into(),
            release_id: "rel1".into(),
            release_version: "a1b2c3d".into(),
            kind: kind.to_owned(),
            status: proto::DeploymentStatus::AwaitingApproval as i32,
            execution_id: "x1".into(),
            started_by: "u1".into(),
            created_at: at(1_700_000_000),
            ..Default::default()
        }
    }

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

            /// One event after a short delay, then the three delivery
            /// entities of CONTRACT.md §22, then open for ever: proves the
            /// merged feed still delivers after another service dropped,
            /// and that it passes entities it has no special case for.
            async fn watch_events(
                &self,
                _: Request<common::WatchEventsRequest>,
            ) -> Result<Response<Self::WatchEventsStream>, Status> {
                let late = stream::once(async {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    Ok(platform_event("execution", "e1", "updated"))
                });
                let delivery = stream::iter(vec![
                    Ok(common::PlatformEvent {
                        status: "awaiting_approval".into(),
                        ..platform_event("deployment", "d1", "updated")
                    }),
                    Ok(common::PlatformEvent {
                        status: "pipeline".into(),
                        ..platform_event("release", "rel1", "created")
                    }),
                    Ok(common::PlatformEvent {
                        status: "deployed".into(),
                        ..platform_event("environment", "env1", "updated")
                    }),
                    // Another workspace's deployment must not leak either.
                    Ok(platform_event_in("w2", "deployment", "other", "updated")),
                ]);
                Ok(Response::new(
                    late.chain(delivery).chain(stream::pending()).boxed(),
                ))
            }

            /// `gitops` executions get an id of their own and are
            /// remembered with their plan; every other one is `e-new`.
            async fn create_execution(
                &self,
                request: Request<proto::CreateExecutionRequest>,
            ) -> Result<Response<proto::Execution>, Status> {
                let caller = principal(&request)?;
                let request = request.into_inner();
                let id = if request.adapter == "gitops" {
                    // The project of `a-blocked`: its start is refused.
                    let id = if request.project_id == "blocked" {
                        "refuse-start".to_owned()
                    } else {
                        format!("x-gitops-{}", NEXT_GITOPS.fetch_add(1, Ordering::SeqCst))
                    };
                    GITOPS_EXECUTIONS.lock().unwrap().push((
                        id.clone(),
                        caller.role.as_str().to_owned(),
                        request.clone(),
                    ));
                    id
                } else {
                    "e-new".to_owned()
                };
                Ok(Response::new(proto::Execution {
                    id,
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

            /// Remembers the id; `refuse-start` is refused the way a policy
            /// that blocks the plan would be.
            async fn start_execution(
                &self,
                request: Request<proto::StartExecutionRequest>,
            ) -> Result<Response<proto::Execution>, Status> {
                principal(&request)?;
                let id = request.into_inner().id;
                if id == "refuse-start" {
                    return Err(Status::failed_precondition("a step of this plan is blocked by policy"));
                }
                STARTED.lock().unwrap().push(id.clone());
                Ok(Response::new(proto::Execution {
                    id,
                    status: proto::ExecutionStatus::WaitingForApproval as i32,
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

            // --- continuous delivery (CONTRACT.md §22) ----------------------

            async fn list_registries(
                &self,
                request: Request<proto::ListRegistriesRequest>,
            ) -> Result<Response<proto::ListRegistriesResponse>, Status> {
                principal(&request)?;
                Ok(Response::new(proto::ListRegistriesResponse {
                    registries: vec![registry("r1")],
                }))
            }

            /// Echoes what it was sent, with an id when it had none.
            async fn upsert_registry(
                &self,
                request: Request<proto::UpsertRegistryRequest>,
            ) -> Result<Response<proto::Registry>, Status> {
                let caller = principal(&request)?;
                caller.require(sdi_common::principal::Role::Admin)?;
                let mut registry = request.into_inner().registry.unwrap_or_default();
                if registry.name == "taken" {
                    return Err(Status::already_exists(
                        "name: this workspace already has a registry named taken",
                    ));
                }
                if registry.id.is_empty() {
                    registry.id = "r-new".into();
                }
                registry.created_at = at(1_700_000_000);
                registry.updated_at = at(1_700_000_100);
                Ok(Response::new(registry))
            }

            async fn delete_registry(
                &self,
                request: Request<proto::DeleteRegistryRequest>,
            ) -> Result<Response<proto::DeleteRegistryResponse>, Status> {
                principal(&request)?;
                if request.into_inner().id == "r1" {
                    return Err(Status::failed_precondition(
                        "the default registry cannot be deleted: make another registry the default first",
                    ));
                }
                Ok(Response::new(proto::DeleteRegistryResponse {}))
            }

            async fn list_environments(
                &self,
                request: Request<proto::ListEnvironmentsRequest>,
            ) -> Result<Response<proto::ListEnvironmentsResponse>, Status> {
                principal(&request)?;
                let project_id = request.into_inner().project_id;
                Ok(Response::new(proto::ListEnvironmentsResponse {
                    environments: vec![environment("e1", &project_id)],
                }))
            }

            /// Echoes what it was sent (position included, so a test sees
            /// what an omitted one travels as) and validates like the real
            /// service where the tests look.
            async fn upsert_environment(
                &self,
                request: Request<proto::UpsertEnvironmentRequest>,
            ) -> Result<Response<proto::Environment>, Status> {
                let caller = principal(&request)?;
                caller.require(sdi_common::principal::Role::Admin)?;
                let mut environment = request.into_inner().environment.unwrap_or_default();
                if environment.kind == "gitops_flux" && !environment.config.contains_key("path") {
                    return Err(Status::invalid_argument(
                        "config.path is required for gitops_flux environments",
                    ));
                }
                if environment.id == "from-config" {
                    return Err(Status::failed_precondition(
                        "this environment comes from .sdi.yml and is read-only here: change the file and rescan",
                    ));
                }
                if environment.id.is_empty() {
                    environment.id = "e-new".into();
                }
                environment.source = "ui".into();
                environment.created_at = at(1_700_000_000);
                environment.updated_at = at(1_700_000_100);
                Ok(Response::new(environment))
            }

            async fn delete_environment(
                &self,
                request: Request<proto::DeleteEnvironmentRequest>,
            ) -> Result<Response<proto::DeleteEnvironmentResponse>, Status> {
                principal(&request)?;
                if request.into_inner().id == "missing" {
                    return Err(Status::not_found("no such environment"));
                }
                Ok(Response::new(proto::DeleteEnvironmentResponse {}))
            }

            async fn get_pipeline(
                &self,
                request: Request<proto::GetPipelineRequest>,
            ) -> Result<Response<proto::DeliveryPipeline>, Status> {
                principal(&request)?;
                let project_id = request.into_inner().project_id;
                Ok(Response::new(proto::DeliveryPipeline {
                    stages: vec![
                        proto::PipelineStage {
                            environment: Some(environment("e1", &project_id)),
                            current: Some(release("rel1", &project_id)),
                            last_run: Some(run("d1", "promote")),
                            candidate: Some(release("rel2", &project_id)),
                            health: "healthy".into(),
                        },
                        proto::PipelineStage {
                            environment: Some(proto::Environment {
                                id: "e2".into(),
                                name: "production".into(),
                                position: 1,
                                kind: "webhook".into(),
                                protected: true,
                                source: "config".into(),
                                current_release_id: String::new(),
                                url: String::new(),
                                health_path: String::new(),
                                config: Default::default(),
                                ..environment("e2", &project_id)
                            }),
                            health: "unknown".into(),
                            ..Default::default()
                        },
                    ],
                    registry: Some(registry("r1")),
                    success_rate: 0.75,
                    deployments_30d: 8,
                    finished_30d: 4,
                    lead_time_seconds: 5400,
                    project_id,
                }))
            }

            /// Two releases; `limit` and `before` are echoed in what comes back.
            async fn list_releases(
                &self,
                request: Request<proto::ListReleasesRequest>,
            ) -> Result<Response<proto::ListReleasesResponse>, Status> {
                principal(&request)?;
                let request = request.into_inner();
                let mut releases = vec![
                    release("rel2", &request.project_id),
                    release("rel1", &request.project_id),
                ];
                if request.limit == 1 {
                    releases.truncate(1);
                }
                if request.before.is_some() {
                    releases.clear();
                }
                Ok(Response::new(proto::ListReleasesResponse { releases }))
            }

            /// Records who asked for what and echoes it as a release. Only
            /// `system` may say a pipeline or a package made the release.
            async fn create_release(
                &self,
                request: Request<proto::CreateReleaseRequest>,
            ) -> Result<Response<proto::Release>, Status> {
                let caller = principal(&request)?;
                let request = request.into_inner();
                if request.source != "manual"
                    && caller.user_id != sdi_common::principal::SYSTEM_USER
                {
                    return Err(Status::permission_denied("only webhooks create such releases"));
                }
                if request.commit_sha == "0000000" {
                    return Err(Status::invalid_argument(
                        "commit_sha: acme/repo has no commit 0000000",
                    ));
                }
                RELEASES
                    .lock()
                    .unwrap()
                    .push((caller.user_id.clone(), request.clone()));
                let mut created = release("rel-new", &request.project_id);
                created.source = request.source;
                created.created_by = caller.user_id;
                created.pipeline_run_id = request.pipeline_run_id;
                created.deployed_to.clear();
                if !request.version.is_empty() {
                    created.version = request.version;
                }
                if !request.commit_sha.is_empty() {
                    created.commit_sha = request.commit_sha;
                }
                if !request.tag.is_empty() {
                    created.tag = request.tag;
                }
                if !request.image.is_empty() {
                    created.image = request.image;
                }
                created.digest = request.digest;
                Ok(Response::new(created))
            }

            async fn get_release(
                &self,
                request: Request<proto::GetReleaseRequest>,
            ) -> Result<Response<proto::Release>, Status> {
                principal(&request)?;
                let id = request.into_inner().id;
                if id == "missing" {
                    return Err(Status::not_found("no such release"));
                }
                Ok(Response::new(release(&id, "p1")))
            }

            async fn promote(
                &self,
                request: Request<proto::PromoteRequest>,
            ) -> Result<Response<proto::DeploymentRun>, Status> {
                let caller = principal(&request)?;
                caller.require(sdi_common::principal::Role::Approver)?;
                let request = request.into_inner();
                if request.environment_id == "busy" {
                    return Err(Status::failed_precondition(
                        "a deployment of a1b2c3d to staging is still open (deploying); finish, resolve or cancel it first",
                    ));
                }
                let mut started = run("d-new", "promote");
                started.environment_id = request.environment_id;
                started.release_id = if request.release_id.is_empty() {
                    "candidate".into()
                } else {
                    request.release_id
                };
                started.started_by = caller.user_id;
                Ok(Response::new(started))
            }

            async fn rollback(
                &self,
                request: Request<proto::RollbackRequest>,
            ) -> Result<Response<proto::DeploymentRun>, Status> {
                let caller = principal(&request)?;
                caller.require(sdi_common::principal::Role::Approver)?;
                let request = request.into_inner();
                let mut started = run("d-back", "rollback");
                started.environment_id = request.environment_id;
                started.release_id = if request.release_id.is_empty() {
                    "previous".into()
                } else {
                    request.release_id
                };
                // The reason, where a test can see it arrived.
                started.external_url = request.reason;
                started.previous_release_id = "rel2".into();
                Ok(Response::new(started))
            }

            async fn list_deployment_runs(
                &self,
                request: Request<proto::ListDeploymentRunsRequest>,
            ) -> Result<Response<proto::ListDeploymentRunsResponse>, Status> {
                principal(&request)?;
                let request = request.into_inner();
                let mut runs = vec![run("d2", "rollback"), run("d1", "promote")];
                if !request.environment_id.is_empty() {
                    runs.retain(|r| r.environment_id == request.environment_id);
                }
                if request.limit == 1 {
                    runs.truncate(1);
                }
                Ok(Response::new(proto::ListDeploymentRunsResponse { runs }))
            }

            async fn get_deployment_run(
                &self,
                request: Request<proto::GetDeploymentRunRequest>,
            ) -> Result<Response<proto::DeploymentRun>, Status> {
                principal(&request)?;
                let id = request.into_inner().id;
                if id == "missing" {
                    return Err(Status::not_found("no such deployment run"));
                }
                let mut found = run(&id, "promote");
                found.status = proto::DeploymentStatus::Succeeded as i32;
                found.finished_at = at(1_700_000_090);
                found.duration_seconds = 90;
                found.external_url = "https://github.com/acme/repo/actions/runs/7".into();
                Ok(Response::new(found))
            }
        }
        unimplemented {
            sync_environments: proto::SyncEnvironmentsRequest => proto::ListEnvironmentsResponse,
            list_executions: proto::ListExecutionsRequest => proto::ListExecutionsResponse,
            get_execution: proto::GetExecutionRequest => proto::Execution,
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

    /// PR 2 is Dependabot's and PR 3 comes from a fork; every other number
    /// is a person's branch in the repository itself.
    pub fn pull_request_summary(
        project_id: &str,
        number: u64,
        state: &str,
    ) -> proto::PullRequestSummary {
        let dependabot = number == 2;
        let fork = number == 3;
        proto::PullRequestSummary {
            project_id: project_id.to_owned(),
            number,
            title: format!("PR {number}"),
            author: match number {
                2 => "dependabot[bot]",
                3 => "eve",
                _ => "ana",
            }
            .into(),
            state: state.to_owned(),
            base_ref: "main".into(),
            head_ref: format!("feat/{number}"),
            head_sha: format!("sha{number}"),
            changed_files: 1,
            additions: 2,
            html_url: format!("https://github.com/acme/repo/pull/{number}"),
            created_at: at(1_700_000_000),
            updated_at: at(1_700_000_000),
            head_repo: if fork { "eve/repo" } else { "acme/repo" }.into(),
            is_fork: fork,
            is_bot: dependabot,
            is_dependabot: dependabot,
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
                    projects: vec![
                        proto::Project {
                            id: "p1".into(),
                            name: "repo".into(),
                            repo_full_name: "acme/repo".into(),
                            ..Default::default()
                        },
                        // Its `.sdi.yml` names the build workflow.
                        proto::Project {
                            id: "p-configured".into(),
                            name: "configured".into(),
                            repo_full_name: "acme/configured".into(),
                            ..Default::default()
                        },
                    ],
                }))
            }

            /// One project in `w1`, none anywhere else; only `system` may
            /// ask for several workspaces, like the real service.
            async fn count_projects(
                &self,
                request: Request<proto::CountProjectsRequest>,
            ) -> Result<Response<proto::CountProjectsResponse>, Status> {
                let caller = principal(&request)?;
                let ids = request.into_inner().workspace_ids;
                if ids.len() > 1 && caller.user_id != sdi_common::principal::SYSTEM_USER {
                    return Err(Status::permission_denied("system only"));
                }
                let counts = ids
                    .into_iter()
                    .filter(|id| id == WORKSPACE)
                    .map(|id| (id, 1))
                    .collect();
                Ok(Response::new(proto::CountProjectsResponse { counts }))
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
                // What the webhook worker reads to recognise a build
                // workflow (CONTRACT.md §22): `p1` has no `.sdi.yml` and a
                // scan that found `ci.yml` building an image;
                // `p-configured` names `release.yml` in its file, which
                // wins over what the scan found.
                let builds_image = |path: &str| proto::Fact {
                    id: format!("f-{path}"),
                    project_id: project_id.clone(),
                    key: format!("ci:build_image:{path}"),
                    label: "CI builds a container image".into(),
                    ..Default::default()
                };
                let (config, facts) = match project_id.as_str() {
                    "p1" => (None, vec![builds_image(".github/workflows/ci.yml")]),
                    "p-configured" => (
                        Some(proto::ProjectConfig {
                            present: true,
                            path: ".sdi.yml".into(),
                            delivery_json: r#"{"registry":{"kind":"ghcr"},"build_workflow":"release.yml","environments":[]}"#.into(),
                            ..Default::default()
                        }),
                        vec![builds_image(".github/workflows/ci.yml")],
                    ),
                    _ => (config, Vec::new()),
                };
                Ok(Response::new(proto::ProjectContext {
                    config,
                    facts,
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

            /// Three pull requests of the project (a person's, Dependabot's
            /// and one from a fork); `refresh` is echoed in `synced_at`.
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
                        pull_request_summary(&request.project_id, 3, &state),
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
                if request.number > 3 {
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
            // Pull request autopilot (CONTRACT.md §23): service-to-service
            // only. Intelligence reads these to answer merge readiness and
            // the security overview; the gateway has no route that calls them.
            get_pull_request_status: proto::GetPullRequestStatusRequest => proto::PullRequestStatus,
            get_repository_security: proto::GetRepositorySecurityRequest => proto::RepositorySecurity,
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
            // Pull request autopilot (§23): the defaults, and a watcher that
            // looked once and had nothing to complain about.
            auto_draft_fixes: false,
            auto_merge: "off".into(),
            merge_requires_review: true,
            pr_watch_interval_seconds: 60,
            pr_watch_last_run_at: at(1_700_000_000),
            pr_watch_last_error: String::new(),
            ..Default::default()
        }
    }

    /// Readiness of a pull request of the fake repository: PR 1 can be
    /// merged, PR 2 (Dependabot's) has a failed check, a running one and no
    /// review, and no other pull request exists.
    pub fn readiness(project_id: &str, pr_number: u64) -> Result<proto::MergeReadiness, Status> {
        if project_id == "missing" {
            return Err(Status::not_found("no such project"));
        }
        let common = proto::MergeReadiness {
            project_id: project_id.to_owned(),
            pr_number,
            title: format!("PR {pr_number}"),
            state: "open".into(),
            head_sha: format!("sha{pr_number}"),
            head_ref: format!("feat/{pr_number}"),
            base_ref: "main".into(),
            checks_total: 3,
            allowed_methods: vec!["squash".into(), "merge".into()],
            base_protected: true,
            checked_at: at(1_700_000_300),
            ..Default::default()
        };
        match pr_number {
            1 => Ok(proto::MergeReadiness {
                mergeable: "mergeable".into(),
                mergeable_state: "clean".into(),
                checks_passed: 3,
                approvals: 1,
                review_id: "r-new".into(),
                review_status: "proposed".into(),
                can_merge: true,
                ..common
            }),
            2 => Ok(proto::MergeReadiness {
                is_dependabot: true,
                mergeable: "mergeable".into(),
                mergeable_state: "unstable".into(),
                checks_passed: 1,
                checks_failed: 1,
                checks_pending: 1,
                failed_checks: vec!["ci / test".into()],
                pending_checks: vec!["ci / lint".into()],
                behind_by: 2,
                blockers: vec![
                    proto::MergeBlocker {
                        code: "checks_failing".into(),
                        message: "1 check failed (ci / test): fix it or re-run it, then ask again"
                            .into(),
                    },
                    proto::MergeBlocker {
                        code: "no_review".into(),
                        message: "SDI has not reviewed this pull request yet: start a review"
                            .into(),
                    },
                ],
                can_merge: false,
                ..common
            }),
            _ => Err(Status::not_found("no such pull request")),
        }
    }

    fn vulnerability(
        source: &str,
        id: &str,
        package: &str,
        severity: &str,
    ) -> proto::Vulnerability {
        proto::Vulnerability {
            source: source.to_owned(),
            id: id.to_owned(),
            ecosystem: "npm".into(),
            package: package.to_owned(),
            severity: severity.to_owned(),
            summary: format!("{package} is vulnerable"),
            manifest_path: "package.json".into(),
            ..Default::default()
        }
    }

    /// The security overview of a project: Dependabot's list, except for
    /// `p-osv`, whose repository has alerts turned off.
    pub fn security_overview(project_id: &str, refresh: bool) -> proto::SecurityOverview {
        // `refresh` is echoed in `checked_at`.
        let checked_at = at(if refresh {
            1_700_000_100
        } else {
            1_700_000_000
        });
        if project_id == "p-osv" {
            return proto::SecurityOverview {
                project_id: project_id.to_owned(),
                source: "osv".into(),
                source_note: "Dependabot alerts are turned off for this repository, so these are the manifest versions looked up at OSV.".into(),
                vulnerabilities: vec![proto::Vulnerability {
                    current_version: "4.17.20".into(),
                    ..vulnerability("osv", "GHSA-35jh-r3h4-6jhm", "lodash", "high")
                }],
                by_severity: vec![proto::SeverityCount { severity: "high".into(), count: 1 }],
                alerts: "disabled".into(),
                security_updates: "disabled".into(),
                checked_at,
                ..Default::default()
            };
        }
        proto::SecurityOverview {
            project_id: project_id.to_owned(),
            source: "dependabot".into(),
            vulnerabilities: vec![
                proto::Vulnerability {
                    cve_id: "CVE-2021-23337".into(),
                    vulnerable_range: "< 4.17.21".into(),
                    fixed_version: "4.17.21".into(),
                    scope: "runtime".into(),
                    url: "https://github.com/acme/repo/security/dependabot/7".into(),
                    fix_pull_request: 2,
                    detected_at: at(1_699_000_000),
                    ..vulnerability("dependabot", "GHSA-35jh-r3h4-6jhm", "lodash", "critical")
                },
                proto::Vulnerability {
                    vulnerable_range: "<= 1.2.5".into(),
                    scope: "development".into(),
                    url: "https://github.com/acme/repo/security/dependabot/9".into(),
                    detected_at: at(1_699_500_000),
                    ..vulnerability("dependabot", "GHSA-vh95-rmgr-6w4m", "minimist", "medium")
                },
            ],
            by_severity: vec![
                proto::SeverityCount {
                    severity: "critical".into(),
                    count: 1,
                },
                proto::SeverityCount {
                    severity: "medium".into(),
                    count: 1,
                },
            ],
            truncated: false,
            config_present: true,
            config_path: ".github/dependabot.yml".into(),
            config_ecosystems: vec!["npm".into()],
            alerts: "enabled".into(),
            security_updates: "paused".into(),
            open_dependabot_pull_requests: 1,
            configure_execution_id: String::new(),
            checked_at,
            ..Default::default()
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

            /// The model answered: what a working configuration reports.
            async fn test_reasoner(
                &self,
                request: Request<proto::TestReasonerRequest>,
            ) -> Result<Response<proto::ReasonerTest>, Status> {
                principal(&request)?;
                Ok(Response::new(proto::ReasonerTest {
                    status: "ok".into(),
                    message: "The model answered in 412 ms.".into(),
                    provider: "openai_compatible".into(),
                    model: "test-model".into(),
                    endpoint: "https://api.openai.com".into(),
                    latency_ms: 412,
                    input_tokens: 21,
                    output_tokens: 3,
                }))
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

            // --- pull request autopilot (CONTRACT.md §23) -------------------

            /// Drafts a replacement for any finding that has none. `f1` has
            /// its mechanical suggestion already; `f-off` stands for a
            /// workspace whose data controls keep source lines from the model.
            async fn draft_fix(
                &self,
                request: Request<proto::DraftFixRequest>,
            ) -> Result<Response<proto::Finding>, Status> {
                let caller = principal(&request)?;
                caller.require(sdi_common::principal::Role::Member)?;
                let id = request.into_inner().finding_id;
                match id.as_str() {
                    "missing" => return Err(Status::not_found("no such finding")),
                    "f1" => {
                        return Err(Status::failed_precondition(
                            "this finding already has a suggestion",
                        ));
                    }
                    "f-off" => {
                        return Err(Status::failed_precondition(
                            "drafting a fix sends source lines to the model: turn the reasoner on and set code_to_model to changed_lines in the data controls",
                        ));
                    }
                    _ => {}
                }
                Ok(Response::new(proto::Finding {
                    suggestion: "FROM node:22-slim".into(),
                    suggestion_source: "model".into(),
                    can_draft_fix: false,
                    ..finding(&id)
                }))
            }

            /// One execution for the accepted fixes of the review; `r-none`
            /// has nothing to commit.
            async fn apply_fixes(
                &self,
                request: Request<proto::ApplyFixesRequest>,
            ) -> Result<Response<proto::Review>, Status> {
                let caller = principal(&request)?;
                caller.require(sdi_common::principal::Role::Member)?;
                let id = request.into_inner().review_id;
                match id.as_str() {
                    "missing" => return Err(Status::not_found("no such review")),
                    "r-none" => {
                        return Err(Status::failed_precondition(
                            "no accepted finding has a suggestion left to commit",
                        ));
                    }
                    _ => {}
                }
                let execution_id = format!("x-fix-{id}");
                let mut review = review("r-ready", "p1", 1);
                review.id = id;
                review.fix_execution_id = execution_id.clone();
                review.fixes_ready = 0;
                review.findings[0].fix_execution_id = execution_id;
                Ok(Response::new(review))
            }

            async fn get_merge_readiness(
                &self,
                request: Request<proto::GetMergeReadinessRequest>,
            ) -> Result<Response<proto::MergeReadiness>, Status> {
                principal(&request)?;
                let request = request.into_inner();
                readiness(&request.project_id, request.pr_number).map(Response::new)
            }

            /// Refuses with the first blocker's sentence; otherwise the
            /// execution id says which method and branch handling arrived.
            async fn merge_pull_request(
                &self,
                request: Request<proto::MergePullRequestRequest>,
            ) -> Result<Response<proto::MergePullRequestResponse>, Status> {
                let caller = principal(&request)?;
                caller.require(sdi_common::principal::Role::Approver)?;
                let request = request.into_inner();
                let mut readiness = readiness(&request.project_id, request.pr_number)?;
                if let Some(blocker) = readiness.blockers.first() {
                    return Err(Status::failed_precondition(blocker.message.clone()));
                }
                let execution_id = format!(
                    "x-merge-{}-{}-{}",
                    request.pr_number,
                    if request.method.is_empty() { "default" } else { &request.method },
                    if request.delete_branch { "delete" } else { "keep" },
                );
                readiness.merge_execution_id = execution_id.clone();
                Ok(Response::new(proto::MergePullRequestResponse {
                    execution_id,
                    readiness: Some(readiness),
                }))
            }

            async fn get_security_overview(
                &self,
                request: Request<proto::GetSecurityOverviewRequest>,
            ) -> Result<Response<proto::SecurityOverview>, Status> {
                principal(&request)?;
                let request = request.into_inner();
                if request.project_id == "missing" {
                    return Err(Status::not_found("no such project"));
                }
                Ok(Response::new(security_overview(&request.project_id, request.refresh)))
            }

            /// The execution id names what was asked for.
            async fn configure_dependabot(
                &self,
                request: Request<proto::ConfigureDependabotRequest>,
            ) -> Result<Response<proto::ConfigureDependabotResponse>, Status> {
                let caller = principal(&request)?;
                caller.require(sdi_common::principal::Role::Admin)?;
                let request = request.into_inner();
                let asked: Vec<&str> = [
                    (request.alerts, "alerts"),
                    (request.security_updates, "updates"),
                    (request.config_file, "config"),
                ]
                .into_iter()
                .filter_map(|(on, name)| on.then_some(name))
                .collect();
                if asked.is_empty() {
                    return Err(Status::invalid_argument(
                        "at least one of alerts, security_updates and config_file must be true",
                    ));
                }
                Ok(Response::new(proto::ConfigureDependabotResponse {
                    execution_id: format!("x-dependabot-{}-{}", request.project_id, asked.join("+")),
                }))
            }
        }
        unimplemented {
            // The delivery setup files are another service's to generate:
            // the gateway must say 501, not 500.
            generate_delivery_artifacts: proto::GenerateDeliveryArtifactsRequest => proto::DeliveryArtifacts,
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

    /// `f1` carries the analyzer's mechanical suggestion; every other
    /// finding has none yet and can be drafted by the model.
    pub fn finding(id: &str) -> proto::Finding {
        let mechanical = id == "f1";
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
            suggestion: if mechanical { "FROM node:22" } else { "" }.into(),
            suggestion_source: if mechanical { "rule" } else { "" }.into(),
            can_draft_fix: !mechanical,
            rule: "docker.latest_tag".into(),
            decision: proto::FindingDecision::Pending as i32,
            ..Default::default()
        }
    }

    /// A proposed review with two undecided findings. `r-ready` is the same
    /// review after `f1` was accepted: one fix waits to be committed.
    pub fn review(id: &str, project_id: &str, pr_number: u64) -> proto::Review {
        let ready = id == "r-ready";
        let mut first = finding("f1");
        if ready {
            first.decision = proto::FindingDecision::Accepted as i32;
            first.decided_by = "u1".into();
            first.decided_at = at(1_700_000_200);
        }
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
            findings: vec![first, finding("f2")],
            files_reviewed: 1,
            findings_total: 2,
            findings_accepted: u32::from(ready),
            fixes_ready: u32::from(ready),
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

pub mod gitops {
    use super::*;
    use sdi_common::principal::Role;
    use sdi_proto::gitops::v1::{
        self as proto,
        gitops_service_server::{GitopsService, GitopsServiceServer},
    };

    pub struct Fake;

    /// The commit the fake renders, and the one it synced before.
    pub const REVISION: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    pub const PREVIOUS: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f001234567";

    /// A connected cluster named after its id (`c1` is `prod`).
    pub fn cluster(id: &str) -> proto::Cluster {
        let name = if id == "c1" { "prod" } else { id };
        proto::Cluster {
            id: id.to_owned(),
            name: name.to_owned(),
            kubeconfig_secret: "PROD_KUBECONFIG".into(),
            context: String::new(),
            server: format!("https://{name}.k8s.example.com:6443"),
            default_namespace: "default".into(),
            namespaces: Vec::new(),
            status: "connected".into(),
            message: String::new(),
            server_version: "v1.31.2".into(),
            application_count: 1,
            checked_at: at(1_700_000_100),
            created_at: at(1_700_000_000),
            updated_at: at(1_700_000_000),
        }
    }

    fn not_found(id: &str) -> Result<(), Status> {
        if id == "missing" {
            Err(Status::not_found("no such application"))
        } else {
            Ok(())
        }
    }

    fn resource(
        group: &str,
        kind: &str,
        namespace: &str,
        name: &str,
        sync: &str,
        health: &str,
    ) -> proto::ResourceStatus {
        proto::ResourceStatus {
            group: group.into(),
            version: "v1".into(),
            kind: kind.into(),
            namespace: namespace.into(),
            name: name.into(),
            sync_status: sync.into(),
            health_status: health.into(),
            ..Default::default()
        }
    }

    /// `web` in `prod/shop`, out of sync and progressing, with a resource
    /// of each kind of status; `never` has never been compared and its
    /// cluster is unreachable. `a-blocked` belongs to the project whose
    /// executions are refused a start.
    pub fn application(id: &str) -> proto::Application {
        let mut application = proto::Application {
            id: id.to_owned(),
            project_id: if id == "a-blocked" { "blocked" } else { "p1" }.into(),
            name: "web".into(),
            cluster_id: "c1".into(),
            cluster_name: "prod".into(),
            namespace: "shop".into(),
            source: Some(proto::ApplicationSource {
                repo: String::new(),
                revision: "main".into(),
                path: "deploy/overlays/prod".into(),
                r#type: "kustomize".into(),
                kustomize_images: vec!["ghcr.io/acme/web=ghcr.io/acme/web:a1b2c3d".into()],
                ..Default::default()
            }),
            sync_policy: Some(proto::SyncPolicy {
                create_namespace: true,
                ..Default::default()
            }),
            sync_status: "out_of_sync".into(),
            health_status: "progressing".into(),
            health_message: "Waiting for rollout to finish".into(),
            revision: REVISION.into(),
            synced_revision: PREVIOUS.into(),
            operation: Some(proto::Operation {
                kind: "sync".into(),
                phase: "succeeded".into(),
                message: "successfully synced (all tasks run)".into(),
                revision: PREVIOUS.into(),
                initiated_by: "u1".into(),
                execution_id: "x1".into(),
                started_at: at(1_700_000_000),
                finished_at: at(1_700_000_060),
            }),
            resources: vec![
                proto::ResourceStatus {
                    health_message: "1 of 2 updated replicas are available".into(),
                    ..resource(
                        "apps",
                        "Deployment",
                        "shop",
                        "web",
                        "out_of_sync",
                        "progressing",
                    )
                },
                resource("", "Service", "shop", "web", "synced", "healthy"),
                // Cluster-scoped, no health check.
                resource(
                    "rbac.authorization.k8s.io",
                    "ClusterRole",
                    "",
                    "web-reader",
                    "missing",
                    "",
                ),
                proto::ResourceStatus {
                    hook: true,
                    sync_wave: -1,
                    ..resource("batch", "Job", "shop", "migrate", "synced", "healthy")
                },
            ],
            conditions: Vec::new(),
            resource_count: 4,
            out_of_sync_count: 2,
            reconciled_at: at(1_700_000_200),
            created_at: at(1_700_000_000),
            updated_at: at(1_700_000_000),
        };
        if id == "never" {
            application.sync_status = "unknown".into();
            application.health_status = "unknown".into();
            application.health_message.clear();
            application.revision.clear();
            application.synced_revision.clear();
            application.operation = None;
            application.resources.clear();
            application.resource_count = 0;
            application.out_of_sync_count = 0;
            application.reconciled_at = None;
            application.conditions = vec![
                "the cluster prod is unreachable: dial tcp 10.0.0.1:6443: i/o timeout".into(),
                "the last comparison kept the statuses it had".into(),
            ];
        }
        application
    }

    /// What `ListApplications` answers: no resources, the first condition only.
    fn summary(mut application: proto::Application) -> proto::Application {
        application.resources.clear();
        application.conditions.truncate(1);
        application
    }

    fn history(application_id: &str) -> Vec<proto::HistoryEntry> {
        let source = application(application_id).source;
        vec![
            proto::HistoryEntry {
                id: "h2".into(),
                application_id: application_id.into(),
                revision: PREVIOUS.into(),
                source: source.clone(),
                phase: "succeeded".into(),
                message: "successfully synced (all tasks run)".into(),
                initiated_by: "u1".into(),
                execution_id: "x1".into(),
                automated: false,
                started_at: at(1_700_000_000),
                finished_at: at(1_700_000_060),
            },
            proto::HistoryEntry {
                id: "h1".into(),
                application_id: application_id.into(),
                revision: "c0ffee0".into(),
                source,
                phase: "failed".into(),
                message: "one or more objects failed to apply: Deployment/web".into(),
                initiated_by: sdi_common::principal::SYSTEM_USER.into(),
                execution_id: String::new(),
                automated: true,
                started_at: at(1_699_990_000),
                finished_at: at(1_699_990_030),
            },
        ]
    }

    fn sync_result(id: String, revision: String, history_id: &str) -> proto::SyncResult {
        proto::SyncResult {
            application_id: id,
            phase: "succeeded".into(),
            message: "successfully synced (all tasks run)".into(),
            revision,
            resources: vec![proto::ResourceResult {
                group: "apps".into(),
                kind: "Deployment".into(),
                namespace: "shop".into(),
                name: "web".into(),
                status: "synced".into(),
                message: "deployment.apps/web configured".into(),
                hook_phase: String::new(),
                sync_wave: 0,
            }],
            history_id: history_id.into(),
            health_status: "healthy".into(),
            started_at: at(1_700_000_300),
            finished_at: at(1_700_000_330),
        }
    }

    const DEPLOYMENT: &str = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n  namespace: shop\nspec:\n  replicas: 2\n";
    const SECRET: &str = "apiVersion: v1\nkind: Secret\nmetadata:\n  name: web\n  namespace: shop\ndata:\n  DATABASE_URL: ++++++++\n";

    fake_service! {
        GitopsService for Fake {
            type WatchEventsStream = ServerStream<common::PlatformEvent>;
            type ExportWorkspaceDataStream = ServerStream<proto::WorkspaceDataRecord>;

            // --- clusters ---------------------------------------------------

            async fn list_clusters(
                &self,
                request: Request<proto::ListClustersRequest>,
            ) -> Result<Response<proto::ListClustersResponse>, Status> {
                principal(&request)?;
                Ok(Response::new(proto::ListClustersResponse {
                    clusters: vec![
                        cluster("c1"),
                        proto::Cluster {
                            status: "unreachable".into(),
                            message: "dial tcp 10.0.0.9:6443: i/o timeout".into(),
                            server_version: String::new(),
                            application_count: 0,
                            context: "staging-admin".into(),
                            namespaces: vec!["shop".into(), "jobs".into()],
                            ..cluster("c2")
                        },
                    ],
                }))
            }

            /// Reads only what the contract says it reads and derives the
            /// rest: a `BROKEN` kubeconfig is saved and reported unreachable.
            async fn upsert_cluster(
                &self,
                request: Request<proto::UpsertClusterRequest>,
            ) -> Result<Response<proto::Cluster>, Status> {
                principal(&request)?.require(Role::Admin)?;
                let sent = request.into_inner().cluster.unwrap_or_default();
                if sent.kubeconfig_secret.is_empty() {
                    return Err(Status::invalid_argument(
                        "kubeconfig_secret: name the workspace secret that holds the kubeconfig",
                    ));
                }
                if sent.name == "taken" {
                    return Err(Status::already_exists(
                        "name: this workspace already has a cluster named taken",
                    ));
                }
                let id = if sent.id.is_empty() { "c-new".to_owned() } else { sent.id };
                let mut cluster = proto::Cluster {
                    name: sent.name,
                    kubeconfig_secret: sent.kubeconfig_secret,
                    context: sent.context,
                    default_namespace: sent.default_namespace,
                    namespaces: sent.namespaces,
                    application_count: 0,
                    updated_at: at(1_700_000_100),
                    ..cluster(&id)
                };
                cluster.server = format!("https://{}.k8s.example.com:6443", cluster.name);
                if cluster.kubeconfig_secret == "BROKEN" {
                    cluster.status = "unreachable".into();
                    cluster.message =
                        "the kubeconfig in BROKEN does not parse: yaml: line 3: did not find expected key".into();
                    cluster.server.clear();
                    cluster.server_version.clear();
                }
                Ok(Response::new(cluster))
            }

            async fn delete_cluster(
                &self,
                request: Request<proto::DeleteClusterRequest>,
            ) -> Result<Response<proto::DeleteClusterResponse>, Status> {
                principal(&request)?.require(Role::Admin)?;
                match request.into_inner().id.as_str() {
                    "missing" => Err(Status::not_found("no such cluster")),
                    "in-use" => Err(Status::failed_precondition(
                        "1 application deploys to this cluster: delete it or move it first",
                    )),
                    _ => Ok(Response::new(proto::DeleteClusterResponse {})),
                }
            }

            // --- applications -------------------------------------------------

            /// `empty` has none; any other project has `a1` and `never`.
            async fn list_applications(
                &self,
                request: Request<proto::ListApplicationsRequest>,
            ) -> Result<Response<proto::ListApplicationsResponse>, Status> {
                principal(&request)?;
                let project_id = request.into_inner().project_id;
                if project_id == "empty" {
                    return Ok(Response::new(proto::ListApplicationsResponse::default()));
                }
                let applications = ["a1", "never"]
                    .into_iter()
                    .map(|id| {
                        let mut application = summary(application(id));
                        if !project_id.is_empty() {
                            application.project_id = project_id.clone();
                        }
                        application
                    })
                    .collect();
                Ok(Response::new(proto::ListApplicationsResponse { applications }))
            }

            async fn get_application(
                &self,
                request: Request<proto::GetApplicationRequest>,
            ) -> Result<Response<proto::Application>, Status> {
                principal(&request)?;
                let id = request.into_inner().id;
                not_found(&id)?;
                let mut found = application(&id);
                // `busy` names an application whose sync is still running.
                if id == "busy"
                    && let Some(operation) = found.operation.as_mut()
                {
                    operation.phase = "running".into();
                    operation.finished_at = None;
                }
                Ok(Response::new(found))
            }

            /// Echoes what it reads, derives the rest as a new application.
            async fn upsert_application(
                &self,
                request: Request<proto::UpsertApplicationRequest>,
            ) -> Result<Response<proto::Application>, Status> {
                principal(&request)?.require(Role::Admin)?;
                let sent = request.into_inner().application.unwrap_or_default();
                if sent.name == "taken" {
                    return Err(Status::already_exists(
                        "name: this project already has an application named taken",
                    ));
                }
                if sent.cluster_id == "missing" {
                    return Err(Status::invalid_argument("cluster_id: no such cluster"));
                }
                let id = if sent.id.is_empty() { "a-new".to_owned() } else { sent.id };
                Ok(Response::new(proto::Application {
                    id,
                    cluster_name: cluster(&sent.cluster_id).name,
                    project_id: sent.project_id,
                    name: sent.name,
                    cluster_id: sent.cluster_id,
                    namespace: sent.namespace,
                    source: sent.source,
                    sync_policy: sent.sync_policy,
                    sync_status: "unknown".into(),
                    health_status: "unknown".into(),
                    created_at: at(1_700_000_000),
                    updated_at: at(1_700_000_100),
                    ..Default::default()
                }))
            }

            async fn delete_application(
                &self,
                request: Request<proto::DeleteApplicationRequest>,
            ) -> Result<Response<proto::DeleteApplicationResponse>, Status> {
                principal(&request)?.require(Role::Admin)?;
                let request = request.into_inner();
                not_found(&request.id)?;
                Ok(Response::new(proto::DeleteApplicationResponse {
                    deleted_resources: if request.cascade { 4 } else { 0 },
                }))
            }

            /// The application, with `hard` said in its health message.
            async fn refresh_application(
                &self,
                request: Request<proto::RefreshApplicationRequest>,
            ) -> Result<Response<proto::Application>, Status> {
                principal(&request)?.require(Role::Member)?;
                let request = request.into_inner();
                not_found(&request.id)?;
                let mut refreshed = application(&request.id);
                refreshed.health_message = if request.hard { "hard refresh" } else { "refresh" }.into();
                refreshed.reconciled_at = at(1_700_000_500);
                Ok(Response::new(refreshed))
            }

            async fn get_manifests(
                &self,
                request: Request<proto::GetManifestsRequest>,
            ) -> Result<Response<proto::Manifests>, Status> {
                principal(&request)?;
                let request = request.into_inner();
                not_found(&request.id)?;
                Ok(Response::new(proto::Manifests {
                    revision: if request.revision.is_empty() {
                        REVISION.into()
                    } else {
                        request.revision
                    },
                    manifests: vec![DEPLOYMENT.into(), SECRET.into()],
                }))
            }

            async fn diff_application(
                &self,
                request: Request<proto::DiffApplicationRequest>,
            ) -> Result<Response<proto::ApplicationDiff>, Status> {
                principal(&request)?;
                let id = request.into_inner().id;
                not_found(&id)?;
                Ok(Response::new(proto::ApplicationDiff {
                    revision: REVISION.into(),
                    resources: vec![
                        proto::ResourceDiff {
                            group: "apps".into(),
                            kind: "Deployment".into(),
                            namespace: "shop".into(),
                            name: "web".into(),
                            sync_status: "out_of_sync".into(),
                            diff: "--- live\n+++ desired\n@@ -6 +6 @@\n-  replicas: 1\n+  replicas: 2\n".into(),
                            live: DEPLOYMENT.replace("replicas: 2", "replicas: 1"),
                            target: DEPLOYMENT.into(),
                        },
                        proto::ResourceDiff {
                            group: String::new(),
                            kind: "Secret".into(),
                            namespace: "shop".into(),
                            name: "web".into(),
                            sync_status: "synced".into(),
                            diff: String::new(),
                            live: SECRET.into(),
                            target: SECRET.into(),
                        },
                        proto::ResourceDiff {
                            group: "rbac.authorization.k8s.io".into(),
                            kind: "ClusterRole".into(),
                            namespace: String::new(),
                            name: "web-reader".into(),
                            sync_status: "missing".into(),
                            diff: "+kind: ClusterRole\n".into(),
                            live: String::new(),
                            target: "kind: ClusterRole\n".into(),
                        },
                    ],
                }))
            }

            // --- operations ---------------------------------------------------

            /// Approvers only (in practice the execution's adapter); `busy`
            /// has an operation running.
            async fn sync_application(
                &self,
                request: Request<proto::SyncApplicationRequest>,
            ) -> Result<Response<proto::SyncResult>, Status> {
                principal(&request)?.require(Role::Approver)?;
                let request = request.into_inner();
                not_found(&request.id)?;
                if request.id == "busy" {
                    return Err(Status::failed_precondition("an operation of web is running"));
                }
                let revision = if request.revision.is_empty() {
                    REVISION.to_owned()
                } else {
                    request.revision
                };
                let history_id = if request.dry_run { "" } else { "h-new" };
                Ok(Response::new(sync_result(request.id, revision, history_id)))
            }

            async fn rollback_application(
                &self,
                request: Request<proto::RollbackApplicationRequest>,
            ) -> Result<Response<proto::SyncResult>, Status> {
                principal(&request)?.require(Role::Approver)?;
                let request = request.into_inner();
                not_found(&request.id)?;
                let Some(entry) = history(&request.id)
                    .into_iter()
                    .find(|entry| entry.id == request.history_id)
                else {
                    return Err(Status::not_found("no such history entry"));
                };
                Ok(Response::new(sync_result(request.id, entry.revision, "h-back")))
            }

            /// `h2` then `h1`, newest first; `limit` keeps the newest.
            async fn list_history(
                &self,
                request: Request<proto::ListHistoryRequest>,
            ) -> Result<Response<proto::ListHistoryResponse>, Status> {
                principal(&request)?;
                let request = request.into_inner();
                not_found(&request.id)?;
                let mut entries = history(&request.id);
                if request.limit > 0 {
                    entries.truncate(request.limit as usize);
                }
                Ok(Response::new(proto::ListHistoryResponse { entries }))
            }

            // --- events and workspace data --------------------------------------

            /// One status change of w1, one of w2 (must not leak), then
            /// open for ever.
            async fn watch_events(
                &self,
                request: Request<common::WatchEventsRequest>,
            ) -> Result<Response<Self::WatchEventsStream>, Status> {
                principal(&request)?;
                let events = stream::iter(vec![
                    Ok(common::PlatformEvent {
                        status: "out_of_sync".into(),
                        ..platform_event("application", "a1", "updated")
                    }),
                    Ok(platform_event_in("w2", "application", "other", "updated")),
                ]);
                Ok(Response::new(events.chain(stream::pending()).boxed()))
            }

            async fn delete_workspace_data(
                &self,
                request: Request<proto::DeleteWorkspaceDataRequest>,
            ) -> Result<Response<proto::DeleteWorkspaceDataResponse>, Status> {
                principal(&request)?;
                Ok(Response::new(proto::DeleteWorkspaceDataResponse {
                    deleted: [
                        ("application".to_owned(), 2u64),
                        ("cluster".to_owned(), 1u64),
                    ]
                    .into(),
                }))
            }

            async fn export_workspace_data(
                &self,
                request: Request<proto::ExportWorkspaceDataRequest>,
            ) -> Result<Response<Self::ExportWorkspaceDataStream>, Status> {
                principal(&request)?;
                let records = vec![
                    Ok(proto::WorkspaceDataRecord {
                        entity: "cluster".into(),
                        json: r#"{"id":"c1","name":"prod","kubeconfig_secret":"PROD_KUBECONFIG"}"#.into(),
                    }),
                    Ok(proto::WorkspaceDataRecord {
                        entity: "application".into(),
                        json: r#"{"id":"a1","name":"web"}"#.into(),
                    }),
                ];
                Ok(Response::new(stream::iter(records).boxed()))
            }
        }
        unimplemented {}
    }

    pub async fn serve() -> SocketAddr {
        super::serve(|router| router.add_service(GitopsServiceServer::new(Fake))).await
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

    /// Counts `RecordAudit` calls and remembers the last action, and holds
    /// the workspace's secrets (name → description; never a value).
    #[derive(Default)]
    pub struct Recorder {
        pub audits: AtomicUsize,
        pub last_action: std::sync::Mutex<String>,
        /// Every recorded entry, oldest first.
        pub entries: std::sync::Mutex<Vec<proto::AuditEntry>>,
        pub secrets: std::sync::Mutex<std::collections::BTreeMap<String, String>>,
        /// Lengths of the values `SetSecret` was given: proof they arrived,
        /// without the fake keeping them.
        pub secret_value_lengths: std::sync::Mutex<Vec<usize>>,
    }

    /// The secrets RPCs are the part of identity that reads the caller: an
    /// admin of the workspace named in the body, or the request is refused.
    fn secrets_caller<T>(request: &Request<T>, workspace_id: &str) -> Result<Principal, Status> {
        let caller = principal(request)?;
        caller.require(sdi_common::principal::Role::Admin)?;
        if workspace_id != caller.workspace_id {
            return Err(Status::permission_denied(
                "workspace_id does not match the caller's workspace",
            ));
        }
        Ok(caller)
    }

    fn secret_info(name: &str, description: &str, updated_by: &str) -> proto::SecretInfo {
        proto::SecretInfo {
            name: name.to_owned(),
            hint: "wxyz".into(),
            description: description.to_owned(),
            updated_by: updated_by.to_owned(),
            updated_at: at(1_700_000_000),
        }
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
            code_to_model: "off".into(),
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

            async fn resolve_users(
                &self,
                request: Request<proto::ResolveUsersRequest>,
            ) -> Result<Response<proto::ResolveUsersResponse>, Status> {
                let req = request.into_inner();
                if req.user_ids.len() > 100 {
                    return Err(Status::invalid_argument("at most 100 user ids per call"));
                }
                // Names `u1` only, and only inside the workspace it was asked for.
                Ok(Response::new(proto::ResolveUsersResponse {
                    users: req
                        .user_ids
                        .iter()
                        .filter(|id| id.as_str() == "u1")
                        .map(|id| proto::UserRef {
                            id: id.clone(),
                            login: format!("octo@{}", req.workspace_id),
                            name: "Octo Cat".into(),
                            avatar_url: "https://avatars.example/u1".into(),
                            member: true,
                        })
                        .collect(),
                }))
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
                *self.recorder.last_action.lock().unwrap() = entry.action.clone();
                self.recorder.entries.lock().unwrap().push(entry);
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

            /// Echoes what it was sent. `code_to_model` behaves as in the
            /// real service: an unknown value is refused, an empty one
            /// keeps what is stored.
            async fn update_data_controls(
                &self,
                request: Request<proto::UpdateDataControlsRequest>,
            ) -> Result<Response<proto::DataControls>, Status> {
                let mut controls = request
                    .into_inner()
                    .controls
                    .ok_or_else(|| Status::invalid_argument("controls missing"))?;
                match controls.code_to_model.as_str() {
                    "off" | "changed_lines" => {}
                    "" => controls.code_to_model = self::controls().code_to_model,
                    _ => {
                        return Err(Status::invalid_argument(
                            "code_to_model must be one of off, changed_lines",
                        ));
                    }
                }
                Ok(Response::new(controls))
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

            async fn list_secrets(
                &self,
                request: Request<proto::ListSecretsRequest>,
            ) -> Result<Response<proto::ListSecretsResponse>, Status> {
                secrets_caller(&request, &request.get_ref().workspace_id)?;
                let secrets = self
                    .recorder
                    .secrets
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(name, description)| secret_info(name, description, "octo"))
                    .collect();
                Ok(Response::new(proto::ListSecretsResponse { secrets }))
            }

            async fn set_secret(
                &self,
                request: Request<proto::SetSecretRequest>,
            ) -> Result<Response<proto::SecretInfo>, Status> {
                secrets_caller(&request, &request.get_ref().workspace_id)?;
                let request = request.into_inner();
                if request.name != request.name.to_ascii_uppercase() || request.name.len() < 2 {
                    return Err(Status::invalid_argument(
                        "name must be upper snake case: a letter, then 1 to 63 letters, digits or underscores (e.g. JENKINS_TOKEN)",
                    ));
                }
                if request.value.is_empty() {
                    return Err(Status::invalid_argument(
                        "value must not be empty; delete the secret to remove it",
                    ));
                }
                self.recorder
                    .secret_value_lengths
                    .lock()
                    .unwrap()
                    .push(request.value.len());
                self.recorder
                    .secrets
                    .lock()
                    .unwrap()
                    .insert(request.name.clone(), request.description.clone());
                Ok(Response::new(secret_info(&request.name, &request.description, "octo")))
            }

            async fn delete_secret(
                &self,
                request: Request<proto::DeleteSecretRequest>,
            ) -> Result<Response<proto::DeleteSecretResponse>, Status> {
                secrets_caller(&request, &request.get_ref().workspace_id)?;
                let name = request.into_inner().name;
                if self.recorder.secrets.lock().unwrap().remove(&name).is_none() {
                    return Err(Status::not_found(format!("no secret named {name}")));
                }
                Ok(Response::new(proto::DeleteSecretResponse {}))
            }
        }
        unimplemented {
            // Service-to-service only: the gateway has no route that could reach it.
            resolve_secret: proto::ResolveSecretRequest => proto::ResolvedSecret,
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
