use chrono::Utc;
use sdi_common::grpc::db_status;
use sdi_proto::intelligence::v1::{
    CreateDecisionRequest, Decision, DecisionScope, DecisionSource, ListDecisionsRequest,
    ListDecisionsResponse, SupersedeDecisionRequest,
};
use tonic::Status;
use uuid::Uuid;

use super::{
    IntelligenceGrpc, parse_id,
    proposals::{emit_decision, index_decision},
    require,
};
use crate::{
    convert,
    db::decisions::{self as repo, DecisionRow},
};

pub(super) async fn list(
    svc: &IntelligenceGrpc,
    request: ListDecisionsRequest,
) -> Result<ListDecisionsResponse, Status> {
    let rows = repo::list(
        &svc.pool,
        request.project_id.trim(),
        request.include_superseded,
    )
    .await
    .map_err(db_status)?;
    Ok(ListDecisionsResponse {
        decisions: rows.iter().map(convert::decision).collect(),
    })
}

pub(super) async fn create(
    svc: &IntelligenceGrpc,
    request: CreateDecisionRequest,
) -> Result<Decision, Status> {
    let title = require("title", &request.title)?;
    let scope = match request.scope() {
        DecisionScope::Unspecified => DecisionScope::Project,
        scope => scope,
    };
    if scope == DecisionScope::Project && request.project_id.trim().is_empty() {
        return Err(Status::invalid_argument(
            "project_id is required for a project-scoped decision",
        ));
    }
    let tags: Vec<String> = request
        .tags
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let row = DecisionRow {
        id: Uuid::now_v7(),
        project_id: request.project_id.trim().to_owned(),
        scope: convert::scope_to_str(scope),
        status: "active".to_owned(),
        source: convert::source_to_str(DecisionSource::Manual),
        title,
        statement: request.statement.trim().to_owned(),
        reasons: request
            .reasons
            .iter()
            .map(|r| r.trim().to_owned())
            .filter(|r| !r.is_empty())
            .collect(),
        approved_by: request.approved_by.trim().to_owned(),
        proposal_id: String::new(),
        tags,
        created_at: Utc::now(),
        superseded_at: None,
        superseded_by: String::new(),
    };
    repo::insert(&svc.pool, &row).await.map_err(db_status)?;
    index_decision(svc, &row).await;
    emit_decision(svc, &row, "created");
    Ok(convert::decision(&row))
}

pub(super) async fn supersede(
    svc: &IntelligenceGrpc,
    request: SupersedeDecisionRequest,
) -> Result<Decision, Status> {
    let id = parse_id("id", &request.id)?;
    let row = repo::get(&svc.pool, id).await.map_err(db_status)?;
    if row.status != "active" {
        return Err(Status::failed_precondition(
            "decision is already superseded",
        ));
    }
    let by = request.superseded_by.trim();
    if !by.is_empty() {
        let by_id = parse_id("superseded_by", by)?;
        if by_id == id {
            return Err(Status::invalid_argument(
                "a decision cannot supersede itself",
            ));
        }
        repo::get(&svc.pool, by_id).await.map_err(db_status)?;
    }
    repo::supersede(&svc.pool, id, by)
        .await
        .map_err(db_status)?;
    let row = repo::get(&svc.pool, id).await.map_err(db_status)?;
    emit_decision(svc, &row, "updated");
    Ok(convert::decision(&row))
}
