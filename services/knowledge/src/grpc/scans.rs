use std::future::ready;

use futures::{StreamExt, stream};
use sdi_common::grpc::db_status;
use sdi_proto::knowledge::v1::*;
use sqlx::PgPool;
use tokio_stream::wrappers::BroadcastStream;
use tonic::Status;
use uuid::Uuid;

use super::{KnowledgeState, ResponseStream, limit, parse_id};
use crate::{convert, db, scanner};

pub(super) async fn start_scan(
    state: &KnowledgeState,
    req: StartScanRequest,
) -> Result<Scan, Status> {
    let project = state.project(&req.project_id).await?;
    if !state.github.token_configured() {
        return Err(Status::failed_precondition(
            "GITHUB_TOKEN is not configured",
        ));
    }
    if db::scans::active_exists(&state.pool, project.id)
        .await
        .map_err(db_status)?
    {
        return Err(Status::failed_precondition(
            "a scan is already running for this project",
        ));
    }
    let scan = scanner::start(&state.pool, &state.github, &state.events, project)
        .await
        .map_err(db_status)?;
    Ok(convert::scan(scan))
}

pub(super) async fn get_scan(state: &KnowledgeState, req: GetScanRequest) -> Result<Scan, Status> {
    let id = parse_id(&req.id, "scan id")?;
    db::scans::get(&state.pool, id)
        .await
        .map(convert::scan)
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => Status::not_found("scan not found"),
            other => db_status(other),
        })
}

pub(super) async fn list_scans(
    state: &KnowledgeState,
    req: ListScansRequest,
) -> Result<ListScansResponse, Status> {
    let project_id = parse_id(&req.project_id, "project id")?;
    let scans = db::scans::list(&state.pool, project_id, limit(req.limit, 20, 200))
        .await
        .map_err(db_status)?;
    Ok(ListScansResponse {
        scans: scans.into_iter().map(convert::scan).collect(),
    })
}

fn is_terminal(status: &str) -> bool {
    matches!(status, "succeeded" | "failed")
}

/// Replays the scan as it is now, then re-reads and forwards it after every
/// event the scanner emits for it, and ends once it is terminal. The bus is
/// subscribed before the first read so an update landing in between is
/// forwarded (as a harmless duplicate) rather than lost. A lagging
/// subscriber re-reads the scan instead of trusting what it missed.
pub(super) async fn watch_scan(
    state: &KnowledgeState,
    req: WatchScanRequest,
) -> Result<ResponseStream<Scan>, Status> {
    let id = parse_id(&req.id, "scan id")?;
    let receiver = state.events.subscribe();
    let current = db::scans::get(&state.pool, id).await.map_err(|e| match e {
        sqlx::Error::RowNotFound => Status::not_found("scan not found"),
        other => db_status(other),
    })?;
    let terminal = is_terminal(&current.status);
    let first = stream::once(ready(Ok(convert::scan(current))));
    if terminal {
        return Ok(Box::pin(first));
    }

    let id_text = id.to_string();
    let updates = stream::unfold(
        (BroadcastStream::new(receiver), state.pool.clone(), false),
        move |(mut events, pool, done)| {
            let id_text = id_text.clone();
            async move {
                if done {
                    return None;
                }
                loop {
                    match events.next().await {
                        None => return None,
                        Some(Ok(event))
                            if !(event.entity == "scan" && event.entity_id == id_text) =>
                        {
                            continue;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(error)) => {
                            tracing::warn!(%error, scan = %id_text, "WatchScan lagged, re-reading");
                        }
                    }
                    return Some(match fetch(&pool, id).await {
                        Ok(row) => {
                            let terminal = is_terminal(&row.status);
                            (Ok(convert::scan(row)), (events, pool, terminal))
                        }
                        Err(status) => (Err(status), (events, pool, true)),
                    });
                }
            }
        },
    );
    Ok(Box::pin(first.chain(updates)))
}

async fn fetch(pool: &PgPool, id: Uuid) -> Result<db::scans::ScanRow, Status> {
    db::scans::get(pool, id).await.map_err(|e| match e {
        // Deleted mid-watch (project removed): the stream ends with the reason.
        sqlx::Error::RowNotFound => Status::not_found("scan no longer exists"),
        other => db_status(other),
    })
}
