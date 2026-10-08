use chrono::Utc;
use sdi_common::grpc::db_status;
use sdi_proto::{
    execution::v1::CreateExecutionRequest,
    intelligence::v1::{
        AcceptProposalRequest, DecisionScope, DecisionSource, GenerateProposalRequest,
        GetProposalRequest, ListProposalsRequest, ListProposalsResponse, Proposal, ProposalKind,
        RejectProposalRequest,
    },
    knowledge::v1::{IndexKnowledgeRequest, KnowledgeKind, ProjectContext},
};
use sqlx::types::Json;
use tonic::Status;
use uuid::Uuid;

use super::{IntelligenceGrpc, parse_id, require};
use crate::{
    convert,
    db::{
        decisions::DecisionRow,
        messages::MessageRow,
        proposals::{self as repo, ProposalRow},
    },
    domain::{
        model::{ConstraintData, RankedOption, Weights},
        narrative, ranking,
    },
};

const SIMILAR_PROJECTS: u32 = 5;

/// Context and candidates from knowledge, decisions from our own schema, then
/// the pure ranker. Similar projects are evidence only, so their failure is
/// logged and ignored.
pub(super) async fn rank_for(
    svc: &IntelligenceGrpc,
    project_id: &str,
    kind: ProposalKind,
    constraints: &[ConstraintData],
    weights: &Weights,
) -> Result<(ProjectContext, Vec<RankedOption>), Status> {
    let category =
        ranking::category_for(kind).ok_or_else(|| Status::invalid_argument("kind is required"))?;
    let context = svc
        .knowledge
        .project_context(project_id)
        .await
        .map_err(|e| e.into_status())?;
    let patterns = svc
        .knowledge
        .list_patterns(category)
        .await
        .map_err(|e| e.into_status())?;
    let decisions: Vec<_> = crate::db::decisions::active_for_project(&svc.pool, project_id)
        .await
        .map_err(db_status)?
        .iter()
        .map(convert::decision)
        .collect();
    let similar = match svc
        .knowledge
        .similar_projects(project_id, SIMILAR_PROJECTS)
        .await
    {
        Ok(similar) => similar,
        Err(error) => {
            tracing::warn!(%error, "similar projects unavailable; ranking without them");
            Vec::new()
        }
    };
    let options = ranking::rank(&ranking::RankingInput {
        context: &context,
        patterns: &patterns,
        decisions: &ranking::applicable_decisions(&decisions, project_id),
        similar: &similar,
        constraints,
        weights,
        now: Utc::now(),
    });
    Ok((context, options))
}

pub(super) async fn load(svc: &IntelligenceGrpc, id: &str) -> Result<ProposalRow, Status> {
    let id = parse_id("id", id)?;
    repo::get(&svc.pool, id).await.map_err(db_status)
}

pub(super) async fn generate(
    svc: &IntelligenceGrpc,
    request: GenerateProposalRequest,
) -> Result<Proposal, Status> {
    let project_id = require("project_id", &request.project_id)?;
    let kind = request.kind();
    if kind == ProposalKind::Unspecified {
        return Err(Status::invalid_argument("kind is required"));
    }

    let weights = Weights::default();
    let (context, options) = rank_for(svc, &project_id, kind, &[], &weights).await?;
    let project_name = context
        .project
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| project_id.clone());
    let (title, summary) = narrative::describe_proposal(
        &convert::kind_to_str(kind).replace('_', "/"),
        &project_name,
        &options,
    );

    let now = Utc::now();
    let row = ProposalRow {
        id: Uuid::now_v7(),
        project_id: project_id.clone(),
        kind: convert::kind_to_str(kind),
        status: "proposed".to_owned(),
        title,
        summary,
        prompt: request.prompt.trim().to_owned(),
        recommended_option_id: options.first().map(|o| o.id.clone()).unwrap_or_default(),
        accepted_option_id: String::new(),
        execution_id: String::new(),
        options: Json(options),
        constraints: Json(Vec::new()),
        weights: Json(weights),
        revision: 1,
        reasoner: svc.reasoner.name().to_owned(),
        created_at: now,
        updated_at: now,
    };
    repo::insert(&svc.pool, &row).await.map_err(db_status)?;
    repo::supersede_open(&svc.pool, &project_id, &row.kind, row.id)
        .await
        .map_err(db_status)?;

    let opening = MessageRow::new(
        row.id,
        "system",
        narrative::opening_message(&row.options.0, &row.weights.0),
        false,
        row.options
            .0
            .first()
            .map(|o| o.evidence.clone())
            .unwrap_or_default(),
    );
    crate::db::messages::insert(&svc.pool, &opening)
        .await
        .map_err(db_status)?;
    if !row.prompt.is_empty() {
        let prompt = MessageRow::new(row.id, "user", row.prompt.clone(), false, Vec::new());
        crate::db::messages::insert(&svc.pool, &prompt)
            .await
            .map_err(db_status)?;
    }

    Ok(convert::proposal(&row))
}

pub(super) async fn list(
    svc: &IntelligenceGrpc,
    request: ListProposalsRequest,
) -> Result<ListProposalsResponse, Status> {
    let rows = repo::list(&svc.pool, request.project_id.trim())
        .await
        .map_err(db_status)?;
    Ok(ListProposalsResponse {
        proposals: rows.iter().map(convert::proposal).collect(),
    })
}

pub(super) async fn get(
    svc: &IntelligenceGrpc,
    request: GetProposalRequest,
) -> Result<Proposal, Status> {
    load(svc, &request.id)
        .await
        .map(|row| convert::proposal(&row))
}

pub(super) async fn accept(
    svc: &IntelligenceGrpc,
    request: AcceptProposalRequest,
) -> Result<Proposal, Status> {
    let row = load(svc, &request.id).await?;
    if !row.is_open() {
        return Err(Status::failed_precondition(format!(
            "proposal is {}",
            row.status
        )));
    }
    let option_id = if request.option_id.trim().is_empty() {
        row.recommended_option_id.clone()
    } else {
        request.option_id.trim().to_owned()
    };
    let option = row.option(&option_id).cloned().ok_or_else(|| {
        Status::invalid_argument(format!("option {option_id} is not part of this proposal"))
    })?;

    let execution_id = if request.create_execution {
        let execution = svc
            .execution
            .create_execution(CreateExecutionRequest {
                project_id: row.project_id.clone(),
                proposal_id: row.id.to_string(),
                pattern_id: option.pattern_id.clone(),
                title: option.title.clone(),
                steps: option.plan.iter().map(convert::plan_step).collect(),
            })
            .await
            .map_err(|e| e.into_status())?;
        execution.id
    } else {
        String::new()
    };

    let decision = DecisionRow {
        id: Uuid::now_v7(),
        project_id: row.project_id.clone(),
        scope: convert::scope_to_str(DecisionScope::Project),
        status: "active".to_owned(),
        source: convert::source_to_str(DecisionSource::ProposalAccepted),
        title: format!("Adopt {}", option.title),
        statement: format!(
            "Accepted option {} of proposal {} ({}).",
            option.title, row.title, option.summary
        ),
        reasons: option
            .evidence
            .iter()
            .map(|e| format!("{}: {}", e.label, e.detail))
            .chain(option.pros.iter().cloned())
            .collect(),
        approved_by: String::new(),
        proposal_id: row.id.to_string(),
        tags: option
            .tags
            .iter()
            .cloned()
            .chain(std::iter::once(format!("pattern:{}", option.pattern_id)))
            .collect(),
        created_at: Utc::now(),
        superseded_at: None,
        superseded_by: String::new(),
    };
    crate::db::decisions::insert(&svc.pool, &decision)
        .await
        .map_err(db_status)?;
    index_decision(svc, &decision).await;

    repo::accept(&svc.pool, row.id, &option_id, &execution_id)
        .await
        .map_err(db_status)?;
    let note = MessageRow::new(
        row.id,
        "system",
        if execution_id.is_empty() {
            format!(
                "Accepted {}; decision {} recorded.",
                option.title, decision.id
            )
        } else {
            format!(
                "Accepted {}; decision {} recorded; execution {execution_id} created.",
                option.title, decision.id
            )
        },
        false,
        option.evidence.clone(),
    );
    crate::db::messages::insert(&svc.pool, &note)
        .await
        .map_err(db_status)?;

    load(svc, &request.id)
        .await
        .map(|row| convert::proposal(&row))
}

pub(super) async fn reject(
    svc: &IntelligenceGrpc,
    request: RejectProposalRequest,
) -> Result<Proposal, Status> {
    let row = load(svc, &request.id).await?;
    if !row.is_open() {
        return Err(Status::failed_precondition(format!(
            "proposal is {}",
            row.status
        )));
    }
    repo::set_status(&svc.pool, row.id, "rejected")
        .await
        .map_err(db_status)?;
    let reason = request.reason.trim();
    let note = MessageRow::new(
        row.id,
        "system",
        if reason.is_empty() {
            "Rejected.".to_owned()
        } else {
            format!("Rejected: {reason}")
        },
        false,
        Vec::new(),
    );
    crate::db::messages::insert(&svc.pool, &note)
        .await
        .map_err(db_status)?;
    load(svc, &request.id)
        .await
        .map(|row| convert::proposal(&row))
}

/// Decisions become semantic memory; losing the index entry is not fatal.
pub(super) async fn index_decision(svc: &IntelligenceGrpc, decision: &DecisionRow) {
    let content = format!(
        "{}\n{}\nReasons: {}\nTags: {}",
        decision.title,
        decision.statement,
        decision.reasons.join("; "),
        decision.tags.join(", ")
    );
    if let Err(error) = svc
        .knowledge
        .index_knowledge(IndexKnowledgeRequest {
            kind: KnowledgeKind::Decision as i32,
            project_id: decision.project_id.clone(),
            ref_id: decision.id.to_string(),
            title: decision.title.clone(),
            content,
            path: String::new(),
        })
        .await
    {
        tracing::warn!(%error, decision = %decision.id, "could not index decision");
    }
}
