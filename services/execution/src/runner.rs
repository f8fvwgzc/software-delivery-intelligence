//! One tokio task per live execution. The task is the only writer of its
//! execution's rows, so gRPC handlers never race it: they send a command
//! and wait for the reply. The loop is driven by the persisted status, which
//! is also how an execution resumes after a restart.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::identity::AuditSink;

use chrono::Utc;
use sdi_common::{grpc::db_status, principal::Principal};
use sdi_proto::execution::v1::{
    EventType, ExecutionEvent, ExecutionStatus, PolicyDecision, StepStatus,
};
use sqlx::PgPool;
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    time::Instant,
};
use tonic::Status;
use uuid::Uuid;

use crate::{
    adapters::{
        self, Fault, LogSink, OutputSink, Redactor, Registry, RunContext, Secrets, StepInput,
        StepOutcome,
    },
    convert,
    db::{
        self, Tx,
        events::{EventRow, NewEvent},
        executions::ExecutionRow,
        steps::StepRow,
    },
    delivery::{self, Delivery},
    domain::{
        dag,
        diagnosis::{self, FailedStep, FailureRecord, Resolution},
        policy::{self, Gate},
        state,
    },
    events::PlatformEvents,
    identity::GithubAccessSource,
    knowledge::PatternOutcomes,
};

pub const GRANT_TTL_MINUTES: i64 = 30;
const BROADCAST_CAPACITY: usize = 512;
const COMMAND_CAPACITY: usize = 16;
/// Adapter log lines are coalesced into one LOG event per window, which
/// caps the event rate at 20/s however chatty a build is.
const LOG_WINDOW: Duration = Duration::from_millis(50);
/// Lines buffered between the adapter and the runner; a faster producer
/// loses lines here but keeps them in the failure's raw output.
const LOG_CHANNEL: usize = 4096;
/// Outputs a step may report while it runs before the oldest waits.
const PROGRESS_CHANNEL: usize = 16;
/// Characters of the tool's first line a diagnosis may quote: enough for a
/// sentence that names a pull request, a commit and ten checks.
const DETAIL_CHARS: usize = 600;

pub type Reply = oneshot::Sender<Result<(), Status>>;

pub enum Command {
    Start {
        fault: Option<Fault>,
        reply: Reply,
    },
    ApprovalDecided {
        approval_id: Uuid,
        reply: Reply,
    },
    Resolve {
        option_id: String,
        instruction: String,
        /// `x-sdi-user` of whoever chose the remediation.
        by: String,
        reply: Reply,
    },
    Cancel {
        reason: String,
        reply: Reply,
    },
}

impl Command {
    fn reject(self, why: &str) {
        let reply = match self {
            Command::Start { reply, .. }
            | Command::ApprovalDecided { reply, .. }
            | Command::Resolve { reply, .. }
            | Command::Cancel { reply, .. } => reply,
        };
        let _ = reply.send(Err(Status::failed_precondition(why)));
    }
}

/// Live event fan-out, one broadcast channel per execution. Senders are
/// looked up per publish, so a channel exists only while someone watches
/// or the runner is alive.
#[derive(Clone, Default)]
pub struct EventBus {
    channels: Arc<Mutex<HashMap<Uuid, broadcast::Sender<ExecutionEvent>>>>,
}

impl EventBus {
    pub fn subscribe(&self, id: Uuid) -> broadcast::Receiver<ExecutionEvent> {
        let mut channels = self.channels.lock().expect("event bus lock");
        channels.retain(|other, sender| *other == id || sender.receiver_count() > 0);
        channels
            .entry(id)
            .or_insert_with(|| broadcast::channel(BROADCAST_CAPACITY).0)
            .subscribe()
    }

    pub fn publish(&self, event: ExecutionEvent) {
        let Ok(id) = Uuid::parse_str(&event.execution_id) else {
            return;
        };
        if let Some(sender) = self.channels.lock().expect("event bus lock").get(&id) {
            let _ = sender.send(event);
        }
    }

    fn remove(&self, id: Uuid) {
        self.channels.lock().expect("event bus lock").remove(&id);
    }
}

/// Everything the runners and the gRPC layer share.
pub struct Runtime {
    pub pool: PgPool,
    pub adapters: Registry,
    pub outcomes: Arc<dyn PatternOutcomes>,
    pub audit: Arc<dyn AuditSink>,
    pub bus: EventBus,
    /// Execution/approval change notifications for `WatchEvents`.
    pub platform: PlatformEvents,
    /// How long the simulated verification phase takes.
    pub verify_delay: Duration,
    /// Whether `GITHUB_TOKEN` is configured. With it every workspace gets
    /// the `github-token` identity; without it a workspace gets one when it
    /// has GitHub access of its own (CONTRACT.md §5.3).
    pub github_token: bool,
    /// Per-workspace GitHub access for the real adapters (CONTRACT.md §18).
    pub github_access: Arc<dyn GithubAccessSource>,
    /// What continuous delivery needs from the other services (CONTRACT.md §22).
    pub delivery: Delivery,
    runners: Mutex<HashMap<Uuid, mpsc::Sender<Command>>>,
    /// Workspaces whose policy and identity seed was checked this process.
    /// Bounded: past `SEEDED_MAX` entries the map is cleared, and the seed
    /// (idempotent) simply runs again for the next request.
    seeded: Mutex<HashMap<Uuid, Seeded>>,
}

/// What a workspace's last seed left behind.
#[derive(Debug, Clone, Copy)]
enum Seeded {
    /// Policies and every identity, the `github-token` one included.
    Complete,
    /// Everything but the `github-token` identity: the workspace had no
    /// GitHub access when it was checked. Looked at again after
    /// `GITHUB_RECHECK`, so installing the App or signing in later is
    /// noticed without a restart.
    WithoutGithub(std::time::Instant),
}

/// Workspaces remembered as seeded before the map is reset.
const SEEDED_MAX: usize = 10_000;
/// How long "this workspace has no GitHub access" is believed.
const GITHUB_RECHECK: Duration = Duration::from_secs(5 * 60);

impl Runtime {
    /// A runtime without the delivery peers: the delivery RPCs that need
    /// the identity or knowledge service answer UNAVAILABLE (tests).
    #[cfg(test)]
    pub fn new(
        pool: PgPool,
        adapters: Registry,
        outcomes: Arc<dyn PatternOutcomes>,
        audit: Arc<dyn AuditSink>,
        verify_delay: Duration,
        github_token: bool,
        github_access: Arc<dyn GithubAccessSource>,
    ) -> Arc<Self> {
        Self::with_delivery(
            pool,
            adapters,
            outcomes,
            audit,
            verify_delay,
            github_token,
            github_access,
            Delivery::disconnected(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_delivery(
        pool: PgPool,
        adapters: Registry,
        outcomes: Arc<dyn PatternOutcomes>,
        audit: Arc<dyn AuditSink>,
        verify_delay: Duration,
        github_token: bool,
        github_access: Arc<dyn GithubAccessSource>,
        delivery: Delivery,
    ) -> Arc<Self> {
        Arc::new(Self {
            pool,
            adapters,
            outcomes,
            audit,
            bus: EventBus::default(),
            platform: PlatformEvents::default(),
            verify_delay,
            github_token,
            github_access,
            delivery,
            runners: Mutex::new(HashMap::new()),
            seeded: Mutex::new(HashMap::new()),
        })
    }

    /// Workspaces currently remembered as seeded (tests).
    #[cfg(test)]
    pub fn seeded_workspaces(&self) -> usize {
        self.seeded.lock().expect("seeded lock").len()
    }

    /// Copies the §5.2 policies and creates the §5.3 identities for a
    /// workspace on its first use. Checked once per process per workspace;
    /// the seed itself is idempotent.
    pub async fn ensure_workspace(&self, workspace_id: Uuid) -> Result<(), Status> {
        let known = self
            .seeded
            .lock()
            .expect("seeded lock")
            .get(&workspace_id)
            .copied();
        match known {
            Some(Seeded::Complete) => return Ok(()),
            Some(Seeded::WithoutGithub(at)) if at.elapsed() < GITHUB_RECHECK => return Ok(()),
            _ => {}
        }
        // The instance-wide token covers every workspace. Without it the
        // workspace's own access decides: an installed App or a signed-in
        // account (asked of identity, which caches what it mints).
        let github = self.github_token || self.github_access.access(workspace_id).await.is_some();
        let mut tx = self.pool.begin().await.map_err(db_status)?;
        db::policies::seed(&mut tx, workspace_id)
            .await
            .map_err(db_status)?;
        db::identities::seed(&mut tx, workspace_id, github)
            .await
            .map_err(db_status)?;
        tx.commit().await.map_err(db_status)?;
        let mut seeded = self.seeded.lock().expect("seeded lock");
        if seeded.len() >= SEEDED_MAX {
            seeded.clear();
        }
        seeded.insert(
            workspace_id,
            if github {
                Seeded::Complete
            } else {
                Seeded::WithoutGithub(std::time::Instant::now())
            },
        );
        Ok(())
    }

    /// The runner for `id`, spawned on first use. Idempotent.
    pub fn ensure_runner(self: &Arc<Self>, workspace: Uuid, id: Uuid) -> mpsc::Sender<Command> {
        let mut runners = self.runners.lock().expect("runners lock");
        if let Some(sender) = runners.get(&id)
            && !sender.is_closed()
        {
            return sender.clone();
        }
        let (sender, receiver) = mpsc::channel(COMMAND_CAPACITY);
        runners.insert(id, sender.clone());
        let runner = Runner {
            rt: Arc::clone(self),
            workspace,
            id,
            rx: receiver,
        };
        sdi_common::tasks::spawn("execution-runner", runner.run());
        sender
    }

    /// Sends a command and waits for the runner's verdict.
    pub async fn command(
        self: &Arc<Self>,
        workspace: Uuid,
        id: Uuid,
        build: impl FnOnce(Reply) -> Command,
    ) -> Result<(), Status> {
        let (reply, verdict) = oneshot::channel();
        self.ensure_runner(workspace, id)
            .send(build(reply))
            .await
            .map_err(|_| Status::unavailable("execution runner is shutting down"))?;
        verdict
            .await
            .map_err(|_| Status::internal("execution runner dropped the request"))?
    }

    /// Resumes every execution that was mid-flight when the service last
    /// stopped. EXECUTING/VERIFYING ones become PAUSED with an UNKNOWN
    /// diagnosis because the step's real outcome is lost; whatever a real
    /// adapter left running for them (containers named `sdi-<id>`) is
    /// stopped in the background. Their workdir stays: the diagnosis offers
    /// a retry of the interrupted step, and `container.build` needs the
    /// checkout that `repo.checkout` (already SUCCEEDED, never re-run)
    /// produced. The directory goes when the run ends or is cancelled.
    pub async fn recover(self: &Arc<Self>) -> sqlx::Result<usize> {
        let statuses = [
            ExecutionStatus::WaitingForApproval,
            ExecutionStatus::Authorized,
            ExecutionStatus::Executing,
            ExecutionStatus::Verifying,
            ExecutionStatus::Paused,
            ExecutionStatus::Diagnosing,
            ExecutionStatus::Replanning,
        ]
        .map(convert::status_to_db);
        let rows = db::executions::with_status(&self.pool, &statuses).await?;
        for row in &rows {
            tracing::info!(execution = %row.id, status = %row.status, "resuming execution");
            let mid_step = matches!(
                convert::status_from_db(&row.status),
                ExecutionStatus::Executing | ExecutionStatus::Verifying
            );
            // A deployment run leaves nothing of its own on this host.
            if mid_step && row.adapter != adapters::SIMULATED && row.adapter != adapters::DELIVERY {
                let adapters = self.adapters.clone();
                let id = row.id;
                sdi_common::tasks::spawn("recovery-stop", async move {
                    for adapter in adapters.real() {
                        adapter.stop(id).await;
                    }
                });
            }
            self.ensure_runner(row.workspace_id, row.id);
        }
        Ok(rows.len())
    }

    /// Drops the seed note for a workspace whose rows were deleted, so a
    /// later use seeds it again.
    pub fn forget_workspace(&self, workspace_id: Uuid) {
        self.seeded
            .lock()
            .expect("seeded lock")
            .remove(&workspace_id);
        self.delivery.forget_workspace(workspace_id);
    }

    fn forget(&self, id: Uuid, terminal: bool) {
        self.runners.lock().expect("runners lock").remove(&id);
        if terminal {
            self.bus.remove(id);
        }
    }
}

enum Flow {
    Continue,
    Stop,
}

/// A transaction holding the execution row lock plus the events to publish
/// once it commits.
struct Batch {
    tx: Tx,
    exec: ExecutionRow,
    events: Vec<EventRow>,
    /// Status when the batch opened, to tell the platform feed what changed.
    initial_status: String,
    /// The batch merged outputs into the execution (a deployment run reads
    /// its link and the verified digest from them).
    outputs_changed: bool,
}

impl Batch {
    fn status(&self) -> ExecutionStatus {
        convert::status_from_db(&self.exec.status)
    }

    /// Moves the execution to `to`, refusing illegal transitions.
    async fn transition(&mut self, to: ExecutionStatus) -> Result<(), Status> {
        state::execution_transition(self.status(), to)
            .map_err(|e| Status::failed_precondition(e.to_string()))?;
        db::executions::set_status(
            &mut self.tx,
            self.exec.workspace_id,
            self.exec.id,
            convert::status_to_db(to),
        )
        .await
        .map_err(db_status)?;
        self.exec.status = convert::status_to_db(to).to_owned();
        Ok(())
    }

    async fn step_status(&mut self, step: &StepRow, to: StepStatus) -> Result<(), Status> {
        state::step_transition(convert::step_status_from_db(&step.status), to)
            .map_err(|e| Status::failed_precondition(format!("step {}: {e}", step.key)))?;
        db::steps::set_status(
            &mut self.tx,
            self.exec.workspace_id,
            step.id,
            convert::step_status_to_db(to),
        )
        .await
        .map_err(db_status)
    }

    async fn event(
        &mut self,
        kind: EventType,
        step_key: &str,
        message: impl Into<String>,
        data: BTreeMap<String, String>,
    ) -> Result<(), Status> {
        let row = db::events::append(
            &mut self.tx,
            NewEvent {
                workspace_id: self.exec.workspace_id,
                execution_id: self.exec.id,
                r#type: convert::event_type_to_db(kind),
                step_key,
                message: message.into(),
                status: &self.exec.status,
                data,
            },
        )
        .await
        .map_err(db_status)?;
        self.events.push(row);
        Ok(())
    }

    /// Adds `outputs` to the execution's.
    async fn merge_outputs(&mut self, outputs: &BTreeMap<String, String>) -> Result<(), Status> {
        if outputs.is_empty() {
            return Ok(());
        }
        db::executions::merge_outputs(&mut self.tx, self.exec.workspace_id, self.exec.id, outputs)
            .await
            .map_err(db_status)?;
        self.outputs_changed = true;
        Ok(())
    }

    /// Commits, then fans the events out: the per-execution log to
    /// `WatchExecution` watchers and, when something other than adapter
    /// output happened, an `execution updated` (plus `approval created` for
    /// each approval requested) to the platform feed.
    ///
    /// A deployment run's status is brought in line with its execution
    /// inside the same transaction (CONTRACT.md §22), so the two can never
    /// disagree; what that changed is announced after the commit.
    async fn commit(mut self, rt: &Arc<Runtime>) -> Result<(), Status> {
        let log_type = convert::event_type_to_db(EventType::Log);
        let requested_type = convert::event_type_to_db(EventType::ApprovalRequested);
        let notable = self.exec.status != self.initial_status
            || self.events.iter().any(|e| e.r#type != log_type);
        let projected = if notable || self.outputs_changed {
            delivery::projection::project(&mut self.tx, &self.exec)
                .await
                .map_err(db_status)?
        } else {
            None
        };
        self.tx.commit().await.map_err(db_status)?;
        if let Some(projected) = projected {
            delivery::projection::announce(rt, projected);
        }
        let mut approvals = Vec::new();
        let mut step_key = String::new();
        for row in self.events {
            if row.r#type == requested_type
                && let Some(id) = row.data.0.get("approval_id")
            {
                approvals.push(id.clone());
            }
            if !row.step_key.is_empty() {
                step_key = row.step_key.clone();
            }
            rt.bus.publish(convert::event_to_proto(row));
        }
        let id = self.exec.id.to_string();
        if notable {
            let mut info =
                BTreeMap::from([("revision".to_owned(), self.exec.revision.to_string())]);
            if !step_key.is_empty() {
                info.insert("step_key".to_owned(), step_key);
            }
            rt.platform.execution(
                self.exec.workspace_id,
                &id,
                &self.exec.project_id,
                "updated",
                &self.exec.status.to_ascii_lowercase(),
                info,
            );
        }
        for approval in approvals {
            rt.platform.approval(
                self.exec.workspace_id,
                &approval,
                &id,
                &self.exec.project_id,
                "created",
                "pending",
            );
        }
        Ok(())
    }
}

fn data<const N: usize>(pairs: [(&str, String); N]) -> BTreeMap<String, String> {
    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

fn nodes(steps: &[StepRow]) -> Vec<dag::Node<'_>> {
    steps
        .iter()
        .map(|s| dag::Node {
            key: &s.key,
            depends_on: &s.depends_on,
        })
        .collect()
}

fn step_is(step: &StepRow, status: StepStatus) -> bool {
    convert::step_status_from_db(&step.status) == status
}

/// How a running step's select loop ended.
enum StepLoop {
    Done(StepOutcome),
    Cancel { reason: String, reply: Reply },
    Stop,
}

struct Runner {
    rt: Arc<Runtime>,
    workspace: Uuid,
    id: Uuid,
    rx: mpsc::Receiver<Command>,
}

impl Runner {
    async fn run(mut self) {
        let terminal = loop {
            let exec = match db::executions::get(&self.rt.pool, self.workspace, self.id).await {
                Ok(exec) => exec,
                Err(error) => {
                    tracing::error!(execution = %self.id, %error, "runner cannot load execution");
                    break false;
                }
            };
            let status = convert::status_from_db(&exec.status);
            let outcome = match status {
                ExecutionStatus::Planning => self.wait_for_start().await,
                ExecutionStatus::WaitingForApproval => self.wait_for_approval(&exec).await,
                ExecutionStatus::Authorized | ExecutionStatus::Replanning => {
                    self.advance(&exec).await
                }
                ExecutionStatus::Executing | ExecutionStatus::Verifying => {
                    self.recover_crashed(&exec).await
                }
                ExecutionStatus::Paused => self.diagnose(&exec).await,
                ExecutionStatus::Diagnosing => self.wait_for_resolution(&exec).await,
                ExecutionStatus::Succeeded
                | ExecutionStatus::Failed
                | ExecutionStatus::Cancelled => break true,
                ExecutionStatus::Unspecified => {
                    tracing::error!(execution = %self.id, status = %exec.status, "unknown status");
                    break false;
                }
            };
            match outcome {
                Ok(Flow::Continue) => {}
                Ok(Flow::Stop) => break false,
                Err(error) => {
                    tracing::error!(execution = %self.id, %error, "runner stopped");
                    break false;
                }
            }
        };
        self.rt.forget(self.id, terminal);
    }

    async fn begin(&self) -> Result<Batch, Status> {
        let mut tx = self.rt.pool.begin().await.map_err(db_status)?;
        let exec = db::executions::lock(&mut tx, self.workspace, self.id)
            .await
            .map_err(db_status)?;
        Ok(Batch {
            tx,
            initial_status: exec.status.clone(),
            exec,
            events: Vec::new(),
            outputs_changed: false,
        })
    }

    async fn steps(&self) -> Result<Vec<StepRow>, Status> {
        db::steps::list_for(&self.rt.pool, self.workspace, self.id)
            .await
            .map_err(db_status)
    }

    /// Closes the loop with knowledge: the pattern's success rate learns
    /// from every finished run. A missing peer is logged, never fatal. The
    /// runner is background work, so it calls as the workspace's system
    /// principal (CONTRACT.md §18), not as whoever started the run.
    async fn record_outcome(&self, exec: &ExecutionRow, succeeded: bool) {
        if exec.pattern_id.is_empty() {
            return;
        }
        let principal = Principal::system(self.workspace.to_string());
        if let Err(error) = self
            .rt
            .outcomes
            .record(&principal, &exec.pattern_id, &exec.project_id, succeeded)
            .await
        {
            tracing::warn!(execution = %self.id, pattern = %exec.pattern_id, %error, "could not record pattern outcome");
        }
    }

    // --- waiting states -----------------------------------------------------

    async fn wait_for_start(&mut self) -> Result<Flow, Status> {
        loop {
            let Some(command) = self.rx.recv().await else {
                return Ok(Flow::Stop);
            };
            match command {
                Command::Start { fault, reply } => {
                    let result = self.start(fault).await;
                    let started = result.is_ok();
                    let _ = reply.send(result);
                    if started {
                        return Ok(Flow::Continue);
                    }
                }
                Command::Cancel { reason, reply } => {
                    let _ = reply.send(self.cancel(&reason).await);
                    return Ok(Flow::Continue);
                }
                other => other.reject("execution has not started"),
            }
        }
    }

    async fn wait_for_approval(&mut self, exec: &ExecutionRow) -> Result<Flow, Status> {
        let steps = self.steps().await?;
        let Some(step) = steps
            .iter()
            .find(|s| step_is(s, StepStatus::WaitingApproval))
            .cloned()
        else {
            let mut batch = self.begin().await?;
            batch.transition(ExecutionStatus::Authorized).await?;
            batch
                .event(
                    EventType::Log,
                    "",
                    "no pending approval found; resuming",
                    BTreeMap::new(),
                )
                .await?;
            batch.commit(&self.rt).await?;
            return Ok(Flow::Continue);
        };
        let Some(approval_id) = step.approval_id else {
            return Err(Status::internal(format!(
                "step {} waits without an approval",
                step.key
            )));
        };

        // Decided while no runner was alive (restart): act on it now.
        let approval = db::approvals::get(&self.rt.pool, self.workspace, approval_id)
            .await
            .map_err(db_status)?;
        if approval.status != "PENDING" {
            self.apply_decision(exec, &step, approval_id).await?;
            return Ok(Flow::Continue);
        }

        loop {
            let Some(command) = self.rx.recv().await else {
                return Ok(Flow::Stop);
            };
            match command {
                Command::ApprovalDecided {
                    approval_id: decided,
                    reply,
                } if decided == approval_id => {
                    let _ = reply.send(self.apply_decision(exec, &step, approval_id).await);
                    return Ok(Flow::Continue);
                }
                Command::ApprovalDecided { reply, .. } => {
                    let _ = reply.send(Err(Status::failed_precondition(
                        "execution is waiting for a different approval",
                    )));
                }
                Command::Cancel { reason, reply } => {
                    let _ = reply.send(self.cancel(&reason).await);
                    return Ok(Flow::Continue);
                }
                other => other.reject("execution is waiting for approval"),
            }
        }
    }

    async fn apply_decision(
        &self,
        exec: &ExecutionRow,
        step: &StepRow,
        approval_id: Uuid,
    ) -> Result<(), Status> {
        let approval = db::approvals::get(&self.rt.pool, self.workspace, approval_id)
            .await
            .map_err(db_status)?;
        let mut batch = self.begin().await?;
        let info = data([
            ("approval_id", approval_id.to_string()),
            ("decision", approval.status.to_ascii_lowercase()),
            ("decided_by", approval.decided_by.clone()),
            ("note", approval.note.clone()),
        ]);
        let by = if approval.decided_by.is_empty() {
            "a human".to_owned()
        } else {
            approval.decided_by.clone()
        };
        match approval.status.as_str() {
            "APPROVED" => {
                batch.transition(ExecutionStatus::Authorized).await?;
                let message = format!("'{}' approved by {by}", step.title);
                batch
                    .event(EventType::ApprovalDecided, &step.key, message, info)
                    .await?;
            }
            "REJECTED" => {
                batch.transition(ExecutionStatus::Cancelled).await?;
                db::steps::cancel_open(&mut batch.tx, self.workspace, exec.id)
                    .await
                    .map_err(db_status)?;
                db::approvals::expire_pending(&mut batch.tx, self.workspace, exec.id)
                    .await
                    .map_err(db_status)?;
                let note = if approval.note.is_empty() {
                    String::new()
                } else {
                    format!(": {}", approval.note)
                };
                let message = format!(
                    "'{}' rejected by {by}{note}; execution cancelled",
                    step.title
                );
                batch
                    .event(EventType::ApprovalDecided, &step.key, message, info)
                    .await?;
            }
            _ => {
                // Expired underneath us: the step goes back in line.
                batch.transition(ExecutionStatus::Authorized).await?;
                db::steps::set_approval(
                    &mut batch.tx,
                    self.workspace,
                    step.id,
                    None,
                    convert::step_status_to_db(StepStatus::Pending),
                )
                .await
                .map_err(db_status)?;
                batch
                    .event(
                        EventType::Log,
                        &step.key,
                        "approval expired; step re-queued",
                        info,
                    )
                    .await?;
            }
        }
        batch.commit(&self.rt).await
    }

    async fn wait_for_resolution(&mut self, exec: &ExecutionRow) -> Result<Flow, Status> {
        loop {
            let Some(command) = self.rx.recv().await else {
                return Ok(Flow::Stop);
            };
            match command {
                Command::Resolve {
                    option_id,
                    instruction,
                    by,
                    reply,
                } => {
                    let result = self.resolve(exec, &option_id, &instruction, &by).await;
                    let resolved = result.is_ok();
                    let _ = reply.send(result);
                    if resolved {
                        return Ok(Flow::Continue);
                    }
                }
                Command::Cancel { reason, reply } => {
                    let _ = reply.send(self.cancel(&reason).await);
                    return Ok(Flow::Continue);
                }
                other => other.reject("execution is waiting for a remediation"),
            }
        }
    }

    // --- transitions driven by commands -------------------------------------

    async fn start(&self, fault: Option<Fault>) -> Result<(), Status> {
        let mut batch = self.begin().await?;
        if batch.status() != ExecutionStatus::Planning {
            return Err(Status::failed_precondition(format!(
                "execution is {}, only PLANNING executions can be started",
                batch.exec.status
            )));
        }
        db::executions::set_fault(&mut batch.tx, self.workspace, self.id, fault.as_ref())
            .await
            .map_err(db_status)?;
        batch.transition(ExecutionStatus::Authorized).await?;
        let mut info = BTreeMap::new();
        let mut message = "execution started".to_owned();
        if let Some(fault) = &fault {
            info.insert("fault_step".to_owned(), fault.step_key.clone());
            info.insert("fault_code".to_owned(), fault.code.clone());
            message.push_str(&format!(
                " with fault {} on step {}",
                fault.code, fault.step_key
            ));
        }
        batch
            .event(EventType::StateChanged, "", message, info)
            .await?;
        batch.commit(&self.rt).await
    }

    async fn cancel(&self, reason: &str) -> Result<(), Status> {
        // A cancelled docker run must leave neither its container nor its
        // workdir behind, whichever state it was cancelled from: a run paused
        // in diagnosis still holds the checkout on disk.
        let adapter_name = db::executions::get(&self.rt.pool, self.workspace, self.id)
            .await
            .map_err(db_status)?
            .adapter;
        self.cleanup_real(&adapter_name).await;
        let mut batch = self.begin().await?;
        batch.transition(ExecutionStatus::Cancelled).await?;
        db::steps::cancel_open(&mut batch.tx, self.workspace, self.id)
            .await
            .map_err(db_status)?;
        db::approvals::expire_pending(&mut batch.tx, self.workspace, self.id)
            .await
            .map_err(db_status)?;
        let message = if reason.trim().is_empty() {
            "execution cancelled".to_owned()
        } else {
            format!("execution cancelled: {}", reason.trim())
        };
        batch
            .event(
                EventType::StateChanged,
                "",
                message,
                data([("reason", reason.trim().to_owned())]),
            )
            .await?;
        batch.commit(&self.rt).await
    }

    async fn resolve(
        &self,
        exec: &ExecutionRow,
        option_id: &str,
        instruction: &str,
        by: &str,
    ) -> Result<(), Status> {
        let Some(failure) = exec.failure.as_ref().map(|json| json.0.clone()) else {
            return Err(Status::failed_precondition(
                "execution has no failure to resolve",
            ));
        };
        let steps = self.steps().await?;
        let Some(step) = steps.iter().find(|s| s.key == failure.step_key).cloned() else {
            return Err(Status::internal(format!(
                "failed step {} is missing",
                failure.step_key
            )));
        };
        let resolution = diagnosis::resolve(option_id, instruction, &failure, &step.params.0)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let other_identity = match &resolution {
            Resolution::UseAnotherIdentity => Some(
                db::identities::holding_all(
                    &self.rt.pool,
                    self.workspace,
                    &step.required_permissions,
                    exec.identity_id,
                )
                .await
                .map_err(db_status)?
                .ok_or_else(|| {
                    Status::failed_precondition(format!(
                        "no other identity holds {}",
                        step.required_permissions.join(", ")
                    ))
                })?,
            ),
            _ => None,
        };

        let mut batch = self.begin().await?;
        if batch.status() != ExecutionStatus::Diagnosing {
            return Err(Status::failed_precondition(format!(
                "execution is {}, only DIAGNOSING executions can be resolved",
                batch.exec.status
            )));
        }

        if resolution == Resolution::GiveUp {
            // Not a replan: the run ends here and the pattern learns from it.
            self.cleanup_real(&batch.exec.adapter).await;
            batch.transition(ExecutionStatus::Failed).await?;
            db::steps::abandon_open(&mut batch.tx, self.workspace, self.id)
                .await
                .map_err(db_status)?;
            db::approvals::expire_pending(&mut batch.tx, self.workspace, self.id)
                .await
                .map_err(db_status)?;
            batch
                .event(
                    EventType::StateChanged,
                    &step.key,
                    format!(
                        "gave up on '{}' after {} attempts ({}); execution failed",
                        step.title, step.attempts, failure.code
                    ),
                    data([
                        ("reason", "give_up".to_owned()),
                        ("code", failure.code.clone()),
                        ("attempts", step.attempts.to_string()),
                    ]),
                )
                .await?;
            batch.commit(&self.rt).await?;
            self.record_outcome(exec, false).await;
            return Ok(());
        }

        let chosen = if option_id.trim().is_empty() {
            format!("instruction \"{}\"", instruction.trim())
        } else {
            format!("option {}", option_id.trim())
        };
        batch.transition(ExecutionStatus::Replanning).await?;
        batch
            .event(
                EventType::StateChanged,
                &step.key,
                format!("replanning from {chosen}"),
                data([
                    ("option_id", option_id.trim().to_owned()),
                    ("instruction", instruction.trim().to_owned()),
                ]),
            )
            .await?;

        let mut unblock = failure.blocked_steps.clone();
        let note = match resolution {
            Resolution::Cancel => {
                batch.transition(ExecutionStatus::Cancelled).await?;
                db::steps::cancel_open(&mut batch.tx, self.workspace, self.id)
                    .await
                    .map_err(db_status)?;
                db::approvals::expire_pending(&mut batch.tx, self.workspace, self.id)
                    .await
                    .map_err(db_status)?;
                batch
                    .event(
                        EventType::StateChanged,
                        &step.key,
                        "execution cancelled by remediation",
                        data([("reason", "cancel".to_owned())]),
                    )
                    .await?;
                return batch.commit(&self.rt).await;
            }
            Resolution::GiveUp => unreachable!("handled before replanning"),
            Resolution::RollbackToPrevious => {
                // This run ends here; the environment goes back to its last
                // good release through a run of its own (same gates).
                batch.transition(ExecutionStatus::Cancelled).await?;
                db::steps::cancel_open(&mut batch.tx, self.workspace, self.id)
                    .await
                    .map_err(db_status)?;
                db::approvals::expire_pending(&mut batch.tx, self.workspace, self.id)
                    .await
                    .map_err(db_status)?;
                batch
                    .event(
                        EventType::StateChanged,
                        &step.key,
                        "execution cancelled; rolling the environment back to its previous release",
                        data([("reason", diagnosis::ROLLBACK_TO_PREVIOUS.to_owned())]),
                    )
                    .await?;
                batch.commit(&self.rt).await?;
                let rollback = delivery::runs::rollback_after_failure(
                    &self.rt,
                    self.workspace,
                    self.id,
                    by,
                )
                .await
                .map_err(|status| {
                    Status::new(
                        status.code(),
                        format!(
                            "the deployment was cancelled, but the rollback did not start: {}",
                            status.message()
                        ),
                    )
                })?;
                tracing::info!(execution = %self.id, rollback = %rollback.id, "rollback run started by remediation");
                return Ok(());
            }
            Resolution::Skip => {
                batch.step_status(&step, StepStatus::Skipped).await?;
                format!("step '{}' skipped", step.title)
            }
            Resolution::SkipBranch => {
                batch.step_status(&step, StepStatus::Skipped).await?;
                db::steps::set_status_for_keys(
                    &mut batch.tx,
                    self.workspace,
                    self.id,
                    &failure.blocked_steps,
                    convert::step_status_to_db(StepStatus::Skipped),
                )
                .await
                .map_err(db_status)?;
                unblock.clear();
                format!(
                    "step '{}' and {} dependents skipped",
                    step.title,
                    failure.blocked_steps.len()
                )
            }
            Resolution::RequestAccess => {
                db::identities::grant(
                    &mut batch.tx,
                    self.workspace,
                    exec.identity_id,
                    &failure.missing_permissions,
                    GRANT_TTL_MINUTES,
                )
                .await
                .map_err(db_status)?;
                batch.step_status(&step, StepStatus::Pending).await?;
                db::steps::reset_for_retry(&mut batch.tx, self.workspace, step.id, &step.params.0)
                    .await
                    .map_err(db_status)?;
                format!(
                    "granted {} to the identity for {GRANT_TTL_MINUTES} minutes",
                    failure.missing_permissions.join(", ")
                )
            }
            Resolution::UseAnotherIdentity => {
                let identity = other_identity.expect("validated above");
                db::executions::set_identity(&mut batch.tx, self.workspace, self.id, identity.id)
                    .await
                    .map_err(db_status)?;
                batch.step_status(&step, StepStatus::Pending).await?;
                db::steps::reset_for_retry(&mut batch.tx, self.workspace, step.id, &step.params.0)
                    .await
                    .map_err(db_status)?;
                format!("switched to identity {}", identity.name)
            }
            Resolution::Retry { patch, note } => {
                let mut params = step.params.0.clone();
                params.extend(patch);
                batch.step_status(&step, StepStatus::Pending).await?;
                db::steps::reset_for_retry(&mut batch.tx, self.workspace, step.id, &params)
                    .await
                    .map_err(db_status)?;
                note
            }
        };

        let revision = db::executions::bump_revision(&mut batch.tx, self.workspace, self.id)
            .await
            .map_err(db_status)?;
        batch.exec.revision = revision;
        let expired = db::approvals::expire_pending(&mut batch.tx, self.workspace, self.id)
            .await
            .map_err(db_status)?;
        db::steps::release_waiting(&mut batch.tx, self.workspace, self.id)
            .await
            .map_err(db_status)?;
        db::steps::set_status_for_keys(
            &mut batch.tx,
            self.workspace,
            self.id,
            &unblock,
            convert::step_status_to_db(StepStatus::Pending),
        )
        .await
        .map_err(db_status)?;
        db::executions::set_failure(&mut batch.tx, self.workspace, self.id, None)
            .await
            .map_err(db_status)?;
        batch.transition(ExecutionStatus::Authorized).await?;
        batch
            .event(
                EventType::Replanned,
                &step.key,
                format!(
                    "revision {revision}: {note}; continuing from checkpoint {}",
                    exec.last_checkpoint.as_deref().unwrap_or("(none)")
                ),
                data([
                    ("revision", revision.to_string()),
                    ("option_id", option_id.trim().to_owned()),
                    ("unblocked", unblock.join(",")),
                    ("approvals_expired", expired.to_string()),
                ]),
            )
            .await?;
        batch.commit(&self.rt).await
    }

    // --- the step pipeline --------------------------------------------------

    async fn advance(&mut self, exec: &ExecutionRow) -> Result<Flow, Status> {
        let steps = self.steps().await?;

        // A step whose approval came through: run it without re-gating.
        if let Some(step) = steps
            .iter()
            .find(|s| step_is(s, StepStatus::WaitingApproval))
            .cloned()
        {
            let approval_id = step.approval_id.ok_or_else(|| {
                Status::internal(format!("step {} waits without an approval", step.key))
            })?;
            let approval = db::approvals::get(&self.rt.pool, self.workspace, approval_id)
                .await
                .map_err(db_status)?;
            return match approval.status.as_str() {
                "APPROVED" => self.run_step(exec, &steps, &step).await,
                "PENDING" => {
                    let mut batch = self.begin().await?;
                    batch
                        .transition(ExecutionStatus::WaitingForApproval)
                        .await?;
                    batch.commit(&self.rt).await?;
                    Ok(Flow::Continue)
                }
                _ => {
                    self.apply_decision(exec, &step, approval_id).await?;
                    Ok(Flow::Continue)
                }
            };
        }

        let graph = nodes(&steps);
        let satisfied: HashSet<&str> = steps
            .iter()
            .filter(|s| step_is(s, StepStatus::Succeeded) || step_is(s, StepStatus::Skipped))
            .map(|s| s.key.as_str())
            .collect();
        let pending: HashSet<&str> = steps
            .iter()
            .filter(|s| step_is(s, StepStatus::Pending) || step_is(s, StepStatus::Ready))
            .map(|s| s.key.as_str())
            .collect();
        let ready = dag::ready(&graph, &satisfied, &pending);

        let Some(next_key) = ready.first().copied() else {
            if satisfied.len() == steps.len() {
                return self.succeed(exec, steps.len()).await;
            }
            return Err(Status::internal(format!(
                "execution {} cannot make progress: {} of {} steps satisfied, none ready",
                self.id,
                satisfied.len(),
                steps.len()
            )));
        };
        let step = steps
            .iter()
            .find(|s| s.key == next_key)
            .cloned()
            .expect("ready key exists");
        self.gate(exec, &steps, &step).await
    }

    /// Permission check -> policy check -> approval or run.
    async fn gate(
        &mut self,
        exec: &ExecutionRow,
        steps: &[StepRow],
        step: &StepRow,
    ) -> Result<Flow, Status> {
        let grants: Vec<policy::Grant> =
            db::identities::grants_for(&self.rt.pool, self.workspace, exec.identity_id)
                .await
                .map_err(db_status)?
                .into_iter()
                .map(Into::into)
                .collect();
        let missing = policy::missing_permissions(&step.required_permissions, &grants, Utc::now());

        let mut batch = self.begin().await?;
        if step_is(step, StepStatus::Pending) {
            batch.step_status(step, StepStatus::Ready).await?;
        }
        let ready_step = StepRow {
            status: convert::step_status_to_db(StepStatus::Ready).to_owned(),
            ..step.clone()
        };

        if !missing.is_empty() {
            batch
                .event(
                    EventType::PermissionDenied,
                    &step.key,
                    format!("identity lacks {}", missing.join(", ")),
                    data([
                        ("required", step.required_permissions.join(",")),
                        ("missing", missing.join(",")),
                    ]),
                )
                .await?;
            let message = format!("missing permissions: {}", missing.join(", "));
            self.fail_step(
                &mut batch,
                steps,
                &ready_step,
                diagnosis::PERMISSION_DENIED,
                &message,
                "",
                missing,
            )
            .await?;
            batch.commit(&self.rt).await?;
            return Ok(Flow::Continue);
        }
        batch
            .event(
                EventType::PermissionChecked,
                &step.key,
                if step.required_permissions.is_empty() {
                    "no permissions required".to_owned()
                } else {
                    format!("identity holds {}", step.required_permissions.join(", "))
                },
                data([("required", step.required_permissions.join(","))]),
            )
            .await?;

        let rules = db::policies::rules(&self.rt.pool, self.workspace)
            .await
            .map_err(db_status)?;
        let rule = policy::lookup(&rules, &step.action_type);
        batch
            .event(
                EventType::PolicyChecked,
                &step.key,
                format!(
                    "{} is {} risk, policy says {}",
                    step.action_type,
                    convert::risk_to_db(rule.risk).to_ascii_lowercase(),
                    convert::decision_to_db(rule.decision).to_ascii_lowercase()
                ),
                data([
                    ("action_type", step.action_type.clone()),
                    ("risk", convert::risk_to_db(rule.risk).to_owned()),
                    (
                        "decision",
                        convert::decision_to_db(rule.decision).to_owned(),
                    ),
                ]),
            )
            .await?;

        match policy::gate(rule.decision) {
            Gate::Block => {
                let message = format!("policy blocks {}", step.action_type);
                self.fail_step(
                    &mut batch,
                    steps,
                    &ready_step,
                    diagnosis::POLICY_BLOCKED,
                    &message,
                    "",
                    vec![],
                )
                .await?;
                batch.commit(&self.rt).await?;
                Ok(Flow::Continue)
            }
            Gate::AskHuman => {
                let approval_id = Uuid::now_v7();
                let reason = match rule.decision {
                    PolicyDecision::Required => format!(
                        "{} always needs a human decision ({} risk)",
                        step.action_type,
                        convert::risk_to_db(rule.risk).to_ascii_lowercase()
                    ),
                    _ => format!(
                        "{} is {} risk and the policy asks a human before running it",
                        step.action_type,
                        convert::risk_to_db(rule.risk).to_ascii_lowercase()
                    ),
                };
                db::approvals::insert(
                    &mut batch.tx,
                    self.workspace,
                    approval_id,
                    self.id,
                    batch.exec.revision,
                    &step.key,
                    &reason,
                )
                .await
                .map_err(db_status)?;
                state::step_transition(StepStatus::Ready, StepStatus::WaitingApproval)
                    .map_err(|e| Status::failed_precondition(e.to_string()))?;
                db::steps::set_approval(
                    &mut batch.tx,
                    self.workspace,
                    step.id,
                    Some(approval_id),
                    convert::step_status_to_db(StepStatus::WaitingApproval),
                )
                .await
                .map_err(db_status)?;
                batch
                    .transition(ExecutionStatus::WaitingForApproval)
                    .await?;
                batch
                    .event(
                        EventType::ApprovalRequested,
                        &step.key,
                        format!("approval requested for '{}': {reason}", step.title),
                        data([
                            ("approval_id", approval_id.to_string()),
                            ("risk", convert::risk_to_db(rule.risk).to_owned()),
                            (
                                "decision",
                                convert::decision_to_db(rule.decision).to_owned(),
                            ),
                        ]),
                    )
                    .await?;
                batch.commit(&self.rt).await?;
                Ok(Flow::Continue)
            }
            Gate::Run => {
                batch.commit(&self.rt).await?;
                self.run_step(exec, steps, &ready_step).await
            }
        }
    }

    /// Writes one LOG event holding the buffered adapter lines.
    async fn log(&self, step_key: &str, lines: &mut Vec<String>) -> Result<(), Status> {
        if lines.is_empty() {
            return Ok(());
        }
        let count = lines.len();
        let message = std::mem::take(lines).join("\n");
        let mut batch = self.begin().await?;
        batch
            .event(
                EventType::Log,
                step_key,
                message,
                data([
                    ("lines", count.to_string()),
                    ("source", "adapter".to_owned()),
                ]),
            )
            .await?;
        batch.commit(&self.rt).await
    }

    async fn run_step(
        &mut self,
        exec: &ExecutionRow,
        steps: &[StepRow],
        step: &StepRow,
    ) -> Result<Flow, Status> {
        let adapter = self
            .rt
            .adapters
            .for_step(&exec.adapter, &step.action_type, &step.adapter)?;

        let mut batch = self.begin().await?;
        batch.transition(ExecutionStatus::Executing).await?;
        state::step_transition(
            convert::step_status_from_db(&step.status),
            StepStatus::Running,
        )
        .map_err(|e| Status::failed_precondition(format!("step {}: {e}", step.key)))?;
        let attempt = db::steps::start_attempt(&mut batch.tx, self.workspace, step.id)
            .await
            .map_err(db_status)?;
        batch
            .event(
                EventType::StepStarted,
                &step.key,
                format!(
                    "running '{}' via {} (attempt {attempt})",
                    step.title, step.adapter
                ),
                data([
                    ("attempt", attempt.to_string()),
                    ("adapter", step.adapter.clone()),
                    ("runner", adapter.name().to_owned()),
                ]),
            )
            .await?;
        batch.commit(&self.rt).await?;

        let (params, repository_note) = self.params_with_repository(exec, step).await;
        let input = StepInput {
            key: step.key.clone(),
            title: step.title.clone(),
            action_type: step.action_type.clone(),
            adapter: step.adapter.clone(),
            params,
            attempt: attempt.max(0) as u32,
        };
        let (log_tx, mut log_rx) = mpsc::channel::<String>(LOG_CHANNEL);
        let (progress_tx, mut progress_rx) = mpsc::channel::<(String, String)>(PROGRESS_CHANNEL);
        // Real adapters read and write GitHub as the workspace, with the
        // token and API base identity mints for it (GHES included).
        let github = if exec.adapter == adapters::SIMULATED {
            None
        } else {
            self.rt.github_access.access(self.workspace).await
        };
        // Whatever this step authenticates with, and every secret it
        // resolves, is masked in its log lines, outputs and failure.
        let redactor = Redactor::default();
        if let Some(access) = &github {
            redactor.protect(&access.token);
        }
        let secrets = match &self.rt.delivery.secrets {
            Some(source) => Secrets::new(Arc::clone(source), self.workspace, redactor.clone()),
            None => Secrets::default(),
        };
        // The outputs earlier steps of this run recorded, read fresh: a
        // step that reported progress wrote to them after `exec` was loaded.
        let outputs = db::executions::get(&self.rt.pool, self.workspace, self.id)
            .await
            .map_err(db_status)?
            .outputs
            .0;
        let ctx = RunContext {
            execution_id: self.id,
            execution_number: exec.number,
            proposal_id: exec.proposal_id.clone(),
            fault: exec.fault.as_ref().map(|json| json.0.clone()),
            outputs,
            artifacts: exec.artifacts.0.clone(),
            logs: LogSink::new(log_tx).with_redactor(redactor.clone()),
            github,
            secrets,
            progress: OutputSink::new(progress_tx),
        };
        let fault_targets_step = ctx.fault.as_ref().is_some_and(|f| f.step_key == step.key);
        if let Some(note) = repository_note {
            ctx.logs.line(note);
        }

        // The adapter future lives in this block so that leaving it (cancel)
        // drops it, which kills whatever child process it was waiting on.
        let mut pending: Vec<String> = Vec::new();
        let mut next_flush: Option<Instant> = None;
        let ended = {
            let run = adapter.run(&input, &ctx);
            tokio::pin!(run);
            loop {
                let flush = async {
                    match next_flush {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    outcome = &mut run => break StepLoop::Done(outcome),
                    line = log_rx.recv() => {
                        if let Some(line) = line {
                            if pending.is_empty() {
                                next_flush = Some(Instant::now() + LOG_WINDOW);
                            }
                            pending.push(line);
                        }
                    }
                    () = flush => {
                        next_flush = None;
                        self.log(&step.key, &mut pending).await?;
                    }
                    Some((key, value)) = progress_rx.recv() => {
                        // Something to follow while the step still runs
                        // (the deployer's own page): record it now.
                        self.record_progress(&key, &redactor.redact(&value)).await?;
                    }
                    command = self.rx.recv() => match command {
                        Some(Command::Cancel { reason, reply }) => break StepLoop::Cancel { reason, reply },
                        Some(other) => other.reject("a step is running"),
                        None => break StepLoop::Stop,
                    },
                }
            }
        };
        while let Ok(line) = log_rx.try_recv() {
            pending.push(line);
        }
        self.log(&step.key, &mut pending).await?;
        while let Ok((key, value)) = progress_rx.try_recv() {
            self.record_progress(&key, &redactor.redact(&value)).await?;
        }

        let outcome = match ended {
            // Whatever the tool said goes through the redactor once more:
            // a deployer that echoes a credential must not get it stored.
            StepLoop::Done(outcome) => outcome.redacted(&redactor),
            StepLoop::Cancel { reason, reply } => {
                adapter.cleanup(self.id).await;
                let _ = reply.send(self.cancel(&reason).await);
                return Ok(Flow::Continue);
            }
            StepLoop::Stop => return Ok(Flow::Stop),
        };

        let running = StepRow {
            status: convert::step_status_to_db(StepStatus::Running).to_owned(),
            attempts: attempt,
            ..step.clone()
        };
        match outcome {
            StepOutcome::Succeeded { output, outputs } => {
                if fault_targets_step {
                    self.clear_fault().await?;
                }
                self.verify(&running, output, outputs).await
            }
            StepOutcome::Failed {
                code,
                raw_output,
                missing_permissions,
            } => {
                let mut batch = self.begin().await?;
                if fault_targets_step {
                    // The rehearsed fault fired once; the retry must pass.
                    db::executions::set_fault(&mut batch.tx, self.workspace, self.id, None)
                        .await
                        .map_err(db_status)?;
                }
                let message = format!("'{}' failed with {code}", step.title);
                self.fail_step(
                    &mut batch,
                    steps,
                    &running,
                    &code,
                    &message,
                    &raw_output,
                    missing_permissions,
                )
                .await?;
                batch.commit(&self.rt).await?;
                Ok(Flow::Continue)
            }
        }
    }

    /// The step's params as its adapter gets them. A step of the github
    /// family acts on the repository of the execution's project (CONTRACT.md
    /// §23.5): when the plan does not name it (`params.repo`), knowledge is
    /// asked, as the workspace's system principal like all background work.
    /// The second value is a line for the step's log when it could not say;
    /// the adapter then fails the step for the missing parameter.
    async fn params_with_repository(
        &self,
        exec: &ExecutionRow,
        step: &StepRow,
    ) -> (BTreeMap<String, String>, Option<String>) {
        let mut params = step.params.0.clone();
        let named = ["repo", "repository"]
            .iter()
            .any(|key| params.get(*key).is_some_and(|v| !v.trim().is_empty()));
        if named
            || exec.adapter == adapters::SIMULATED
            || exec.adapter == adapters::DELIVERY
            || !adapters::GITHUB_ACTIONS.contains(&step.action_type.as_str())
            || exec.project_id.trim().is_empty()
        {
            return (params, None);
        }
        let principal = Principal::system(self.workspace.to_string());
        match self
            .rt
            .delivery
            .projects
            .project(&principal, exec.project_id.trim())
            .await
        {
            Ok(project) if !project.repo_full_name.trim().is_empty() => {
                params.insert("repo".to_owned(), project.repo_full_name.trim().to_owned());
                (params, None)
            }
            Ok(_) => (
                params,
                Some("the execution's project has no repository on record".to_owned()),
            ),
            Err(status) => (
                params,
                Some(format!(
                    "could not look up the repository of the execution's project: {}",
                    status.message()
                )),
            ),
        }
    }

    /// Merges one output a step reported while it is still running.
    async fn record_progress(&self, key: &str, value: &str) -> Result<(), Status> {
        let mut batch = self.begin().await?;
        batch
            .merge_outputs(&BTreeMap::from([(key.to_owned(), value.to_owned())]))
            .await?;
        batch.commit(&self.rt).await
    }

    async fn clear_fault(&self) -> Result<(), Status> {
        let mut tx = self.rt.pool.begin().await.map_err(db_status)?;
        db::executions::set_fault(&mut tx, self.workspace, self.id, None)
            .await
            .map_err(db_status)?;
        tx.commit().await.map_err(db_status)
    }

    async fn verify(
        &mut self,
        step: &StepRow,
        output: String,
        outputs: BTreeMap<String, String>,
    ) -> Result<Flow, Status> {
        let mut batch = self.begin().await?;
        batch.transition(ExecutionStatus::Verifying).await?;
        batch.step_status(step, StepStatus::Verifying).await?;
        let check = if step.verification.trim().is_empty() {
            "no verification defined; accepting the tool's exit status".to_owned()
        } else {
            format!("verifying: {}", step.verification.trim())
        };
        batch
            .event(
                EventType::Log,
                &step.key,
                check,
                data([("verification", step.verification.clone())]),
            )
            .await?;
        batch.commit(&self.rt).await?;

        tokio::time::sleep(self.rt.verify_delay).await;

        let verifying = StepRow {
            status: convert::step_status_to_db(StepStatus::Verifying).to_owned(),
            ..step.clone()
        };
        let mut batch = self.begin().await?;
        state::step_transition(StepStatus::Verifying, StepStatus::Succeeded)
            .map_err(|e| Status::failed_precondition(e.to_string()))?;
        db::steps::finish(
            &mut batch.tx,
            self.workspace,
            verifying.id,
            convert::step_status_to_db(StepStatus::Succeeded),
            &output,
            true,
        )
        .await
        .map_err(db_status)?;
        db::executions::set_checkpoint(&mut batch.tx, self.workspace, self.id, &step.key)
            .await
            .map_err(db_status)?;
        batch.merge_outputs(&outputs).await?;
        batch.transition(ExecutionStatus::Authorized).await?;
        let mut info = data([("attempt", step.attempts.to_string())]);
        info.extend(
            outputs
                .iter()
                .map(|(k, v)| (format!("output.{k}"), v.clone())),
        );
        batch
            .event(
                EventType::StepSucceeded,
                &step.key,
                format!("'{}' succeeded and verified", step.title),
                info,
            )
            .await?;
        batch
            .event(
                EventType::Checkpoint,
                &step.key,
                format!("checkpoint {}", step.key),
                data([("last_checkpoint", step.key.clone())]),
            )
            .await?;
        batch.commit(&self.rt).await?;
        Ok(Flow::Continue)
    }

    /// Marks the step FAILED, blocks its transitive dependents and pauses the
    /// execution, all inside `batch`. The diagnosis is written in the next
    /// loop iteration (PAUSED -> DIAGNOSING).
    #[allow(clippy::too_many_arguments)]
    async fn fail_step(
        &self,
        batch: &mut Batch,
        steps: &[StepRow],
        step: &StepRow,
        code: &str,
        message: &str,
        raw_output: &str,
        missing_permissions: Vec<String>,
    ) -> Result<(), Status> {
        let code = diagnosis::normalize_code(code);
        state::step_transition(
            convert::step_status_from_db(&step.status),
            StepStatus::Failed,
        )
        .map_err(|e| Status::failed_precondition(format!("step {}: {e}", step.key)))?;
        db::steps::finish(
            &mut batch.tx,
            self.workspace,
            step.id,
            convert::step_status_to_db(StepStatus::Failed),
            raw_output,
            false,
        )
        .await
        .map_err(db_status)?;

        let graph = nodes(steps);
        let blocked: Vec<String> = dag::dependents(&graph, &step.key)
            .into_iter()
            .filter(|key| {
                steps.iter().any(|s| {
                    s.key == *key
                        && (step_is(s, StepStatus::Pending)
                            || step_is(s, StepStatus::Ready)
                            || step_is(s, StepStatus::Blocked))
                })
            })
            .map(str::to_owned)
            .collect();
        db::steps::set_status_for_keys(
            &mut batch.tx,
            self.workspace,
            self.id,
            &blocked,
            convert::step_status_to_db(StepStatus::Blocked),
        )
        .await
        .map_err(db_status)?;

        batch.transition(ExecutionStatus::Paused).await?;
        batch
            .event(
                EventType::StepFailed,
                &step.key,
                message,
                data([
                    ("code", code.to_owned()),
                    ("attempt", step.attempts.to_string()),
                ]),
            )
            .await?;
        for key in &blocked {
            batch
                .event(
                    EventType::StepBlocked,
                    key,
                    format!("blocked until {} is resolved", step.key),
                    data([("blocked_by", step.key.clone())]),
                )
                .await?;
        }
        let record = FailureRecord {
            step_key: step.key.clone(),
            code: code.to_owned(),
            message: message.to_owned(),
            raw_output: raw_output.to_owned(),
            diagnosis: String::new(),
            options: Vec::new(),
            blocked_steps: blocked.clone(),
            missing_permissions,
            occurred_at: Utc::now(),
        };
        db::executions::set_failure(&mut batch.tx, self.workspace, self.id, Some(&record))
            .await
            .map_err(db_status)?;
        batch
            .event(
                EventType::StateChanged,
                &step.key,
                format!(
                    "execution paused on '{}' ({} dependent steps blocked)",
                    step.title,
                    blocked.len()
                ),
                data([("code", code.to_owned()), ("blocked", blocked.join(","))]),
            )
            .await?;
        Ok(())
    }

    async fn diagnose(&self, exec: &ExecutionRow) -> Result<Flow, Status> {
        let steps = self.steps().await?;
        let mut record = exec
            .failure
            .as_ref()
            .map(|json| json.0.clone())
            .unwrap_or_else(|| FailureRecord {
                step_key: String::new(),
                code: diagnosis::UNKNOWN.to_owned(),
                message: "execution paused without a recorded failure".to_owned(),
                raw_output: String::new(),
                diagnosis: String::new(),
                options: Vec::new(),
                blocked_steps: Vec::new(),
                missing_permissions: Vec::new(),
                occurred_at: Utc::now(),
            });
        let empty = BTreeMap::new();
        let step = steps.iter().find(|s| s.key == record.step_key);
        // A deployment whose deploy or health step failed may go back to
        // the environment's last good release, when it has one.
        let can_rollback = exec.adapter == adapters::DELIVERY
            && step.is_some_and(|s| {
                adapters::is_deploy_action(&s.action_type)
                    || s.action_type == adapters::VERIFY_HEALTH
            })
            && delivery::runs::can_rollback(&self.rt, self.workspace, self.id).await;
        let detail: String = record
            .raw_output
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or_default()
            .chars()
            .take(DETAIL_CHARS)
            .collect();
        let failed = FailedStep {
            title: step.map(|s| s.title.as_str()).unwrap_or(&record.step_key),
            action_type: step.map(|s| s.action_type.as_str()).unwrap_or(""),
            params: step.map(|s| &s.params.0).unwrap_or(&empty),
            message: &record.message,
            attempts: step.map_or(0, |s| s.attempts.max(0) as u32),
            // The pre-flight grant check writes no raw output; a tool does.
            refused_by_tool: record.code == diagnosis::PERMISSION_DENIED
                && !record.raw_output.is_empty(),
            detail: &detail,
            can_rollback,
        };
        let (text, options) = diagnosis::diagnose(
            &record.code,
            &failed,
            &record.blocked_steps,
            &record.missing_permissions,
        );
        record.diagnosis = text;
        record.options = options;

        let mut batch = self.begin().await?;
        db::executions::set_failure(&mut batch.tx, self.workspace, self.id, Some(&record))
            .await
            .map_err(db_status)?;
        batch.transition(ExecutionStatus::Diagnosing).await?;
        let recommended = record
            .options
            .iter()
            .find(|o| o.recommended)
            .map(|o| o.id.clone())
            .unwrap_or_default();
        batch
            .event(
                EventType::Diagnosis,
                &record.step_key,
                record.diagnosis.clone(),
                data([
                    ("code", record.code.clone()),
                    ("recommended", recommended),
                    (
                        "options",
                        record
                            .options
                            .iter()
                            .map(|o| o.id.as_str())
                            .collect::<Vec<_>>()
                            .join(","),
                    ),
                    ("blocked", record.blocked_steps.join(",")),
                ]),
            )
            .await?;
        batch.commit(&self.rt).await?;
        Ok(Flow::Continue)
    }

    /// A step was mid-flight when the service stopped; its outcome is lost.
    async fn recover_crashed(&self, exec: &ExecutionRow) -> Result<Flow, Status> {
        let steps = self.steps().await?;
        let mut batch = self.begin().await?;
        match steps
            .iter()
            .find(|s| step_is(s, StepStatus::Running) || step_is(s, StepStatus::Verifying))
        {
            Some(step) => {
                self.fail_step(
                    &mut batch,
                    &steps,
                    step,
                    diagnosis::UNKNOWN,
                    "service restarted mid-step",
                    &step.output,
                    vec![],
                )
                .await?;
            }
            None => {
                batch.transition(ExecutionStatus::Paused).await?;
                let record = FailureRecord {
                    step_key: exec.last_checkpoint.clone().unwrap_or_default(),
                    code: diagnosis::UNKNOWN.to_owned(),
                    message: "service restarted mid-step".to_owned(),
                    raw_output: String::new(),
                    diagnosis: String::new(),
                    options: Vec::new(),
                    blocked_steps: Vec::new(),
                    missing_permissions: Vec::new(),
                    occurred_at: Utc::now(),
                };
                db::executions::set_failure(&mut batch.tx, self.workspace, self.id, Some(&record))
                    .await
                    .map_err(db_status)?;
                batch
                    .event(
                        EventType::StateChanged,
                        "",
                        "service restarted mid-step; execution paused",
                        BTreeMap::new(),
                    )
                    .await?;
            }
        }
        batch.commit(&self.rt).await?;
        Ok(Flow::Continue)
    }

    async fn succeed(&self, exec: &ExecutionRow, step_count: usize) -> Result<Flow, Status> {
        let mut batch = self.begin().await?;
        batch.transition(ExecutionStatus::Succeeded).await?;
        batch
            .event(
                EventType::StateChanged,
                "",
                format!("all {step_count} steps checkpointed; execution succeeded"),
                data([
                    ("steps", step_count.to_string()),
                    ("revision", exec.revision.to_string()),
                ]),
            )
            .await?;
        batch.commit(&self.rt).await?;
        self.record_outcome(exec, true).await;
        // A finished real run leaves nothing on disk: the github plan has no
        // env.destroy_temporary step, yet its checkout sits under SDI_RUN_DIR.
        self.cleanup_real(&exec.adapter).await;
        Ok(Flow::Continue)
    }

    async fn cleanup_real(&self, adapter: &str) {
        // A deployment run keeps nothing on this host under its execution
        // id: a `docker_local` environment's container is named after the
        // environment and has to outlive the run that started it.
        if adapter != adapters::SIMULATED && adapter != adapters::DELIVERY {
            for adapter in self.rt.adapters.real() {
                adapter.cleanup(self.id).await;
            }
        }
    }
}
