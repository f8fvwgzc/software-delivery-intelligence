//! The audit trail (CONTRACT.md §19).

use chrono::Utc;
use sdi_common::{grpc::db_status, time};
use sdi_proto::identity::v1::{
    ListAuditRequest, ListAuditResponse, RecordAuditRequest, RecordAuditResponse,
};
use tonic::Status;
use uuid::Uuid;

use super::{IdentityState, parse_id};
use crate::{convert, db::audit::AuditRow};

const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 1000;

pub async fn record(
    state: &IdentityState,
    req: RecordAuditRequest,
) -> Result<RecordAuditResponse, Status> {
    let entry = req
        .entry
        .ok_or_else(|| Status::invalid_argument("entry is required"))?;
    if entry.action.trim().is_empty() {
        return Err(Status::invalid_argument("action is required"));
    }
    let row = AuditRow {
        id: Uuid::parse_str(entry.id.trim()).unwrap_or_else(|_| Uuid::now_v7()),
        workspace_id: parse_id(&entry.workspace_id, "workspace_id")?,
        user_id: entry.user_id,
        user_login: entry.user_login,
        action: entry.action.trim().to_owned(),
        entity: entry.entity,
        entity_id: entry.entity_id,
        detail: entry.detail,
        request_id: entry.request_id,
        ip: entry.ip,
        at: time::opt_from_proto(entry.at.as_ref()).unwrap_or_else(Utc::now),
    };
    crate::db::audit::insert(&state.pool, &row)
        .await
        .map_err(db_status)?;
    Ok(RecordAuditResponse {})
}

pub async fn list(
    state: &IdentityState,
    req: ListAuditRequest,
) -> Result<ListAuditResponse, Status> {
    let workspace_id = parse_id(&req.workspace_id, "workspace_id")?;
    let limit = if req.limit == 0 {
        DEFAULT_LIMIT
    } else {
        req.limit.min(MAX_LIMIT)
    };
    let rows = crate::db::audit::list(
        &state.pool,
        workspace_id,
        req.action_prefix.trim(),
        time::opt_from_proto(req.before.as_ref()),
        i64::from(limit),
    )
    .await
    .map_err(db_status)?;
    Ok(ListAuditResponse {
        entries: rows.into_iter().map(convert::audit_to_proto).collect(),
    })
}
