use chrono::Utc;
use sdi_common::grpc::db_status;
use sdi_proto::intelligence::v1::{
    Document, DocumentKind, GenerateDocumentRequest, GetDocumentRequest, ListDocumentsRequest,
    ListDocumentsResponse,
};
use tonic::Status;
use uuid::Uuid;

use super::{IntelligenceGrpc, parse_id, require};
use crate::{
    convert,
    db::{
        documents::{self as repo, DocumentRow},
        proposals as proposals_repo,
    },
    domain::{documents, model::RankedOption},
};

const PIPELINE_RUNS: u32 = 50;

pub(super) async fn generate(
    svc: &IntelligenceGrpc,
    request: GenerateDocumentRequest,
) -> Result<Document, Status> {
    let project_id = require("project_id", &request.project_id)?;
    let kind = request.kind();
    if kind == DocumentKind::Unspecified {
        return Err(Status::invalid_argument("kind is required"));
    }

    let context = svc
        .knowledge
        .project_context(&project_id)
        .await
        .map_err(|e| e.into_status())?;
    let decisions: Vec<_> = crate::db::decisions::active_for_project(&svc.pool, &project_id)
        .await
        .map_err(db_status)?
        .iter()
        .map(convert::decision)
        .collect();

    // The accepted deployment wins; otherwise the newest open deployment proposal's recommendation.
    let proposals = proposals_repo::list(&svc.pool, &project_id)
        .await
        .map_err(db_status)?;
    let deployment = proposals.iter().filter(|p| p.kind == "deployment");
    let accepted = deployment.clone().find(|p| p.status == "accepted");
    let plan_option: Option<RankedOption> = accepted
        .and_then(|p| p.option(&p.accepted_option_id).cloned())
        .or_else(|| {
            deployment
                .clone()
                .find(|p| p.is_open())
                .and_then(|p| p.option(&p.recommended_option_id).cloned())
        });

    let pipeline_runs = match svc
        .knowledge
        .pipeline_runs(&project_id, PIPELINE_RUNS)
        .await
    {
        Ok(runs) => runs,
        Err(error) => {
            tracing::warn!(%error, "pipeline runs unavailable for document");
            Vec::new()
        }
    };
    let delivery = match svc.execution.delivery_stats(&project_id).await {
        Ok(stats) => Some(stats),
        Err(error) => {
            tracing::warn!(%error, "delivery stats unavailable for document");
            None
        }
    };
    let execution = match accepted
        .map(|p| p.execution_id.as_str())
        .filter(|id| !id.is_empty())
    {
        Some(id) => match svc.execution.get_execution(id).await {
            Ok(execution) => Some(execution),
            Err(error) => {
                tracing::warn!(%error, execution = id, "execution unavailable for document");
                None
            }
        },
        None => None,
    };

    let now = Utc::now();
    let generated = documents::generate(
        kind,
        &documents::DocumentInput {
            context: &context,
            decisions: &decisions,
            plan_option: plan_option.as_ref(),
            pipeline_runs: &pipeline_runs,
            delivery: delivery.as_ref(),
            execution: execution.as_ref(),
            now,
        },
    )
    .ok_or_else(|| Status::invalid_argument("kind is required"))?;

    let row = DocumentRow {
        id: Uuid::now_v7(),
        project_id,
        kind: convert::document_kind_to_str(kind),
        title: generated.title,
        markdown: generated.markdown,
        commit_sha: context
            .understanding
            .as_ref()
            .map(|u| u.commit_sha.clone())
            .unwrap_or_default(),
        facts_used: generated.facts_used as i32,
        generated_at: now,
    };
    repo::insert(&svc.pool, &row).await.map_err(db_status)?;
    Ok(convert::document(&row))
}

pub(super) async fn list(
    svc: &IntelligenceGrpc,
    request: ListDocumentsRequest,
) -> Result<ListDocumentsResponse, Status> {
    let project_id = require("project_id", &request.project_id)?;
    let rows = repo::list(&svc.pool, &project_id)
        .await
        .map_err(db_status)?;
    Ok(ListDocumentsResponse {
        documents: rows.iter().map(convert::document).collect(),
    })
}

pub(super) async fn get(
    svc: &IntelligenceGrpc,
    request: GetDocumentRequest,
) -> Result<Document, Status> {
    let id = parse_id("id", &request.id)?;
    let row = repo::get(&svc.pool, id).await.map_err(db_status)?;
    Ok(convert::document(&row))
}
