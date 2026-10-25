//! SDI's GitOps engine from the execution service (CONTRACT.md §24.7), end
//! to end and in process: executions on the `gitops` runtime created the
//! way the gateway creates them, and deployment runs of an `sdi_gitops`
//! environment, run by the real runner through the real gates with the real
//! `gitops` adapter against a fake engine that speaks the real gRPC
//! contract (`fakes::gitops`), next to the fake registry and GitHub the
//! other deployment tests use.

use std::{collections::HashMap, sync::Arc, time::Duration};

use sdi_common::principal::{Principal, Role};
use sdi_proto::{
    common::v1::{PlanStep, RiskLevel},
    execution::v1::{
        Approval, ApprovalStatus, CreateExecutionRequest, CreateReleaseRequest,
        DecideApprovalRequest, DeploymentRun, DeploymentStatus, Environment, EventType, Execution,
        ExecutionStatus, Failure, GetDeploymentRunRequest, GetExecutionRequest,
        GrantPermissionRequest, Identity, ListApprovalsRequest, ListDeploymentRunsRequest,
        ListEnvironmentsRequest, ListEventsRequest, ListIdentitiesRequest, ListPoliciesRequest,
        PolicyDecision, PromoteRequest, Registry, Release, ResolveFailureRequest,
        StartExecutionRequest, StepStatus, UpdatePolicyRequest, UpsertEnvironmentRequest,
        UpsertRegistryRequest, execution_service_server::ExecutionService,
    },
    gitops::v1::{ApplicationSource, ResourceResult, SyncResult},
};
use sqlx::PgPool;
use tonic::{Code, Request};
use uuid::Uuid;

use super::fakes::{
    github::{self as fake_github, FakeGithub},
    gitops::{self as fake, FakeGitops, Scripted},
    registry::FakeRegistry,
};
use crate::{
    adapters::{
        Adapter, DeliveryTools, DockerAdapter, GithubAccess, GithubAdapter, Registry as Adapters,
        RunContext, SimulatedAdapter, StepInput, StepOutcome, azure_devops,
        gitops::GitopsAdapter,
        gitops_engine::GitopsEngineAdapter,
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
const POLL: Duration = Duration::from_millis(10);

const WS: &str = "00000000-0000-7000-8000-0000000000d1";
const PROJECT: &str = "0192f6a0-0000-7000-8000-0000000000d2";
const REPO: &str = "acme/api";
const COMMIT_1: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
const COMMIT_2: &str = "b2c3d4e5f60718293a4b5c6d7e8f9012345678a1";
const DIGEST_1: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DIGEST_2: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

fn as_user<T>(user: &str, role: Role, message: T) -> Request<T> {
    let mut request = Request::new(message);
    Principal {
        user_id: user.to_owned(),
        workspace_id: WS.to_owned(),
        role,
    }
    .apply(&mut request);
    request
}

fn admin<T>(message: T) -> Request<T> {
    as_user("user-admin", Role::Admin, message)
}

fn approver<T>(message: T) -> Request<T> {
    as_user("user-approver", Role::Approver, message)
}

/// A step the way the gateway writes the plan of `POST
/// /applications/{id}/sync` (CONTRACT.md §24.8).
fn plan_step(
    key: &str,
    action_type: &str,
    permissions: &[&str],
    depends_on: &[&str],
    params: &[(&str, &str)],
) -> PlanStep {
    PlanStep {
        key: key.into(),
        title: key.replace('_', " "),
        description: String::new(),
        action_type: action_type.into(),
        adapter: "gitops".into(),
        depends_on: depends_on.iter().map(|s| (*s).to_owned()).collect(),
        required_permissions: permissions.iter().map(|s| (*s).to_owned()).collect(),
        params: params
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        verification: String::new(),
    }
}

/// preview → sync → verify_health. The sync names no permission: the
/// service adds `cluster:write` itself.
fn sync_plan() -> Vec<PlanStep> {
    vec![
        plan_step(
            "preview",
            "gitops.preview",
            &["cluster:read"],
            &[],
            &[("application", fake::APP), ("prune", "true")],
        ),
        plan_step(
            "sync",
            "gitops.sync",
            &[],
            &["preview"],
            &[("application", fake::APP), ("prune", "true")],
        ),
        plan_step(
            "verify_health",
            "gitops.verify_health",
            &["cluster:read"],
            &["sync"],
            &[("application", fake::APP), ("timeout_seconds", "10")],
        ),
    ]
}

fn step_of<'a>(execution: &'a Execution, key: &str) -> &'a sdi_proto::execution::v1::Step {
    execution
        .steps
        .iter()
        .find(|s| s.key == key)
        .unwrap_or_else(|| panic!("no step {key}"))
}

fn option_ids(failure: &Failure) -> Vec<&str> {
    failure.options.iter().map(|o| o.id.as_str()).collect()
}

/// A helm application next to the kustomize one.
fn helm_app() -> sdi_proto::gitops::v1::Application {
    let mut app = fake::kustomize_app();
    app.id = fake::HELM_APP.into();
    app.name = "web".into();
    app.source = Some(ApplicationSource {
        repo: "acme/fleet".into(),
        revision: "main".into(),
        path: "charts/web".into(),
        r#type: "helm".into(),
        ..Default::default()
    });
    app
}

struct Harness {
    pool: PgPool,
    schema: String,
    rt: Arc<Runtime>,
    svc: ExecutionGrpc,
    gitops: Arc<FakeGitops>,
    gitops_url: String,
    github: Arc<FakeGithub>,
    github_api: String,
    registry: Arc<FakeRegistry>,
    registry_host: String,
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
        let (gitops, gitops_url) = FakeGitops::default()
            .with_app(fake::kustomize_app())
            .with_app(helm_app())
            .serve()
            .await;
        let (github, github_api) = FakeGithub::default()
            .with_head(REPO, COMMIT_1, "Add health endpoint")
            .serve()
            .await;
        let (registry, registry_host) = FakeRegistry::serve().await;
        let rt = Self::runtime(&pool, &gitops_url, &github_api);
        Some(Self {
            pool,
            schema,
            svc: ExecutionGrpc::new(Arc::clone(&rt)),
            rt,
            gitops,
            gitops_url,
            github,
            github_api,
            registry,
            registry_host,
        })
    }

    /// The service as a process builds it, with every wait shortened.
    fn runtime(pool: &PgPool, gitops_url: &str, github_api: &str) -> Arc<Runtime> {
        let net = Net::new(NetPolicy::ALLOW_PRIVATE);
        let fast = Duration::from_millis(10);
        let adapters = Adapters::new(
            Arc::new(SimulatedAdapter::new(1)),
            Arc::new(DockerAdapter::new(
                std::env::temp_dir().join("sdi-execution-tests"),
                None,
                github_api,
            )),
            Arc::new(GithubAdapter::new(None, github_api).with_poll(fast, Duration::from_secs(5))),
        )
        .with_delivery(DeliveryTools {
            registry: Arc::new(RegistryAdapter::new(net.clone(), None)),
            gitops: Arc::new(GitopsAdapter::new(None, github_api).with_poll(fast)),
            jenkins: Arc::new(JenkinsAdapter::new(net.clone()).with_poll(fast)),
            azure_devops: Arc::new(
                azure_devops::AzureDevopsAdapter::new(net.clone(), azure_devops::DEFAULT_BASE)
                    .with_poll(fast),
            ),
            webhook: Arc::new(WebhookAdapter::new(net.clone())),
            http: Arc::new(HttpHealthAdapter::new(net).with_poll(fast)),
        })
        .with_gitops(Arc::new(
            GitopsEngineAdapter::new(gitops_url)
                .expect("endpoint")
                .with_poll(fast),
        ));
        let projects = StaticProjects::default();
        projects.add(
            WS,
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
                ("REGISTRY_USER", "robot"),
                ("REGISTRY_PASSWORD", "registry-password-6b1f0c2e"),
            ],
        );
        Runtime::with_delivery(
            pool.clone(),
            adapters,
            Arc::new(RecordingPatternOutcomes::default()),
            Arc::new(RecordingAudit::default()),
            Duration::ZERO,
            false,
            Arc::new(StaticGithubAccess(GithubAccess {
                token: fake_github::TOKEN.into(),
                api_base_url: github_api.to_owned(),
            })),
            Delivery::new(
                Arc::new(projects),
                Arc::new(workspaces.clone()),
                Some(Arc::new(workspaces.secrets.clone())),
                None,
                NetPolicy::ALLOW_PRIVATE,
            ),
        )
    }

    /// The service after a restart: a new runtime over the same database.
    fn restarted(&mut self) {
        self.rt = Self::runtime(&self.pool, &self.gitops_url, &self.github_api);
        self.svc = ExecutionGrpc::new(Arc::clone(&self.rt));
    }

    async fn teardown(self) {
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

    async fn create(&self, title: &str, steps: Vec<PlanStep>) -> Execution {
        self.svc
            .create_execution(approver(CreateExecutionRequest {
                project_id: PROJECT.into(),
                title: title.into(),
                steps,
                adapter: "gitops".into(),
                ..Default::default()
            }))
            .await
            .expect("create execution")
            .into_inner()
    }

    async fn start(&self, id: &str) {
        self.svc
            .start_execution(approver(StartExecutionRequest {
                id: id.to_owned(),
                fault: None,
            }))
            .await
            .expect("start execution");
    }

    async fn get(&self, id: &str) -> Execution {
        self.svc
            .get_execution(admin(GetExecutionRequest { id: id.to_owned() }))
            .await
            .expect("get execution")
            .into_inner()
    }

    async fn wait_status(&self, id: &str, status: ExecutionStatus) -> Execution {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let execution = self.get(id).await;
            if execution.status == status as i32 {
                return execution;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "timed out waiting for {status:?}; the execution is {:?}, failure {:?}, steps {:?}",
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

    /// Waits for the execution to be paused and diagnosed on `step_key`.
    async fn diagnosed(&self, id: &str, step_key: &str) -> Failure {
        let execution = self.wait_status(id, ExecutionStatus::Diagnosing).await;
        let failure = execution.failure.expect("a failure while diagnosing");
        assert_eq!(failure.step_key, step_key, "{failure:?}");
        assert_eq!(
            failure.options.iter().filter(|o| o.recommended).count(),
            1,
            "{failure:?}"
        );
        failure
    }

    async fn resolve(&self, id: &str, option_id: &str) {
        self.svc
            .resolve_failure(approver(ResolveFailureRequest {
                execution_id: id.to_owned(),
                option_id: option_id.to_owned(),
                instruction: String::new(),
            }))
            .await
            .expect("resolve failure");
    }

    /// Waits for the one pending approval of the execution.
    async fn pending_approval(&self, id: &str) -> Approval {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let pending = self
                .svc
                .list_approvals(admin(ListApprovalsRequest {
                    status: ApprovalStatus::Pending as i32,
                    execution_id: id.to_owned(),
                }))
                .await
                .expect("list approvals")
                .into_inner()
                .approvals;
            if let Some(approval) = pending.into_iter().next() {
                return approval;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for an approval of {id}"
            );
            tokio::time::sleep(POLL).await;
        }
    }

    async fn approve_as(&self, user: &str, approval: &Approval) {
        self.svc
            .decide_approval(as_user(
                user,
                Role::Approver,
                DecideApprovalRequest {
                    id: approval.id.clone(),
                    approve: true,
                    note: String::new(),
                    decided_by: String::new(),
                },
            ))
            .await
            .expect("decide approval");
    }

    async fn identity(&self, name: &str) -> Identity {
        self.svc
            .list_identities(admin(ListIdentitiesRequest {}))
            .await
            .expect("list identities")
            .into_inner()
            .identities
            .into_iter()
            .find(|i| i.name == name)
            .unwrap_or_else(|| panic!("no identity {name}"))
    }

    async fn grant(&self, identity: &str, permissions: &[&str]) {
        let identity = self.identity(identity).await;
        self.svc
            .grant_permission(approver(GrantPermissionRequest {
                identity_id: identity.id,
                permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
                ttl_minutes: 0,
            }))
            .await
            .expect("grant");
    }

    async fn set_policy(&self, action_type: &str, decision: PolicyDecision) {
        self.svc
            .update_policy(admin(UpdatePolicyRequest {
                action_type: action_type.into(),
                decision: decision as i32,
            }))
            .await
            .expect("update policy");
    }

    // --- delivery -------------------------------------------------------------

    async fn fake_registry_as_default(&self) {
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
            .expect("upsert registry");
    }

    async fn upsert(&self, environment: Environment) -> Result<Environment, tonic::Status> {
        self.svc
            .upsert_environment(admin(UpsertEnvironmentRequest {
                environment: Some(environment),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    async fn release(&self, commit: &str) -> Release {
        self.svc
            .create_release(as_user(
                "user-member",
                Role::Member,
                CreateReleaseRequest {
                    project_id: PROJECT.into(),
                    commit_sha: commit.into(),
                    ..Default::default()
                },
            ))
            .await
            .expect("create release")
            .into_inner()
    }

    async fn promote(&self, environment: &Environment, release: &Release) -> DeploymentRun {
        self.svc
            .promote(approver(PromoteRequest {
                environment_id: environment.id.clone(),
                release_id: release.id.clone(),
            }))
            .await
            .expect("promote")
            .into_inner()
    }

    async fn run(&self, id: &str) -> DeploymentRun {
        self.svc
            .get_deployment_run(admin(GetDeploymentRunRequest { id: id.to_owned() }))
            .await
            .expect("get deployment run")
            .into_inner()
    }

    async fn wait_run(&self, id: &str, status: DeploymentStatus) -> DeploymentRun {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let run = self.run(id).await;
            if run.status == status as i32 {
                return run;
            }
            if tokio::time::Instant::now() >= deadline {
                let execution = self.get(&run.execution_id).await;
                panic!(
                    "timed out waiting for {status:?}; the run is {:?}, its execution {:?}, failure {:?}",
                    DeploymentStatus::try_from(run.status),
                    ExecutionStatus::try_from(execution.status),
                    execution.failure.map(|f| (f.code, f.raw_output)),
                );
            }
            tokio::time::sleep(POLL).await;
        }
    }

    async fn environment(&self, name: &str) -> Environment {
        self.svc
            .list_environments(admin(ListEnvironmentsRequest {
                project_id: PROJECT.into(),
            }))
            .await
            .expect("list environments")
            .into_inner()
            .environments
            .into_iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("no environment {name}"))
    }
}

/// The gateway's plan (§24.8): the preview runs by itself, the sync stops
/// before it runs for the timed `cluster:write` grant nobody holds, then
/// for a person; it runs as the person who approved it, and the health
/// check waits for the application.
#[tokio::test]
async fn a_gitops_run_waits_for_cluster_write_and_an_approval_then_syncs_as_the_approver() {
    let Some(h) = Harness::new().await else {
        return;
    };

    // Creating a run that changes a cluster is an approver's.
    let refused = h
        .svc
        .create_execution(as_user(
            "user-member",
            Role::Member,
            CreateExecutionRequest {
                project_id: PROJECT.into(),
                title: "Sync api".into(),
                steps: sync_plan(),
                adapter: "gitops".into(),
                ..Default::default()
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::PermissionDenied);
    // Only the engine's steps run on the gitops runtime.
    let mixed = h
        .svc
        .create_execution(approver(CreateExecutionRequest {
            project_id: PROJECT.into(),
            title: "Mixed".into(),
            steps: vec![plan_step("build", "container.build", &[], &[], &[])],
            adapter: "gitops".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(mixed.code(), Code::InvalidArgument);
    assert!(mixed.message().contains("cannot run on the gitops adapter"));

    // §24.7's four policies and the identity the runtime executes as.
    let policies = h
        .svc
        .list_policies(admin(ListPoliciesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .policies;
    for (action_type, risk, decision, editable) in [
        (
            "gitops.preview",
            RiskLevel::Safe,
            PolicyDecision::Auto,
            false,
        ),
        (
            "gitops.sync",
            RiskLevel::High,
            PolicyDecision::Approval,
            true,
        ),
        (
            "gitops.rollback",
            RiskLevel::High,
            PolicyDecision::Approval,
            true,
        ),
        (
            "gitops.verify_health",
            RiskLevel::Safe,
            PolicyDecision::Auto,
            false,
        ),
    ] {
        let policy = policies
            .iter()
            .find(|p| p.action_type == action_type)
            .unwrap_or_else(|| panic!("no policy for {action_type}"));
        assert_eq!(policy.risk, risk as i32, "{action_type}");
        assert_eq!(policy.decision, decision as i32, "{action_type}");
        assert_eq!(policy.editable, editable, "{action_type}");
    }
    let identity = h.identity("gitops").await;
    assert_eq!(
        identity
            .grants
            .iter()
            .map(|g| g.permission.as_str())
            .collect::<Vec<_>>(),
        ["cluster:read"]
    );
    assert!(identity.grants.iter().all(|g| g.expires_at.is_none()));

    let created = h.create("Sync api", sync_plan()).await;
    assert_eq!(created.adapter, "gitops");
    assert_eq!(created.identity_id, identity.id);
    assert_eq!(
        step_of(&created, "sync").required_permissions,
        ["cluster:write"],
        "added although the plan named none"
    );
    assert_eq!(
        step_of(&created, "sync").policy_decision,
        PolicyDecision::Approval as i32
    );
    h.start(&created.id).await;

    // The preview ran by itself; the sync stopped before it ran.
    let failure = h.diagnosed(&created.id, "sync").await;
    assert_eq!(failure.code, "PERMISSION_DENIED");
    assert_eq!(failure.missing_permissions, ["cluster:write"]);
    assert_eq!(option_ids(&failure)[0], "request_access");
    let paused = h.get(&created.id).await;
    let preview = step_of(&paused, "preview");
    assert_eq!(preview.status, StepStatus::Succeeded as i32);
    assert!(
        preview
            .output
            .contains("1 to create, 1 to change, 1 to prune"),
        "{}",
        preview.output
    );
    assert!(preview.output.contains("would create: Service shop/api"));
    assert!(
        preview
            .output
            .contains("would prune: ConfigMap shop/legacy")
    );
    assert!(h.gitops.applied().is_empty(), "nothing reached the cluster");
    let syncs = h.gitops.syncs();
    assert_eq!(syncs.len(), 1);
    assert!(syncs[0].1.dry_run);
    // Nobody approved the preview: it ran as whoever created the run.
    assert_eq!(syncs[0].0.user_id, "user-approver");
    assert_eq!(syncs[0].0.role, Role::Approver);
    assert_eq!(syncs[0].0.workspace_id, WS);

    // The timed grant, then a person.
    h.resolve(&created.id, "request_access").await;
    let approval = h.pending_approval(&created.id).await;
    assert_eq!(approval.step_key, "sync");
    assert_eq!(approval.action_type, "gitops.sync");
    assert!(h.gitops.applied().is_empty());
    h.approve_as("alice", &approval).await;
    let done = h.wait_status(&created.id, ExecutionStatus::Succeeded).await;

    let applied = h.gitops.applied();
    assert_eq!(applied.len(), 1);
    let (principal, request) = &applied[0];
    assert_eq!(principal.user_id, "alice", "the approver is forwarded");
    assert_eq!(principal.role, Role::Approver);
    assert_eq!(request.id, fake::APP);
    assert!(request.prune);
    assert_eq!(request.execution_id, created.id);
    let history = h.gitops.history(fake::APP);
    assert_eq!(history[0].initiated_by, "alice");
    assert_eq!(done.outputs["revision"], fake::REVISION);
    assert_eq!(done.outputs["history_id"], history[0].id);
    assert_eq!(done.outputs["health"], "healthy");
    assert!(
        step_of(&done, "sync")
            .output
            .starts_with("synced application api")
    );
    assert!(
        step_of(&done, "verify_health")
            .output
            .contains("is healthy")
    );
    // The grant is a timed one.
    let identity = h.identity("gitops").await;
    let write = identity
        .grants
        .iter()
        .find(|g| g.permission == "cluster:write")
        .expect("granted");
    assert!(write.expires_at.is_some());
    let events = h
        .svc
        .list_events(admin(ListEventsRequest {
            execution_id: created.id.clone(),
            after_seq: 0,
            limit: 500,
        }))
        .await
        .unwrap()
        .into_inner()
        .events;
    assert!(
        events
            .iter()
            .any(|e| e.r#type == EventType::StepStarted as i32
                && e.step_key == "sync"
                && e.data.get("runner").map(String::as_str) == Some("gitops"))
    );
    h.teardown().await;
}

/// The service stops right after the engine synced and before the step
/// was checkpointed. After the restart the step's outcome is unknown;
/// running it again finds this execution's sync in the engine's history
/// and syncs nothing twice.
#[tokio::test]
async fn a_restart_in_the_middle_of_a_sync_does_not_sync_twice() {
    let Some(mut h) = Harness::new().await else {
        return;
    };
    h.grant("gitops", &["cluster:write"]).await;
    let created = h.create("Sync api", sync_plan()).await;
    h.start(&created.id).await;
    let approval = h.pending_approval(&created.id).await;

    // What the crash leaves behind: the approval was given, the step was
    // running, the engine did sync, and nothing of that outcome was stored.
    let sync = &sync_plan()[1];
    let execution = Uuid::parse_str(&created.id).unwrap();
    let lost = GitopsEngineAdapter::new(&h.gitops_url)
        .unwrap()
        .run(
            &StepInput {
                key: sync.key.clone(),
                title: sync.title.clone(),
                action_type: sync.action_type.clone(),
                adapter: "gitops".into(),
                params: sync.params.clone().into_iter().collect(),
                attempt: 1,
            },
            &RunContext {
                execution_id: execution,
                actor: Some(Principal {
                    user_id: "alice".into(),
                    workspace_id: WS.into(),
                    role: Role::Approver,
                }),
                ..Default::default()
            },
        )
        .await;
    assert!(matches!(lost, StepOutcome::Succeeded { .. }), "{lost:?}");
    assert_eq!(h.gitops.applied().len(), 1);
    for statement in [
        "UPDATE approvals SET status = 'APPROVED', decided_by = 'alice', decided_at = now() WHERE execution_id = $1",
        "UPDATE steps SET status = 'RUNNING', attempts = 1 WHERE execution_id = $1 AND key = 'sync'",
        "UPDATE executions SET status = 'EXECUTING' WHERE id = $1",
    ] {
        sqlx::query(statement)
            .bind(execution)
            .execute(&h.pool)
            .await
            .unwrap();
    }
    let _ = approval;

    h.restarted();
    assert_eq!(h.rt.recover().await.unwrap(), 1);
    let failure = h.diagnosed(&created.id, "sync").await;
    assert_eq!(failure.code, "UNKNOWN");
    assert!(failure.diagnosis.contains("service restarted mid-step"));
    assert!(option_ids(&failure).contains(&"retry"));
    assert!(!option_ids(&failure).contains(&"skip_step"));

    // The retry is gated like any run of the step, then finds the sync done.
    h.resolve(&created.id, "retry").await;
    let approval = h.pending_approval(&created.id).await;
    h.approve_as("alice", &approval).await;
    let done = h.wait_status(&created.id, ExecutionStatus::Succeeded).await;
    let output = &step_of(&done, "sync").output;
    assert!(output.contains("already synced"), "{output}");
    assert!(output.contains("nothing was synced again"), "{output}");
    assert_eq!(h.gitops.applied().len(), 1, "the engine synced once");
    assert_eq!(
        done.outputs["history_id"],
        h.gitops.history(fake::APP)[0].id
    );
    h.teardown().await;
}

/// A sync that ran and failed pauses the run with SYNC_FAILED, names the
/// resources, offers retry, give up and cancel and never "skip"; a source
/// that does not render is RENDER_FAILED; the retry syncs.
#[tokio::test]
async fn a_failed_sync_is_diagnosed_and_retried_but_never_skipped() {
    let Some(h) = Harness::new().await else {
        return;
    };
    h.grant("gitops", &["cluster:write"]).await;
    h.set_policy("gitops.sync", PolicyDecision::Auto).await;
    h.gitops.script(Scripted::Result(SyncResult {
        phase: "failed".into(),
        message: "one or more objects failed to apply".into(),
        revision: fake::REVISION.into(),
        resources: vec![ResourceResult {
            kind: "Deployment".into(),
            group: "apps".into(),
            namespace: "shop".into(),
            name: "api".into(),
            status: "failed".into(),
            message: "admission webhook denied the request".into(),
            ..Default::default()
        }],
        ..Default::default()
    }));
    h.gitops.script(Scripted::Result(SyncResult {
        phase: "error".into(),
        message: "rendering apps/api: kustomize build: accumulating resources: missing file".into(),
        ..Default::default()
    }));
    // Only the sync: nothing depends on it, and skipping it would still be
    // a run that changed nothing reading as a success.
    let created = h
        .create(
            "Sync api",
            vec![plan_step(
                "sync",
                "gitops.sync",
                &["cluster:write"],
                &[],
                &[("application", fake::APP)],
            )],
        )
        .await;
    h.start(&created.id).await;

    let failure = h.diagnosed(&created.id, "sync").await;
    assert_eq!(failure.code, "SYNC_FAILED");
    assert_eq!(option_ids(&failure), ["retry", "give_up", "cancel"]);
    assert!(
        failure
            .diagnosis
            .contains("Deployment shop/api (admission webhook denied the request)"),
        "{}",
        failure.diagnosis
    );
    h.resolve(&created.id, "retry").await;
    let failure = h.diagnosed(&created.id, "sync").await;
    assert_eq!(failure.code, "RENDER_FAILED");
    assert_eq!(option_ids(&failure), ["retry", "give_up", "cancel"]);
    assert!(
        failure
            .diagnosis
            .contains("Nothing was applied to the cluster")
    );
    assert!(failure.diagnosis.contains("kustomize build"));
    let refused = h
        .svc
        .resolve_failure(approver(ResolveFailureRequest {
            execution_id: created.id.clone(),
            option_id: "skip_step".into(),
            instruction: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);

    h.resolve(&created.id, "retry").await;
    let done = h.wait_status(&created.id, ExecutionStatus::Succeeded).await;
    assert_eq!(done.outputs["health"], "healthy");
    assert_eq!(h.gitops.applied().len(), 3);
    h.teardown().await;
}

/// CONTRACT.md §24.7 in continuous delivery: an `sdi_gitops` environment
/// is validated when it is saved; promoting runs verify image → sync with
/// the release image as the override the source takes → health → record;
/// a degraded application offers the rollback, which syncs the previous
/// release's image the same way; a helm source gets image.repository and
/// image.tag.
#[tokio::test]
async fn an_sdi_gitops_environment_is_promoted_degrades_and_is_rolled_back() {
    let Some(h) = Harness::new().await else {
        return;
    };
    h.fake_registry_as_default().await;
    let environment = |name: &str, position: u32, config: &[(&str, &str)]| Environment {
        project_id: PROJECT.into(),
        name: name.into(),
        position,
        kind: "sdi_gitops".into(),
        config: config
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<HashMap<_, _>>(),
        ..Default::default()
    };

    // Saved only with an application id and the keys of the kind.
    for (config, says) in [
        (vec![], "config.application is required"),
        (
            vec![("application", "api")],
            "config.application must be the id",
        ),
        (
            vec![("application", fake::APP), ("path", "apps/api")],
            "config.path is not a setting of sdi_gitops environments",
        ),
    ] {
        let error = h
            .upsert(environment("staging", 0, &config))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument);
        assert!(error.message().contains(says), "{}", error.message());
    }
    let staging = h
        .upsert(environment(
            "staging",
            0,
            &[("application", fake::APP), ("health_timeout_seconds", "10")],
        ))
        .await
        .expect("upsert staging");
    assert_eq!(staging.kind, "sdi_gitops");
    assert_eq!(staging.config["application"], fake::APP);
    let web = h
        .upsert(environment("web", 1, &[("application", fake::HELM_APP)]))
        .await
        .expect("upsert web");

    // The delivery identity reads applications; syncing needs grants.
    let delivery = h.identity("delivery").await;
    assert!(
        delivery
            .grants
            .iter()
            .any(|g| g.permission == "cluster:read" && g.expires_at.is_none())
    );
    h.grant(
        "delivery",
        &["deploy:development", "deploy:staging", "cluster:write"],
    )
    .await;

    // --- promote the first release ---------------------------------------------
    let first = h.release(COMMIT_1).await;
    h.registry.publish("acme/api", &first.tag, DIGEST_1);
    // The first stage is deploy.development: medium risk, automatic, as
    // for every environment kind.
    let run = h.promote(&staging, &first).await;
    h.wait_run(&run.id, DeploymentStatus::Succeeded).await;
    let execution = h.get(&run.execution_id).await;
    assert_eq!(
        execution
            .steps
            .iter()
            .map(|s| (s.key.as_str(), s.action_type.as_str(), s.adapter.as_str()))
            .collect::<Vec<_>>(),
        [
            ("verify_image", "registry.verify_image", "registry"),
            ("deploy", "deploy.development", "gitops"),
            ("verify_health", "gitops.verify_health", "gitops"),
            ("record", "deploy.record", "github"),
        ]
    );
    let deploy = step_of(&execution, "deploy");
    assert_eq!(deploy.params["operation"], "gitops.sync");
    assert_eq!(
        deploy.required_permissions,
        ["deploy:development", "cluster:write"]
    );
    assert_eq!(deploy.policy_decision, PolicyDecision::Auto as i32);
    let first_image = format!("{}:{}", first.image, first.tag);
    let applied = h.gitops.applied();
    assert_eq!(applied.len(), 1);
    // Nobody approved it: it ran as the approver who promoted.
    assert_eq!(applied[0].0.user_id, "user-approver");
    assert_eq!(
        applied[0].1.kustomize_images,
        std::slice::from_ref(&first_image)
    );
    assert!(applied[0].1.helm_parameters.is_empty());
    assert_eq!(applied[0].1.execution_id, run.execution_id);
    assert!(!applied[0].1.prune);
    assert_eq!(execution.outputs["digest"], DIGEST_1);
    assert_eq!(execution.outputs["health"], "healthy");
    assert_eq!(h.environment("staging").await.current_release_id, first.id);
    assert_eq!(h.github.deployments.lock().unwrap().len(), 1);

    // --- the second release degrades the application -----------------------------
    let second = h.release(COMMIT_2).await;
    h.registry.publish("acme/api", &second.tag, DIGEST_2);
    // The sync step reads the application once, then health is polled.
    h.gitops
        .health_sequence(&["healthy", "progressing", "degraded", "degraded"]);
    let run = h.promote(&staging, &second).await;
    let failure = {
        let execution = h
            .wait_status(&run.execution_id, ExecutionStatus::Diagnosing)
            .await;
        execution.failure.expect("diagnosed")
    };
    assert_eq!(failure.step_key, "verify_health");
    assert_eq!(failure.code, "HEALTH_CHECK_FAILED");
    assert_eq!(
        option_ids(&failure)[0],
        "rollback_to_previous",
        "{:?}",
        failure.options
    );
    assert!(failure.diagnosis.contains("did not become healthy"));
    assert_eq!(
        h.gitops.applied()[1].1.kustomize_images,
        [format!("{}:{}", second.image, second.tag)]
    );

    // The rollback is a run of its own: the same sync with the first image.
    h.gitops.health_sequence(&["healthy"]);
    h.resolve(&run.execution_id, "rollback_to_previous").await;
    let rollback = {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let runs = h
                .svc
                .list_deployment_runs(admin(ListDeploymentRunsRequest {
                    project_id: PROJECT.into(),
                    environment_id: staging.id.clone(),
                    ..Default::default()
                }))
                .await
                .unwrap()
                .into_inner()
                .runs;
            if let Some(run) = runs.into_iter().find(|r| r.kind == "rollback") {
                break run;
            }
            assert!(tokio::time::Instant::now() < deadline, "no rollback run");
            tokio::time::sleep(POLL).await;
        }
    };
    assert_eq!(rollback.release_id, first.id);
    // deploy.rollback: high risk, a person decides.
    let approval = h.pending_approval(&rollback.execution_id).await;
    assert_eq!(approval.action_type, "deploy.rollback");
    assert_eq!(h.gitops.applied().len(), 2, "nothing before the approval");
    h.approve_as("alice", &approval).await;
    h.wait_run(&rollback.id, DeploymentStatus::Succeeded).await;
    let applied = h.gitops.applied();
    assert_eq!(applied.len(), 3);
    assert_eq!(
        applied[2].1.kustomize_images,
        std::slice::from_ref(&first_image)
    );
    assert_eq!(applied[2].1.execution_id, rollback.execution_id);
    assert_eq!(
        h.gitops.app(fake::APP).source.unwrap().kustomize_images,
        [first_image]
    );
    let execution = h.get(&rollback.execution_id).await;
    assert!(
        step_of(&execution, "deploy")
            .title
            .starts_with("Roll back staging to")
    );
    assert_eq!(h.environment("staging").await.current_release_id, first.id);

    // --- a helm source ----------------------------------------------------------
    let run = h.promote(&web, &second).await;
    let approval = h.pending_approval(&run.execution_id).await;
    assert_eq!(approval.action_type, "deploy.staging");
    h.approve_as("alice", &approval).await;
    h.wait_run(&run.id, DeploymentStatus::Succeeded).await;
    let applied = h.gitops.applied();
    let (principal, request) = applied.last().unwrap();
    assert_eq!(principal.user_id, "alice");
    assert_eq!(request.id, fake::HELM_APP);
    assert!(request.kustomize_images.is_empty());
    assert_eq!(request.helm_parameters["image.repository"], second.image);
    assert_eq!(request.helm_parameters["image.tag"], second.tag);
    h.teardown().await;
}

/// The rows of §24.7 reach workspaces that existed before it: the
/// migration adds the four policies, the `gitops` identity and
/// `cluster:read` where they are missing, and leaves an edited policy as
/// it was. The bootstrap workspace got them from the migrations already.
#[tokio::test]
async fn the_gitops_policies_and_identity_reach_existing_workspaces() {
    let Some(h) = Harness::new().await else {
        return;
    };
    // First use seeds the workspace; then it is made to look older.
    h.set_policy("gitops.sync", PolicyDecision::Required).await;
    let workspace = Uuid::parse_str(WS).unwrap();
    for statement in [
        "DELETE FROM policies WHERE workspace_id = $1 AND action_type IN ('gitops.preview', 'gitops.rollback', 'gitops.verify_health')",
        "DELETE FROM grants WHERE workspace_id = $1 AND permission = 'cluster:read'",
        "DELETE FROM identities WHERE workspace_id = $1 AND name = 'gitops'",
    ] {
        sqlx::query(statement)
            .bind(workspace)
            .execute(&h.pool)
            .await
            .unwrap();
    }
    let identities = h
        .svc
        .list_identities(admin(ListIdentitiesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .identities;
    assert!(!identities.iter().any(|i| i.name == "gitops"));

    let migration = include_str!("../../migrations/0007_gitops.sql");
    sqlx::raw_sql(sqlx::AssertSqlSafe(migration))
        .execute(&h.pool)
        .await
        .unwrap();
    let policies = h
        .svc
        .list_policies(admin(ListPoliciesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .policies;
    assert_eq!(policies.len(), crate::db::policies::SEED.len());
    let decision = |action_type: &str| {
        policies
            .iter()
            .find(|p| p.action_type == action_type)
            .map(|p| p.decision)
            .unwrap_or_else(|| panic!("no policy for {action_type}"))
    };
    assert_eq!(decision("gitops.preview"), PolicyDecision::Auto as i32);
    assert_eq!(decision("gitops.rollback"), PolicyDecision::Approval as i32);
    assert_eq!(
        decision("gitops.verify_health"),
        PolicyDecision::Auto as i32
    );
    assert_eq!(
        decision("gitops.sync"),
        PolicyDecision::Required as i32,
        "the workspace's own decision stays"
    );
    for name in ["gitops", "delivery"] {
        let identity = h.identity(name).await;
        assert!(
            identity
                .grants
                .iter()
                .any(|g| g.permission == "cluster:read" && g.expires_at.is_none()),
            "{name}: {:?}",
            identity.grants
        );
        assert!(
            !identity
                .grants
                .iter()
                .any(|g| g.permission == "cluster:write"),
            "{name}"
        );
    }

    // The bootstrap workspace: the policies and the identity, from the
    // migrations alone.
    let (policies, identities): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM policies WHERE workspace_id = '00000000-0000-7000-8000-00000000dead' AND action_type LIKE 'gitops.%'), \
                (SELECT count(*) FROM identities i JOIN grants g ON g.identity_id = i.id \
                  WHERE i.workspace_id = '00000000-0000-7000-8000-00000000dead' AND i.name = 'gitops' AND g.permission = 'cluster:read')",
    )
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert_eq!((policies, identities), (4, 1));
    h.teardown().await;
}

/// The tier gates of continuous delivery hold for `sdi_gitops`: a protected
/// stage deploys as `deploy.production`, required and not editable, so
/// setting `gitops.sync` (standalone application syncs) to auto never takes
/// the person out of a production deploy. The knob still does what it says
/// where it belongs.
#[tokio::test]
async fn a_protected_sdi_gitops_stage_waits_for_a_person_whatever_gitops_sync_says() {
    let Some(h) = Harness::new().await else {
        return;
    };
    h.fake_registry_as_default().await;
    let production = h
        .upsert(Environment {
            project_id: PROJECT.into(),
            name: "production".into(),
            position: 0,
            kind: "sdi_gitops".into(),
            config: HashMap::from([("application".to_owned(), fake::APP.to_owned())]),
            protected: true,
            ..Default::default()
        })
        .await
        .expect("upsert production");
    h.set_policy("gitops.sync", PolicyDecision::Auto).await;
    let locked = h
        .svc
        .update_policy(admin(UpdatePolicyRequest {
            action_type: "deploy.production".into(),
            decision: PolicyDecision::Auto as i32,
        }))
        .await
        .unwrap_err();
    assert_eq!(locked.code(), Code::FailedPrecondition);
    h.grant("delivery", &["deploy:production", "cluster:write"])
        .await;

    let release = h.release(COMMIT_1).await;
    h.registry.publish("acme/api", &release.tag, DIGEST_1);
    let run = h.promote(&production, &release).await;
    let approval = h.pending_approval(&run.execution_id).await;
    assert_eq!(approval.action_type, "deploy.production");
    assert_eq!(approval.step_key, "deploy");
    let waiting = h.get(&run.execution_id).await;
    let deploy = step_of(&waiting, "deploy");
    assert_eq!(deploy.action_type, "deploy.production");
    assert_eq!(deploy.params["operation"], "gitops.sync");
    assert_eq!(deploy.policy_decision, PolicyDecision::Required as i32);
    assert_eq!(deploy.risk, RiskLevel::High as i32);
    assert_eq!(
        deploy.required_permissions,
        ["deploy:production", "cluster:write"]
    );
    assert_eq!(deploy.status, StepStatus::WaitingApproval as i32);
    assert!(h.gitops.applied().is_empty(), "nothing before the person");

    h.approve_as("alice", &approval).await;
    h.wait_run(&run.id, DeploymentStatus::Succeeded).await;
    let applied = h.gitops.applied();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].0.user_id, "alice");
    assert_eq!(
        applied[0].1.kustomize_images,
        [format!("{}:{}", release.image, release.tag)]
    );

    // A standalone sync of the application, on the gitops runtime: the
    // relaxed policy lets it run without asking.
    h.grant("gitops", &["cluster:write"]).await;
    let created = h
        .create(
            "Sync api",
            vec![plan_step(
                "sync",
                "gitops.sync",
                &["cluster:write"],
                &[],
                &[("application", fake::APP)],
            )],
        )
        .await;
    assert_eq!(
        step_of(&created, "sync").policy_decision,
        PolicyDecision::Auto as i32
    );
    h.start(&created.id).await;
    h.wait_status(&created.id, ExecutionStatus::Succeeded).await;
    assert_eq!(h.gitops.applied().len(), 2);
    h.teardown().await;
}
