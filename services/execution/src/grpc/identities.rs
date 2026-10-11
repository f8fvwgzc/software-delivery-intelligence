use std::sync::Arc;

use sdi_common::grpc::db_status;
use sdi_proto::execution::v1::{GrantPermissionRequest, Identity, ListIdentitiesResponse};
use tonic::Status;

use super::{Caller, parse_id};
use crate::{
    convert, db,
    runner::{GRANT_TTL_MINUTES, Runtime},
};

pub async fn list(rt: &Arc<Runtime>, caller: &Caller) -> Result<ListIdentitiesResponse, Status> {
    let identities = db::identities::list(&rt.pool, caller.workspace)
        .await
        .map_err(db_status)?;
    let grants = db::identities::all_grants(&rt.pool, caller.workspace)
        .await
        .map_err(db_status)?;
    Ok(ListIdentitiesResponse {
        identities: convert::identities_to_proto(identities, grants),
    })
}

pub async fn grant(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: GrantPermissionRequest,
) -> Result<Identity, Status> {
    let workspace = caller.workspace;
    let id = parse_id(&req.identity_id, "identity_id")?;
    let permissions: Vec<String> = req
        .permissions
        .iter()
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect();
    if permissions.is_empty() {
        return Err(Status::invalid_argument("permissions must not be empty"));
    }
    if let Some(bad) = permissions.iter().find(|p| !p.contains(':')) {
        return Err(Status::invalid_argument(format!(
            "permission {bad} must look like <resource>:<verb>"
        )));
    }
    let ttl = match req.ttl_minutes {
        0 => GRANT_TTL_MINUTES,
        n => i64::from(n),
    };
    let identity = db::identities::get(&rt.pool, workspace, id)
        .await
        .map_err(db_status)?;

    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    db::identities::grant(&mut tx, workspace, id, &permissions, ttl)
        .await
        .map_err(db_status)?;
    tx.commit().await.map_err(db_status)?;
    caller.audit(
        rt,
        "identity.grant",
        "identity",
        &id.to_string(),
        format!(
            "{} granted {} for {ttl} minutes",
            identity.name,
            permissions.join(", ")
        ),
    );

    let grants = db::identities::grants_for(&rt.pool, identity.workspace_id, id)
        .await
        .map_err(db_status)?;
    Ok(convert::identity_to_proto(identity, grants))
}
