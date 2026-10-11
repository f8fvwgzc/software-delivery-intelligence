use std::sync::Arc;

use sdi_common::grpc::db_status;
use sdi_proto::execution::v1::{
    Approval, ApprovalStatus, DecideApprovalRequest, ListApprovalsRequest, ListApprovalsResponse,
};
use tonic::Status;

use super::parse_id;
use crate::{
    convert, db,
    domain::state,
    runner::{Command, Runtime},
};

pub async fn list(
    rt: &Arc<Runtime>,
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
    let rows = db::approvals::list(&rt.pool, status, execution_id)
        .await
        .map_err(db_status)?;
    Ok(ListApprovalsResponse {
        approvals: rows.into_iter().map(convert::approval_to_proto).collect(),
    })
}

/// Records the decision, then wakes the runner parked on it. The decision
/// stands even when the runner cannot act (e.g. the execution was cancelled
/// meanwhile), which is why it is written first.
pub async fn decide(rt: &Arc<Runtime>, req: DecideApprovalRequest) -> Result<Approval, Status> {
    let id = parse_id(&req.id, "id")?;
    let current = db::approvals::get(&rt.pool, id).await.map_err(db_status)?;
    if current.status != "PENDING" {
        return Err(Status::failed_precondition(format!(
            "approval is already {}",
            current.status.to_ascii_lowercase()
        )));
    }

    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    let decided = db::approvals::decide(
        &mut tx,
        id,
        req.approve,
        req.decided_by.trim(),
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
    rt.platform.approval(
        &id.to_string(),
        &current.execution_id.to_string(),
        &current.project_id,
        "updated",
        if req.approve { "approved" } else { "rejected" },
    );

    let execution = db::executions::get(&rt.pool, current.execution_id)
        .await
        .map_err(db_status)?;
    if !state::is_terminal(convert::status_from_db(&execution.status))
        && let Err(error) = rt
            .command(current.execution_id, |reply| Command::ApprovalDecided {
                approval_id: id,
                reply,
            })
            .await
    {
        tracing::warn!(approval = %id, execution = %current.execution_id, %error, "runner did not take the decision");
    }

    db::approvals::get(&rt.pool, id)
        .await
        .map(convert::approval_to_proto)
        .map_err(db_status)
}
