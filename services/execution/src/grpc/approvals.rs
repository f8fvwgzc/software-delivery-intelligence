use std::sync::Arc;

use sdi_common::grpc::db_status;
use sdi_proto::execution::v1::{
    Approval, ApprovalStatus, DecideApprovalRequest, ListApprovalsRequest, ListApprovalsResponse,
};
use tonic::Status;

use super::{Caller, parse_id};
use crate::{
    convert, db,
    domain::state,
    runner::{Command, Runtime},
};

pub async fn list(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: ListApprovalsRequest,
) -> Result<ListApprovalsResponse, Status> {
    let status = match ApprovalStatus::try_from(req.status).unwrap_or_default() {
        ApprovalStatus::Unspecified => "",
        other => convert::approval_status_to_db(other),
    };
    let execution_id = if req.execution_id.trim().is_empty() {
        None
    } else {
        Some(parse_id(&req.execution_id, "execution_id")?)
    };
    let rows = db::approvals::list(&rt.pool, caller.workspace, status, execution_id)
        .await
        .map_err(db_status)?;
    Ok(ListApprovalsResponse {
        approvals: rows.into_iter().map(convert::approval_to_proto).collect(),
    })
}

/// Records the decision, then wakes the runner parked on it. The decision
/// stands even when the runner cannot act (e.g. the execution was cancelled
/// meanwhile), which is why it is written first. `decided_by` defaults to
/// the caller.
pub async fn decide(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: DecideApprovalRequest,
) -> Result<Approval, Status> {
    let workspace = caller.workspace;
    let id = parse_id(&req.id, "id")?;
    let current = db::approvals::get(&rt.pool, workspace, id)
        .await
        .map_err(db_status)?;
    if current.status != "PENDING" {
        return Err(Status::failed_precondition(format!(
            "approval is already {}",
            current.status.to_ascii_lowercase()
        )));
    }
    let decided_by = if req.decided_by.trim().is_empty() {
        caller.principal.user_id.as_str()
    } else {
        req.decided_by.trim()
    };

    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    let decided = db::approvals::decide(
        &mut tx,
        workspace,
        id,
        req.approve,
        decided_by,
        req.note.trim(),
    )
    .await
    .map_err(db_status)?;
    if !decided {
        return Err(Status::failed_precondition(
            "approval was decided concurrently",
        ));
    }
    tx.commit().await.map_err(db_status)?;
    let verdict = if req.approve { "approved" } else { "rejected" };
    rt.platform.approval(
        current.workspace_id,
        &id.to_string(),
        &current.execution_id.to_string(),
        &current.project_id,
        "updated",
        verdict,
    );
    caller.audit(
        rt,
        "approval.decide",
        "approval",
        &id.to_string(),
        format!(
            "{verdict} '{}' ({}) of execution {}",
            current.step_title, current.action_type, current.execution_id
        ),
    );

    let execution = db::executions::get(&rt.pool, workspace, current.execution_id)
        .await
        .map_err(db_status)?;
    if !state::is_terminal(convert::status_from_db(&execution.status))
        && let Err(error) = rt
            .command(workspace, current.execution_id, |reply| {
                Command::ApprovalDecided {
                    approval_id: id,
                    reply,
                }
            })
            .await
    {
        tracing::warn!(approval = %id, execution = %current.execution_id, %error, "runner did not take the decision");
    }

    db::approvals::get(&rt.pool, workspace, id)
        .await
        .map(convert::approval_to_proto)
        .map_err(db_status)
}
