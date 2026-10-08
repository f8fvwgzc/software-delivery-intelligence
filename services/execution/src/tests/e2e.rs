use std::{collections::HashMap, sync::Arc, time::Duration};

use sdi_proto::{
    common::v1::PlanStep,
    execution::v1::{
        Approval, ApprovalStatus, CancelExecutionRequest, CreateExecutionRequest,
        DecideApprovalRequest, EventType, Execution, ExecutionStatus, FaultInjection,
        GetDeliveryStatsRequest, GetExecutionRequest, GrantPermissionRequest, ListApprovalsRequest,
        ListEventsRequest, ListIdentitiesRequest, ListPoliciesRequest, PolicyDecision,
        ResolveFailureRequest, StartExecutionRequest, StepStatus, UpdatePolicyRequest,
        WatchExecutionRequest, execution_service_server::ExecutionService,
    },
};
use sqlx::PgPool;
use tokio_stream::StreamExt;
use tonic::{Code, Request};
use uuid::Uuid;

use crate::{
    adapters::{Registry, SimulatedAdapter},
    grpc::ExecutionGrpc,
    knowledge::RecordingPatternOutcomes,
    runner::Runtime,
};

const WAIT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(10);

struct Harness {
    pool: PgPool,
    schema: String,
    rt: Arc<Runtime>,
    svc: ExecutionGrpc,
    outcomes: RecordingPatternOutcomes,
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
        let rt = Runtime::new(
            pool.clone(),
            Registry::new(Arc::new(SimulatedAdapter::new(1))),
            Arc::new(outcomes.clone()),
            Duration::ZERO,
        );
        Some(Self {
            pool,
            schema,
            svc: ExecutionGrpc::new(Arc::clone(&rt)),
            rt,
            outcomes,
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
        self.svc
            .get_execution(Request::new(GetExecutionRequest { id: id.to_owned() }))
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
                .list_approvals(Request::new(ListApprovalsRequest {
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
            .decide_approval(Request::new(DecideApprovalRequest {
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
            .resolve_failure(Request::new(ResolveFailureRequest {
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
        .create_execution(Request::new(CreateExecutionRequest {
            project_id: "proj-1".into(),
            proposal_id: "prop-1".into(),
            pattern_id: "pattern-aws".into(),
            title: "Ship it".into(),
            steps: five_step_plan(),
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
        .resolve_failure(Request::new(ResolveFailureRequest {
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
        .watch_execution(Request::new(WatchExecutionRequest {
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
        .start_execution(Request::new(StartExecutionRequest {
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
        .start_execution(Request::new(StartExecutionRequest {
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
        .decide_approval(Request::new(DecideApprovalRequest {
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
        .resolve_failure(Request::new(ResolveFailureRequest {
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
        .list_identities(Request::new(ListIdentitiesRequest {}))
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
        .list_events(Request::new(ListEventsRequest {
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
        .watch_execution(Request::new(WatchExecutionRequest {
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
        .get_delivery_stats(Request::new(GetDeliveryStatsRequest {
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
        .list_executions(Request::new(
            sdi_proto::execution::v1::ListExecutionsRequest {
                project_id: String::new(),
                limit: 0,
            },
        ))
        .await
        .unwrap()
        .into_inner()
        .executions;
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].steps.len(), 5);
    assert!(listing[0].failure.is_none());

    h.teardown().await;
}

#[tokio::test]
async fn invalid_plans_are_rejected_and_cancel_works_from_planning() {
    let Some(h) = Harness::new().await else {
        return;
    };

    let cyclic = h
        .svc
        .create_execution(Request::new(CreateExecutionRequest {
            project_id: "p".into(),
            proposal_id: String::new(),
            pattern_id: String::new(),
            title: "cycle".into(),
            steps: vec![
                step("a", "repo.read", "local", &["b"], &[]),
                step("b", "repo.read", "local", &["a"], &[]),
            ],
        }))
        .await
        .unwrap_err();
    assert_eq!(cyclic.code(), Code::InvalidArgument);

    let unknown = h
        .svc
        .create_execution(Request::new(CreateExecutionRequest {
            project_id: "p".into(),
            proposal_id: String::new(),
            pattern_id: String::new(),
            title: "ghost".into(),
            steps: vec![step("a", "repo.read", "local", &["ghost"], &[])],
        }))
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), Code::InvalidArgument);

    let exec = h
        .svc
        .create_execution(Request::new(CreateExecutionRequest {
            project_id: "p".into(),
            proposal_id: String::new(),
            pattern_id: String::new(),
            title: String::new(),
            steps: vec![step("a", "something.new", "local", &[], &[])],
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
        .cancel_execution(Request::new(CancelExecutionRequest {
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
        .cancel_execution(Request::new(CancelExecutionRequest {
            id: exec.id.clone(),
            reason: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(twice.code(), Code::FailedPrecondition);

    let events = h
        .svc
        .list_events(Request::new(ListEventsRequest {
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
        .list_policies(Request::new(ListPoliciesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .policies;
    assert_eq!(policies.len(), 19);
    let production = policies
        .iter()
        .find(|p| p.action_type == "deploy.production")
        .unwrap();
    assert!(!production.editable);

    let locked = h
        .svc
        .update_policy(Request::new(UpdatePolicyRequest {
            action_type: "deploy.production".into(),
            decision: PolicyDecision::Auto as i32,
        }))
        .await
        .unwrap_err();
    assert_eq!(locked.code(), Code::FailedPrecondition);

    let blocked = h
        .svc
        .update_policy(Request::new(UpdatePolicyRequest {
            action_type: "container.run".into(),
            decision: PolicyDecision::Blocked as i32,
        }))
        .await
        .unwrap_err();
    assert_eq!(blocked.code(), Code::FailedPrecondition);

    let relaxed = h
        .svc
        .update_policy(Request::new(UpdatePolicyRequest {
            action_type: "container.run".into(),
            decision: PolicyDecision::Auto as i32,
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(relaxed.decision, PolicyDecision::Auto as i32);

    let missing = h
        .svc
        .update_policy(Request::new(UpdatePolicyRequest {
            action_type: "nope".into(),
            decision: PolicyDecision::Auto as i32,
        }))
        .await
        .unwrap_err();
    assert_eq!(missing.code(), Code::NotFound);

    let identities = h
        .svc
        .list_identities(Request::new(ListIdentitiesRequest {}))
        .await
        .unwrap()
        .into_inner()
        .identities;
    assert_eq!(identities.len(), 1);
    let simulated = &identities[0];
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
        .grant_permission(Request::new(GrantPermissionRequest {
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
        .grant_permission(Request::new(GrantPermissionRequest {
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
        .create_execution(Request::new(CreateExecutionRequest {
            project_id: "p".into(),
            proposal_id: String::new(),
            pattern_id: String::new(),
            title: "crashed".into(),
            steps: vec![
                step("a", "repo.read", "local", &[], &[]),
                step("b", "tests.run_local", "local", &["a"], &[]),
            ],
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
