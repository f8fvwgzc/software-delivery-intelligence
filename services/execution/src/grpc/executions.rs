use std::sync::Arc;

use sdi_common::grpc::db_status;
use sdi_proto::execution::v1::{
    CancelExecutionRequest, CreateExecutionRequest, EventType, Execution, ExecutionStatus,
    ListExecutionsRequest, ListExecutionsResponse, ResolveFailureRequest, StartExecutionRequest,
    StepStatus,
};
use sqlx::types::Json;
use tonic::Status;
use uuid::Uuid;

use super::{load_execution, parse_id};
use crate::{
    adapters::{self, Fault},
    convert,
    db::{self, events::NewEvent, executions::NewExecution, steps::StepRow},
    domain::{dag, diagnosis, policy, state},
    runner::{Command, Runtime},
};

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 200;

pub async fn create(rt: &Arc<Runtime>, req: CreateExecutionRequest) -> Result<Execution, Status> {
    let plan: Vec<_> = req
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
    let nodes: Vec<dag::Node<'_>> = plan
        .iter()
        .map(|(key, depends_on)| dag::Node { key, depends_on })
        .collect();
    dag::validate(&nodes).map_err(|e| Status::invalid_argument(e.to_string()))?;
    for step in &req.steps {
        if step.action_type.trim().is_empty() {
            return Err(Status::invalid_argument(format!(
                "step {}: action_type is required",
                step.key.trim()
            )));
        }
    }

    let rules = db::policies::rules(&rt.pool).await.map_err(db_status)?;
    let identity = db::identities::by_name(&rt.pool, adapters::SIMULATED)
        .await
        .map_err(|_| Status::internal("the simulated identity is missing"))?;

    let id = Uuid::now_v7();
    let title = if req.title.trim().is_empty() {
        "Untitled execution"
    } else {
        req.title.trim()
    };
    let planning = convert::status_to_db(ExecutionStatus::Planning);

    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    db::executions::insert(
        &mut tx,
        NewExecution {
            id,
            project_id: req.project_id.trim(),
            project_name: "",
            proposal_id: req.proposal_id.trim(),
            pattern_id: req.pattern_id.trim(),
            title,
            status: planning,
            adapter: adapters::SIMULATED,
            identity_id: identity.id,
        },
    )
    .await
    .map_err(db_status)?;

    for (position, (step, (key, depends_on))) in req.steps.iter().zip(plan).enumerate() {
        let rule = policy::lookup(&rules, step.action_type.trim());
        let row = StepRow {
            id: Uuid::now_v7(),
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
            required_permissions: step
                .required_permissions
                .iter()
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty())
                .collect(),
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
        db::steps::insert(&mut tx, &row).await.map_err(db_status)?;
    }
    let event = db::events::append(
        &mut tx,
        NewEvent {
            execution_id: id,
            r#type: convert::event_type_to_db(EventType::StateChanged),
            step_key: "",
            message: format!("planned {} steps", req.steps.len()),
            status: planning,
            data: [("steps".to_owned(), req.steps.len().to_string())].into(),
        },
    )
    .await
    .map_err(db_status)?;
    tx.commit().await.map_err(db_status)?;
    rt.bus.publish(convert::event_to_proto(event));

    load_execution(rt, id, true).await
}

pub async fn list(
    rt: &Arc<Runtime>,
    req: ListExecutionsRequest,
) -> Result<ListExecutionsResponse, Status> {
    let limit = match i64::from(req.limit) {
        0 => DEFAULT_LIST_LIMIT,
        n => n.min(MAX_LIST_LIMIT),
    };
    let rows = db::executions::list(&rt.pool, req.project_id.trim(), limit)
        .await
        .map_err(db_status)?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mut steps = db::steps::list_for_many(&rt.pool, &ids)
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

pub async fn start(rt: &Arc<Runtime>, req: StartExecutionRequest) -> Result<Execution, Status> {
    let id = parse_id(&req.id, "id")?;
    let row = db::executions::get(&rt.pool, id).await.map_err(db_status)?;
    if convert::status_from_db(&row.status) != ExecutionStatus::Planning {
        return Err(Status::failed_precondition(format!(
            "execution is {}, only PLANNING executions can be started",
            row.status
        )));
    }
    let fault = match req.fault {
        Some(fault) if !fault.step_key.trim().is_empty() => {
            let steps = db::steps::list_for(&rt.pool, id).await.map_err(db_status)?;
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
    rt.command(id, |reply| Command::Start { fault, reply })
        .await?;
    load_execution(rt, id, true).await
}

pub async fn resolve(rt: &Arc<Runtime>, req: ResolveFailureRequest) -> Result<Execution, Status> {
    let id = parse_id(&req.execution_id, "execution_id")?;
    let row = db::executions::get(&rt.pool, id).await.map_err(db_status)?;
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
    rt.command(id, |reply| Command::Resolve {
        option_id: req.option_id,
        instruction: req.instruction,
        reply,
    })
    .await?;
    load_execution(rt, id, true).await
}

pub async fn cancel(rt: &Arc<Runtime>, req: CancelExecutionRequest) -> Result<Execution, Status> {
    let id = parse_id(&req.id, "id")?;
    let row = db::executions::get(&rt.pool, id).await.map_err(db_status)?;
    if state::is_terminal(convert::status_from_db(&row.status)) {
        return Err(Status::failed_precondition(format!(
            "execution is already {}",
            row.status
        )));
    }
    rt.command(id, |reply| Command::Cancel {
        reason: req.reason,
        reply,
    })
    .await?;
    load_execution(rt, id, true).await
}
