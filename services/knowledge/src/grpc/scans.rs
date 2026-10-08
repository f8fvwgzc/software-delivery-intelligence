use sdi_common::grpc::db_status;
use sdi_proto::knowledge::v1::*;
use tonic::Status;

use super::{KnowledgeState, limit, parse_id};
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
    let scan = scanner::start(&state.pool, &state.github, project)
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
