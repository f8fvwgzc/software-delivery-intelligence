//! Per-workspace data controls and the workspace-owned reasoner key
//! (CONTRACT.md §19).

use sdi_common::grpc::db_status;
use sdi_proto::identity::v1::{
    DataControls, GetDataControlsRequest, GetReasonerKeyRequest, ReasonerKey,
    SetReasonerKeyRequest, UpdateDataControlsRequest,
};
use tonic::Status;

use super::{IdentityState, parse_id};
use crate::{convert, db};

pub const CODE_CACHE_MODES: [&str; 3] = ["none", "session", "persistent"];
pub const REASONERS: [&str; 2] = ["off", "claude"];

pub async fn get(
    state: &IdentityState,
    req: GetDataControlsRequest,
) -> Result<DataControls, Status> {
    let id = parse_id(&req.workspace_id, "workspace_id")?;
    // Workspaces created before this table existed get their defaults here.
    db::workspaces::ensure_controls(&state.pool, id)
        .await
        .map_err(db_status)?;
    let row = db::controls::get(&state.pool, id)
        .await
        .map_err(db_status)?;
    Ok(convert::controls_to_proto(row))
}

pub async fn update(
    state: &IdentityState,
    req: UpdateDataControlsRequest,
) -> Result<DataControls, Status> {
    let controls = req
        .controls
        .ok_or_else(|| Status::invalid_argument("controls is required"))?;
    let id = parse_id(&controls.workspace_id, "workspace_id")?;
    let code_cache = controls.code_cache.trim().to_ascii_lowercase();
    if !CODE_CACHE_MODES.contains(&code_cache.as_str()) {
        return Err(Status::invalid_argument(
            "code_cache must be one of none, session, persistent",
        ));
    }
    let reasoner = controls.reasoner.trim().to_ascii_lowercase();
    if !REASONERS.contains(&reasoner.as_str()) {
        return Err(Status::invalid_argument("reasoner must be off or claude"));
    }
    if controls.code_cache_ttl_hours == 0 || controls.code_cache_ttl_hours > 24 * 365 {
        return Err(Status::invalid_argument(
            "code_cache_ttl_hours must be between 1 and 8760",
        ));
    }
    if controls.audit_retention_days > 3650 {
        return Err(Status::invalid_argument(
            "audit_retention_days must be at most 3650 (0 keeps forever)",
        ));
    }
    db::workspaces::ensure_controls(&state.pool, id)
        .await
        .map_err(db_status)?;
    db::controls::update(
        &state.pool,
        id,
        &code_cache,
        controls.code_cache_ttl_hours as i32,
        controls.minimal_chunks,
        &reasoner,
        controls.audit_retention_days as i32,
    )
    .await
    .map_err(db_status)?;
    let row = db::controls::get(&state.pool, id)
        .await
        .map_err(db_status)?;
    Ok(convert::controls_to_proto(row))
}

pub async fn set_key(
    state: &IdentityState,
    req: SetReasonerKeyRequest,
) -> Result<DataControls, Status> {
    let id = parse_id(&req.workspace_id, "workspace_id")?;
    db::workspaces::ensure_controls(&state.pool, id)
        .await
        .map_err(db_status)?;
    let key = req.api_key.trim();
    if key.is_empty() {
        db::controls::delete_key(&state.pool, id)
            .await
            .map_err(db_status)?;
    } else {
        let hint: String = key
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        db::controls::set_key(&state.pool, id, &state.keys.seal(key.as_bytes()), &hint)
            .await
            .map_err(db_status)?;
    }
    db::controls::touch(&state.pool, id)
        .await
        .map_err(db_status)?;
    let row = db::controls::get(&state.pool, id)
        .await
        .map_err(db_status)?;
    Ok(convert::controls_to_proto(row))
}

/// Internal (intelligence only): the key in clear, for one request.
pub async fn get_key(
    state: &IdentityState,
    req: GetReasonerKeyRequest,
) -> Result<ReasonerKey, Status> {
    let id = parse_id(&req.workspace_id, "workspace_id")?;
    let sealed = db::controls::get_sealed_key(&state.pool, id)
        .await
        .map_err(db_status)?
        .ok_or_else(|| Status::not_found("no reasoner key is stored for this workspace"))?;
    let api_key = state.keys.open_str(&sealed).map_err(|error| {
        tracing::error!(%error, "stored reasoner key does not open");
        Status::internal("stored key is unreadable")
    })?;
    Ok(ReasonerKey {
        api_key,
        model: String::new(),
    })
}
