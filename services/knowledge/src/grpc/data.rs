//! Data controls (CONTRACT.md §19): purge the code cache, delete everything
//! the workspace stored, export everything it stored.

use futures::StreamExt;
use sdi_common::{grpc::db_status, principal::Role};
use sdi_proto::knowledge::v1::*;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Status;

use super::{KnowledgeState, ResponseStream, opt_id, own_workspace};
use crate::{auth::Caller, db};

/// Rows buffered between the database and the client.
const EXPORT_BUFFER: usize = 64;

pub(super) async fn purge_code_cache(
    state: &KnowledgeState,
    caller: &Caller,
    req: PurgeCodeCacheRequest,
) -> Result<PurgeCodeCacheResponse, Status> {
    caller.require(Role::Admin)?;
    own_workspace(caller, &req.workspace_id)?;
    let ws = caller.workspace_id;
    let project_id = match opt_id(&req.project_id, "project id")? {
        // The project must be the workspace's own, or the answer is NOT_FOUND.
        Some(_) => Some(state.project(caller, &req.project_id).await?.id),
        None => None,
    };
    let mut conn = state.pool.acquire().await.map_err(db_status)?;
    let purged = db::code::purge(&mut conn, ws, project_id)
        .await
        .map_err(db_status)?;
    state.identity.audit(
        caller,
        "data.purge",
        "workspace",
        &ws.to_string(),
        &format!(
            "files={} trees={} project={}",
            purged.files,
            purged.trees,
            project_id.map(|id| id.to_string()).unwrap_or_default()
        ),
    );
    tracing::info!(workspace = %ws, ?project_id, files = purged.files, trees = purged.trees, "code cache purged");
    Ok(PurgeCodeCacheResponse {
        files_deleted: purged.files,
        trees_deleted: purged.trees,
    })
}

/// Everything, in one transaction; the in-memory caches for the workspace
/// (GitHub token, ETags, data controls) go with it.
pub(super) async fn delete_workspace_data(
    state: &KnowledgeState,
    caller: &Caller,
    req: DeleteWorkspaceDataRequest,
) -> Result<DeleteWorkspaceDataResponse, Status> {
    caller.require(Role::Owner)?;
    own_workspace(caller, &req.workspace_id)?;
    let ws = caller.workspace_id;
    let projects = db::projects::list(&state.pool, ws)
        .await
        .map_err(db_status)?;
    let mut tx = state.pool.begin().await.map_err(db_status)?;
    let deleted = db::workspace::delete_all(&mut tx, ws)
        .await
        .map_err(db_status)?;
    tx.commit().await.map_err(db_status)?;
    state.github.forget_cache(ws);
    state.github.forget_token(ws);
    state.controls.invalidate(ws);
    for project in projects {
        state.events.project(ws, project.id, "deleted", "");
    }
    let summary: Vec<String> = deleted
        .iter()
        .filter(|(_, rows)| **rows > 0)
        .map(|(table, rows)| format!("{table}={rows}"))
        .collect();
    state.identity.audit(
        caller,
        "data.delete",
        "workspace",
        &ws.to_string(),
        &summary.join(" "),
    );
    tracing::info!(workspace = %ws, deleted = ?deleted, "workspace data deleted");
    Ok(DeleteWorkspaceDataResponse {
        deleted: deleted.into_iter().collect(),
    })
}

/// Streams every row of every tenant table as `(entity, record)`. Rows are
/// read table by table through a bounded channel, so a large workspace
/// never sits in memory at once.
pub(super) async fn export_workspace_data(
    state: &KnowledgeState,
    caller: &Caller,
    req: ExportWorkspaceDataRequest,
) -> Result<ResponseStream<ExportRecord>, Status> {
    caller.require(Role::Admin)?;
    own_workspace(caller, &req.workspace_id)?;
    let ws = caller.workspace_id;
    state
        .identity
        .audit(caller, "data.export", "workspace", &ws.to_string(), "");

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<ExportRecord, Status>>(EXPORT_BUFFER);
    let pool = state.pool.clone();
    tokio::spawn(async move {
        for table in db::workspace::TENANT_TABLES {
            let mut rows = db::workspace::export_rows(&pool, ws, table);
            while let Some(row) = rows.next().await {
                let item = row
                    .map(|record| ExportRecord {
                        entity: table.to_owned(),
                        record,
                    })
                    .map_err(db_status);
                let failed = item.is_err();
                if tx.send(item).await.is_err() || failed {
                    // The client went away, or the database did: either ends the export.
                    return;
                }
            }
        }
        tracing::info!(workspace = %ws, "workspace data exported");
    });
    Ok(Box::pin(ReceiverStream::new(rx)))
}
