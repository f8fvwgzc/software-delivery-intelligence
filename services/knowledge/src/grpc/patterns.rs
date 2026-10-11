use sdi_common::{grpc::db_status, principal::Role};
use sdi_proto::knowledge::v1::*;
use tonic::Status;

use super::{KnowledgeState, parse_id};
use crate::{auth::Caller, convert, db};

/// The catalog plus the workspace's own learned patterns.
pub(super) async fn list_patterns(
    state: &KnowledgeState,
    caller: &Caller,
    req: ListPatternsRequest,
) -> Result<ListPatternsResponse, Status> {
    let category = req.category.trim();
    let patterns = db::patterns::list(
        &state.pool,
        caller.workspace_id,
        (!category.is_empty()).then_some(category),
        req.include_deprecated,
    )
    .await
    .map_err(db_status)?;
    Ok(ListPatternsResponse {
        patterns: patterns.into_iter().map(convert::pattern).collect(),
    })
}

pub(super) async fn get_pattern(
    state: &KnowledgeState,
    caller: &Caller,
    req: GetPatternRequest,
) -> Result<Pattern, Status> {
    let id = req.id.trim();
    if id.is_empty() {
        return Err(Status::invalid_argument("pattern id is required"));
    }
    db::patterns::get(&state.pool, caller.workspace_id, id)
        .await
        .map(convert::pattern)
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => Status::not_found("pattern not found"),
            other => db_status(other),
        })
}

/// Called by execution with the forwarded principal once a run finished.
pub(super) async fn record_pattern_outcome(
    state: &KnowledgeState,
    caller: &Caller,
    req: RecordPatternOutcomeRequest,
) -> Result<Pattern, Status> {
    caller.require(Role::Member)?;
    let ws = caller.workspace_id;
    let pattern_id = req.pattern_id.trim();
    if pattern_id.is_empty() {
        return Err(Status::invalid_argument("pattern id is required"));
    }
    let project_id = parse_id(&req.project_id, "project id")?;
    if !db::patterns::exists(&state.pool, ws, pattern_id)
        .await
        .map_err(db_status)?
    {
        return Err(Status::not_found("pattern not found"));
    }
    state.project(caller, &req.project_id).await?;
    db::patterns::record_outcome(
        &state.pool,
        ws,
        pattern_id,
        project_id,
        req.succeeded,
        req.incident,
    )
    .await
    .map_err(db_status)?;
    let pattern = db::patterns::get(&state.pool, ws, pattern_id)
        .await
        .map_err(db_status)?;
    state
        .events
        .pattern(ws, pattern_id, "updated", &pattern.status);
    Ok(convert::pattern(pattern))
}
