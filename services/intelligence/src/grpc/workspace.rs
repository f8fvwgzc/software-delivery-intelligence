//! Whole-workspace data RPCs (CONTRACT.md §19): a complete export and a
//! real delete. The workspace named in the request must be the caller's:
//! metadata is the authority, the body only confirms the intent.

use std::collections::HashMap;

use sdi_common::grpc::db_status;
use sdi_proto::intelligence::v1::{
    DeleteWorkspaceDataRequest, DeleteWorkspaceDataResponse, ExportWorkspaceDataRequest,
    WorkspaceDataRecord,
};
use tokio::sync::mpsc;
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tonic::Status;

use super::{Caller, IntelligenceGrpc};
use crate::db;

const EXPORT_BUFFER: usize = 64;

/// Rejects a body that names another workspace than the caller's.
pub(super) fn check_workspace(caller: &Caller, workspace_id: &str) -> Result<(), Status> {
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

pub(super) async fn delete(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: DeleteWorkspaceDataRequest,
) -> Result<DeleteWorkspaceDataResponse, Status> {
    check_workspace(caller, &request.workspace_id)?;
    let removed = db::workspace::delete(&svc.pool, caller.workspace)
        .await
        .map_err(db_status)?;
    let deleted: HashMap<String, u64> = removed
        .iter()
        .map(|(entity, count)| ((*entity).to_owned(), *count))
        .collect();
    tracing::info!(workspace = %caller.workspace, ?deleted, "workspace data deleted");
    svc.events.emit(
        caller.workspace,
        "workspace",
        &caller.principal.workspace_id,
        "",
        "deleted",
        "",
        &[],
    );
    Ok(DeleteWorkspaceDataResponse { deleted })
}

pub(super) fn export(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: ExportWorkspaceDataRequest,
) -> Result<ReceiverStream<Result<WorkspaceDataRecord, Status>>, Status> {
    check_workspace(caller, &request.workspace_id)?;
    let mut rows = db::workspace::export(svc.pool.clone(), caller.workspace);
    let (tx, rx) = mpsc::channel(EXPORT_BUFFER);
    tokio::spawn(async move {
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
