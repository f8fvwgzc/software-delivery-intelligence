//! Continuous delivery end to end (CONTRACT.md §22), in process, against a
//! throwaway schema and local fakes of GitHub, an image registry and a
//! deploy webhook. Nothing here simulates a success: every step runs its
//! real adapter against a real HTTP server.

use std::{collections::HashMap, sync::Arc, time::Duration};

use sdi_common::principal::{Principal, Role};
use sdi_proto::{
    common::v1::PlatformEvent,
    execution::v1::{
        ApprovalStatus, CreateExecutionRequest, CreateReleaseRequest, DecideApprovalRequest,
        DeleteEnvironmentRequest, DeleteRegistryRequest, DeleteWorkspaceDataRequest, DeploymentRun,
        DeploymentStatus, Environment, Execution, ExecutionStatus, ExportWorkspaceDataRequest,
        Failure, GetDeploymentRunRequest, GetExecutionRequest, GetPipelineRequest,
        GetReleaseRequest, GrantPermissionRequest, ListApprovalsRequest, ListDeploymentRunsRequest,
        ListEnvironmentsRequest, ListIdentitiesRequest, ListRegistriesRequest, ListReleasesRequest,
        PolicyDecision, PromoteRequest, Registry, Release, ResolveFailureRequest, RollbackRequest,
        StepStatus, SyncEnvironmentsRequest, UpsertEnvironmentRequest, UpsertRegistryRequest,
        execution_service_server::ExecutionService,
    },
};
use sqlx::PgPool;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tonic::{Code, Request};
use uuid::Uuid;

use super::fakes::{
    github::{self as fake_github, FakeGithub},
    registry::FakeRegistry,
    webhook::FakeWebhook,
};
use crate::{
    adapters::{
        DeliveryTools, DockerAdapter, GithubAccess, GithubAdapter, Registry as Adapters,
        SimulatedAdapter, azure_devops,
        gitops::GitopsAdapter,
        http_health::HttpHealthAdapter,
        jenkins::JenkinsAdapter,
        net::{Net, NetPolicy},
        registry::RegistryAdapter,
        webhook::WebhookAdapter,
    },
    delivery::Delivery,
    grpc::ExecutionGrpc,
    identity::{RecordingAudit, StaticGithubAccess, StaticWorkspaces},
    knowledge::{ProjectInfo, RecordingPatternOutcomes, StaticProjects},
    runner::Runtime,
};

const WAIT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(15);

const WS_A: &str = "00000000-0000-7000-8000-00000000000a";
const WS_B: &str = "00000000-0000-7000-8000-00000000000b";
const PROJECT: &str = "0192f6a0-0000-7000-8000-0000000000a1";
const REPO: &str = "acme/api";

const HOOK_SECRET: &str = "whsec_9f8e7d6c5b4a39281706f5e4d3c2b1a0";
const REGISTRY_PASSWORD: &str = "registry-password-6b1f0c2e";
const COMMIT_1: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
const COMMIT_2: &str = "b2c3d4e5f60718293a4b5c6d7e8f9012345678a1";
const DIGEST_1: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DIGEST_2: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

fn as_role<T>(role: Role, workspace: &str, message: T) -> Request<T> {
    let mut request = Request::new(message);
    Principal {
        user_id: format!("user-{}", role.as_str()),
        workspace_id: workspace.to_owned(),
        role,
    }
    .apply(&mut request);
    request
}

/// The default caller: an admin of workspace A.
fn admin<T>(message: T) -> Request<T> {
    as_role(Role::Admin, WS_A, message)
}

fn approver<T>(message: T) -> Request<T> {
    as_role(Role::Approver, WS_A, message)
}

struct Harness {
    pool: PgPool,
    schema: String,
    rt: Arc<Runtime>,
    svc: ExecutionGrpc,
    github: Arc<FakeGithub>,
    registry: Arc<FakeRegistry>,
    /// `host:port` of the fake registry.
    registry_host: String,
    webhook: Arc<FakeWebhook>,
    /// Base URL of the fake deploy hook (`/deploy`, `/health`).
    webhook_url: String,
    workspaces: StaticWorkspaces,
    audit: RecordingAudit,
    feed: broadcast::Receiver<PlatformEvent>,
}

impl Harness {
    /// None (with a note on stderr) when no database is configured.
    async fn new() -> Option<Self> {
        let Some(url) = std::env::var("DATABASE_URL")
            .ok()
            .filter(|u| !u.trim().is_empty())
        else {
            eprintln!("skipping: DATABASE_URL is not set");
            return None;
        };
        let suffix: String = Uuid::now_v7()
            .as_bytes()
            .iter()
            .rev()
            .take(12)
            .map(|b| char::from(b'a' + b % 26))
            .collect();
        let schema = format!("execution_test_{suffix}");
        let pool = sdi_common::db::connect(&url, &schema)
            .await
            .expect("connect to dev postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");

        let (github, github_api) = FakeGithub::default()
            .with_workflow(REPO, "deploy.yml")
            .with_head(REPO, COMMIT_1, "Add health endpoint\n\nWith a body.")
            .serve()
            .await;
        let (registry, registry_host) = FakeRegistry::serve().await;
        let (webhook, webhook_url) = FakeWebhook::serve(HOOK_SECRET).await;

        // Every deployer is the real adapter; only the waits are shortened.
        let net = Net::new(NetPolicy::ALLOW_PRIVATE);
        let fast = Duration::from_millis(10);
        let adapters = Adapters::new(
            Arc::new(SimulatedAdapter::new(1)),
            Arc::new(DockerAdapter::new(
                std::env::temp_dir().join("sdi-execution-tests"),
                None,
                &github_api,
            )),
            Arc::new(GithubAdapter::new(None, &github_api).with_poll(fast, Duration::from_secs(5))),
        )
        .with_delivery(DeliveryTools {
            registry: Arc::new(RegistryAdapter::new(net.clone(), None)),
            gitops: Arc::new(GitopsAdapter::new(None, &github_api).with_poll(fast)),
            jenkins: Arc::new(JenkinsAdapter::new(net.clone()).with_poll(fast)),
            azure_devops: Arc::new(
                azure_devops::AzureDevopsAdapter::new(net.clone(), azure_devops::DEFAULT_BASE)
                    .with_poll(fast),
            ),
            webhook: Arc::new(WebhookAdapter::new(net.clone())),
            http: Arc::new(HttpHealthAdapter::new(net).with_poll(fast)),
        });

        let projects = StaticProjects::default();
        projects.add(
            WS_A,
            ProjectInfo {
                id: PROJECT.into(),
                name: "api".into(),
                repo_full_name: REPO.into(),
                default_branch: "main".into(),
            },
        );
        let workspaces = StaticWorkspaces::new(
            "Acme",
            &[
                ("DEPLOY_HOOK_SECRET", HOOK_SECRET),
                ("REGISTRY_USER", "robot"),
                ("REGISTRY_PASSWORD", REGISTRY_PASSWORD),
            ],
        );
        let audit = RecordingAudit::default();
        let rt = Runtime::with_delivery(
            pool.clone(),
            adapters,
            Arc::new(RecordingPatternOutcomes::default()),
            Arc::new(audit.clone()),
            Duration::ZERO,
            false,
            Arc::new(StaticGithubAccess(GithubAccess {
                token: fake_github::TOKEN.into(),
                api_base_url: github_api,
            })),
            Delivery::new(
                Arc::new(projects),
                Arc::new(workspaces.clone()),
                Some(Arc::new(workspaces.secrets.clone())),
                None,
                NetPolicy::ALLOW_PRIVATE,
            ),
        );
        let feed = rt.platform.subscribe();
        Some(Self {
            pool,
            schema,
            svc: ExecutionGrpc::new(Arc::clone(&rt)),
            rt,
            github,
            registry,
            registry_host,
            webhook,
            webhook_url,
            workspaces,
            audit,
            feed,
        })
    }

    async fn teardown(self) {
        // Background promotions may still hold a connection for a moment.
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(self.svc);
        drop(self.rt);
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.pool)
        .await;
    }

    /// A `generic` registry on the fake, made the workspace default.
    async fn fake_registry_as_default(&self) -> Registry {
        self.svc
            .upsert_registry(admin(UpsertRegistryRequest {
                registry: Some(Registry {
                    name: "internal".into(),
                    kind: "generic".into(),
                    host: self.registry_host.clone(),
                    namespace: "acme".into(),
                    auth: "secret".into(),
                    username_secret: "REGISTRY_USER".into(),
                    password_secret: "REGISTRY_PASSWORD".into(),
                    is_default: true,
                    ..Default::default()
                }),
            }))
            .await
            .expect("upsert registry")
            .into_inner()
    }

    fn staging(&self) -> Environment {
        Environment {
            project_id: PROJECT.into(),
            name: "staging".into(),
            position: 0,
            kind: "github_actions".into(),
            config: HashMap::from([("workflow".to_owned(), "deploy.yml".to_owned())]),
            ..Default::default()
        }
    }

    fn production(&self) -> Environment {
        Environment {
            project_id: PROJECT.into(),
            name: "production".into(),
            position: 1,
            kind: "webhook".into(),
            config: HashMap::from([
                ("url".to_owned(), format!("{}/deploy", self.webhook_url)),
                ("secret".to_owned(), "DEPLOY_HOOK_SECRET".to_owned()),
            ]),
            url: self.webhook_url.clone(),
            health_path: "/health".into(),
            protected: true,
            auto_promote: true,
            ..Default::default()
        }
    }

    async fn upsert(&self, environment: Environment) -> Environment {
        self.svc
            .upsert_environment(admin(UpsertEnvironmentRequest {
                environment: Some(environment),
            }))
            .await
            .expect("upsert environment")
            .into_inner()
    }

    async fn environments(&self) -> Vec<Environment> {
        self.svc
            .list_environments(admin(ListEnvironmentsRequest {
                project_id: PROJECT.into(),
            }))
            .await
            .expect("list environments")
            .into_inner()
            .environments
    }

    async fn environment(&self, name: &str) -> Environment {
        self.environments()
            .await
            .into_iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("no environment {name}"))
    }

    async fn release(&self, id: &str) -> Release {
        self.svc
            .get_release(admin(GetReleaseRequest { id: id.to_owned() }))
            .await
            .expect("get release")
            .into_inner()
    }

    async fn run(&self, id: &str) -> DeploymentRun {
        self.svc
            .get_deployment_run(admin(GetDeploymentRunRequest { id: id.to_owned() }))
            .await
            .expect("get deployment run")
            .into_inner()
    }

    async fn execution(&self, id: &str) -> Execution {
        self.svc
            .get_execution(admin(GetExecutionRequest { id: id.to_owned() }))
            .await
            .expect("get execution")
            .into_inner()
    }

    async fn runs_of(&self, environment_id: &str) -> Vec<DeploymentRun> {
        self.svc
            .list_deployment_runs(admin(ListDeploymentRunsRequest {
                project_id: PROJECT.into(),
                environment_id: environment_id.to_owned(),
                ..Default::default()
            }))
            .await
            .expect("list runs")
            .into_inner()
            .runs
    }

    /// Waits until the run reads `status`.
    async fn wait_run(&self, id: &str, status: DeploymentStatus) -> DeploymentRun {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let run = self.run(id).await;
            if run.status == status as i32 {
                return run;
            }
            if tokio::time::Instant::now() >= deadline {
                let execution = self.execution(&run.execution_id).await;
                panic!(
                    "timed out waiting for {status:?}; the run is {:?}, its execution {:?}, failure {:?}, steps {:?}",
                    DeploymentStatus::try_from(run.status),
                    ExecutionStatus::try_from(execution.status),
                    execution.failure.map(|f| (f.code, f.raw_output)),
                    execution
                        .steps
                        .iter()
                        .map(|s| (
                            s.key.clone(),
                            StepStatus::try_from(s.status),
                            s.output.clone()
                        ))
                        .collect::<Vec<_>>()
                );
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Waits for the newest run of an environment to be another one than
    /// `not` and returns it.
    async fn wait_new_run(&self, environment_id: &str, not: &[&str]) -> DeploymentRun {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            if let Some(run) = self
                .runs_of(environment_id)
                .await
                .into_iter()
                .find(|run| !not.contains(&run.id.as_str()))
            {
                return run;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for a new run"
            );
            tokio::time::sleep(POLL).await;
        }
    }

    /// Waits until the run's execution is paused on `step_key` and
    /// diagnosed (the options are there), and returns the failure.
    async fn diagnosed(&self, run: &DeploymentRun, step_key: &str) -> Failure {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let execution = self.execution(&run.execution_id).await;
            if let Some(failure) = execution
                .failure
                .filter(|f| f.step_key == step_key && !f.options.is_empty())
            {
                return failure;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for a diagnosed failure of {step_key}; the execution is {:?}",
                ExecutionStatus::try_from(execution.status)
            );
            tokio::time::sleep(POLL).await;
        }
    }

    /// Approves the one pending approval of the run's execution and says
    /// which action it was for.
    async fn approve(&self, run: &DeploymentRun) -> String {
        let deadline = tokio::time::Instant::now() + WAIT;
        let approval = loop {
            let pending = self
                .svc
                .list_approvals(admin(ListApprovalsRequest {
                    status: ApprovalStatus::Pending as i32,
                    execution_id: run.execution_id.clone(),
                }))
                .await
                .expect("list approvals")
                .into_inner()
                .approvals;
            if let Some(approval) = pending.into_iter().next() {
                break approval;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for an approval"
            );
            tokio::time::sleep(POLL).await;
        };
        self.svc
            .decide_approval(approver(DecideApprovalRequest {
                id: approval.id.clone(),
                approve: true,
                note: "ship it".into(),
                decided_by: String::new(),
            }))
            .await
            .expect("approve");
        approval.action_type
    }

    /// Gives the delivery identity `permissions` for half an hour.
    async fn grant(&self, permissions: &[&str]) {
        let identities = self
            .svc
            .list_identities(admin(ListIdentitiesRequest {}))
            .await
            .unwrap()
            .into_inner()
            .identities;
        let delivery = identities
            .iter()
            .find(|i| i.name == "delivery")
            .expect("the delivery identity is seeded");
        self.svc
            .grant_permission(approver(GrantPermissionRequest {
                identity_id: delivery.id.clone(),
                permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
                ttl_minutes: 0,
            }))
            .await
            .expect("grant");
    }

    async fn resolve(&self, run: &DeploymentRun, option: &str) -> Result<Execution, tonic::Status> {
        self.svc
            .resolve_failure(approver(ResolveFailureRequest {
                execution_id: run.execution_id.clone(),
                option_id: option.to_owned(),
                instruction: String::new(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Rows of the test schema that mention `needle` anywhere a deployer's
    /// words could have ended up.
    async fn occurrences(&self, needle: &str) -> i64 {
        let like = format!("%{needle}%");
        let mut total = 0i64;
        for sql in [
            "SELECT count(*) FROM events WHERE message LIKE $1 OR data::text LIKE $1",
            "SELECT count(*) FROM steps WHERE output LIKE $1 OR params::text LIKE $1",
            "SELECT count(*) FROM executions WHERE failure::text LIKE $1 OR outputs::text LIKE $1",
            "SELECT count(*) FROM deployment_runs WHERE external_url LIKE $1 OR reason LIKE $1",
            "SELECT count(*) FROM environments WHERE config::text LIKE $1",
            "SELECT count(*) FROM registries WHERE username_secret LIKE $1 OR password_secret LIKE $1",
        ] {
            let (count,): (i64,) = sqlx::query_as(sql)
                .bind(&like)
                .fetch_one(&self.pool)
                .await
                .unwrap();
            total += count;
        }
        total
    }

    /// Everything the platform feed carried since the harness started.
    fn drain_feed(&mut self) -> Vec<PlatformEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.feed.try_recv() {
            events.push(event);
        }
        events
    }
}

fn status_of(run: &DeploymentRun) -> DeploymentStatus {
    DeploymentStatus::try_from(run.status).unwrap_or_default()
}

/// The whole road: the seeded registry, two environments (GitHub Actions,
/// then a protected signed webhook), a release from the default branch
/// head, a promotion that first lacks its permission, the automatic
/// promotion into production that waits for a human, a second release
/// whose production deploy fails and is rolled back through the
/// remediation, and an explicit rollback of staging.
#[tokio::test]
async fn a_release_is_promoted_approved_fails_and_is_rolled_back() {
    let Some(mut h) = Harness::new().await else {
        return;
    };

    // --- registries ---------------------------------------------------------
    // The first look seeds `ghcr.io/<workspace slug>` with nothing to configure.
    let seeded = h
        .svc
        .list_registries(as_role(Role::Viewer, WS_A, ListRegistriesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .registries;
    assert_eq!(seeded.len(), 1);
    let ghcr = &seeded[0];
    assert_eq!(
        (ghcr.name.as_str(), ghcr.kind.as_str(), ghcr.host.as_str()),
        ("ghcr", "ghcr", "ghcr.io")
    );
    assert_eq!(ghcr.namespace, "acme", "the workspace slug, lowercased");
    assert_eq!(ghcr.auth, "github_token");
    assert!(ghcr.is_default);
    assert!(ghcr.username_secret.is_empty() && ghcr.password_secret.is_empty());
    // The tests look images up in the fake registry: it becomes the default.
    let internal = h.fake_registry_as_default().await;
    assert!(internal.is_default);
    let registries = h
        .svc
        .list_registries(admin(ListRegistriesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .registries;
    assert_eq!(registries.len(), 2);
    assert_eq!(
        registries.iter().filter(|r| r.is_default).count(),
        1,
        "only one default registry"
    );
    assert_eq!(
        registries[0].name, "internal",
        "the default is listed first"
    );

    // --- environments -------------------------------------------------------
    let staging = h.upsert(h.staging()).await;
    let production = h.upsert(h.production()).await;
    assert_eq!(staging.source, "ui");
    assert!(production.protected && production.auto_promote);
    assert_eq!(production.health_path, "/health");
    assert_eq!(
        h.environments()
            .await
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["staging", "production"],
        "promotion order"
    );
    // Production deploys need the permission up front in this test; staging
    // goes without, to see the pre-flight check stop the run.
    h.grant(&["deploy:production"]).await;

    // --- a release from the default branch head ------------------------------
    let create = |role: Role| {
        as_role(
            role,
            WS_A,
            CreateReleaseRequest {
                project_id: PROJECT.into(),
                ..Default::default()
            },
        )
    };
    assert_eq!(
        h.svc
            .create_release(create(Role::Viewer))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let release = h
        .svc
        .create_release(create(Role::Member))
        .await
        .expect("create release")
        .into_inner();
    assert_eq!(release.commit_sha, COMMIT_1);
    assert_eq!(release.tag, "a1b2c3d", "the short SHA");
    assert_eq!(release.version, "a1b2c3d");
    assert_eq!(
        release.image,
        format!("{}/acme/api", h.registry_host),
        "<default registry host>/<namespace>/<repository name>"
    );
    assert_eq!(release.source, "manual");
    assert_eq!(release.created_by, "user-member");
    assert_eq!(release.notes, "- Add health endpoint (a1b2c3d)");
    assert!(!release.verified);
    assert!(release.digest.is_empty());
    assert!(release.deployed_to.is_empty());
    // Idempotent on (project, tag).
    let again = h
        .svc
        .create_release(create(Role::Member))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(again.id, release.id);

    // Before anything was deployed: staging may take the release,
    // production has nothing proven yet.
    let pipeline = h
        .svc
        .get_pipeline(admin(GetPipelineRequest {
            project_id: PROJECT.into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(pipeline.stages.len(), 2);
    assert_eq!(
        pipeline.stages[0].candidate.as_ref().map(|r| r.id.as_str()),
        Some(release.id.as_str())
    );
    assert!(pipeline.stages[0].current.is_none());
    assert!(pipeline.stages[0].last_run.is_none());
    assert_eq!(pipeline.stages[0].health, "unknown", "staging has no URL");
    assert!(pipeline.stages[1].candidate.is_none());
    assert_eq!(pipeline.stages[1].health, "healthy");
    assert_eq!(pipeline.registry.as_ref().unwrap().name, "internal");
    assert_eq!(pipeline.deployments_30d, 0);
    assert_eq!(pipeline.success_rate, 0.0);

    // --- promote staging: the image is not in the registry yet --------------
    let promote = |role: Role, environment: &Environment, release_id: &str| {
        as_role(
            role,
            WS_A,
            PromoteRequest {
                environment_id: environment.id.clone(),
                release_id: release_id.to_owned(),
            },
        )
    };
    assert_eq!(
        h.svc
            .promote(promote(Role::Member, &staging, ""))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied,
        "promoting is an approver's"
    );
    let run = h
        .svc
        .promote(promote(Role::Approver, &staging, ""))
        .await
        .expect("promote staging")
        .into_inner();
    assert_eq!(run.kind, "promote");
    assert_eq!(run.environment_name, "staging");
    assert_eq!(run.release_id, release.id, "the candidate");
    assert_eq!(run.started_by, "user-approver");
    assert!(!run.execution_id.is_empty());
    assert!(run.previous_release_id.is_empty());
    // A paused run reads FAILED and is still open (no end time).
    let paused = h.wait_run(&run.id, DeploymentStatus::Failed).await;
    assert!(paused.finished_at.is_none());
    let execution = h.execution(&run.execution_id).await;
    assert_eq!(execution.adapter, "delivery");
    assert_eq!(execution.title, "Deploy api a1b2c3d to staging");
    assert_eq!(
        execution
            .steps
            .iter()
            .map(|s| (s.key.as_str(), s.action_type.as_str(), s.adapter.as_str()))
            .collect::<Vec<_>>(),
        [
            ("verify_image", "registry.verify_image", "registry"),
            ("deploy", "deploy.development", "github"),
            ("verify_health", "deploy.verify_health", "http"),
            ("record", "deploy.record", "github"),
        ]
    );
    assert_eq!(
        execution.steps[0].policy_decision,
        PolicyDecision::Auto as i32
    );
    let failure = h.diagnosed(&run, "verify_image").await;
    assert_eq!(failure.code, "IMAGE_NOT_FOUND");
    assert!(
        failure.diagnosis.contains("is not in the registry"),
        "{}",
        failure.diagnosis
    );
    assert!(failure.diagnosis.contains("Nothing was deployed"));
    let options: Vec<&str> = failure.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(options, ["retry", "give_up", "cancel"]);
    assert_eq!(failure.blocked_steps, ["deploy", "verify_health", "record"]);
    assert_eq!(
        *h.registry.lookups.lock().unwrap(),
        ["acme/api/manifests/a1b2c3d"]
    );
    // One deployment to an environment at a time.
    let busy = h
        .svc
        .promote(promote(Role::Approver, &staging, &release.id))
        .await
        .unwrap_err();
    assert_eq!(busy.code(), Code::FailedPrecondition);
    assert!(busy.message().contains("still open"), "{}", busy.message());

    // CI pushes the image; the retry finds it, and the deploy step then
    // stops before running because the identity lacks deploy:development.
    h.registry.publish("acme/api", "a1b2c3d", DIGEST_1);
    h.resolve(&run, "retry").await.expect("retry");
    let failure = h.diagnosed(&run, "deploy").await;
    assert_eq!(failure.code, "PERMISSION_DENIED");
    assert_eq!(failure.missing_permissions, ["deploy:development"]);
    assert!(
        h.github.dispatches.lock().unwrap().is_empty(),
        "nothing ran: the check happens before the action"
    );
    // The registry already confirmed the image: the release says so.
    let verified = h.release(&release.id).await;
    assert!(verified.verified);
    assert_eq!(verified.digest, DIGEST_1);

    // Granting the permission lets the run go on: the workflow is
    // dispatched, followed to success, health is skipped (no URL) and the
    // deployment is recorded on GitHub.
    h.resolve(&run, "request_access")
        .await
        .expect("request access");
    let done = h.wait_run(&run.id, DeploymentStatus::Succeeded).await;
    assert!(done.finished_at.is_some());
    let dispatches = h.github.dispatches.lock().unwrap().clone();
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0]["repo"], REPO);
    assert_eq!(dispatches[0]["workflow"], "deploy.yml");
    assert_eq!(dispatches[0]["ref"], "main");
    assert_eq!(dispatches[0]["inputs"]["environment"], "staging");
    assert_eq!(dispatches[0]["inputs"]["image"], release.image.as_str());
    assert_eq!(dispatches[0]["inputs"]["digest"], DIGEST_1);
    assert_eq!(dispatches[0]["inputs"]["version"], "a1b2c3d");
    assert_eq!(
        done.external_url,
        format!(
            "https://github.com/{REPO}/actions/runs/{}",
            dispatches[0]["run_id"]
        ),
        "the workflow run the dispatch started"
    );
    let execution = h.execution(&run.execution_id).await;
    assert_eq!(execution.status, ExecutionStatus::Succeeded as i32);
    assert!(
        execution
            .steps
            .iter()
            .all(|s| s.status == StepStatus::Succeeded as i32 && s.checkpointed)
    );
    assert!(
        execution.steps[2].output.starts_with("skipped:"),
        "{}",
        execution.steps[2].output
    );
    assert_eq!(execution.outputs["digest"], DIGEST_1);
    assert_eq!(execution.outputs["verified"], "true");
    {
        let deployments = h.github.deployments.lock().unwrap();
        assert_eq!(deployments.len(), 1);
        assert_eq!(deployments[0]["environment"], "staging");
        assert_eq!(deployments[0]["ref"], COMMIT_1);
        assert_eq!(deployments[0]["production_environment"], false);
        let statuses = h.github.deployment_statuses.lock().unwrap();
        assert_eq!(statuses[0]["state"], "success");
        assert_eq!(statuses[0]["log_url"], done.external_url.as_str());
    }
    assert_eq!(
        h.environment("staging").await.current_release_id,
        release.id
    );
    assert_eq!(h.release(&release.id).await.deployed_to, ["staging"]);

    // --- production follows by itself, and waits for a human ------------------
    let auto = h.wait_new_run(&production.id, &[]).await;
    assert_eq!(auto.kind, "promote");
    assert_eq!(auto.release_id, release.id);
    assert_eq!(auto.started_by, "system", "an automatic promotion");
    let waiting = h
        .wait_run(&auto.id, DeploymentStatus::AwaitingApproval)
        .await;
    assert!(waiting.finished_at.is_none());
    assert!(
        h.webhook.deliveries.lock().unwrap().is_empty(),
        "nothing is deployed before the approval"
    );
    let pipeline = h
        .svc
        .get_pipeline(admin(GetPipelineRequest {
            project_id: PROJECT.into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(pipeline.stages[0].current.as_ref().unwrap().id, release.id);
    assert!(
        pipeline.stages[0].candidate.is_none(),
        "staging is up to date"
    );
    assert_eq!(
        pipeline.stages[1].last_run.as_ref().unwrap().status,
        DeploymentStatus::AwaitingApproval as i32
    );
    assert_eq!(
        pipeline.stages[1].candidate.as_ref().unwrap().id,
        release.id,
        "proven in staging, not yet in production"
    );
    assert_eq!(
        h.approve(&auto).await,
        "deploy.production",
        "a protected environment always asks"
    );
    let shipped = h.wait_run(&auto.id, DeploymentStatus::Succeeded).await;
    assert!(shipped.duration_seconds < 60);
    {
        let deliveries = h.webhook.deliveries.lock().unwrap();
        assert_eq!(deliveries.len(), 1, "the signature verified");
        assert_eq!(deliveries[0]["kind"], "promote");
        assert_eq!(deliveries[0]["environment"], "production");
        assert_eq!(deliveries[0]["release"]["tag"], "a1b2c3d");
        assert_eq!(deliveries[0]["release"]["digest"], DIGEST_1);
        assert_eq!(deliveries[0]["project"]["repository"], REPO);
    }
    assert!(
        h.webhook
            .health_hits
            .load(std::sync::atomic::Ordering::SeqCst)
            >= 1
    );
    let execution = h.execution(&auto.execution_id).await;
    assert!(
        execution.steps[2].output.starts_with("healthy:"),
        "{}",
        execution.steps[2].output
    );
    assert_eq!(
        h.github.deployments.lock().unwrap()[1]["production_environment"],
        true
    );
    assert_eq!(
        h.release(&release.id).await.deployed_to,
        ["staging", "production"]
    );
    let pipeline = h
        .svc
        .get_pipeline(admin(GetPipelineRequest {
            project_id: PROJECT.into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(pipeline.deployments_30d, 2);
    assert_eq!(pipeline.success_rate, 1.0);
    assert!(pipeline.lead_time_seconds < 120);
    assert!(pipeline.stages.iter().all(|s| s.candidate.is_none()));

    // --- a second release whose production deploy fails -----------------------
    h.github
        .push(REPO, COMMIT_2, &["Fix login redirect", "Bump dependencies"]);
    h.registry.publish("acme/api", "b2c3d4e", DIGEST_2);
    let second = h
        .svc
        .create_release(as_role(
            Role::Member,
            WS_A,
            CreateReleaseRequest {
                project_id: PROJECT.into(),
                version: "v1.1.0".into(),
                source: "pipeline".into(),
                pipeline_run_id: "8841".into(),
                ..Default::default()
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(second.tag, "b2c3d4e");
    assert_eq!(second.version, "v1.1.0");
    assert_eq!(second.source, "pipeline");
    // The commit subjects since the first release, newest first.
    assert!(
        second.notes.starts_with("- Bump dependencies (")
            && second.notes.contains("\n- Fix login redirect ("),
        "{}",
        second.notes
    );
    assert_eq!(
        *h.github.compare_calls.lock().unwrap(),
        [format!("{REPO} {COMMIT_1}...{COMMIT_2}")]
    );
    let staging_2 = h
        .svc
        .promote(promote(Role::Approver, &staging, ""))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(staging_2.release_id, second.id, "the new candidate");
    assert_eq!(staging_2.previous_release_id, release.id);
    h.wait_run(&staging_2.id, DeploymentStatus::Succeeded).await;

    h.webhook
        .fail_next
        .store(1, std::sync::atomic::Ordering::SeqCst);
    let production_2 = h.wait_new_run(&production.id, &[&auto.id]).await;
    assert_eq!(production_2.release_id, second.id);
    h.wait_run(&production_2.id, DeploymentStatus::AwaitingApproval)
        .await;
    assert_eq!(h.approve(&production_2).await, "deploy.production");
    let failure = h.diagnosed(&production_2, "deploy").await;
    assert_eq!(failure.step_key, "deploy");
    assert_eq!(failure.code, "DEPLOYER_FAILED");
    assert!(
        failure.raw_output.contains("500 Internal Server Error"),
        "{}",
        failure.raw_output
    );
    // The hook printed its secret; what was stored has it masked.
    assert!(
        failure.raw_output.contains("DEPLOY_HOOK_SECRET=[redacted]"),
        "{}",
        failure.raw_output
    );
    let options: Vec<&str> = failure.options.iter().map(|o| o.id.as_str()).collect();
    for wanted in ["retry", "rollback_to_previous", "cancel"] {
        assert!(options.contains(&wanted), "{options:?}");
    }
    assert_eq!(
        status_of(&h.run(&production_2.id).await),
        DeploymentStatus::Failed
    );
    assert_eq!(
        h.environment("production").await.current_release_id,
        release.id,
        "a failed deploy does not become current"
    );

    // --- the remediation rolls production back --------------------------------
    h.resolve(&production_2, "rollback_to_previous")
        .await
        .expect("rollback_to_previous");
    let cancelled = h
        .wait_run(&production_2.id, DeploymentStatus::Cancelled)
        .await;
    assert!(cancelled.finished_at.is_some());
    let rollback = h
        .wait_new_run(&production.id, &[&auto.id, &production_2.id])
        .await;
    assert_eq!(rollback.kind, "rollback");
    assert_eq!(rollback.release_id, release.id, "the last good release");
    assert_eq!(rollback.started_by, "user-approver");
    h.wait_run(&rollback.id, DeploymentStatus::AwaitingApproval)
        .await;
    let execution = h.execution(&rollback.execution_id).await;
    assert_eq!(execution.title, "Roll back api in production to a1b2c3d");
    assert_eq!(execution.steps[1].action_type, "deploy.rollback");
    assert_eq!(
        execution.steps[1].required_permissions,
        ["deploy:production"]
    );
    assert_eq!(h.approve(&rollback).await, "deploy.rollback");
    h.wait_run(&rollback.id, DeploymentStatus::Succeeded).await;
    {
        let deliveries = h.webhook.deliveries.lock().unwrap();
        assert_eq!(deliveries.len(), 2);
        assert_eq!(deliveries[1]["kind"], "rollback");
        assert_eq!(deliveries[1]["release"]["tag"], "a1b2c3d");
    }
    assert_eq!(
        h.environment("production").await.current_release_id,
        release.id
    );
    // A rollback is not followed by an automatic promotion.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(h.runs_of(&production.id).await.len(), 3);

    // --- an explicit rollback of staging --------------------------------------
    let rollback_request = |role: Role, environment: &Environment, release_id: &str| {
        as_role(
            role,
            WS_A,
            RollbackRequest {
                environment_id: environment.id.clone(),
                release_id: release_id.to_owned(),
                reason: "login is broken".into(),
            },
        )
    };
    assert_eq!(
        h.svc
            .rollback(rollback_request(Role::Member, &staging, ""))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let back = h
        .svc
        .rollback(rollback_request(Role::Approver, &staging, ""))
        .await
        .expect("rollback staging")
        .into_inner();
    assert_eq!(back.kind, "rollback");
    assert_eq!(
        back.release_id, release.id,
        "the previous successful release"
    );
    assert_eq!(back.previous_release_id, second.id);
    assert_eq!(h.approve(&back).await, "deploy.rollback");
    h.wait_run(&back.id, DeploymentStatus::Succeeded).await;
    assert_eq!(
        h.environment("staging").await.current_release_id,
        release.id
    );
    assert_eq!(
        h.github.dispatches.lock().unwrap().last().unwrap()["inputs"]["version"],
        "a1b2c3d"
    );
    assert_eq!(
        h.release(&release.id).await.deployed_to,
        ["staging", "production"]
    );
    assert!(h.release(&second.id).await.deployed_to.is_empty());
    // Rolling back to something that never ran there is refused.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let third = h
        .svc
        .create_release(as_role(
            Role::Member,
            WS_A,
            CreateReleaseRequest {
                project_id: PROJECT.into(),
                commit_sha: "c3d4e5f60718293a4b5c6d7e8f9012345678a1b2".into(),
                ..Default::default()
            },
        ))
        .await
        .unwrap()
        .into_inner();
    let never = h
        .svc
        .rollback(rollback_request(Role::Approver, &production, &third.id))
        .await
        .unwrap_err();
    assert_eq!(never.code(), Code::FailedPrecondition);
    assert!(never.message().contains("never ran successfully"));

    // --- what the pipeline says now -------------------------------------------
    let pipeline = h
        .svc
        .get_pipeline(admin(GetPipelineRequest {
            project_id: PROJECT.into(),
        }))
        .await
        .unwrap()
        .into_inner();
    // Six runs: five succeeded, one was cancelled by the remediation.
    assert_eq!(pipeline.deployments_30d, 6);
    assert_eq!(
        pipeline.success_rate, 1.0,
        "cancelled runs are not failures"
    );
    assert_eq!(
        pipeline.stages[0].candidate.as_ref().unwrap().id,
        third.id,
        "the newest release is what staging could take"
    );
    assert_eq!(
        pipeline.stages[1].candidate.as_ref().unwrap().id,
        second.id,
        "proven in staging and ahead of what production runs"
    );
    let listed = h
        .svc
        .list_releases(as_role(
            Role::Viewer,
            WS_A,
            ListReleasesRequest {
                project_id: PROJECT.into(),
                ..Default::default()
            },
        ))
        .await
        .unwrap()
        .into_inner()
        .releases;
    assert_eq!(
        listed.iter().map(|r| r.tag.as_str()).collect::<Vec<_>>(),
        ["c3d4e5f", "b2c3d4e", "a1b2c3d"],
        "newest first"
    );
    let all_runs = h.runs_of("").await;
    assert_eq!(all_runs.len(), 6);
    assert_eq!(all_runs[0].id, back.id, "newest first");

    // --- nothing secret was written anywhere ----------------------------------
    for secret in [HOOK_SECRET, REGISTRY_PASSWORD, fake_github::TOKEN] {
        assert_eq!(h.occurrences(secret).await, 0, "{secret} leaked");
    }
    assert!(
        h.occurrences("DEPLOY_HOOK_SECRET").await > 0,
        "names are fine"
    );

    // --- the platform feed and the audit trail saw it ---------------------------
    let events = h.drain_feed();
    for event in &events {
        assert_eq!(event.data["workspace_id"], WS_A, "{}", event.entity);
    }
    let seen = |entity: &str, action: &str, status: &str| {
        events
            .iter()
            .any(|e| e.entity == entity && e.action == action && e.status == status)
    };
    assert!(seen("release", "created", "manual"));
    assert!(seen("release", "created", "pipeline"));
    assert!(seen("release", "updated", "verified"));
    assert!(seen("release", "updated", "deployed"));
    assert!(seen("environment", "created", "ui"));
    assert!(seen("environment", "updated", "deployed"));
    assert!(seen("deployment", "created", "pending"));
    assert!(seen("deployment", "updated", "awaiting_approval"));
    assert!(seen("deployment", "updated", "deploying"));
    assert!(seen("deployment", "updated", "verifying"));
    assert!(seen("deployment", "updated", "succeeded"));
    assert!(seen("deployment", "updated", "failed"));
    assert!(seen("deployment", "updated", "cancelled"));
    let deployment_event = events
        .iter()
        .find(|e| e.entity == "deployment" && e.entity_id == run.id)
        .unwrap();
    assert_eq!(deployment_event.project_id, PROJECT);
    assert_eq!(deployment_event.data["environment"], "staging");
    assert_eq!(deployment_event.data["execution_id"], run.execution_id);
    let actions = h.audit.actions();
    for wanted in [
        "registry.upsert",
        "environment.upsert",
        "release.create",
        "deployment.promote",
        "deployment.rollback",
    ] {
        assert!(actions.iter().any(|a| a == wanted), "{wanted} not audited");
    }
    {
        let entries = h.audit.entries.lock().unwrap();
        let automatic = entries
            .iter()
            .find(|(principal, entry)| {
                entry.action == "deployment.promote" && principal.user_id == "system"
            })
            .expect("the automatic promotion is audited as system");
        assert!(automatic.1.detail.contains("auto-promote after staging"));
    }

    h.teardown().await;
}

/// What an admin may configure, what is refused and why, who may do what,
/// and how `.sdi.yml` environments replace each other.
#[tokio::test]
async fn registries_and_environments_are_validated_and_config_managed() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let registry = |name: &str, default: bool| Registry {
        name: name.into(),
        kind: "acr".into(),
        host: "acme.azurecr.io".into(),
        namespace: "shop".into(),
        auth: "secret".into(),
        username_secret: "REGISTRY_USER".into(),
        password_secret: "REGISTRY_PASSWORD".into(),
        is_default: default,
        ..Default::default()
    };
    let upsert_registry = |role: Role, registry: Registry| {
        h.svc.upsert_registry(as_role(
            role,
            WS_A,
            UpsertRegistryRequest {
                registry: Some(registry),
            },
        ))
    };

    // --- registries -----------------------------------------------------------
    for role in [Role::Viewer, Role::Member, Role::Approver] {
        assert_eq!(
            upsert_registry(role, registry("acr", false))
                .await
                .unwrap_err()
                .code(),
            Code::PermissionDenied,
            "{role:?}"
        );
    }
    let acr = upsert_registry(Role::Admin, registry("acr", false))
        .await
        .unwrap()
        .into_inner();
    assert!(!acr.is_default);
    assert_eq!(
        h.workspaces.listed_by.lock().unwrap()[0].role,
        Role::Admin,
        "secret names are asked for as the caller"
    );
    // A name taken, an unknown secret, a bad kind.
    assert_eq!(
        upsert_registry(Role::Admin, registry("acr", false))
            .await
            .unwrap_err()
            .code(),
        Code::AlreadyExists
    );
    let missing = upsert_registry(
        Role::Admin,
        Registry {
            password_secret: "NOT_STORED".into(),
            ..registry("other", false)
        },
    )
    .await
    .unwrap_err();
    assert_eq!(missing.code(), Code::InvalidArgument);
    assert!(
        missing.message().contains("no secret named NOT_STORED"),
        "{}",
        missing.message()
    );
    assert_eq!(
        upsert_registry(
            Role::Admin,
            Registry {
                kind: "quay".into(),
                ..registry("other", false)
            }
        )
        .await
        .unwrap_err()
        .code(),
        Code::InvalidArgument
    );
    // Making it the default takes the flag from ghcr; there is always one.
    let acr = upsert_registry(
        Role::Admin,
        Registry {
            id: acr.id.clone(),
            ..registry("acr", true)
        },
    )
    .await
    .unwrap()
    .into_inner();
    assert!(acr.is_default);
    let listed = h
        .svc
        .list_registries(admin(ListRegistriesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .registries;
    assert_eq!(listed.len(), 2);
    assert_eq!(listed.iter().filter(|r| r.is_default).count(), 1);
    let ghcr = listed.iter().find(|r| r.name == "ghcr").unwrap().clone();
    let unmark = upsert_registry(
        Role::Admin,
        Registry {
            id: acr.id.clone(),
            ..registry("acr", false)
        },
    )
    .await
    .unwrap_err();
    assert_eq!(unmark.code(), Code::FailedPrecondition);
    let delete_registry = |id: &str| {
        h.svc
            .delete_registry(admin(DeleteRegistryRequest { id: id.to_owned() }))
    };
    assert_eq!(
        delete_registry(&acr.id).await.unwrap_err().code(),
        Code::FailedPrecondition,
        "the default cannot be deleted"
    );

    // --- environments ---------------------------------------------------------
    let upsert = |role: Role, environment: Environment| {
        h.svc.upsert_environment(as_role(
            role,
            WS_A,
            UpsertEnvironmentRequest {
                environment: Some(environment),
            },
        ))
    };
    for role in [Role::Viewer, Role::Member, Role::Approver] {
        assert_eq!(
            upsert(role, h.staging()).await.unwrap_err().code(),
            Code::PermissionDenied,
            "{role:?}"
        );
    }
    // Refusals name the field: the editor maps the message back to it.
    let cases: Vec<(Environment, Code, &str)> = vec![
        (
            Environment {
                kind: "kubernetes".into(),
                ..h.staging()
            },
            Code::InvalidArgument,
            "kind must be one of",
        ),
        (
            Environment {
                config: HashMap::from([("replicas".to_owned(), "3".to_owned())]),
                ..h.staging()
            },
            Code::InvalidArgument,
            "config.replicas is not a setting of github_actions environments",
        ),
        (
            Environment {
                kind: "gitops_flux".into(),
                config: HashMap::new(),
                ..h.staging()
            },
            Code::InvalidArgument,
            "config.path is required for gitops_flux",
        ),
        (
            Environment {
                config: HashMap::from([
                    ("url".to_owned(), "https://deploy.example.com".to_owned()),
                    ("secret".to_owned(), "NOT_STORED".to_owned()),
                ]),
                ..h.production()
            },
            Code::InvalidArgument,
            "config.secret: this workspace has no secret named NOT_STORED",
        ),
        (
            Environment {
                registry_id: Uuid::now_v7().to_string(),
                ..h.staging()
            },
            Code::InvalidArgument,
            "registryId",
        ),
        (
            Environment {
                project_id: "0192f6a0-0000-7000-8000-00000000ffff".into(),
                ..h.staging()
            },
            Code::NotFound,
            "no such project",
        ),
        (
            Environment {
                source: "config".into(),
                ..h.staging()
            },
            Code::FailedPrecondition,
            ".sdi.yml",
        ),
    ];
    for (environment, code, why) in cases {
        let error = upsert(Role::Admin, environment.clone()).await.unwrap_err();
        assert_eq!(error.code(), code, "{}", error.message());
        assert!(error.message().contains(why), "{}", error.message());
    }
    assert!(h.environments().await.is_empty(), "nothing was stored");

    let staging = h
        .upsert(Environment {
            registry_id: ghcr.id.clone(),
            ..h.staging()
        })
        .await;
    assert_eq!(staging.registry_id, ghcr.id);
    let production = h.upsert(h.production()).await;
    assert_eq!(
        upsert(Role::Admin, h.staging()).await.unwrap_err().code(),
        Code::AlreadyExists,
        "names are unique within a project"
    );
    // A registry an environment uses stays.
    let in_use = delete_registry(&ghcr.id).await.unwrap_err();
    assert_eq!(in_use.code(), Code::FailedPrecondition);
    assert!(in_use.message().contains("1 environment"));
    // Editing keeps the id and the project.
    let edited = h
        .upsert(Environment {
            id: staging.id.clone(),
            url: "https://staging.example.com/".into(),
            health_path: "healthz".into(),
            registry_id: String::new(),
            ..h.staging()
        })
        .await;
    assert_eq!(edited.id, staging.id);
    assert_eq!(edited.url, "https://staging.example.com");
    assert_eq!(edited.health_path, "/healthz");
    assert!(edited.registry_id.is_empty());
    assert_eq!(
        upsert(
            Role::Admin,
            Environment {
                id: staging.id.clone(),
                project_id: "another".into(),
                ..h.staging()
            }
        )
        .await
        .unwrap_err()
        .code(),
        Code::InvalidArgument
    );
    delete_registry(&ghcr.id).await.expect("now unused");

    // --- .sdi.yml: SyncEnvironments ---------------------------------------------
    let sync = |role: Role, environments: Vec<Environment>| {
        h.svc.sync_environments(as_role(
            role,
            WS_A,
            SyncEnvironmentsRequest {
                project_id: PROJECT.into(),
                environments,
            },
        ))
    };
    let declared = |name: &str, kind: &str, config: &[(&str, &str)]| Environment {
        name: name.into(),
        kind: kind.into(),
        config: config
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        ..Default::default()
    };
    assert_eq!(
        sync(Role::Viewer, vec![]).await.unwrap_err().code(),
        Code::PermissionDenied
    );
    // One bad entry and nothing changes; the message names it and the key.
    let bad = sync(
        Role::Member,
        vec![
            declared("staging", "github_actions", &[]),
            declared(
                "canary",
                "gitops_flux",
                &[("path", "k.yaml"), ("replicas", "2")],
            ),
        ],
    )
    .await
    .unwrap_err();
    assert_eq!(bad.code(), Code::InvalidArgument);
    assert!(
        bad.message()
            .contains("environment canary: config.replicas is not a setting of gitops_flux"),
        "{}",
        bad.message()
    );
    assert!(h.environments().await.iter().all(|e| e.source == "ui"));

    // The file declares staging (already there, made in the UI) and qa. A
    // secret that is not stored yet does not stop the sync.
    let synced = sync(
        Role::Member,
        vec![
            declared(
                "qa",
                "webhook",
                &[
                    ("url", "https://qa.example.com/hook"),
                    ("secret", "QA_HOOK_KEY"),
                ],
            ),
            declared("staging", "github_actions", &[("workflow", "release.yml")]),
        ],
    )
    .await
    .expect("sync")
    .into_inner()
    .environments;
    let by_name = |name: &str| synced.iter().find(|e| e.name == name).unwrap();
    assert_eq!(synced.len(), 3);
    assert_eq!(by_name("qa").source, "config");
    assert_eq!(by_name("qa").position, 0, "the order of the file");
    assert_eq!(by_name("staging").source, "config", "the file wins");
    assert_eq!(by_name("staging").position, 1);
    assert_eq!(
        by_name("staging").id,
        staging.id,
        "it keeps its id and history"
    );
    assert_eq!(by_name("staging").config["workflow"], "release.yml");
    assert_eq!(by_name("production").source, "ui", "UI environments stay");
    assert_eq!(by_name("production").id, production.id);

    // Config-managed environments are read-only through the edit RPCs.
    let read_only = upsert(
        Role::Admin,
        Environment {
            id: staging.id.clone(),
            ..h.staging()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(read_only.code(), Code::FailedPrecondition);
    assert!(read_only.message().contains(".sdi.yml"));
    let delete = |id: &str| {
        h.svc
            .delete_environment(admin(DeleteEnvironmentRequest { id: id.to_owned() }))
    };
    assert_eq!(
        delete(&staging.id).await.unwrap_err().code(),
        Code::FailedPrecondition
    );
    // Dropping it from the file removes it; qa stays, the UI one stays.
    let synced = sync(
        Role::Member,
        vec![declared(
            "qa",
            "webhook",
            &[
                ("url", "https://qa.example.com/hook"),
                ("secret", "QA_HOOK_KEY"),
            ],
        )],
    )
    .await
    .unwrap()
    .into_inner()
    .environments;
    assert_eq!(
        synced.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["qa", "production"]
    );
    // An empty file section removes every config environment, only those.
    let synced = sync(Role::Member, vec![])
        .await
        .unwrap()
        .into_inner()
        .environments;
    assert_eq!(synced.len(), 1);
    assert_eq!(synced[0].name, "production");
    // A UI environment is deleted by an admin.
    assert_eq!(
        h.svc
            .delete_environment(as_role(
                Role::Approver,
                WS_A,
                DeleteEnvironmentRequest {
                    id: production.id.clone()
                }
            ))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    delete(&production.id).await.expect("delete");
    assert_eq!(
        delete(&production.id).await.unwrap_err().code(),
        Code::NotFound
    );

    // --- a hand-written delivery plan is refused --------------------------------
    let refused = h
        .svc
        .create_execution(approver(CreateExecutionRequest {
            project_id: PROJECT.into(),
            title: "sneaky".into(),
            adapter: "delivery".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);
    assert!(refused.message().contains("promoting or rolling back"));

    h.teardown().await;
}

/// Delivery rows belong to their workspace: another one cannot see or use
/// them, the export carries them (secret names, never values), and deleting
/// the workspace removes them.
#[tokio::test]
async fn delivery_rows_are_isolated_exported_and_deleted_with_the_workspace() {
    let Some(h) = Harness::new().await else {
        return;
    };
    h.fake_registry_as_default().await;
    let staging = h.upsert(h.staging()).await;
    h.registry.publish("acme/api", "a1b2c3d", DIGEST_1);
    h.grant(&["deploy:development"]).await;
    let release = h
        .svc
        .create_release(as_role(
            Role::Member,
            WS_A,
            CreateReleaseRequest {
                project_id: PROJECT.into(),
                // A webhook that knows the image names it in full.
                image: format!("{}/acme/api:a1b2c3d", h.registry_host),
                digest: DIGEST_1.into(),
                commit_sha: COMMIT_1.into(),
                source: "package".into(),
                ..Default::default()
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(release.tag, "a1b2c3d");
    assert_eq!(release.image, format!("{}/acme/api", h.registry_host));
    assert_eq!(release.digest, DIGEST_1);
    let run = h
        .svc
        .promote(approver(PromoteRequest {
            environment_id: staging.id.clone(),
            release_id: release.id.clone(),
        }))
        .await
        .unwrap()
        .into_inner();
    h.wait_run(&run.id, DeploymentStatus::Succeeded).await;

    // Malformed input is refused before anything is asked of GitHub.
    for (request, why) in [
        (
            CreateReleaseRequest {
                project_id: PROJECT.into(),
                commit_sha: "not-a-sha".into(),
                ..Default::default()
            },
            "commit_sha",
        ),
        (
            CreateReleaseRequest {
                project_id: PROJECT.into(),
                tag: "bad tag".into(),
                ..Default::default()
            },
            "tag",
        ),
        (
            CreateReleaseRequest {
                project_id: PROJECT.into(),
                digest: "sha256:abc".into(),
                ..Default::default()
            },
            "digest",
        ),
        (
            CreateReleaseRequest {
                project_id: PROJECT.into(),
                image: "GHCR.io/Acme/Api Server".into(),
                ..Default::default()
            },
            "image",
        ),
        (
            CreateReleaseRequest {
                project_id: PROJECT.into(),
                source: "cron".into(),
                ..Default::default()
            },
            "source",
        ),
        (CreateReleaseRequest::default(), "project_id"),
    ] {
        let error = h
            .svc
            .create_release(as_role(Role::Member, WS_A, request))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument, "{why}");
        assert!(error.message().contains(why), "{}", error.message());
    }

    // --- workspace B sees none of it ------------------------------------------
    let other = Role::Owner;
    assert!(
        h.svc
            .list_environments(as_role(
                other,
                WS_B,
                ListEnvironmentsRequest {
                    project_id: PROJECT.into()
                }
            ))
            .await
            .unwrap()
            .into_inner()
            .environments
            .is_empty()
    );
    assert!(
        h.svc
            .list_releases(as_role(
                other,
                WS_B,
                ListReleasesRequest {
                    project_id: PROJECT.into(),
                    ..Default::default()
                }
            ))
            .await
            .unwrap()
            .into_inner()
            .releases
            .is_empty()
    );
    assert_eq!(
        h.svc
            .get_release(as_role(
                other,
                WS_B,
                GetReleaseRequest {
                    id: release.id.clone()
                }
            ))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        h.svc
            .get_deployment_run(as_role(
                other,
                WS_B,
                GetDeploymentRunRequest { id: run.id.clone() }
            ))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        h.svc
            .promote(as_role(
                other,
                WS_B,
                PromoteRequest {
                    environment_id: staging.id.clone(),
                    release_id: release.id.clone(),
                }
            ))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound,
        "another workspace's environment does not exist"
    );
    assert_eq!(
        h.svc
            .delete_environment(as_role(
                other,
                WS_B,
                DeleteEnvironmentRequest {
                    id: staging.id.clone()
                }
            ))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    // B has its own seeded registry, not A's.
    let registries_b = h
        .svc
        .list_registries(as_role(Role::Viewer, WS_B, ListRegistriesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .registries;
    assert_eq!(registries_b.len(), 1);
    assert_eq!(registries_b[0].name, "ghcr");
    let pipeline_b = h
        .svc
        .get_pipeline(as_role(
            Role::Viewer,
            WS_B,
            GetPipelineRequest {
                project_id: PROJECT.into(),
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(pipeline_b.stages.is_empty());
    assert_eq!(pipeline_b.deployments_30d, 0);

    // --- the export carries the delivery rows ---------------------------------
    let mut stream = h
        .svc
        .export_workspace_data(admin(ExportWorkspaceDataRequest {
            workspace_id: WS_A.into(),
        }))
        .await
        .unwrap()
        .into_inner();
    let mut records = Vec::new();
    while let Some(record) = stream.next().await {
        records.push(record.unwrap());
    }
    let count = |entity: &str| records.iter().filter(|r| r.entity == entity).count();
    assert_eq!(count("registry"), 2);
    assert_eq!(count("environment"), 1);
    assert_eq!(count("release"), 1);
    assert_eq!(count("deployment_run"), 1);
    let exported: String = records.iter().map(|r| r.json.as_str()).collect();
    assert!(
        exported.contains("REGISTRY_PASSWORD"),
        "secret names are data"
    );
    for secret in [HOOK_SECRET, REGISTRY_PASSWORD, fake_github::TOKEN] {
        assert!(!exported.contains(secret), "{secret} is in the export");
    }

    // --- deleting the workspace removes them ----------------------------------
    let deleted = h
        .svc
        .delete_workspace_data(as_role(
            Role::Owner,
            WS_A,
            DeleteWorkspaceDataRequest {
                workspace_id: WS_A.into(),
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(deleted.deleted["registry"], 2);
    assert_eq!(deleted.deleted["environment"], 1);
    assert_eq!(deleted.deleted["release"], 1);
    assert_eq!(deleted.deleted["deployment_run"], 1);
    assert_eq!(deleted.deleted["execution"], 1);
    for table in ["registries", "environments", "releases", "deployment_runs"] {
        let (rows,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table} WHERE workspace_id = $1"
        )))
        .bind(Uuid::parse_str(WS_A).unwrap())
        .fetch_one(&h.pool)
        .await
        .unwrap();
        assert_eq!(rows, 0, "{table}");
    }
    // B's registry is untouched, and A is seeded afresh on its next use.
    let (rows,): (i64,) = sqlx::query_as("SELECT count(*) FROM registries WHERE workspace_id = $1")
        .bind(Uuid::parse_str(WS_B).unwrap())
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
    let again = h
        .svc
        .list_registries(admin(ListRegistriesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .registries;
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].name, "ghcr");

    h.teardown().await;
}
