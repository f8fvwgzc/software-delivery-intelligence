use std::sync::Arc;

use sdi_common::grpc::db_status;
use sdi_proto::execution::v1::{ListPoliciesResponse, Policy, PolicyDecision, UpdatePolicyRequest};
use tonic::Status;

use crate::{convert, db, domain::policy, runner::Runtime};

pub async fn list(rt: &Arc<Runtime>) -> Result<ListPoliciesResponse, Status> {
    let rows = db::policies::list(&rt.pool).await.map_err(db_status)?;
    Ok(ListPoliciesResponse {
        policies: rows.into_iter().map(convert::policy_to_proto).collect(),
    })
}

pub async fn update(rt: &Arc<Runtime>, req: UpdatePolicyRequest) -> Result<Policy, Status> {
    let action_type = req.action_type.trim();
    if action_type.is_empty() {
        return Err(Status::invalid_argument("action_type is required"));
    }
    let decision = PolicyDecision::try_from(req.decision)
        .map_err(|_| Status::invalid_argument("decision is not a known PolicyDecision"))?;
    let current = db::policies::get(&rt.pool, action_type)
        .await
        .map_err(db_status)?;
    policy::update_allowed(current.editable, decision).map_err(Status::failed_precondition)?;

    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    let row = db::policies::update(&mut tx, action_type, convert::decision_to_db(decision))
        .await
        .map_err(db_status)?;
    tx.commit().await.map_err(db_status)?;
    Ok(convert::policy_to_proto(row))
}
