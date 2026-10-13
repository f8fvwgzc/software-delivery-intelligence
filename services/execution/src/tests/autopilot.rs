//! The pull request autopilot end to end (CONTRACT.md §23), in process:
//! executions created the way the intelligence service creates them (the
//! step keys and params of §23.5, no repository named on any step), run by
//! the real runner through the real gates, with the real `github` and
//! checkout adapters against a repository that keeps real git state
//! (`fakes::repo`). Nothing is simulated except where a test says it runs
//! on the `simulated` runtime.

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use sdi_common::principal::{Principal, Role};
use sdi_proto::{
    common::v1::{PlanStep, RiskLevel},
    execution::v1::{
        Approval, ApprovalStatus, CreateExecutionRequest, DecideApprovalRequest, Execution,
        ExecutionEvent, ExecutionStatus, Failure, GetExecutionRequest, Identity,
        ListApprovalsRequest, ListEventsRequest, ListIdentitiesRequest, ListPoliciesRequest,
        Policy, PolicyDecision, ResolveFailureRequest, StartExecutionRequest, StepStatus,
        UpdatePolicyRequest, execution_service_server::ExecutionService,
    },
};
use sqlx::PgPool;
use tonic::{Code, Request};
use uuid::Uuid;

use super::fakes::repo::{self as fake, FakeRepo};
use crate::{
    adapters::{
        Adapter, DockerAdapter, GithubAccess, GithubAdapter, Registry, RunContext,
        SimulatedAdapter, StepInput, net::NetPolicy,
    },
    delivery::Delivery,
    grpc::ExecutionGrpc,
    identity::{RecordingAudit, StaticGithubAccess, StaticWorkspaces},
    knowledge::{ProjectInfo, RecordingPatternOutcomes, StaticProjects},
    runner::Runtime,
};

const WAIT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(10);

const WS: &str = "00000000-0000-7000-8000-0000000000c1";
const PROJECT: &str = "0192f6a0-0000-7000-8000-0000000000b1";
const REPO: &str = "acme/api";
const BRANCH: &str = "fix/lint";
const BROKEN: &str = "pub fn add(a: i32, b: i32) -> i32 {\n    a - b\n}\n";
const FIXED: &str = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
const DEPENDABOT: &str = ".github/dependabot.yml";
const DEPENDABOT_YML: &str = "version: 2\nupdates:\n  - package-ecosystem: cargo\n    directory: /\n    schedule:\n      interval: weekly\n";

fn as_role<T>(role: Role, message: T) -> Request<T> {
    let mut request = Request::new(message);
    Principal {
        user_id: format!("user-{}", role.as_str()),
        workspace_id: WS.to_owned(),
        role,
    }
    .apply(&mut request);
    request
}

fn admin<T>(message: T) -> Request<T> {
    as_role(Role::Admin, message)
}

/// `acme/api` with pull request #1 from `fix/lint`, one commit ahead of
/// `main`, and the commit that pull request is at.
fn repository() -> (FakeRepo, u64, String) {
    let repo = FakeRepo::new(
        REPO,
        &[
            ("README.md", "# api\n"),
            (
                "src/lib.rs",
                "pub fn add(a: i32, b: i32) -> i32 {\n    0\n}\n",
            ),
        ],
    );
    repo.branch(BRANCH, "main");
    let head = repo.push(BRANCH, &[("src/lib.rs", BROKEN)], "Implement add");
    let number = repo.open_pull(BRANCH, "Implement add");
    (repo, number, head)
}

/// A step as §23.5 writes it: `adapter = github`, the fixed key, and only
/// the params of the table.
fn plan_step(
    key: &str,
    action_type: &str,
    permission: &str,
    depends_on: &[&str],
    params: &[(&str, &str)],
) -> PlanStep {
    PlanStep {
        key: key.into(),
        title: key.replace('_', " "),
        description: String::new(),
        action_type: action_type.into(),
        adapter: "github".into(),
        depends_on: depends_on.iter().map(|s| (*s).to_owned()).collect(),
        required_permissions: vec![permission.to_owned()],
        params: params
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        verification: String::new(),
    }
}

/// §23.2: `checkout` → `commit_fixes`.
fn fixes_plan(head: &str, number: u64) -> Vec<PlanStep> {
    vec![
        plan_step("checkout", "repo.checkout", "repository:read", &[], &[]),
        plan_step(
            "commit_fixes",
            "repo.commit_fixes",
            "repository:write",
            &["checkout"],
            &[
                ("branch", BRANCH),
                ("expected_head_sha", head),
                ("message", "Apply 1 review fix to #1"),
                ("pull_request", &number.to_string()),
            ],
        ),
    ]
}

/// §23.3: `verify_mergeable` → `merge` → `delete_branch`.
fn merge_plan(branch: &str, head: &str, number: u64) -> Vec<PlanStep> {
    let number = number.to_string();
    vec![
        plan_step(
            "verify_mergeable",
            "repo.verify_mergeable",
            "repository:read",
            &[],
            &[("pull_request", &number), ("expected_head_sha", head)],
        ),
        plan_step(
            "merge",
            "repo.merge_pull_request",
            "repository:write",
            &["verify_mergeable"],
            &[
                ("pull_request", &number),
                ("expected_head_sha", head),
                ("merge_method", "squash"),
            ],
        ),
        plan_step(
            "delete_branch",
            "repo.delete_branch",
            "repository:write",
            &["merge"],
            &[("branch", branch), ("pull_request", &number)],
        ),
    ]
}

/// §23.4: `checkout` → `enable_security_updates` → `open_pull_request`.
fn dependabot_plan() -> Vec<PlanStep> {
    vec![
        plan_step("checkout", "repo.checkout", "repository:read", &[], &[]),
        plan_step(
            "enable_security_updates",
            "repo.enable_security_updates",
            "repository:admin",
            &["checkout"],
            &[("alerts", "true"), ("security_updates", "true")],
        ),
        plan_step(
            "open_pull_request",
            "repo.open_pull_request",
            "repository:write",
            &["enable_security_updates"],
            &[
                ("branch", "sdi/dependabot"),
                ("title", "Turn on Dependabot version updates"),
                (
                    "body",
                    "Adds the Dependabot configuration for this repository.",
                ),
                ("skip_if_exists", DEPENDABOT),
            ],
        ),
    ]
}

fn status_of(execution: &Execution, key: &str) -> StepStatus {
    let step = execution
        .steps
        .iter()
        .find(|s| s.key == key)
        .unwrap_or_else(|| panic!("no step {key}"));
    StepStatus::try_from(step.status).unwrap_or_default()
}

fn output_of<'a>(execution: &'a Execution, key: &str) -> &'a str {
    execution
        .steps
        .iter()
        .find(|s| s.key == key)
        .map(|s| s.output.as_str())
        .unwrap_or_else(|| panic!("no step {key}"))
}

fn option_ids(failure: &Failure) -> Vec<&str> {
    failure.options.iter().map(|o| o.id.as_str()).collect()
}

struct Harness {
    pool: PgPool,
    schema: String,
    rt: Arc<Runtime>,
    svc: ExecutionGrpc,
    /// API base of the fake repository.
    api: String,
    /// Where `repo.checkout` unpacks.
    run_dir: PathBuf,
}

impl Harness {
    /// None (with a note on stderr) when no database is configured.
    async fn new(repo: FakeRepo) -> Option<Self> {
        let Some(url) = std::env::var("DATABASE_URL")
            .ok()
            .filter(|u| !u.trim().is_empty())
        else {
            eprintln!("skipping: DATABASE_URL is not set");
            return None;
        };
        let id = Uuid::now_v7();
        // Schema names must be lowercase letters only (sdi_common::db::connect).
        let suffix: String = id
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
        let api = repo.serve().await;
        let run_dir = std::env::temp_dir().join(format!("sdi-autopilot-{}", id.simple()));
        let rt = Self::runtime(&pool, &api, &run_dir);
        Some(Self {
            pool,
            schema,
            svc: ExecutionGrpc::new(Arc::clone(&rt)),
            rt,
            api,
            run_dir,
        })
    }

    /// The service as a process would build it: the real adapters, whose
    /// process-wide GitHub configuration points nowhere, and the
    /// workspace's GitHub access and project from the two peers.
    fn runtime(pool: &PgPool, api: &str, run_dir: &PathBuf) -> Arc<Runtime> {
        const NOWHERE: &str = "http://127.0.0.1:1";
        let adapters = Registry::new(
            Arc::new(SimulatedAdapter::new(1)),
            Arc::new(DockerAdapter::new(run_dir, None, NOWHERE)),
            Arc::new(
                GithubAdapter::new(None, NOWHERE)
                    .with_mergeable_poll(Duration::from_millis(5), Duration::from_millis(200)),
            ),
        );
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
        Runtime::with_delivery(
            pool.clone(),
            adapters,
            Arc::new(RecordingPatternOutcomes::default()),
            Arc::new(RecordingAudit::default()),
            Duration::ZERO,
            // GITHUB_TOKEN is configured: the workspace gets `github-token`.
            true,
            Arc::new(StaticGithubAccess(GithubAccess {
                token: fake::TOKEN.into(),
                api_base_url: api.to_owned(),
            })),
            Delivery::new(
                Arc::new(projects),
                Arc::new(StaticWorkspaces::new("Acme", &[])),
                None,
                None,
                NetPolicy::ALLOW_PRIVATE,
            ),
        )
    }

    /// The service after a restart: a new runtime over the same database,
    /// with no memory of what the old one was doing.
    fn restarted(&mut self) {
        self.rt = Self::runtime(&self.pool, &self.api, &self.run_dir);
        self.svc = ExecutionGrpc::new(Arc::clone(&self.rt));
    }

    async fn teardown(self) {
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(self.svc);
        drop(self.rt);
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.pool)
        .await;
        let _ = std::fs::remove_dir_all(&self.run_dir);
    }

    async fn create(
        &self,
        title: &str,
        adapter: &str,
        steps: Vec<PlanStep>,
        artifacts: &[(&str, &str)],
    ) -> Execution {
        self.svc
            .create_execution(admin(CreateExecutionRequest {
                project_id: PROJECT.into(),
                title: title.into(),
                steps,
                adapter: adapter.into(),
                artifacts: artifacts
                    .iter()
                    .map(|(path, content)| ((*path).to_owned(), (*content).to_owned()))
                    .collect::<HashMap<_, _>>(),
                ..Default::default()
            }))
            .await
            .expect("create execution")
            .into_inner()
    }

    /// Creates and starts a run on the `github` adapter.
    async fn run(&self, title: &str, steps: Vec<PlanStep>, artifacts: &[(&str, &str)]) -> String {
        let created = self.create(title, "github", steps, artifacts).await;
        assert_eq!(created.adapter, "github");
        self.start(&created.id).await;
        created.id
    }

    async fn start(&self, id: &str) {
        self.svc
            .start_execution(admin(StartExecutionRequest {
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
            .resolve_failure(admin(ResolveFailureRequest {
                execution_id: id.to_owned(),
                option_id: option_id.to_owned(),
                instruction: String::new(),
            }))
            .await
            .expect("resolve failure");
    }

    async fn approvals(&self, id: &str, status: ApprovalStatus) -> Vec<Approval> {
        self.svc
            .list_approvals(admin(ListApprovalsRequest {
                status: status as i32,
                execution_id: id.to_owned(),
            }))
            .await
            .expect("list approvals")
            .into_inner()
            .approvals
    }

    /// Waits for the execution to ask a person, and returns what it asks.
    async fn pending_approval(&self, id: &str, step_key: &str) -> Approval {
        self.wait_status(id, ExecutionStatus::WaitingForApproval)
            .await;
        let pending = self.approvals(id, ApprovalStatus::Pending).await;
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].step_key, step_key);
        pending.into_iter().next().unwrap()
    }

    async fn approve(&self, approval: &Approval) {
        self.svc
            .decide_approval(as_role(
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

    async fn policies(&self) -> Vec<Policy> {
        self.svc
            .list_policies(admin(ListPoliciesRequest {}))
            .await
            .expect("list policies")
            .into_inner()
            .policies
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

    async fn identities(&self) -> Vec<Identity> {
        self.svc
            .list_identities(admin(ListIdentitiesRequest {}))
            .await
            .expect("list identities")
            .into_inner()
            .identities
    }

    async fn events(&self, id: &str) -> Vec<ExecutionEvent> {
        self.svc
            .list_events(admin(ListEventsRequest {
                execution_id: id.to_owned(),
                after_seq: 0,
                limit: 500,
            }))
            .await
            .expect("list events")
            .into_inner()
            .events
    }
}

/// §23.2: `checkout` → `commit_fixes`. The commit waits for a person by
/// default; a branch that moved ends the run instead of being overwritten.
#[tokio::test]
async fn review_fixes_are_committed_once_a_person_approves_them() {
    let (repo, number, reviewed) = repository();
    let Some(h) = Harness::new(repo.clone()).await else {
        return;
    };

    let created = h
        .create(
            "Apply 1 review fix to #1",
            "github",
            fixes_plan(&reviewed, number),
            &[("src/lib.rs", FIXED)],
        )
        .await;
    assert_eq!(
        created.steps[0].policy_decision,
        PolicyDecision::Auto as i32
    );
    assert_eq!(
        created.steps[1].policy_decision,
        PolicyDecision::Approval as i32
    );
    assert_eq!(created.steps[1].risk, RiskLevel::Medium as i32);
    let identities = h.identities().await;
    let token = identities
        .iter()
        .find(|i| i.name == "github-token")
        .unwrap();
    assert_eq!(created.identity_id, token.id);
    h.start(&created.id).await;

    // The checkout ran by itself; the commit asks first, and until somebody
    // answers nothing has been written to the repository.
    let approval = h.pending_approval(&created.id, "commit_fixes").await;
    assert_eq!(approval.action_type, "repo.commit_fixes");
    let waiting = h.get(&created.id).await;
    assert_eq!(status_of(&waiting, "checkout"), StepStatus::Succeeded);
    assert!(output_of(&waiting, "checkout").contains(REPO));
    // No step names a repository: it is the one of the execution's project.
    let checkout = h.run_dir.join(&created.id);
    assert!(checkout.join("README.md").is_file());
    assert_eq!(repo.head(BRANCH), Some(reviewed.clone()));
    assert!(repo.writes().is_empty(), "{:?}", repo.writes());

    h.approve(&approval).await;
    let done = h.wait_status(&created.id, ExecutionStatus::Succeeded).await;
    // One commit on top of the reviewed head, and the run says which.
    let sha = done.outputs["commit_sha"].clone();
    assert_eq!(repo.head(BRANCH).as_deref(), Some(sha.as_str()));
    assert_eq!(
        repo.commit(&sha),
        (
            vec![reviewed.clone()],
            "Apply 1 review fix to #1".to_owned()
        )
    );
    assert_eq!(
        done.outputs["commit_url"],
        format!("https://github.com/{REPO}/commit/{sha}")
    );
    assert_eq!(repo.file(BRANCH, "src/lib.rs").as_deref(), Some(FIXED));
    assert_eq!(repo.pull(number).head_sha, sha);
    assert!(output_of(&done, "commit_fixes").contains("committed 1 file"));
    // The checkout is removed once the run is over (right after the status
    // is stored), and the token never reached an event.
    let deadline = tokio::time::Instant::now() + WAIT;
    while checkout.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} was left behind",
            checkout.display()
        );
        tokio::time::sleep(POLL).await;
    }
    let events = h.events(&created.id).await;
    assert!(events.iter().any(|e| e.message.contains("/git/commits")));
    assert!(
        events.iter().all(|e| !e.message.contains(fake::TOKEN)
            && e.data.values().all(|v| !v.contains(fake::TOKEN)))
    );

    // The author pushes; a second set of fixes, still written for the head
    // of the first review, must not land on top of that.
    let pushed = repo.push(BRANCH, &[("README.md", "# api v2\n")], "Update the readme");
    repo.push(BRANCH, &[("README.md", "# api v3\n")], "Again");
    let moved = repo.head(BRANCH).unwrap();
    let stale = h
        .run(
            "Apply 1 review fix to #1",
            fixes_plan(&sha, number),
            &[(
                "src/lib.rs",
                "pub fn add(a: i32, b: i32) -> i32 {\n    a.wrapping_add(b)\n}\n",
            )],
        )
        .await;
    let approval = h.pending_approval(&stale, "commit_fixes").await;
    h.approve(&approval).await;
    let failure = h.diagnosed(&stale, "commit_fixes").await;
    assert_eq!(failure.code, "HEAD_MOVED");
    assert!(
        failure.raw_output.contains("Nothing was committed"),
        "{failure:?}"
    );
    assert!(
        failure.diagnosis.contains(&moved[..7]),
        "{}",
        failure.diagnosis
    );
    assert!(
        failure.diagnosis.contains("cannot simply run again"),
        "{}",
        failure.diagnosis
    );
    // The run ends or is cancelled: retrying or skipping are not offered.
    assert_eq!(option_ids(&failure), ["give_up", "cancel"]);
    assert_eq!(
        failure.options[0].label,
        "Review the new head and ask again"
    );
    assert!(failure.options[0].recommended);
    assert_eq!(repo.head(BRANCH), Some(moved.clone()));
    assert_ne!(moved, pushed);

    h.resolve(&stale, "give_up").await;
    let failed = h.wait_status(&stale, ExecutionStatus::Failed).await;
    assert!(!failed.outputs.contains_key("commit_sha"));
    assert_eq!(repo.head(BRANCH), Some(moved));
    assert_eq!(repo.file(BRANCH, "src/lib.rs").as_deref(), Some(FIXED));

    h.teardown().await;
}

/// §23.3: `verify_mergeable` → `merge` → `delete_branch`. With every
/// default left alone the merge waits for a person; the one policy set to
/// `auto` is the only thing that takes the person out.
#[tokio::test]
async fn a_merge_waits_for_a_person_unless_its_policy_is_set_to_auto() {
    let (repo, number, head) = repository();
    repo.check_run(&head, "build", "completed", Some("success"));
    repo.status(&head, "ci/legacy", "success");
    let Some(h) = Harness::new(repo.clone()).await else {
        return;
    };

    let created = h
        .create(
            "Merge #1: Implement add",
            "github",
            merge_plan(BRANCH, &head, number),
            &[],
        )
        .await;
    let gates: Vec<(i32, i32)> = created
        .steps
        .iter()
        .map(|s| (s.risk, s.policy_decision))
        .collect();
    assert_eq!(
        gates,
        [
            (RiskLevel::Safe as i32, PolicyDecision::Auto as i32),
            (RiskLevel::High as i32, PolicyDecision::Approval as i32),
            (RiskLevel::Medium as i32, PolicyDecision::Auto as i32),
        ]
    );
    h.start(&created.id).await;

    // Verified without asking; merged only after somebody says so.
    let approval = h.pending_approval(&created.id, "merge").await;
    assert_eq!(approval.action_type, "repo.merge_pull_request");
    assert_eq!(approval.risk, RiskLevel::High as i32);
    let waiting = h.get(&created.id).await;
    assert_eq!(
        status_of(&waiting, "verify_mergeable"),
        StepStatus::Succeeded
    );
    assert!(output_of(&waiting, "verify_mergeable").contains("all 2 checks passed"));
    assert!(!repo.pull(number).merged);
    assert!(repo.merges().is_empty());

    h.approve(&approval).await;
    let done = h.wait_status(&created.id, ExecutionStatus::Succeeded).await;
    let merged = repo.pull(number);
    assert!(merged.merged);
    assert_eq!(done.outputs["merge_commit_sha"], merged.merge_commit_sha);
    assert_eq!(
        repo.head("main").as_deref(),
        Some(merged.merge_commit_sha.as_str())
    );
    assert_eq!(
        done.outputs["pull_request_url"],
        format!("https://github.com/{REPO}/pull/{number}")
    );
    assert_eq!(
        repo.merges(),
        [serde_json::json!({ "sha": head, "merge_method": "squash" })]
    );
    // The branch went with it, without a second approval.
    assert_eq!(repo.head(BRANCH), None);
    assert!(output_of(&done, "delete_branch").contains("deleted fix/lint"));
    assert_eq!(
        h.approvals(&created.id, ApprovalStatus::Unspecified)
            .await
            .len(),
        1
    );

    // A workspace that wants hands-free merges sets that one policy.
    h.set_policy("repo.merge_pull_request", PolicyDecision::Auto)
        .await;
    repo.branch("fix/docs", "main");
    let head = repo.push(
        "fix/docs",
        &[("README.md", "# api\n\nAdds numbers.\n")],
        "Explain",
    );
    let second = repo.open_pull("fix/docs", "Explain the crate");
    repo.check_run(&head, "build", "completed", Some("success"));
    let hands_free = h
        .run(
            "Merge #2: Explain the crate",
            merge_plan("fix/docs", &head, second),
            &[],
        )
        .await;
    let done = h.wait_status(&hands_free, ExecutionStatus::Succeeded).await;
    assert!(repo.pull(second).merged);
    assert_eq!(
        done.outputs["merge_commit_sha"],
        repo.pull(second).merge_commit_sha
    );
    assert_eq!(repo.head("fix/docs"), None);
    assert!(
        h.approvals(&hands_free, ApprovalStatus::Unspecified)
            .await
            .is_empty(),
        "nobody was asked"
    );
    assert_eq!(
        repo.file("main", "README.md").as_deref(),
        Some("# api\n\nAdds numbers.\n")
    );

    h.teardown().await;
}

/// A check that fails pauses the run before the merge is even asked for;
/// once it is green a retry carries on, through the same approval.
#[tokio::test]
async fn failing_checks_pause_the_merge_and_a_retry_carries_on_when_they_are_green() {
    let (repo, number, head) = repository();
    repo.check_run(&head, "build", "completed", Some("success"));
    repo.check_run(&head, "unit", "completed", Some("failure"));
    repo.check_run(&head, "e2e", "in_progress", None);
    let Some(h) = Harness::new(repo.clone()).await else {
        return;
    };

    let id = h
        .run(
            "Merge #1: Implement add",
            merge_plan(BRANCH, &head, number),
            &[],
        )
        .await;
    let failure = h.diagnosed(&id, "verify_mergeable").await;
    assert_eq!(failure.code, "CHECKS_FAILING");
    assert_eq!(
        failure.diagnosis,
        format!(
            "Pull request #1 is not ready to merge at {}: 1 check failed (unit) and 1 check is still running (e2e). Nothing was merged.",
            &head[..7]
        )
    );
    assert_eq!(failure.blocked_steps, ["merge", "delete_branch"]);
    assert_eq!(option_ids(&failure), ["retry", "give_up", "cancel"]);
    assert_eq!(failure.options[0].label, "Retry when the checks are green");
    assert!(failure.options[0].recommended);
    let paused = h.get(&id).await;
    assert_eq!(status_of(&paused, "merge"), StepStatus::Blocked);
    assert!(
        h.approvals(&id, ApprovalStatus::Unspecified)
            .await
            .is_empty()
    );
    assert!(!repo.pull(number).merged);
    assert!(repo.writes().is_empty(), "{:?}", repo.writes());

    // Still red: the retry stops at the same place.
    repo.check_run(&head, "e2e", "completed", Some("success"));
    h.resolve(&id, "retry").await;
    let failure = h.diagnosed(&id, "verify_mergeable").await;
    assert_eq!(failure.code, "CHECKS_FAILING");
    assert!(
        failure.diagnosis.contains("1 check failed (unit)"),
        "{}",
        failure.diagnosis
    );
    assert!(
        !failure.diagnosis.contains("still running"),
        "{}",
        failure.diagnosis
    );

    // Green: verified, then the merge asks a person as it always does.
    repo.check_run(&head, "unit", "completed", Some("success"));
    h.resolve(&id, "retry").await;
    let approval = h.pending_approval(&id, "merge").await;
    assert!(!repo.pull(number).merged);
    h.approve(&approval).await;
    let done = h.wait_status(&id, ExecutionStatus::Succeeded).await;
    assert!(repo.pull(number).merged);
    assert_eq!(done.revision, 2);
    assert_eq!(
        done.outputs["merge_commit_sha"],
        repo.pull(number).merge_commit_sha
    );

    // A conflict and a head that moved, on a second pull request.
    repo.branch("fix/docs", "main");
    let head = repo.push("fix/docs", &[("README.md", "# api docs\n")], "Docs");
    let second = repo.open_pull("fix/docs", "Docs");
    repo.update_pull(second, |pull| {
        pull.mergeable = Some(false);
        pull.mergeable_state = "dirty".into();
    });
    let id = h
        .run("Merge #2: Docs", merge_plan("fix/docs", &head, second), &[])
        .await;
    let failure = h.diagnosed(&id, "verify_mergeable").await;
    assert_eq!(failure.code, "MERGE_CONFLICT");
    assert_eq!(option_ids(&failure), ["retry", "give_up", "cancel"]);
    assert_eq!(
        failure.options[0].label,
        "Retry after the branch is updated"
    );
    assert!(
        failure.diagnosis.contains("conflicts with main"),
        "{}",
        failure.diagnosis
    );
    // Resolving the conflict added a commit: the retry sees a new head and
    // sends the run back to a person instead of merging unreviewed code.
    let resolved = repo.push(
        "fix/docs",
        &[("README.md", "# api docs, rebased\n")],
        "Resolve",
    );
    repo.update_pull(second, |pull| {
        pull.mergeable = Some(true);
        pull.mergeable_state = "clean".into();
    });
    h.resolve(&id, "retry").await;
    let failure = h.diagnosed(&id, "verify_mergeable").await;
    assert_eq!(failure.code, "HEAD_MOVED");
    assert!(
        failure.diagnosis.contains(&resolved[..7]),
        "{}",
        failure.diagnosis
    );
    assert_eq!(option_ids(&failure), ["give_up", "cancel"]);
    h.resolve(&id, "cancel").await;
    h.wait_status(&id, ExecutionStatus::Cancelled).await;
    assert!(!repo.pull(second).merged);
    assert_eq!(repo.head("fix/docs"), Some(resolved));

    h.teardown().await;
}

/// The service stops right after GitHub merged and before the step was
/// checkpointed. After the restart the step's outcome is unknown; running
/// it again finds the pull request merged and merges nothing twice.
#[tokio::test]
async fn a_restart_in_the_middle_of_a_merge_does_not_merge_twice() {
    let (repo, number, head) = repository();
    let Some(mut h) = Harness::new(repo.clone()).await else {
        return;
    };
    let id = h
        .run(
            "Merge #1: Implement add",
            merge_plan(BRANCH, &head, number),
            &[],
        )
        .await;
    h.pending_approval(&id, "merge").await;

    // What the crash leaves behind: the approval was given, the step was
    // running, GitHub did merge, and nothing of that outcome was stored.
    let merge = &merge_plan(BRANCH, &head, number)[1];
    let mut params: std::collections::BTreeMap<String, String> =
        merge.params.clone().into_iter().collect();
    params.insert("repo".into(), REPO.into());
    let lost = GithubAdapter::new(None, "http://127.0.0.1:1")
        .run(
            &StepInput {
                key: merge.key.clone(),
                title: merge.title.clone(),
                action_type: merge.action_type.clone(),
                adapter: "github".into(),
                params,
                attempt: 1,
            },
            &RunContext {
                github: Some(GithubAccess {
                    token: fake::TOKEN.into(),
                    api_base_url: h.api.clone(),
                }),
                ..Default::default()
            },
        )
        .await;
    assert!(
        matches!(lost, crate::adapters::StepOutcome::Succeeded { .. }),
        "{lost:?}"
    );
    let merge_commit = repo.pull(number).merge_commit_sha;
    let execution = Uuid::parse_str(&id).unwrap();
    for statement in [
        "UPDATE approvals SET status = 'APPROVED', decided_at = now() WHERE execution_id = $1",
        "UPDATE steps SET status = 'RUNNING', attempts = 1 WHERE execution_id = $1 AND key = 'merge'",
        "UPDATE executions SET status = 'EXECUTING' WHERE id = $1",
    ] {
        sqlx::query(statement)
            .bind(execution)
            .execute(&h.pool)
            .await
            .unwrap();
    }

    h.restarted();
    assert_eq!(h.rt.recover().await.unwrap(), 1);
    let failure = h.diagnosed(&id, "merge").await;
    assert_eq!(failure.code, "UNKNOWN");
    assert!(failure.diagnosis.contains("service restarted mid-step"));
    assert!(option_ids(&failure).contains(&"retry"));

    // The retry is gated like any run of the step, then finds the merge done.
    h.resolve(&id, "retry").await;
    let approval = h.pending_approval(&id, "merge").await;
    h.approve(&approval).await;
    let done = h.wait_status(&id, ExecutionStatus::Succeeded).await;
    assert_eq!(done.outputs["merge_commit_sha"], merge_commit);
    assert!(output_of(&done, "merge").contains("already merged"));
    assert!(output_of(&done, "merge").contains("nothing was merged again"));
    assert_eq!(repo.merges().len(), 1, "GitHub was asked to merge once");
    assert_eq!(repo.head("main"), Some(merge_commit));
    assert_eq!(repo.head(BRANCH), None);

    h.teardown().await;
}

/// §23.4: `checkout` → `enable_security_updates` → `open_pull_request`.
/// `repository:admin` is held by nobody, so the first run pauses for
/// access before a setting is touched; a configuration file that is
/// already on the default branch is skipped, not overwritten.
#[tokio::test]
async fn dependabot_waits_for_a_timed_admin_grant_and_skips_a_file_that_is_already_there() {
    let (repo, _, _) = repository();
    let Some(h) = Harness::new(repo.clone()).await else {
        return;
    };

    let id = h
        .run(
            "Turn on Dependabot for acme/api",
            dependabot_plan(),
            &[(DEPENDABOT, DEPENDABOT_YML)],
        )
        .await;
    // No seeded identity holds the permission: the step is refused before
    // it runs, and GitHub has not been asked to change anything.
    let failure = h.diagnosed(&id, "enable_security_updates").await;
    assert_eq!(failure.code, "PERMISSION_DENIED");
    assert_eq!(failure.missing_permissions, ["repository:admin"]);
    assert!(failure.raw_output.is_empty(), "nothing ran: {failure:?}");
    assert!(
        failure.diagnosis.contains("lacks repository:admin"),
        "{}",
        failure.diagnosis
    );
    assert!(
        failure.diagnosis.contains("Nothing ran"),
        "{}",
        failure.diagnosis
    );
    assert_eq!(failure.blocked_steps, ["open_pull_request"]);
    assert_eq!(
        failure.options.iter().find(|o| o.recommended).unwrap().id,
        "request_access"
    );
    for identity in h.identities().await {
        assert!(
            identity
                .grants
                .iter()
                .all(|g| g.permission != "repository:admin"),
            "{} holds repository:admin",
            identity.name
        );
    }
    assert!(!repo.alerts() && !repo.security_updates());
    assert!(repo.writes().is_empty(), "{:?}", repo.writes());

    // Access is granted for a while, and the step still asks a person.
    h.resolve(&id, "request_access").await;
    let approval = h.pending_approval(&id, "enable_security_updates").await;
    assert_eq!(approval.action_type, "repo.enable_security_updates");
    assert_eq!(approval.risk, RiskLevel::Medium as i32);
    let identities = h.identities().await;
    let token = identities
        .iter()
        .find(|i| i.name == "github-token")
        .unwrap();
    let grant = token
        .grants
        .iter()
        .find(|g| g.permission == "repository:admin")
        .expect("the timed grant");
    assert!(
        grant.expires_at.is_some(),
        "repository:admin is never a base scope"
    );
    assert!(!repo.alerts());

    h.approve(&approval).await;
    let approval = h.pending_approval(&id, "open_pull_request").await;
    assert!(repo.alerts() && repo.security_updates());
    let waiting = h.get(&id).await;
    assert_eq!(
        output_of(&waiting, "enable_security_updates"),
        "Dependabot alerts and Dependabot security updates are on for acme/api"
    );
    assert!(
        repo.pulls()
            .iter()
            .all(|pull| pull.head_ref != "sdi/dependabot")
    );

    h.approve(&approval).await;
    let done = h.wait_status(&id, ExecutionStatus::Succeeded).await;
    let opened = repo
        .pulls()
        .into_iter()
        .find(|pull| pull.head_ref == "sdi/dependabot")
        .expect("the pull request");
    assert_eq!(opened.title, "Turn on Dependabot version updates");
    assert_eq!(opened.base_ref, "main");
    assert_eq!(
        done.outputs["pull_request_url"],
        format!("https://github.com/{REPO}/pull/{}", opened.number)
    );
    assert_eq!(
        repo.file("sdi/dependabot", DEPENDABOT).as_deref(),
        Some(DEPENDABOT_YML)
    );
    assert_eq!(repo.file("main", DEPENDABOT), None);

    // Somebody merges that pull request. Asked again, the run turns the
    // (already on) settings on, finds the file and opens nothing.
    repo.push("main", &[(DEPENDABOT, DEPENDABOT_YML)], "Add Dependabot");
    let pulls = repo.pulls().len();
    let again = h
        .run(
            "Turn on Dependabot for acme/api",
            dependabot_plan(),
            &[(DEPENDABOT, "version: 2\nupdates: []\n")],
        )
        .await;
    // The grant is still live, so this run goes straight to the approvals.
    let approval = h.pending_approval(&again, "enable_security_updates").await;
    h.approve(&approval).await;
    let approval = h.pending_approval(&again, "open_pull_request").await;
    repo.forget_calls();
    h.approve(&approval).await;
    let done = h.wait_status(&again, ExecutionStatus::Succeeded).await;
    assert_eq!(
        output_of(&done, "open_pull_request"),
        "skipped: .github/dependabot.yml already exists on main of acme/api, so no branch, commit or pull request was created"
    );
    assert_eq!(status_of(&done, "open_pull_request"), StepStatus::Succeeded);
    assert!(
        !done.outputs.contains_key("pull_request_url"),
        "{:?}",
        done.outputs
    );
    assert_eq!(repo.pulls().len(), pulls);
    assert!(repo.writes().is_empty(), "{:?}", repo.writes());
    assert_eq!(
        repo.file("main", DEPENDABOT).as_deref(),
        Some(DEPENDABOT_YML)
    );

    h.teardown().await;
}

/// A step names no repository (§23.5): it is the one of the execution's
/// project. When knowledge does not know that project the step fails for
/// the missing repository, and the log says why it is missing; a plan that
/// does name one is taken at its word.
#[tokio::test]
async fn the_repository_is_the_one_of_the_executions_project() {
    let (repo, number, head) = repository();
    let Some(h) = Harness::new(repo.clone()).await else {
        return;
    };
    let unknown_project = |steps: Vec<PlanStep>| CreateExecutionRequest {
        project_id: "0192f6a0-0000-7000-8000-00000000ffff".into(),
        title: "Merge #1: Implement add".into(),
        steps,
        adapter: "github".into(),
        ..Default::default()
    };
    let created = h
        .svc
        .create_execution(admin(unknown_project(merge_plan(BRANCH, &head, number))))
        .await
        .unwrap()
        .into_inner();
    h.start(&created.id).await;
    let failure = h.diagnosed(&created.id, "verify_mergeable").await;
    assert_eq!(failure.code, "UNKNOWN");
    assert!(
        failure
            .raw_output
            .contains("params.repo (owner/name) is required for repo.verify_mergeable"),
        "{failure:?}"
    );
    assert!(
        failure.diagnosis.contains("params.repo"),
        "{}",
        failure.diagnosis
    );
    let events = h.events(&created.id).await;
    assert!(
        events.iter().any(|e| e
            .message
            .contains("could not look up the repository of the execution's project")),
        "{:?}",
        events.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    assert!(repo.calls().is_empty(), "{:?}", repo.calls());

    // The same plan with the repository on its steps (as the older github
    // plans carry it, under either name) needs no lookup.
    let mut steps = merge_plan(BRANCH, &head, number);
    steps[0].params.insert("repository".into(), REPO.into());
    steps[1].params.insert("repo".into(), REPO.into());
    steps.truncate(2);
    let created = h
        .svc
        .create_execution(admin(unknown_project(steps)))
        .await
        .unwrap()
        .into_inner();
    h.start(&created.id).await;
    let approval = h.pending_approval(&created.id, "merge").await;
    h.approve(&approval).await;
    h.wait_status(&created.id, ExecutionStatus::Succeeded).await;
    assert!(repo.pull(number).merged);

    h.teardown().await;
}

/// The `simulated` runtime dry-runs the same plans through the same gates
/// and never reaches GitHub.
#[tokio::test]
async fn the_simulated_runtime_dry_runs_the_pull_request_steps() {
    let (repo, number, head) = repository();
    let Some(h) = Harness::new(repo.clone()).await else {
        return;
    };
    let mut steps = merge_plan(BRANCH, &head, number);
    steps.extend(
        fixes_plan(&head, number)
            .into_iter()
            .skip(1)
            .map(|mut step| {
                step.depends_on = vec!["delete_branch".into()];
                step
            }),
    );
    steps.push(PlanStep {
        depends_on: vec!["commit_fixes".into()],
        ..dependabot_plan().remove(1)
    });
    let created = h
        .create("Rehearse", "", steps, &[("src/lib.rs", FIXED)])
        .await;
    assert_eq!(created.adapter, "simulated");
    h.start(&created.id).await;

    // The simulated identity may only read: writing and administering
    // need grants, and the policies ask a person, exactly as for real.
    let mut asked = Vec::new();
    let done = loop {
        let execution = h.get(&created.id).await;
        match ExecutionStatus::try_from(execution.status).unwrap() {
            ExecutionStatus::Succeeded => break execution,
            ExecutionStatus::Diagnosing => {
                let failure = execution.failure.unwrap();
                assert_eq!(failure.code, "PERMISSION_DENIED", "{failure:?}");
                h.resolve(&created.id, "request_access").await;
            }
            ExecutionStatus::WaitingForApproval => {
                let pending = h.approvals(&created.id, ApprovalStatus::Pending).await;
                if let Some(approval) = pending.first() {
                    asked.push(approval.action_type.clone());
                    h.approve(approval).await;
                }
            }
            ExecutionStatus::Failed | ExecutionStatus::Cancelled => {
                panic!("the rehearsal ended as {:?}", execution.status)
            }
            _ => tokio::time::sleep(POLL).await,
        }
    };
    assert_eq!(
        asked,
        [
            "repo.merge_pull_request",
            "repo.commit_fixes",
            "repo.enable_security_updates",
        ]
    );
    assert!(output_of(&done, "merge").contains("Nothing was merged (dry run"));
    assert!(output_of(&done, "commit_fixes").contains("Nothing was committed (dry run"));
    assert!(output_of(&done, "delete_branch").contains("Nothing was deleted (dry run"));
    assert!(
        output_of(&done, "enable_security_updates").contains("No repository setting was changed")
    );
    // A dry run produces no commit, no merge and no link, and GitHub never
    // heard of it.
    assert!(done.outputs.is_empty(), "{:?}", done.outputs);
    assert!(repo.calls().is_empty(), "{:?}", repo.calls());
    assert!(!repo.pull(number).merged);
    assert_eq!(repo.head(BRANCH), Some(head));

    h.teardown().await;
}

/// §5.2: the five rows, for a new workspace (the seed), for the workspace
/// the migrations create rows for, and for one that existed before them.
#[tokio::test]
async fn the_autopilot_policies_follow_the_contract_and_reach_existing_workspaces() {
    let (repo, _, _) = repository();
    let Some(h) = Harness::new(repo).await else {
        return;
    };
    let expected = [
        (
            "repo.commit_fixes",
            "Commit review fixes to a pull request branch",
            RiskLevel::Medium,
            PolicyDecision::Approval,
            true,
        ),
        (
            "repo.verify_mergeable",
            "Check that a pull request can be merged",
            RiskLevel::Safe,
            PolicyDecision::Auto,
            false,
        ),
        (
            "repo.merge_pull_request",
            "Merge a pull request",
            RiskLevel::High,
            PolicyDecision::Approval,
            true,
        ),
        (
            "repo.delete_branch",
            "Delete a merged branch",
            RiskLevel::Medium,
            PolicyDecision::Auto,
            true,
        ),
        (
            "repo.enable_security_updates",
            "Turn on Dependabot alerts and security updates",
            RiskLevel::Medium,
            PolicyDecision::Approval,
            true,
        ),
    ];
    let check = |policies: &[Policy]| {
        for (action_type, label, risk, decision, editable) in expected {
            let policy = policies
                .iter()
                .find(|p| p.action_type == action_type)
                .unwrap_or_else(|| panic!("no policy for {action_type}"));
            assert_eq!(policy.label, label);
            assert_eq!(policy.risk, risk as i32, "{action_type}");
            assert_eq!(policy.decision, decision as i32, "{action_type}");
            assert_eq!(policy.editable, editable, "{action_type}");
        }
    };

    // A workspace on its first use.
    let policies = h.policies().await;
    assert_eq!(policies.len(), crate::db::policies::SEED.len());
    check(&policies);
    // Verifying is safe and stays automatic; the others can be changed.
    let locked = h
        .svc
        .update_policy(admin(UpdatePolicyRequest {
            action_type: "repo.verify_mergeable".into(),
            decision: PolicyDecision::Approval as i32,
        }))
        .await
        .unwrap_err();
    assert_eq!(locked.code(), Code::FailedPrecondition);
    h.set_policy("repo.delete_branch", PolicyDecision::Approval)
        .await;

    // The workspace whose rows the migrations wrote has exactly the seed.
    let bootstrap: Vec<(String, String, String, String, bool)> = sqlx::query_as(
        "SELECT action_type, label, risk, decision, editable FROM policies \
         WHERE workspace_id = '00000000-0000-7000-8000-00000000dead' ORDER BY action_type",
    )
    .fetch_all(&h.pool)
    .await
    .unwrap();
    let mut seed: Vec<(String, String, String, String, bool)> = crate::db::policies::SEED
        .iter()
        .map(|(a, l, r, d, e)| {
            (
                (*a).to_owned(),
                (*l).to_owned(),
                (*r).to_owned(),
                (*d).to_owned(),
                *e,
            )
        })
        .collect();
    seed.sort();
    assert_eq!(bootstrap, seed);

    // A workspace from before the feature: it has its own copy without the
    // five rows, and an edit of its own. The migration's statement adds
    // what is missing and leaves the rest alone.
    let workspace = Uuid::parse_str(WS).unwrap();
    sqlx::query("DELETE FROM policies WHERE workspace_id = $1 AND action_type = ANY($2)")
        .bind(workspace)
        .bind(
            expected
                .iter()
                .filter(|(action_type, ..)| *action_type != "repo.delete_branch")
                .map(|(action_type, ..)| (*action_type).to_owned())
                .collect::<Vec<_>>(),
        )
        .execute(&h.pool)
        .await
        .unwrap();
    assert_eq!(
        h.policies().await.len(),
        crate::db::policies::SEED.len() - 4
    );
    let migration = include_str!("../../migrations/0006_pull_request_autopilot.sql");
    sqlx::raw_sql(sqlx::AssertSqlSafe(migration))
        .execute(&h.pool)
        .await
        .unwrap();
    let policies = h.policies().await;
    assert_eq!(policies.len(), crate::db::policies::SEED.len());
    let kept = policies
        .iter()
        .find(|p| p.action_type == "repo.delete_branch")
        .unwrap();
    assert_eq!(
        kept.decision,
        PolicyDecision::Approval as i32,
        "a row the workspace already has keeps its decision"
    );
    h.set_policy("repo.delete_branch", PolicyDecision::Auto)
        .await;
    check(&h.policies().await);

    // The same through the seed, which runs on a workspace's first use
    // after every start.
    sqlx::query(
        "DELETE FROM policies WHERE workspace_id = $1 AND action_type = 'repo.merge_pull_request'",
    )
    .bind(workspace)
    .execute(&h.pool)
    .await
    .unwrap();
    h.rt.forget_workspace(workspace);
    check(&h.policies().await);

    h.teardown().await;
}
