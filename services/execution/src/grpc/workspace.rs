//! Whole-workspace data RPCs (CONTRACT.md §19): a complete export and a
//! real delete. The workspace named in the request must be the caller's:
//! metadata is the authority, the body only confirms the intent.
//!
//! Delete first cancels whatever is still running (through the runners, so
//! containers are stopped the normal way), then removes every workdir and
//! container a real run may have left, then deletes the rows.

use std::{collections::HashMap, sync::Arc};

use sdi_common::grpc::db_status;
use sdi_proto::execution::v1::{
    DeleteWorkspaceDataRequest, DeleteWorkspaceDataResponse, ExecutionStatus,
    ExportWorkspaceDataRequest, WorkspaceDataRecord,
};
use tokio::sync::mpsc;
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tonic::Status;

use super::Caller;
use crate::{
    adapters, convert, db,
    runner::{Command, Runtime},
};

const EXPORT_BUFFER: usize = 64;
const DELETE_REASON: &str = "workspace data deleted";

/// Rejects a body that names another workspace than the caller's.
fn check_workspace(caller: &Caller, workspace_id: &str) -> Result<(), Status> {
    let named = workspace_id.trim();
    if named.is_empty() {
        return Err(Status::invalid_argument("workspace_id is required"));
    }
    if named != caller.principal.workspace_id {
        return Err(Status::permission_denied(
            "workspace_id does not match the caller's workspace",
        ));
    }
    Ok(())
}

pub async fn delete(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: DeleteWorkspaceDataRequest,
) -> Result<DeleteWorkspaceDataResponse, Status> {
    check_workspace(caller, &req.workspace_id)?;
    let workspace = caller.workspace;

    // 1. Cancel every live run through its runner.
    let live_statuses = [
        ExecutionStatus::Planning,
        ExecutionStatus::WaitingForApproval,
        ExecutionStatus::Authorized,
        ExecutionStatus::Executing,
        ExecutionStatus::Verifying,
        ExecutionStatus::Paused,
        ExecutionStatus::Diagnosing,
        ExecutionStatus::Replanning,
    ]
    .map(convert::status_to_db);
    let live = db::executions::with_status_in(&rt.pool, workspace, &live_statuses)
        .await
        .map_err(db_status)?;
    let mut cancelled = 0u32;
    for row in &live {
        match rt
            .command(workspace, row.id, |reply| Command::Cancel {
                reason: DELETE_REASON.to_owned(),
                reply,
            })
            .await
        {
            Ok(()) => cancelled += 1,
            // It may have finished in the meantime; the rows go anyway.
            Err(error) => {
                tracing::warn!(execution = %row.id, %error, "could not cancel before deleting")
            }
        }
    }

    // 2. Nothing of a real run stays on disk or in the daemon.
    let executions = db::executions::ids_and_adapters(&rt.pool, workspace)
        .await
        .map_err(db_status)?;
    for (id, adapter) in &executions {
        if adapter != adapters::SIMULATED {
            for adapter in rt.adapters.real() {
                adapter.cleanup(*id).await;
            }
        }
    }

    // 3. The rows.
    let removed = db::workspace::delete(&rt.pool, workspace)
        .await
        .map_err(db_status)?;
    rt.forget_workspace(workspace);
    let deleted: HashMap<String, u64> = removed
        .iter()
        .map(|(entity, count)| ((*entity).to_owned(), *count))
        .collect();
    tracing::info!(%workspace, cancelled, ?deleted, "workspace data deleted");
    Ok(DeleteWorkspaceDataResponse { deleted, cancelled })
}

pub fn export(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: ExportWorkspaceDataRequest,
) -> Result<ReceiverStream<Result<WorkspaceDataRecord, Status>>, Status> {
    check_workspace(caller, &req.workspace_id)?;
    let mut rows = db::workspace::export(rt.pool.clone(), caller.workspace);
    let (tx, rx) = mpsc::channel(EXPORT_BUFFER);
    sdi_common::tasks::spawn("workspace-export", async move {
        while let Some(row) = rows.next().await {
            let item = row
                .map(|(entity, json)| WorkspaceDataRecord {
                    entity: entity.to_owned(),
                    json,
                })
                .map_err(db_status);
            let failed = item.is_err();
            if tx.send(item).await.is_err() || failed {
                return;
            }
        }
    });
    Ok(ReceiverStream::new(rx))
}
