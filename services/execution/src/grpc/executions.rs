use std::{collections::BTreeMap, sync::Arc};

use sdi_common::grpc::db_status;
use sdi_proto::{
    common::v1::PlanStep,
    execution::v1::{
        CancelExecutionRequest, CreateExecutionRequest, EventType, Execution, ExecutionStatus,
        ListExecutionsRequest, ListExecutionsResponse, ResolveFailureRequest,
        StartExecutionRequest, StepStatus,
    },
};
use sqlx::types::Json;
use tonic::Status;
use uuid::Uuid;

use super::{Caller, load_execution, parse_id};
use crate::{
    adapters::{self, Fault},
    convert,
    db::{
        self,
        events::{EventRow, NewEvent},
        executions::NewExecution,
        steps::StepRow,
    },
    domain::{dag, diagnosis, policy, state},
    runner::{Command, Runtime},
};

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 200;

/// What `insert_plan` stores: a validated plan on a runtime.
pub(crate) struct NewPlan<'a> {
    pub project_id: &'a str,
    pub project_name: &'a str,
    pub proposal_id: &'a str,
    pub pattern_id: &'a str,
    pub title: &'a str,
    pub steps: &'a [PlanStep],
    /// A name `adapters::runtime_name` returned.
    pub runtime: &'static str,
    pub artifacts: &'a BTreeMap<String, String>,
    /// `x-sdi-user` of whoever asked for the run.
    pub created_by: &'a str,
}

/// The one way an execution comes to exist: validates the plan against the
/// runtime, decorates every step with its policy, and writes the execution,
/// its steps and its first event inside the caller's transaction (a
/// deployment run stores its own row next to them before committing). The
/// caller publishes with [`publish_created`] once it committed.
pub(crate) async fn insert_plan(
    tx: &mut db::Tx,
    rt: &Runtime,
    workspace: Uuid,
    plan: NewPlan<'_>,
) -> Result<(Uuid, EventRow), Status> {
    let runtime = plan.runtime;
    let keys: Vec<_> = plan
        .steps
        .iter()
        .map(|s| {
            (
                s.key.trim().to_owned(),
                s.depends_on
                    .iter()
                    .map(|d| d.trim().to_owned())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let nodes: Vec<dag::Node<'_>> = keys
        .iter()
        .map(|(key, depends_on)| dag::Node { key, depends_on })
        .collect();
    dag::validate(&nodes).map_err(|e| Status::invalid_argument(e.to_string()))?;
    for step in plan.steps {
        let action_type = step.action_type.trim();
        if action_type.is_empty() {
            return Err(Status::invalid_argument(format!(
                "step {}: action_type is required",
                step.key.trim()
            )));
        }
        if !adapters::runnable(runtime, action_type) {
            return Err(Status::invalid_argument(format!(
                "step {}: {action_type} cannot run on the {runtime} adapter",
                step.key.trim()
            )));
        }
    }

    let rules = db::policies::rules(&rt.pool, workspace)
        .await
        .map_err(db_status)?;
    let identity_name = adapters::identity_for(runtime);
    let identity = match db::identities::by_name(&rt.pool, workspace, identity_name).await {
        Ok(identity) => identity,
        Err(sqlx::Error::RowNotFound) if runtime == adapters::GITHUB => {
            return Err(Status::failed_precondition(
                "this workspace has no GitHub access to act with: install the GitHub App on the organisation, sign in with GitHub, or set GITHUB_TOKEN on the execution service",
            ));
        }
        Err(sqlx::Error::RowNotFound) => {
            return Err(Status::internal(format!(
                "the {identity_name} identity is missing"
            )));
        }
        Err(error) => return Err(db_status(error)),
    };

    let id = Uuid::now_v7();
    let title = if plan.title.trim().is_empty() {
        "Untitled execution"
    } else {
        plan.title.trim()
    };
    let planning = convert::status_to_db(ExecutionStatus::Planning);
    db::executions::insert(
        tx,
        NewExecution {
            id,
            workspace_id: workspace,
            project_id: plan.project_id.trim(),
            project_name: plan.project_name,
            proposal_id: plan.proposal_id.trim(),
            pattern_id: plan.pattern_id.trim(),
            title,
            status: planning,
            adapter: runtime,
            identity_id: identity.id,
            created_by: plan.created_by,
            artifacts: plan.artifacts,
        },
    )
    .await
    .map_err(db_status)?;

    for (position, (step, (key, depends_on))) in plan.steps.iter().zip(keys).enumerate() {
        let rule = policy::lookup(&rules, step.action_type.trim());
        let row = StepRow {
            id: Uuid::now_v7(),
            workspace_id: workspace,
            execution_id: id,
            title: if step.title.trim().is_empty() {
                key.clone()
            } else {
                step.title.trim().to_owned()
            },
            key,
            position: position as i32,
            description: step.description.trim().to_owned(),
            action_type: step.action_type.trim().to_owned(),
            adapter: if step.adapter.trim().is_empty() {
                "local".to_owned()
            } else {
                step.adapter.trim().to_owned()
            },
            depends_on,
            status: convert::step_status_to_db(StepStatus::Pending).to_owned(),
            risk: convert::risk_to_db(rule.risk).to_owned(),
            policy_decision: convert::decision_to_db(rule.decision).to_owned(),
            required_permissions: required_permissions(step),
            approval_id: None,
            params: Json(
                step.params
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
            verification: step.verification.trim().to_owned(),
            output: String::new(),
            checkpointed: false,
            attempts: 0,
            started_at: None,
            finished_at: None,
        };
        db::steps::insert(tx, &row).await.map_err(db_status)?;
    }
    let event = db::events::append(
        tx,
        NewEvent {
            workspace_id: workspace,
            execution_id: id,
            r#type: convert::event_type_to_db(EventType::StateChanged),
            step_key: "",
            message: format!("planned {} steps", plan.steps.len()),
            status: planning,
            data: [("steps".to_owned(), plan.steps.len().to_string())].into(),
        },
    )
    .await
    .map_err(db_status)?;
    Ok((id, event))
}

/// What the step's plan asks for, plus what its action type always needs
/// (`adapters::implied_permissions`): a GitOps sync whose plan forgot
/// `cluster:write` still pauses for the timed grant (CONTRACT.md §24.7).
fn required_permissions(step: &PlanStep) -> Vec<String> {
    let mut permissions: Vec<String> = step
        .required_permissions
        .iter()
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect();
    let operation = step.params.get("operation").map_or("", |op| op.trim());
    for implied in adapters::implied_permissions(step.action_type.trim(), operation) {
        if !permissions.iter().any(|p| p == implied) {
            permissions.push((*implied).to_owned());
        }
    }
    permissions
}

/// Tells the watchers about an execution `insert_plan` stored, once its
/// transaction committed.
pub(crate) fn publish_created(
    rt: &Runtime,
    workspace: Uuid,
    id: Uuid,
    project_id: &str,
    runtime: &str,
    event: EventRow,
) {
    let planning = convert::status_to_db(ExecutionStatus::Planning);
    rt.bus.publish(convert::event_to_proto(event));
    rt.platform.execution(
        workspace,
        &id.to_string(),
        project_id.trim(),
        "created",
        &planning.to_ascii_lowercase(),
        BTreeMap::from([
            ("revision".to_owned(), "0".to_owned()),
            ("adapter".to_owned(), runtime.to_owned()),
        ]),
    );
}

pub async fn create(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: CreateExecutionRequest,
) -> Result<Execution, Status> {
    // The runtime decides which steps may run at all (CONTRACT.md §5.4): a
    // plan that needs a tool the runtime cannot provide is refused here,
    // naming the step, rather than failing mid-flight. It also decides who
    // may create the run: a dry run is a member's, a real one an approver's.
    let runtime = adapters::runtime_name(&req.adapter)?;
    if runtime == adapters::DELIVERY {
        // A deployment run's plan is built from an environment and a
        // release; a hand-written one would skip that validation.
        return Err(Status::invalid_argument(
            "delivery runs are created by promoting or rolling back an environment, not by CreateExecution",
        ));
    }
    caller.require_adapter(runtime)?;
    let workspace = caller.workspace;

    let artifacts: BTreeMap<String, String> = req
        .artifacts
        .iter()
        .filter(|(path, _)| !path.trim().is_empty())
        .map(|(path, content)| (path.trim().to_owned(), content.clone()))
        .collect();
    let project_name = rt
        .outcomes
        .project_name(&caller.principal, req.project_id.trim())
        .await;

    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    let (id, event) = insert_plan(
        &mut tx,
        rt,
        workspace,
        NewPlan {
            project_id: &req.project_id,
            project_name: &project_name,
            proposal_id: &req.proposal_id,
            pattern_id: &req.pattern_id,
            title: &req.title,
            steps: &req.steps,
            runtime,
            artifacts: &artifacts,
            created_by: &caller.principal.user_id,
        },
    )
    .await?;
    tx.commit().await.map_err(db_status)?;
    publish_created(rt, workspace, id, &req.project_id, runtime, event);

    load_execution(rt, workspace, id, true).await
}

pub async fn list(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: ListExecutionsRequest,
) -> Result<ListExecutionsResponse, Status> {
    let limit = match i64::from(req.limit) {
        0 => DEFAULT_LIST_LIMIT,
        n => n.min(MAX_LIST_LIMIT),
    };
    let before = sdi_common::time::opt_from_proto(req.before.as_ref());
    let rows = db::executions::list(
        &rt.pool,
        caller.workspace,
        req.project_id.trim(),
        limit,
        before,
    )
    .await
    .map_err(db_status)?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mut steps = db::steps::list_for_many(&rt.pool, caller.workspace, &ids)
        .await
        .map_err(db_status)?;
    let executions = rows
        .into_iter()
        .map(|row| {
            let (mine, rest): (Vec<_>, Vec<_>) =
                steps.drain(..).partition(|s| s.execution_id == row.id);
            steps = rest;
            convert::execution_to_proto(row, mine, false)
        })
        .collect();
    Ok(ListExecutionsResponse { executions })
}

/// The run named by a mutating RPC: scoped to the workspace (another
/// workspace's id is NOT_FOUND), then gated by the adapter's role.
async fn load_for_action(
    rt: &Runtime,
    caller: &Caller,
    id: Uuid,
) -> Result<db::executions::ExecutionRow, Status> {
    let row = db::executions::get(&rt.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    caller.require_adapter(&row.adapter)?;
    Ok(row)
}

pub async fn start(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: StartExecutionRequest,
) -> Result<Execution, Status> {
    let id = parse_id(&req.id, "id")?;
    let row = load_for_action(rt, caller, id).await?;
    if convert::status_from_db(&row.status) != ExecutionStatus::Planning {
        return Err(Status::failed_precondition(format!(
            "execution is {}, only PLANNING executions can be started",
            row.status
        )));
    }
    let fault = match req.fault {
        Some(fault) if !fault.step_key.trim().is_empty() => {
            let steps = db::steps::list_for(&rt.pool, caller.workspace, id)
                .await
                .map_err(db_status)?;
            let key = fault.step_key.trim();
            if !steps.iter().any(|s| s.key == key) {
                return Err(Status::invalid_argument(format!(
                    "fault.step_key {key} is not a step of this plan"
                )));
            }
            Some(Fault {
                step_key: key.to_owned(),
                code: diagnosis::normalize_code(&fault.code).to_owned(),
            })
        }
        _ => None,
    };
    let mut detail = match &fault {
        Some(fault) => format!(
            "{} ({} adapter) with fault {} on {}",
            row.title, row.adapter, fault.code, fault.step_key
        ),
        None => format!("{} ({} adapter)", row.title, row.adapter),
    };
    if !row.created_by.is_empty() && row.created_by != caller.principal.user_id {
        detail.push_str(&format!(", created by {}", row.created_by));
    }
    rt.command(caller.workspace, id, |reply| Command::Start {
        fault,
        reply,
    })
    .await?;
    caller.audit(rt, "execution.start", "execution", &id.to_string(), detail);
    load_execution(rt, caller.workspace, id, true).await
}

pub async fn resolve(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: ResolveFailureRequest,
) -> Result<Execution, Status> {
    let id = parse_id(&req.execution_id, "execution_id")?;
    let row = load_for_action(rt, caller, id).await?;
    if convert::status_from_db(&row.status) != ExecutionStatus::Diagnosing {
        return Err(Status::failed_precondition(format!(
            "execution is {}, only DIAGNOSING executions can be resolved",
            row.status
        )));
    }
    if req.option_id.trim().is_empty() && req.instruction.trim().is_empty() {
        return Err(Status::invalid_argument(
            "option_id or instruction is required",
        ));
    }
    let detail = if req.option_id.trim().is_empty() {
        format!("instruction \"{}\"", req.instruction.trim())
    } else {
        format!("option {}", req.option_id.trim())
    };
    let by = caller.principal.user_id.clone();
    rt.command(caller.workspace, id, |reply| Command::Resolve {
        option_id: req.option_id,
        instruction: req.instruction,
        by,
        reply,
    })
    .await?;
    caller.audit(
        rt,
        "execution.resolve",
        "execution",
        &id.to_string(),
        format!("{}: {detail}", row.title),
    );
    load_execution(rt, caller.workspace, id, true).await
}

pub async fn cancel(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: CancelExecutionRequest,
) -> Result<Execution, Status> {
    let id = parse_id(&req.id, "id")?;
    let row = load_for_action(rt, caller, id).await?;
    if state::is_terminal(convert::status_from_db(&row.status)) {
        return Err(Status::failed_precondition(format!(
            "execution is already {}",
            row.status
        )));
    }
    rt.command(caller.workspace, id, |reply| Command::Cancel {
        reason: req.reason,
        reply,
    })
    .await?;
    load_execution(rt, caller.workspace, id, true).await
}
