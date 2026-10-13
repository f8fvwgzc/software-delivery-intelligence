//! What the three autopilot actions share (CONTRACT.md §23): creating the
//! execution, recording it, and finding out whether a recorded one is
//! still open. Intelligence never runs anything itself: it writes the plan
//! and hands it to the execution service, where permission → policy →
//! approval → act → verify applies as for every other execution.

use std::collections::HashMap;

use chrono::Utc;
use sdi_common::grpc::db_status;
use sdi_proto::execution::v1::{CreateExecutionRequest, ExecutionStatus};
use tonic::{Code, Status};
use uuid::Uuid;

use super::{Caller, IntelligenceGrpc};
use crate::{
    convert,
    db::{
        autopilot::{self as repo, AutopilotExecutionRow, KIND_FIX},
        reviews,
    },
    domain::{model::PlanStepData, plans},
};

/// Whether a recorded execution still stands in the way of a new one.
#[derive(Debug)]
pub(super) enum Liveness {
    /// Not terminal: planning, waiting for approval, running, paused on a
    /// failure. Carries the status in the execution's own words.
    Open(String),
    /// It ended: "succeeded", "failed", "cancelled", or "gone" (execution
    /// no longer knows it).
    Ended(String),
    /// The execution service could not say.
    Unknown(Status),
}

fn status_word(status: ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Planning => "planning: it was created and not started yet",
        ExecutionStatus::WaitingForApproval => "waiting for approval",
        ExecutionStatus::Authorized => "authorized",
        ExecutionStatus::Executing => "executing",
        ExecutionStatus::Verifying => "verifying",
        ExecutionStatus::Paused => "paused on a failure",
        ExecutionStatus::Diagnosing => "paused on a failure, waiting for a decision",
        ExecutionStatus::Replanning => "replanning",
        ExecutionStatus::Succeeded => "succeeded",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::Cancelled => "cancelled",
        ExecutionStatus::Unspecified => "in an unknown state",
    }
}

/// What `autopilot_executions.outcome` says of an execution that did
/// what it was created for.
pub(super) const SUCCEEDED: &str = "succeeded";

/// How an execution ended, in the words `autopilot_executions.outcome` keeps.
fn outcome_of(status: ExecutionStatus) -> Option<&'static str> {
    match status {
        ExecutionStatus::Succeeded => Some(SUCCEEDED),
        ExecutionStatus::Failed => Some("failed"),
        ExecutionStatus::Cancelled => Some("cancelled"),
        _ => None,
    }
}

/// Asks the execution service about a recorded execution, unless we
/// already know it ended. The first terminal answer is remembered, and a
/// fix execution that ended without committing (failed, cancelled, gone)
/// frees its findings so their suggestions can be applied again.
pub(super) async fn liveness(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    row: &AutopilotExecutionRow,
) -> Liveness {
    if row.settled() {
        return Liveness::Ended(row.outcome.clone());
    }
    let outcome = match svc
        .execution
        .get_execution(&caller.principal, &row.execution_id)
        .await
    {
        Ok(execution) => match outcome_of(execution.status()) {
            Some(outcome) => outcome,
            None => return Liveness::Open(status_word(execution.status()).to_owned()),
        },
        // Deleted with the workspace's execution data: nothing is running.
        Err(error) if error.status.code() == Code::NotFound => "gone",
        Err(error) => return Liveness::Unknown(error.into_status()),
    };
    if let Err(error) = repo::set_outcome(&svc.pool, caller.workspace, row.id, outcome).await {
        tracing::warn!(%error, execution = %row.execution_id, "could not record the execution's outcome");
    }
    if row.kind == KIND_FIX && outcome != SUCCEEDED {
        match reviews::release_fix_execution(&svc.pool, caller.workspace, &row.execution_id).await {
            Ok(0) => {}
            Ok(freed) => tracing::info!(
                execution = %row.execution_id, outcome, freed,
                "fix execution ended without a commit; its findings are available again"
            ),
            Err(error) => {
                tracing::warn!(%error, execution = %row.execution_id, "could not free the findings of an ended fix execution")
            }
        }
    }
    Liveness::Ended(outcome.to_owned())
}

/// The plan as one `github` execution, created and set going. The caller's
/// principal is forwarded, so the execution service applies its own role
/// check for the adapter. Started means "runs until its first approval":
/// with the default policies that is before anything is written, and a
/// workspace that set a step's policy to automatic has decided that the
/// step runs without one (CONTRACT.md §23).
///
/// A start that fails does not undo the request: the execution exists, is
/// recorded by the caller and can be started from its page, so its id is
/// returned and the failure is logged.
pub(super) async fn create_execution(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    project_id: &str,
    title: String,
    steps: &[PlanStepData],
    artifacts: HashMap<String, String>,
) -> Result<String, Status> {
    let execution = svc
        .execution
        .create_execution(
            &caller.principal,
            CreateExecutionRequest {
                project_id: project_id.to_owned(),
                proposal_id: String::new(),
                pattern_id: String::new(),
                title,
                steps: steps.iter().map(convert::plan_step).collect(),
                adapter: plans::GITHUB.to_owned(),
                artifacts,
            },
        )
        .await
        .map_err(|e| e.into_status())?;
    if execution.id.trim().is_empty() {
        return Err(Status::internal(
            "the execution service created an execution without an id",
        ));
    }
    if let Err(error) = svc
        .execution
        .start_execution(&caller.principal, &execution.id)
        .await
    {
        tracing::warn!(
            execution = %execution.id,
            status = %error.status,
            "execution created but not started; it waits in planning until someone starts it"
        );
    }
    Ok(execution.id)
}

/// One request at a time for one thing (the fixes of a review, the merge
/// of a pull request, a project's Dependabot setup). Each handler checks
/// "is one already open?" and then creates; without this, two requests
/// arriving together both pass the check and both create an execution.
/// Held until dropped, by a transaction-scoped advisory lock: a crash
/// releases it, and it works across replicas. The schema is part of the
/// key so two installs sharing a database never block each other.
pub(super) struct Exclusive {
    _held: sqlx::Transaction<'static, sqlx::Postgres>,
}

/// Takes the lock for `what`, or refuses at once when someone holds it:
/// the request already being handled will create the execution.
pub(super) async fn exclusive(
    svc: &IntelligenceGrpc,
    workspace: Uuid,
    what: &str,
) -> Result<Exclusive, Status> {
    let mut tx = svc.pool.begin().await.map_err(db_status)?;
    let taken: bool = sqlx::query_scalar(
        "SELECT pg_try_advisory_xact_lock(hashtextextended(current_schema() || ':' || $1, 0))",
    )
    .bind(format!("sdi.autopilot:{workspace}:{what}"))
    .fetch_one(&mut *tx)
    .await
    .map_err(db_status)?;
    if taken {
        Ok(Exclusive { _held: tx })
    } else {
        Err(Status::failed_precondition(
            "the same request is being handled right now; open the execution it creates instead of asking again",
        ))
    }
}

/// What to remember about an execution the autopilot asked for.
pub(super) struct Recorded<'a> {
    pub kind: &'a str,
    pub project_id: &'a str,
    pub pr_number: i64,
    pub review_id: Option<Uuid>,
    pub head_sha: &'a str,
    pub execution_id: &'a str,
}

pub(super) async fn record(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    recorded: Recorded<'_>,
) -> Result<AutopilotExecutionRow, Status> {
    let row = AutopilotExecutionRow {
        id: Uuid::now_v7(),
        workspace_id: caller.workspace,
        project_id: recorded.project_id.to_owned(),
        kind: recorded.kind.to_owned(),
        pr_number: recorded.pr_number,
        review_id: recorded.review_id,
        head_sha: recorded.head_sha.to_owned(),
        execution_id: recorded.execution_id.to_owned(),
        requested_by: caller.principal.user_id.clone(),
        outcome: String::new(),
        created_at: Utc::now(),
    };
    repo::insert(&svc.pool, &row).await.map_err(|error| {
        // The execution exists; without this row nothing here knows it.
        tracing::error!(%error, execution = %row.execution_id, kind = %row.kind, "execution created but not recorded");
        db_status(error)
    })?;
    Ok(row)
}
