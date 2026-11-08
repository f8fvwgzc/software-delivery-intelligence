use std::{collections::HashMap, sync::Arc, time::Duration};

use sdi_common::principal::{Principal, Role};
use sdi_proto::{
    common::v1::{PlanStep, WatchEventsRequest},
    execution::v1::{
        Approval, ApprovalStatus, CancelExecutionRequest, CreateExecutionRequest,
        DecideApprovalRequest, DeleteWorkspaceDataRequest, EventType, Execution, ExecutionStatus,
        ExportWorkspaceDataRequest, FaultInjection, GetDeliveryStatsRequest, GetExecutionRequest,
        GrantPermissionRequest, ListApprovalsRequest, ListEventsRequest, ListExecutionsRequest,
        ListIdentitiesRequest, ListPoliciesRequest, PolicyDecision, ResolveFailureRequest,
        StartExecutionRequest, StepStatus, UpdatePolicyRequest, WatchExecutionRequest,
        execution_service_server::ExecutionService,
    },
};
use sqlx::PgPool;
use tokio_stream::StreamExt;
use tonic::{Code, Request};
use uuid::Uuid;

use crate::{
    adapters::{self, Adapter, Registry, RunContext, StepInput, StepOutcome},
    grpc::ExecutionGrpc,
    identity::RecordingAudit,
    knowledge::RecordingPatternOutcomes,
    runner::Runtime,
};

const WAIT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(10);

/// The two workspaces the tests play in. Every call carries one of them as
/// metadata, the way the gateway forwards it (CONTRACT.md §18).
const WS_A: &str = "00000000-0000-7000-8000-00000000000a";
const WS_B: &str = "00000000-0000-7000-8000-00000000000b";

fn principal(role: Role, workspace: &str) -> Principal {
    Principal {
        user_id: format!("user-{}", role.as_str()),
        workspace_id: workspace.to_owned(),
        role,
    }
}

fn as_role<T>(role: Role, workspace: &str, message: T) -> Request<T> {
    let mut request = Request::new(message);
    principal(role, workspace).apply(&mut request);
    request
}

/// The default caller: an admin of workspace A.
fn req<T>(message: T) -> Request<T> {
    as_role(Role::Admin, WS_A, message)
}

struct Harness {
    pool: PgPool,
    schema: String,
    rt: Arc<Runtime>,
    svc: ExecutionGrpc,
    outcomes: RecordingPatternOutcomes,
    audit: RecordingAudit,
}

impl Harness {
    /// None (with a note on stderr) when no database is configured.
    async fn new() -> Option<Self> {
        Self::with_registry(adapters::test_registry()).await
    }

    async fn with_registry(registry: Registry) -> Option<Self> {
        let Some(url) = std::env::var("DATABASE_URL")
            .ok()
            .filter(|u| !u.trim().is_empty())
        else {
            eprintln!("skipping: DATABASE_URL is not set");
            return None;
        };
        // Schema names must be lowercase letters only (sdi_common::db::connect).
        let suffix: String = Uuid::now_v7()
            .as_bytes()
            .iter()
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

        let outcomes = RecordingPatternOutcomes::default();
        let audit = RecordingAudit::default();
        let rt = Runtime::new(
            pool.clone(),
            registry,
            Arc::new(outcomes.clone()),
            Arc::new(audit.clone()),
            Duration::ZERO,
            false,
            Arc::new(crate::identity::NoGithubAccess),
        );
        Some(Self {
            pool,
            schema,
            svc: ExecutionGrpc::new(Arc::clone(&rt)),
            rt,
            outcomes,
            audit,
        })
    }

    async fn teardown(self) {
        drop(self.svc);
        drop(self.rt);
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.pool)
        .await
        .expect("drop test schema");
    }

    async fn get(&self, id: &str) -> Execution {
        self.get_as(Role::Admin, WS_A, id).await
    }

    async fn get_as(&self, role: Role, workspace: &str, id: &str) -> Execution {
        self.svc
            .get_execution(as_role(
                role,
                workspace,
                GetExecutionRequest { id: id.to_owned() },
            ))
            .await
            .expect("get execution")
            .into_inner()
    }

    async fn wait_status(&self, id: &str, status: ExecutionStatus) -> Execution {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let exec = self.get(id).await;
            if exec.status == status as i32 {
                return exec;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {status:?}; execution is {:?}",
                ExecutionStatus::try_from(exec.status)
            );
            tokio::time::sleep(POLL).await;
        }
    }

    async fn wait_pending_approval(&self, execution_id: &str) -> Approval {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let approvals = self
                .svc
                .list_approvals(req(ListApprovalsRequest {
                    status: ApprovalStatus::Pending as i32,
                    execution_id: execution_id.to_owned(),
                }))
                .await
                .expect("list approvals")
                .into_inner()
                .approvals;
            if let Some(approval) = approvals.into_iter().next() {
                return approval;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for an approval"
            );
            tokio::time::sleep(POLL).await;
        }
    }

    async fn approve(&self, approval: &Approval) -> Approval {
        self.svc
            .decide_approval(req(DecideApprovalRequest {
                id: approval.id.clone(),
                approve: true,
                note: "looks good".into(),
                decided_by: "ops@example.test".into(),
            }))
            .await
            .expect("decide approval")
            .into_inner()
    }

    async fn resolve_recommended(&self, id: &str) -> Execution {
        let exec = self.get(id).await;
        let failure = exec.failure.expect("failure present while diagnosing");
        let option = failure
            .options
            .iter()
            .find(|o| o.recommended)
            .expect("a recommended option");
        self.svc
            .resolve_failure(req(ResolveFailureRequest {
                execution_id: id.to_owned(),
                option_id: option.id.clone(),
                instruction: String::new(),
            }))
            .await
            .expect("resolve failure")
            .into_inner()
    }
}

fn step(
    key: &str,
    action_type: &str,
    adapter: &str,
    depends_on: &[&str],
    permissions: &[&str],
) -> PlanStep {
    PlanStep {
        key: key.into(),
        title: format!("Step {key}"),
        description: String::new(),
        action_type: action_type.into(),
        adapter: adapter.into(),
        depends_on: depends_on.iter().map(|s| s.to_string()).collect(),
        required_permissions: permissions.iter().map(|s| s.to_string()).collect(),
        params: HashMap::from([("region".to_string(), "westeurope".to_string())]),
        verification: format!("{key} reports healthy"),
    }
}

/// read -> run(approval) -> env(fault) -> test -> deploy(permission + required approval).
fn five_step_plan() -> Vec<PlanStep> {
    vec![
        step("read", "repo.read", "github", &[], &["repository:read"]),
        step("run", "container.run", "docker", &["read"], &[]),
        step(
            "env",
            "env.create_temporary",
            "cloud",
            &["run"],
            &["environment:write"],
        ),
        step("test", "tests.run_local", "local", &["env"], &[]),
        step(
            "deploy",
            "deploy.production",
            "kubernetes",
            &["test"],
            &["deploy:production"],
        ),
    ]
}

fn status_of(step: &sdi_proto::execution::v1::Step) -> StepStatus {
    StepStatus::try_from(step.status).unwrap_or_default()
}

#[tokio::test]
async fn full_flight_with_approvals_fault_and_replan() {
    let Some(h) = Harness::new().await else {
        return;
    };

    let created = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "proj-1".into(),
            proposal_id: "prop-1".into(),
            pattern_id: "pattern-aws".into(),
            title: "Ship it".into(),
            steps: five_step_plan(),
            ..Default::default()
        }))
        .await
        .expect("create")
        .into_inner();
    assert_eq!(created.status, ExecutionStatus::Planning as i32);
    assert!(created.number >= 1);
    assert_eq!(created.adapter, "simulated");
    assert_eq!(created.steps.len(), 5);
    let deploy = &created.steps[4];
    assert_eq!(deploy.policy_decision, PolicyDecision::Required as i32);
    assert_eq!(
        created.steps[1].policy_decision,
        PolicyDecision::Approval as i32
    );
    assert_eq!(
        created.steps[0].policy_decision,
        PolicyDecision::Auto as i32
    );
    let id = created.id.clone();

    // Resolving before anything failed is a precondition error.
    let err = h
        .svc
        .resolve_failure(req(ResolveFailureRequest {
            execution_id: id.clone(),
            option_id: "retry".into(),
            instruction: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);

    // Watch from the beginning before starting so the replay/live seam is covered.
    let stream = h
        .svc
        .watch_execution(req(WatchExecutionRequest {
            execution_id: id.clone(),
            after_seq: 0,
        }))
        .await
        .expect("watch")
        .into_inner();
    let collector = tokio::spawn(async move {
        let mut stream = stream;
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event.expect("event"));
        }
        events
    });

    let started = h
        .svc
        .start_execution(req(StartExecutionRequest {
            id: id.clone(),
            fault: Some(FaultInjection {
                step_key: "env".into(),
                code: "TIMEOUT".into(),
            }),
        }))
        .await
        .expect("start")
        .into_inner();
    // The runner is already on the first step by the time the reply lands,
    // so any post-PLANNING status is acceptable here.
    assert_ne!(started.status, ExecutionStatus::Planning as i32);
    assert!(started.started_at.is_some());
    let twice = h
        .svc
        .start_execution(req(StartExecutionRequest {
            id: id.clone(),
            fault: None,
        }))
        .await
        .unwrap_err();
    assert_eq!(twice.code(), Code::FailedPrecondition);

    // Step 2 needs a human.
    let approval = h.wait_pending_approval(&id).await;
    assert_eq!(approval.step_key, "run");
    assert_eq!(approval.policy_decision, PolicyDecision::Approval as i32);
    let waiting = h
        .wait_status(&id, ExecutionStatus::WaitingForApproval)
        .await;
    assert_eq!(status_of(&waiting.steps[1]), StepStatus::WaitingApproval);
    assert_eq!(waiting.steps[1].approval_id, approval.id);
    let decided = h.approve(&approval).await;
    assert_eq!(decided.status, ApprovalStatus::Approved as i32);
    let again = h
        .svc
        .decide_approval(req(DecideApprovalRequest {
            id: approval.id.clone(),
            approve: false,
            note: String::new(),
            decided_by: String::new(),
        }))
        .await;
    assert_eq!(again.unwrap_err().code(), Code::FailedPrecondition);

    // Step 3 hits the injected fault: paused, diagnosed, dependents blocked.
    let diagnosing = h.wait_status(&id, ExecutionStatus::Diagnosing).await;
    let failure = diagnosing.failure.clone().expect("failure");
    assert_eq!(failure.step_key, "env");
    assert_eq!(failure.code, "TIMEOUT");
    assert_eq!(
        failure.blocked_steps,
        vec!["test".to_string(), "deploy".to_string()]
    );
    assert!(failure.raw_output.contains("timed out"));
    assert!(!failure.diagnosis.is_empty());
    assert_eq!(failure.options.iter().filter(|o| o.recommended).count(), 1);
    assert_eq!(
        failure.options.iter().find(|o| o.recommended).unwrap().id,
        "retry"
    );
    assert!(!failure.options.iter().any(|o| o.id == "skip_step"));
    assert_eq!(status_of(&diagnosing.steps[2]), StepStatus::Failed);
    assert_eq!(status_of(&diagnosing.steps[3]), StepStatus::Blocked);
    assert_eq!(status_of(&diagnosing.steps[4]), StepStatus::Blocked);
    assert_eq!(diagnosing.last_checkpoint, "run");
    assert!(diagnosing.steps[0].checkpointed && diagnosing.steps[1].checkpointed);

    let bad = h
        .svc
        .resolve_failure(req(ResolveFailureRequest {
            execution_id: id.clone(),
            option_id: "teleport".into(),
            instruction: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(bad.code(), Code::InvalidArgument);

    let replanned = h.resolve_recommended(&id).await;
    assert_eq!(replanned.status, ExecutionStatus::Authorized as i32);
    assert_eq!(replanned.revision, 1);
    assert!(replanned.failure.is_none());

    // Step 5 is denied before it runs; request_access grants and retries.
    let denied = h.wait_status(&id, ExecutionStatus::Diagnosing).await;
    let failure = denied.failure.clone().expect("failure");
    assert_eq!(failure.step_key, "deploy");
    assert_eq!(failure.code, "PERMISSION_DENIED");
    assert_eq!(
        failure.missing_permissions,
        vec!["deploy:production".to_string()]
    );
    assert!(failure.blocked_steps.is_empty());
    assert!(failure.options.iter().any(|o| o.id == "skip_step"));
    assert_eq!(
        failure.options.iter().find(|o| o.recommended).unwrap().id,
        "request_access"
    );
    assert_eq!(status_of(&denied.steps[3]), StepStatus::Succeeded);
    assert_eq!(denied.steps[2].attempts, 2);

    let replanned = h.resolve_recommended(&id).await;
    assert_eq!(replanned.revision, 2);

    let identities = h
        .svc
        .list_identities(req(ListIdentitiesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .identities;
    let simulated = identities
        .iter()
        .find(|i| i.name == "simulated")
        .expect("simulated identity");
    let grant = simulated
        .grants
        .iter()
        .find(|g| g.permission == "deploy:production")
        .expect("timed grant");
    assert!(grant.expires_at.is_some());

    // deploy.production is REQUIRED: a second approval, then success.
    let approval = h.wait_pending_approval(&id).await;
    assert_eq!(approval.step_key, "deploy");
    assert_eq!(approval.policy_decision, PolicyDecision::Required as i32);
    h.approve(&approval).await;

    let done = h.wait_status(&id, ExecutionStatus::Succeeded).await;
    assert!(done.steps.iter().all(|s| s.checkpointed));
    assert_eq!(done.last_checkpoint, "deploy");
    assert!(done.finished_at.is_some());
    assert_eq!(done.revision, 2);
    assert!(done.steps[1].output.contains("Health check passed"));

    let events = tokio::time::timeout(WAIT, collector)
        .await
        .expect("watch ended")
        .expect("collector");
    let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(
        seqs,
        (1..=seqs.len() as u64).collect::<Vec<_>>(),
        "seq must be contiguous"
    );
    let kinds: Vec<EventType> = events
        .iter()
        .map(|e| EventType::try_from(e.r#type).unwrap())
        .collect();

    use EventType as T;
    let auto_step = [
        T::PermissionChecked,
        T::PolicyChecked,
        T::StepStarted,
        T::Log,
        T::StepSucceeded,
        T::Checkpoint,
    ];
    let approved_step = [
        T::PermissionChecked,
        T::PolicyChecked,
        T::ApprovalRequested,
        T::ApprovalDecided,
        T::StepStarted,
        T::Log,
        T::StepSucceeded,
        T::Checkpoint,
    ];
    let mut expected = vec![T::StateChanged, T::StateChanged];
    expected.extend(auto_step); // read
    expected.extend(approved_step); // run
    expected.extend([
        T::PermissionChecked,
        T::PolicyChecked,
        T::StepStarted,
        T::StepFailed,
        T::StepBlocked,
        T::StepBlocked,
        T::StateChanged,
        T::Diagnosis,
        T::StateChanged,
        T::Replanned,
    ]); // env fails, replanned
    expected.extend(auto_step); // env retry
    expected.extend(auto_step); // test
    expected.extend([
        T::PermissionDenied,
        T::StepFailed,
        T::StateChanged,
        T::Diagnosis,
        T::StateChanged,
        T::Replanned,
    ]); // deploy denied, access granted
    expected.extend(approved_step); // deploy
    expected.push(T::StateChanged); // succeeded
    assert_eq!(kinds, expected);
    let last = events.last().unwrap();
    assert_eq!(last.status, ExecutionStatus::Succeeded as i32);

    let listed = h
        .svc
        .list_events(req(ListEventsRequest {
            execution_id: id.clone(),
            after_seq: 50,
            limit: 0,
        }))
        .await
        .unwrap()
        .into_inner()
        .events;
    assert_eq!(listed.len(), events.len() - 50);

    // A watch opened after the end replays and closes on its own.
    let mut late = h
        .svc
        .watch_execution(req(WatchExecutionRequest {
            execution_id: id.clone(),
            after_seq: events.len() as u64 - 2,
        }))
        .await
        .unwrap()
        .into_inner();
    let mut tail = Vec::new();
    while let Some(event) = tokio::time::timeout(WAIT, late.next())
        .await
        .expect("late watch ended")
    {
        tail.push(event.unwrap().seq);
    }
    assert_eq!(tail, vec![events.len() as u64 - 1, events.len() as u64]);

    assert_eq!(
        h.outcomes.calls.lock().unwrap().as_slice(),
        &[("pattern-aws".to_string(), "proj-1".to_string(), true)]
    );

    let stats = h
        .svc
        .get_delivery_stats(req(GetDeliveryStatsRequest {
            project_id: "proj-1".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(stats.executions_total, 1);
    assert_eq!(stats.executions_succeeded, 1);
    assert_eq!(stats.success_rate, 1.0);
    assert_eq!(stats.failures_recovered, 2);
    // Adapter attempts only: read, run, env x2, test, deploy. The denied
    // deploy attempt never reached the adapter.
    assert_eq!(stats.steps_executed, 6);
    assert_eq!(stats.approvals_pending, 0);

    let listing = h
        .svc
        .list_executions(req(ListExecutionsRequest {
            project_id: String::new(),
            limit: 0,
            before: None,
        }))
        .await
        .unwrap()
        .into_inner()
        .executions;
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].steps.len(), 5);
    assert!(listing[0].failure.is_none());
    // The `before` cursor excludes rows updated at or after it.
    let cut = listing[0].updated_at;
    let paged = h
        .svc
        .list_executions(req(ListExecutionsRequest {
            project_id: String::new(),
            limit: 0,
            before: cut,
        }))
        .await
        .unwrap()
        .into_inner()
        .executions;
    assert!(paged.is_empty());

    h.teardown().await;
}

#[tokio::test]
async fn invalid_plans_are_rejected_and_cancel_works_from_planning() {
    let Some(h) = Harness::new().await else {
        return;
    };

    let cyclic = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "p".into(),
            proposal_id: String::new(),
            pattern_id: String::new(),
            title: "cycle".into(),
            steps: vec![
                step("a", "repo.read", "local", &["b"], &[]),
                step("b", "repo.read", "local", &["a"], &[]),
            ],
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(cyclic.code(), Code::InvalidArgument);

    let unknown = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "p".into(),
            proposal_id: String::new(),
            pattern_id: String::new(),
            title: "ghost".into(),
            steps: vec![step("a", "repo.read", "local", &["ghost"], &[])],
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), Code::InvalidArgument);

    let exec = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "p".into(),
            proposal_id: String::new(),
            pattern_id: String::new(),
            title: String::new(),
            steps: vec![step("a", "something.new", "local", &[], &[])],
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(exec.title, "Untitled execution");
    assert_eq!(
        exec.steps[0].policy_decision,
        PolicyDecision::Required as i32
    );

    let cancelled = h
        .svc
        .cancel_execution(req(CancelExecutionRequest {
            id: exec.id.clone(),
            reason: "changed my mind".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(cancelled.status, ExecutionStatus::Cancelled as i32);
    assert_eq!(status_of(&cancelled.steps[0]), StepStatus::Cancelled);
    let twice = h
        .svc
        .cancel_execution(req(CancelExecutionRequest {
            id: exec.id.clone(),
            reason: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(twice.code(), Code::FailedPrecondition);

    let events = h
        .svc
        .list_events(req(ListEventsRequest {
            execution_id: exec.id.clone(),
            after_seq: 0,
            limit: 0,
        }))
        .await
        .unwrap()
        .into_inner()
        .events;
    assert_eq!(events.len(), 2);
    assert!(events[1].message.contains("changed my mind"));

    h.teardown().await;
}

#[tokio::test]
async fn policies_and_grants_follow_the_contract() {
    let Some(h) = Harness::new().await else {
        return;
    };

    let policies = h
        .svc
        .list_policies(req(ListPoliciesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .policies;
    assert_eq!(
        policies.len(),
        37,
        "§5.2 with the §22, §22.1, §23 and §24.7 rows"
    );
    let production = policies
        .iter()
        .find(|p| p.action_type == "deploy.production")
        .unwrap();
    assert!(!production.editable);

    let locked = h
        .svc
        .update_policy(req(UpdatePolicyRequest {
            action_type: "deploy.production".into(),
            decision: PolicyDecision::Auto as i32,
        }))
        .await
        .unwrap_err();
    assert_eq!(locked.code(), Code::FailedPrecondition);

    let blocked = h
        .svc
        .update_policy(req(UpdatePolicyRequest {
            action_type: "container.run".into(),
            decision: PolicyDecision::Blocked as i32,
        }))
        .await
        .unwrap_err();
    assert_eq!(blocked.code(), Code::FailedPrecondition);

    let relaxed = h
        .svc
        .update_policy(req(UpdatePolicyRequest {
            action_type: "container.run".into(),
            decision: PolicyDecision::Auto as i32,
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(relaxed.decision, PolicyDecision::Auto as i32);

    let missing = h
        .svc
        .update_policy(req(UpdatePolicyRequest {
            action_type: "nope".into(),
            decision: PolicyDecision::Auto as i32,
        }))
        .await
        .unwrap_err();
    assert_eq!(missing.code(), Code::NotFound);

    let identities = h
        .svc
        .list_identities(req(ListIdentitiesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .identities;
    assert_eq!(
        identities.len(),
        4,
        "simulated + local-docker + delivery + gitops; github-token only with a token"
    );
    // The delivery identity (§22) may look images up, trigger pipelines
    // and read GitOps applications (§24.7); every deploy permission and
    // every cluster write needs a timed grant.
    let delivery = identities.iter().find(|i| i.name == "delivery").unwrap();
    let scopes: Vec<&str> = delivery
        .grants
        .iter()
        .map(|g| g.permission.as_str())
        .collect();
    assert_eq!(
        scopes,
        vec![
            "cluster:read",
            "pipeline:trigger",
            "registry:read",
            "repository:read"
        ]
    );
    assert!(delivery.grants.iter().all(|g| g.expires_at.is_none()));
    let gitops = identities.iter().find(|i| i.name == "gitops").unwrap();
    assert_eq!(
        gitops
            .grants
            .iter()
            .map(|g| g.permission.as_str())
            .collect::<Vec<_>>(),
        ["cluster:read"]
    );
    let simulated = identities.iter().find(|i| i.name == "simulated").unwrap();
    let docker = identities
        .iter()
        .find(|i| i.name == "local-docker")
        .unwrap();
    assert!(
        docker
            .grants
            .iter()
            .any(|g| g.permission == "container:run")
    );
    let base: Vec<&str> = simulated
        .grants
        .iter()
        .map(|g| g.permission.as_str())
        .collect();
    assert_eq!(
        base,
        vec!["container:build", "environment:write", "repository:read"]
    );
    assert!(simulated.grants.iter().all(|g| g.expires_at.is_none()));

    let granted = h
        .svc
        .grant_permission(req(GrantPermissionRequest {
            identity_id: simulated.id.clone(),
            permissions: vec!["cloud:write".into(), "repository:read".into()],
            ttl_minutes: 0,
        }))
        .await
        .unwrap()
        .into_inner();
    let cloud = granted
        .grants
        .iter()
        .find(|g| g.permission == "cloud:write")
        .unwrap();
    assert!(cloud.expires_at.is_some());
    let repo = granted
        .grants
        .iter()
        .find(|g| g.permission == "repository:read")
        .unwrap();
    assert!(repo.expires_at.is_none(), "base scope is never shortened");

    let bad = h
        .svc
        .grant_permission(req(GrantPermissionRequest {
            identity_id: simulated.id.clone(),
            permissions: vec!["admin".into()],
            ttl_minutes: 5,
        }))
        .await
        .unwrap_err();
    assert_eq!(bad.code(), Code::InvalidArgument);

    h.teardown().await;
}

#[tokio::test]
async fn startup_recovery_pauses_executions_left_mid_step() {
    let Some(h) = Harness::new().await else {
        return;
    };

    let exec = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "p".into(),
            proposal_id: String::new(),
            pattern_id: String::new(),
            title: "crashed".into(),
            steps: vec![
                step("a", "repo.read", "local", &[], &[]),
                step("b", "tests.run_local", "local", &["a"], &[]),
            ],
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    let id = Uuid::parse_str(&exec.id).unwrap();

    // Forge the state a crash leaves behind: no runner, a step mid-flight.
    sqlx::query("UPDATE executions SET status = 'EXECUTING', started_at = now() WHERE id = $1")
        .bind(id)
        .execute(&h.pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE steps SET status = 'RUNNING', attempts = 1 WHERE execution_id = $1 AND key = 'a'",
    )
    .bind(id)
    .execute(&h.pool)
    .await
    .unwrap();

    assert_eq!(h.rt.recover().await.unwrap(), 1);
    let paused = h.wait_status(&exec.id, ExecutionStatus::Diagnosing).await;
    let failure = paused.failure.expect("failure");
    assert_eq!(failure.code, "UNKNOWN");
    assert_eq!(failure.step_key, "a");
    assert!(failure.diagnosis.contains("service restarted mid-step"));
    assert_eq!(failure.blocked_steps, vec!["b".to_string()]);
    assert_eq!(status_of(&paused.steps[1]), StepStatus::Blocked);

    // The recovered execution is live again: retry runs it to the end.
    h.resolve_recommended(&exec.id).await;
    let done = h.wait_status(&exec.id, ExecutionStatus::Succeeded).await;
    assert_eq!(done.revision, 1);
    assert!(done.steps.iter().all(|s| s.checkpointed));

    h.teardown().await;
}

/// Fails every attempt with the same code, to drive the retry cap.
struct AlwaysFailing;

#[async_trait::async_trait]
impl Adapter for AlwaysFailing {
    fn name(&self) -> &'static str {
        "simulated"
    }

    async fn run(&self, step: &StepInput, _ctx: &RunContext) -> StepOutcome {
        StepOutcome::failed("BUILD_FAILED", format!("attempt {} exploded", step.attempt))
    }
}

/// Emits many lines at once, to exercise LOG coalescing and ordering.
struct Chatty;

#[async_trait::async_trait]
impl Adapter for Chatty {
    fn name(&self) -> &'static str {
        "simulated"
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        for n in 1..=120 {
            ctx.logs
                .line(format!("{} line {n} {}", step.key, "x".repeat(3000)));
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
        StepOutcome::Succeeded {
            output: "done".into(),
            outputs: [("image".to_string(), format!("sdi/{}", step.key))].into(),
        }
    }
}

fn with_simulated(adapter: Arc<dyn Adapter>) -> Registry {
    let dir = std::env::temp_dir().join("sdi-execution-tests");
    Registry::new(
        adapter,
        Arc::new(adapters::DockerAdapter::new(
            dir,
            None,
            adapters::github::DEFAULT_API,
        )),
        Arc::new(adapters::GithubAdapter::new(
            None,
            adapters::github::DEFAULT_API,
        )),
    )
}

#[tokio::test]
async fn adapter_choice_validates_steps_and_picks_the_identity() {
    let Some(h) = Harness::new().await else {
        return;
    };

    let docker_plan = || {
        vec![
            step(
                "checkout",
                "repo.checkout",
                "docker",
                &[],
                &["repository:read"],
            ),
            step(
                "build",
                "container.build",
                "docker",
                &["checkout"],
                &["container:build"],
            ),
            step(
                "pr",
                "repo.open_pull_request",
                "github",
                &["build"],
                &["repository:write"],
            ),
        ]
    };
    let created = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "p".into(),
            title: "local build".into(),
            steps: docker_plan(),
            adapter: "docker".into(),
            artifacts: HashMap::from([("Dockerfile".to_string(), "FROM scratch\n".to_string())]),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(created.adapter, "docker");
    let identities = h
        .svc
        .list_identities(req(ListIdentitiesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .identities;
    let local_docker = identities
        .iter()
        .find(|i| i.name == "local-docker")
        .unwrap();
    assert_eq!(created.identity_id, local_docker.id);
    let (artifacts,): (sqlx::types::Json<HashMap<String, String>>,) =
        sqlx::query_as("SELECT artifacts FROM executions WHERE id = $1")
            .bind(Uuid::parse_str(&created.id).unwrap())
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(artifacts.0["Dockerfile"], "FROM scratch\n");

    let refused = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "p".into(),
            title: "too much".into(),
            steps: vec![
                step("build", "container.build", "docker", &[], &[]),
                step("deploy", "deploy.production", "kubernetes", &["build"], &[]),
            ],
            adapter: "docker".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);
    assert!(
        refused.message().contains("step deploy"),
        "{}",
        refused.message()
    );
    assert!(refused.message().contains("deploy.production"));

    let reserved = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "p".into(),
            steps: vec![step("a", "repo.read", "local", &[], &[])],
            adapter: "kubernetes".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(reserved.code(), Code::Unimplemented);

    let unknown = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "p".into(),
            steps: vec![step("a", "repo.read", "local", &[], &[])],
            adapter: "mystery".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), Code::InvalidArgument);

    // github needs the token identity, which only exists while GITHUB_TOKEN is set.
    let github_plan = || {
        vec![
            step("checkout", "repo.checkout", "docker", &[], &[]),
            step(
                "pr",
                "repo.open_pull_request",
                "github",
                &["checkout"],
                &["repository:write"],
            ),
        ]
    };
    let no_token = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "p".into(),
            steps: github_plan(),
            adapter: "github".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(no_token.code(), Code::FailedPrecondition);
    assert!(
        no_token.message().contains("no GitHub access")
            && no_token.message().contains("GITHUB_TOKEN"),
        "{}",
        no_token.message()
    );

    crate::db::identities::seed_github_token(&h.pool, true)
        .await
        .unwrap();
    let github = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "p".into(),
            steps: github_plan(),
            adapter: "github".into(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(github.adapter, "github");
    let identities = h
        .svc
        .list_identities(req(ListIdentitiesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .identities;
    let token = identities
        .iter()
        .find(|i| i.name == "github-token")
        .unwrap();
    assert_eq!(github.identity_id, token.id);
    let scopes: Vec<&str> = token.grants.iter().map(|g| g.permission.as_str()).collect();
    assert_eq!(
        scopes,
        vec!["pipeline:trigger", "repository:read", "repository:write"],
        "§22: the connected token may also trigger pipelines"
    );

    // A github run cannot build containers.
    let refused = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "p".into(),
            steps: vec![step("build", "container.build", "docker", &[], &[])],
            adapter: "github".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);

    // Without the token the base grants go; the identity stays because an
    // execution references it, but it can no longer be chosen.
    crate::db::identities::seed_github_token(&h.pool, false)
        .await
        .unwrap();
    let identities = h
        .svc
        .list_identities(req(ListIdentitiesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .identities;
    let token = identities
        .iter()
        .find(|i| i.name == "github-token")
        .unwrap();
    assert!(token.grants.is_empty());

    h.teardown().await;
}

#[tokio::test]
async fn platform_events_report_executions_and_approvals() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let mut feed = h
        .svc
        .watch_events(req(sdi_proto::common::v1::WatchEventsRequest {}))
        .await
        .unwrap()
        .into_inner();

    let created = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "proj-live".into(),
            title: "live".into(),
            steps: vec![
                step("read", "repo.read", "github", &[], &[]),
                step("run", "container.run", "docker", &["read"], &[]),
            ],
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    let first = tokio::time::timeout(WAIT, feed.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(first.entity, "execution");
    assert_eq!(first.entity_id, created.id);
    assert_eq!(first.project_id, "proj-live");
    assert_eq!(first.action, "created");
    assert_eq!(first.status, "planning");
    assert_eq!(first.data["adapter"], "simulated");

    h.svc
        .start_execution(req(StartExecutionRequest {
            id: created.id.clone(),
            fault: None,
        }))
        .await
        .unwrap();
    let approval = h.wait_pending_approval(&created.id).await;

    // Everything up to the approval request is an execution update; the
    // request itself also announces the approval.
    let mut seen_statuses = Vec::new();
    let requested = loop {
        let event = tokio::time::timeout(WAIT, feed.next())
            .await
            .expect("feed alive")
            .unwrap()
            .unwrap();
        if event.entity == "approval" {
            break event;
        }
        assert_eq!(event.entity, "execution");
        assert_eq!(event.action, "updated");
        assert_eq!(event.data["revision"], "0");
        seen_statuses.push(event.status);
    };
    assert!(
        seen_statuses.contains(&"executing".to_string()),
        "{seen_statuses:?}"
    );
    assert!(
        seen_statuses.contains(&"waiting_for_approval".to_string()),
        "{seen_statuses:?}"
    );
    assert_eq!(requested.action, "created");
    assert_eq!(requested.status, "pending");
    assert_eq!(requested.entity_id, approval.id);
    assert_eq!(requested.data["execution_id"], created.id);

    h.approve(&approval).await;
    let decided = tokio::time::timeout(WAIT, feed.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(decided.entity, "approval");
    assert_eq!(decided.action, "updated");
    assert_eq!(decided.status, "approved");

    h.wait_status(&created.id, ExecutionStatus::Succeeded).await;
    let mut last = None;
    while let Ok(Some(Ok(event))) =
        tokio::time::timeout(Duration::from_millis(300), feed.next()).await
    {
        last = Some(event);
    }
    let last = last.expect("a final execution event");
    assert_eq!(last.entity, "execution");
    assert_eq!(last.status, "succeeded");

    h.teardown().await;
}

#[tokio::test]
async fn the_retry_cap_forces_give_up_which_fails_the_run() {
    let Some(h) = Harness::with_registry(with_simulated(Arc::new(AlwaysFailing))).await else {
        return;
    };
    let created = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "proj-cap".into(),
            pattern_id: "pattern-flaky".into(),
            title: "never builds".into(),
            steps: vec![
                step("build", "container.build", "docker", &[], &[]),
                step("test", "tests.run_local", "docker", &["build"], &[]),
            ],
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    let id = created.id.clone();
    h.svc
        .start_execution(req(StartExecutionRequest {
            id: id.clone(),
            fault: None,
        }))
        .await
        .unwrap();

    for attempt in 1..5u32 {
        let diagnosing = h.wait_status(&id, ExecutionStatus::Diagnosing).await;
        let failure = diagnosing.failure.expect("failure");
        assert_eq!(failure.code, "BUILD_FAILED");
        assert_eq!(diagnosing.steps[0].attempts, attempt);
        assert!(failure.raw_output.contains(&format!("attempt {attempt}")));
        let ids: Vec<&str> = failure.options.iter().map(|o| o.id.as_str()).collect();
        assert!(ids.contains(&"retry"), "{ids:?}");
        assert!(ids.contains(&"give_up"), "{ids:?}");
        let replanned = h
            .svc
            .resolve_failure(req(ResolveFailureRequest {
                execution_id: id.clone(),
                option_id: "retry".into(),
                instruction: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(replanned.revision, attempt);
    }

    let exhausted = h.wait_status(&id, ExecutionStatus::Diagnosing).await;
    assert_eq!(exhausted.steps[0].attempts, 5);
    let failure = exhausted.failure.expect("failure");
    let ids: Vec<&str> = failure.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, vec!["give_up", "cancel"]);
    assert!(failure.diagnosis.contains("no longer offered"));
    let retry = h
        .svc
        .resolve_failure(req(ResolveFailureRequest {
            execution_id: id.clone(),
            option_id: "retry".into(),
            instruction: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(retry.code(), Code::InvalidArgument);

    let failed = h
        .svc
        .resolve_failure(req(ResolveFailureRequest {
            execution_id: id.clone(),
            option_id: "give_up".into(),
            instruction: String::new(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(failed.status, ExecutionStatus::Failed as i32);
    assert_eq!(status_of(&failed.steps[0]), StepStatus::Failed);
    assert_eq!(status_of(&failed.steps[1]), StepStatus::Cancelled);
    assert!(failed.finished_at.is_some());
    assert!(failed.failure.is_some(), "the last failure stays readable");
    assert_eq!(
        h.outcomes.calls.lock().unwrap().as_slice(),
        &[("pattern-flaky".to_string(), "proj-cap".to_string(), false)]
    );
    let stats = h
        .svc
        .get_delivery_stats(req(GetDeliveryStatsRequest {
            project_id: "proj-cap".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(stats.executions_failed, 1);
    assert_eq!(stats.success_rate, 0.0);

    h.teardown().await;
}

#[tokio::test]
async fn adapter_output_becomes_coalesced_log_events_and_outputs() {
    let Some(h) = Harness::with_registry(with_simulated(Arc::new(Chatty))).await else {
        return;
    };
    let created = h
        .svc
        .create_execution(req(CreateExecutionRequest {
            project_id: "proj-logs".into(),
            title: "chatty".into(),
            steps: vec![step("build", "container.build", "docker", &[], &[])],
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    h.svc
        .start_execution(req(StartExecutionRequest {
            id: created.id.clone(),
            fault: None,
        }))
        .await
        .unwrap();
    let done = h.wait_status(&created.id, ExecutionStatus::Succeeded).await;
    assert_eq!(done.outputs["image"], "sdi/build");

    let events = h
        .svc
        .list_events(req(ListEventsRequest {
            execution_id: created.id.clone(),
            after_seq: 0,
            limit: 0,
        }))
        .await
        .unwrap()
        .into_inner()
        .events;
    let logs: Vec<_> = events
        .iter()
        .filter(|e| {
            e.r#type == EventType::Log as i32
                && e.data.get("source").map(String::as_str) == Some("adapter")
        })
        .collect();
    assert!(!logs.is_empty());
    assert!(logs.len() < 120, "{} events for 120 lines", logs.len());
    let lines: usize = logs
        .iter()
        .map(|e| e.data["lines"].parse::<usize>().unwrap())
        .sum();
    assert_eq!(lines, 120);
    let text: Vec<&str> = logs.iter().flat_map(|e| e.message.lines()).collect();
    assert_eq!(text.len(), 120);
    assert!(text[0].starts_with("build line 1 "));
    assert!(text[119].starts_with("build line 120 "));
    assert!(text.iter().all(|l| l.len() <= 2048 && l.ends_with("...")));
    // Adapter output lands before the step is verified.
    let last_log = events
        .iter()
        .rposition(|e| e.data.get("source").map(String::as_str) == Some("adapter"))
        .unwrap();
    let succeeded = events
        .iter()
        .position(|e| e.r#type == EventType::StepSucceeded as i32)
        .unwrap();
    assert!(last_log < succeeded);
    assert_eq!(events[succeeded].data["output.image"], "sdi/build");

    h.teardown().await;
}

/// Tenancy (CONTRACT.md §18): metadata is mandatory, roles gate writes by
/// adapter, one workspace never sees another's runs, policies and
/// identities are seeded per workspace on first use, peers get the caller
/// (or the workspace's system principal) forwarded, events carry the
/// workspace, actions are audited, and the export / delete RPCs are
/// complete.
#[tokio::test]
async fn workspaces_are_isolated_and_roles_enforced() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let plan = || {
        vec![
            step("read", "repo.read", "github", &[], &[]),
            step("stage", "deploy.staging", "cloud", &["read"], &[]),
        ]
    };
    let create = |adapter: &str| CreateExecutionRequest {
        project_id: "proj-t".into(),
        pattern_id: "pattern-t".into(),
        title: format!("{adapter} run"),
        steps: plan(),
        adapter: adapter.into(),
        ..Default::default()
    };

    // No metadata: not even a list.
    let err = h
        .svc
        .list_executions(Request::new(ListExecutionsRequest::default()))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::Unauthenticated);

    // Roles follow the adapter: dry runs are a member's, docker an approver's.
    let err = h
        .svc
        .create_execution(as_role(Role::Viewer, WS_A, create("")))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);
    let err = h
        .svc
        .create_execution(as_role(Role::Member, WS_A, create("docker")))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);
    let docker = h
        .svc
        .create_execution(as_role(
            Role::Approver,
            WS_A,
            CreateExecutionRequest {
                steps: vec![step("checkout", "repo.checkout", "docker", &[], &[])],
                ..create("docker")
            },
        ))
        .await
        .unwrap()
        .into_inner();
    let err = h
        .svc
        .start_execution(as_role(
            Role::Member,
            WS_A,
            StartExecutionRequest {
                id: docker.id.clone(),
                fault: None,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);

    // A member of A creates a dry run; knowledge was asked as that member.
    let created = h
        .svc
        .create_execution(as_role(Role::Member, WS_A, create("")))
        .await
        .unwrap()
        .into_inner();
    assert!(
        h.outcomes
            .callers
            .lock()
            .unwrap()
            .contains(&principal(Role::Member, WS_A))
    );
    let (created_by,): (String,) =
        sqlx::query_as("SELECT created_by FROM executions WHERE id = $1")
            .bind(Uuid::parse_str(&created.id).unwrap())
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(created_by, "user-member");

    // B cannot see it: the id alone is nothing.
    let err = h
        .svc
        .get_execution(as_role(
            Role::Admin,
            WS_B,
            GetExecutionRequest {
                id: created.id.clone(),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
    let err = h
        .svc
        .start_execution(as_role(
            Role::Admin,
            WS_B,
            StartExecutionRequest {
                id: created.id.clone(),
                fault: None,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
    let err = h
        .svc
        .cancel_execution(as_role(
            Role::Admin,
            WS_B,
            CancelExecutionRequest {
                id: created.id.clone(),
                reason: String::new(),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
    let err = h
        .svc
        .watch_execution(as_role(
            Role::Admin,
            WS_B,
            WatchExecutionRequest {
                execution_id: created.id.clone(),
                after_seq: 0,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
    let listed = h
        .svc
        .list_executions(as_role(
            Role::Viewer,
            WS_B,
            ListExecutionsRequest::default(),
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(listed.executions.is_empty());
    let events = h
        .svc
        .list_events(as_role(
            Role::Viewer,
            WS_B,
            ListEventsRequest {
                execution_id: created.id.clone(),
                ..Default::default()
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(events.events.is_empty());

    // Policies: B gets its own §5.2 copy on first use; A's edit stays in A.
    let policies_b = h
        .svc
        .list_policies(as_role(Role::Viewer, WS_B, ListPoliciesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .policies;
    assert_eq!(policies_b.len(), 37);
    let err = h
        .svc
        .update_policy(as_role(
            Role::Approver,
            WS_A,
            UpdatePolicyRequest {
                action_type: "container.run".into(),
                decision: PolicyDecision::Auto as i32,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);
    h.svc
        .update_policy(req(UpdatePolicyRequest {
            action_type: "container.run".into(),
            decision: PolicyDecision::Auto as i32,
        }))
        .await
        .unwrap();
    let decision_in = |policies: &[sdi_proto::execution::v1::Policy]| {
        policies
            .iter()
            .find(|p| p.action_type == "container.run")
            .map(|p| p.decision)
            .unwrap()
    };
    let policies_a = h
        .svc
        .list_policies(req(ListPoliciesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .policies;
    let policies_b = h
        .svc
        .list_policies(as_role(Role::Viewer, WS_B, ListPoliciesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .policies;
    assert_eq!(decision_in(&policies_a), PolicyDecision::Auto as i32);
    assert_eq!(decision_in(&policies_b), PolicyDecision::Approval as i32);

    // Identities: each workspace has its own four, with their own ids.
    let identities_a = h
        .svc
        .list_identities(req(ListIdentitiesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .identities;
    let identities_b = h
        .svc
        .list_identities(as_role(Role::Viewer, WS_B, ListIdentitiesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .identities;
    assert_eq!((identities_a.len(), identities_b.len()), (4, 4));
    assert!(
        identities_a
            .iter()
            .all(|a| identities_b.iter().all(|b| a.id != b.id))
    );
    let simulated_a = identities_a.iter().find(|i| i.name == "simulated").unwrap();
    assert_eq!(created.identity_id, simulated_a.id);
    let err = h
        .svc
        .grant_permission(as_role(
            Role::Admin,
            WS_B,
            GrantPermissionRequest {
                identity_id: simulated_a.id.clone(),
                permissions: vec!["cloud:write".into()],
                ttl_minutes: 0,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound, "A's identity is invisible in B");
    let err = h
        .svc
        .grant_permission(as_role(
            Role::Member,
            WS_A,
            GrantPermissionRequest {
                identity_id: simulated_a.id.clone(),
                permissions: vec!["cloud:write".into()],
                ttl_minutes: 0,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);
    h.svc
        .grant_permission(as_role(
            Role::Approver,
            WS_A,
            GrantPermissionRequest {
                identity_id: simulated_a.id.clone(),
                permissions: vec!["cloud:write".into()],
                ttl_minutes: 0,
            },
        ))
        .await
        .unwrap();

    // Events are scoped: B's feed stays silent while A runs.
    let mut feed_a = h
        .svc
        .watch_events(as_role(Role::Viewer, WS_A, WatchEventsRequest {}))
        .await
        .unwrap()
        .into_inner();
    let mut feed_b = h
        .svc
        .watch_events(as_role(Role::Viewer, WS_B, WatchEventsRequest {}))
        .await
        .unwrap()
        .into_inner();

    // The member starts the dry run; deploy.staging asks a human, who must
    // be an approver; the outcome goes to knowledge as the system principal.
    h.svc
        .start_execution(as_role(
            Role::Member,
            WS_A,
            StartExecutionRequest {
                id: created.id.clone(),
                fault: None,
            },
        ))
        .await
        .unwrap();
    let approval = h.wait_pending_approval(&created.id).await;
    let err = h
        .svc
        .decide_approval(as_role(
            Role::Member,
            WS_A,
            DecideApprovalRequest {
                id: approval.id.clone(),
                approve: true,
                ..Default::default()
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);
    let err = h
        .svc
        .decide_approval(as_role(
            Role::Approver,
            WS_B,
            DecideApprovalRequest {
                id: approval.id.clone(),
                approve: true,
                ..Default::default()
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
    let decided = h
        .svc
        .decide_approval(as_role(
            Role::Approver,
            WS_A,
            DecideApprovalRequest {
                id: approval.id.clone(),
                approve: true,
                ..Default::default()
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        decided.decided_by, "user-approver",
        "defaults to the caller"
    );
    h.wait_status(&created.id, ExecutionStatus::Succeeded).await;
    assert_eq!(
        h.outcomes.callers.lock().unwrap().last(),
        Some(&Principal::system(WS_A))
    );
    let event = tokio::time::timeout(WAIT, feed_a.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        event.data.get("workspace_id").map(String::as_str),
        Some(WS_A)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), feed_b.next())
            .await
            .is_err(),
        "B saw A's event"
    );

    // Everything that matters was audited, as the caller, in the workspace.
    let actions = h.audit.actions();
    for expected in [
        "policy.update",
        "identity.grant",
        "execution.start",
        "approval.decide",
    ] {
        assert!(actions.contains(&expected.to_owned()), "{actions:?}");
    }
    {
        let entries = h.audit.entries.lock().unwrap();
        assert!(entries.iter().all(|(p, e)| p.workspace_id == WS_A
            && e.workspace_id == WS_A
            && e.user_id == p.user_id
            && e.at.is_some()));
        let (p, start) = entries
            .iter()
            .find(|(_, e)| e.action == "execution.start")
            .unwrap();
        assert_eq!(p.user_id, "user-member");
        assert_eq!(
            (start.entity.as_str(), start.entity_id.as_str()),
            ("execution", created.id.as_str())
        );
        let (_, decide) = entries
            .iter()
            .find(|(_, e)| e.action == "approval.decide")
            .unwrap();
        assert_eq!(decide.entity_id, approval.id);
        assert!(decide.detail.starts_with("approved"));
    }

    // Export is complete and scoped.
    let export = |role: Role, workspace: &str| {
        as_role(
            role,
            WS_A,
            ExportWorkspaceDataRequest {
                workspace_id: workspace.to_owned(),
            },
        )
    };
    assert_eq!(
        h.svc
            .export_workspace_data(export(Role::Admin, WS_B))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        h.svc
            .export_workspace_data(export(Role::Approver, WS_A))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let mut stream = h
        .svc
        .export_workspace_data(export(Role::Admin, WS_A))
        .await
        .unwrap()
        .into_inner();
    let mut records = Vec::new();
    while let Some(record) = stream.next().await {
        records.push(record.unwrap());
    }
    let count = |entity: &str| records.iter().filter(|r| r.entity == entity).count();
    assert_eq!(count("execution"), 2);
    assert_eq!(count("step"), 3);
    assert_eq!(count("approval"), 1);
    assert_eq!(count("policy"), 37);
    assert_eq!(count("identity"), 4);
    assert!(count("grant") >= 8);
    assert!(count("event") > 5);
    for record in &records {
        let json: serde_json::Value = serde_json::from_str(&record.json).unwrap();
        assert_eq!(json["workspace_id"], WS_A, "{}", record.entity);
    }

    // Delete cancels what is live, then removes everything; B is untouched.
    let live = h
        .svc
        .create_execution(as_role(Role::Member, WS_A, create("")))
        .await
        .unwrap()
        .into_inner();
    h.svc
        .start_execution(as_role(
            Role::Member,
            WS_A,
            StartExecutionRequest {
                id: live.id.clone(),
                fault: None,
            },
        ))
        .await
        .unwrap();
    h.wait_pending_approval(&live.id).await;
    let other = h
        .svc
        .create_execution(as_role(Role::Member, WS_B, create("")))
        .await
        .unwrap()
        .into_inner();
    let delete = |role: Role, workspace: &str| {
        as_role(
            role,
            WS_A,
            DeleteWorkspaceDataRequest {
                workspace_id: workspace.to_owned(),
            },
        )
    };
    assert_eq!(
        h.svc
            .delete_workspace_data(delete(Role::Admin, WS_A))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        h.svc
            .delete_workspace_data(delete(Role::Owner, WS_B))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let deleted = h
        .svc
        .delete_workspace_data(delete(Role::Owner, WS_A))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        deleted.cancelled, 2,
        "the docker run in PLANNING and the waiting one"
    );
    assert_eq!(deleted.deleted["execution"], 3);
    assert_eq!(deleted.deleted["policy"], 37);
    assert_eq!(deleted.deleted["identity"], 4);
    assert!(deleted.deleted["approval"] >= 2);
    let listed = h
        .svc
        .list_executions(req(ListExecutionsRequest::default()))
        .await
        .unwrap()
        .into_inner();
    assert!(listed.executions.is_empty());
    let (rows,): (i64,) = sqlx::query_as("SELECT count(*) FROM steps WHERE workspace_id = $1")
        .bind(Uuid::parse_str(WS_A).unwrap())
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
    assert_eq!(h.get_as(Role::Member, WS_B, &other.id).await.id, other.id);
    // A is seeded again on its next use.
    let policies_a = h
        .svc
        .list_policies(req(ListPoliciesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .policies;
    assert_eq!(policies_a.len(), 37);
    assert_eq!(decision_in(&policies_a), PolicyDecision::Approval as i32);
    // The seed note is per process and bounded; both workspaces are in it.
    assert_eq!(h.rt.seeded_workspaces(), 2);

    h.teardown().await;
}

/// Without an instance-wide `GITHUB_TOKEN`, a workspace still gets the
/// `github-token` identity when it has GitHub access of its own (the
/// installed App, or the signed-in account), and one that has none does
/// not. That is what lets an organisation that only installed the App open
/// pull requests, commit fixes and merge.
#[tokio::test]
async fn a_workspace_with_its_own_github_access_gets_the_github_identity() {
    let Some(h) = Harness::new().await else {
        return;
    };
    // The harness has neither a token nor per-workspace access.
    let without = h
        .svc
        .list_identities(req(ListIdentitiesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .identities;
    assert!(without.iter().all(|i| i.name != "github-token"));

    // Same database, a runtime whose workspaces do have access.
    let rt = Runtime::new(
        h.pool.clone(),
        adapters::test_registry(),
        Arc::new(h.outcomes.clone()),
        Arc::new(h.audit.clone()),
        Duration::ZERO,
        false,
        Arc::new(crate::identity::StaticGithubAccess(
            crate::adapters::GithubAccess {
                token: "ghs_installation".into(),
                api_base_url: "http://127.0.0.1:9".into(),
            },
        )),
    );
    let svc = ExecutionGrpc::new(Arc::clone(&rt));
    let identities = svc
        .list_identities(req(ListIdentitiesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .identities;
    let github = identities
        .iter()
        .find(|i| i.name == "github-token")
        .expect("seeded from the workspace's own access");
    let scopes: Vec<&str> = github
        .grants
        .iter()
        .map(|g| g.permission.as_str())
        .collect();
    assert_eq!(
        scopes,
        vec!["pipeline:trigger", "repository:read", "repository:write"]
    );
    assert!(
        !scopes.contains(&"repository:admin"),
        "changing repository settings is always a timed grant"
    );
    assert!(
        !format!("{github:?}").contains("ghs_installation"),
        "the token is never part of an identity"
    );
    drop(svc);
    drop(rt);
    h.teardown().await;
}
