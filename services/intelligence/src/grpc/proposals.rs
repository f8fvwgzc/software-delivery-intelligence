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
        model::{self, ConstraintData, RankedOption, Weights},
        narrative, plans, ranking,
    },
    generators,
};

const SIMILAR_PROJECTS: u32 = 5;

/// Broadcasts a proposal change with its status and revision.
pub(super) fn emit_proposal(svc: &IntelligenceGrpc, row: &ProposalRow, action: &str) {
    svc.events.emit(
        "proposal",
        &row.id.to_string(),
        &row.project_id,
        action,
        &row.status,
        &[
            ("revision", row.revision.to_string()),
            ("kind", row.kind.clone()),
            ("recommended_option_id", row.recommended_option_id.clone()),
        ],
    );
}

pub(super) fn emit_decision(svc: &IntelligenceGrpc, row: &DecisionRow, action: &str) {
    svc.events.emit(
        "decision",
        &row.id.to_string(),
        &row.project_id,
        action,
        &row.status,
        &[("scope", row.scope.clone()), ("source", row.source.clone())],
    );
}

/// Context and candidates from knowledge, decisions from our own schema, then
/// the pure ranker. Similar projects are evidence only, so their failure is
/// logged and ignored.
pub(super) async fn rank_for(
    svc: &IntelligenceGrpc,
    project_id: &str,
    kind: ProposalKind,
    constraints: &[ConstraintData],
    weights: &Weights,
    cost_ceiling: f64,
) -> Result<(ProjectContext, Vec<RankedOption>), Status> {
    let context = svc
        .knowledge
        .project_context(project_id)
        .await
        .map_err(|e| e.into_status())?;
    let options = rank_with(svc, &context, kind, constraints, weights, cost_ceiling).await?;
    Ok((context, options))
}

/// `rank_for` with a context the caller already holds.
pub(super) async fn rank_with(
    svc: &IntelligenceGrpc,
    context: &ProjectContext,
    kind: ProposalKind,
    constraints: &[ConstraintData],
    weights: &Weights,
    cost_ceiling: f64,
) -> Result<Vec<RankedOption>, Status> {
    let category =
        ranking::category_for(kind).ok_or_else(|| Status::invalid_argument("kind is required"))?;
    let project_id = context
        .project
        .as_ref()
        .map(|p| p.id.as_str())
        .unwrap_or_default();
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
    Ok(ranking::rank(&ranking::RankingInput {
        context,
        patterns: &patterns,
        decisions: &ranking::applicable_decisions(&decisions, project_id),
        similar: &similar,
        constraints,
        weights,
        now: Utc::now(),
        kind,
        cost_ceiling,
    }))
}

/// The budget constraint of a new proposal: the request wins, then `.sdi.yml`.
fn initial_constraints(request_budget: f64, context: &ProjectContext) -> Vec<ConstraintData> {
    let configured = generators::input::usable_config(context)
        .map(|c| c.budget_monthly_usd)
        .filter(|b| *b > 0.0);
    let budget = if request_budget > 0.0 {
        Some(request_budget)
    } else {
        configured
    };
    budget.map(ConstraintData::budget).into_iter().collect()
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

    if request.budget_monthly_usd < 0.0 || !request.budget_monthly_usd.is_finite() {
        return Err(Status::invalid_argument(
            "budget_monthly_usd must be positive",
        ));
    }
    let settings = svc.settings().await?;
    let weights = settings.default_weights.0.clone();
    // The budget can only be known once the context (with `.sdi.yml`) is
    // loaded, so a budgeted proposal ranks twice: the first pass is cheap.
    let (context, options) = rank_for(
        svc,
        &project_id,
        kind,
        &[],
        &weights,
        settings.cost_ceiling_usd,
    )
    .await?;
    let constraints = initial_constraints(request.budget_monthly_usd, &context);
    let options = if constraints.is_empty() {
        options
    } else {
        rank_for(
            svc,
            &project_id,
            kind,
            &constraints,
            &weights,
            settings.cost_ceiling_usd,
        )
        .await?
        .1
    };
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
        constraints: Json(constraints),
        weights: Json(weights),
        revision: 1,
        reasoner: svc.reasoner_for(&settings).name().to_owned(),
        created_at: now,
        updated_at: now,
    };
    repo::insert(&svc.pool, &row).await.map_err(db_status)?;
    let superseded = repo::supersede_open(&svc.pool, &project_id, &row.kind, row.id)
        .await
        .map_err(db_status)?;
    emit_proposal(svc, &row, "created");
    for id in superseded {
        svc.events.emit(
            "proposal",
            &id.to_string(),
            &project_id,
            "updated",
            "superseded",
            &[("superseded_by", row.id.to_string())],
        );
    }

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
    let adapter = match request.adapter.trim().to_lowercase().as_str() {
        "" => "simulated".to_owned(),
        a if plans::ADAPTERS.contains(&a) => a.to_owned(),
        other => {
            return Err(Status::invalid_argument(format!(
                "adapter {other:?} is not one of simulated, docker, github"
            )));
        }
    };

    let execution_id = if request.create_execution {
        let (title, steps, artifacts) = match adapter.as_str() {
            "docker" => {
                let context = svc
                    .knowledge
                    .project_context(&row.project_id)
                    .await
                    .map_err(|e| e.into_status())?;
                let input =
                    generators::input::build(&context, &model::params_of(&row.constraints.0), "");
                // The generated files ride along so the checkout is overlaid
                // with them: a containerize option verifies its own Dockerfile.
                (
                    format!("Verify {} locally", option.title),
                    plans::verification_plan(&context, &input),
                    option.artifacts.clone(),
                )
            }
            "github" => {
                if option.artifacts.is_empty() {
                    return Err(Status::failed_precondition(
                        "this option generated no files to open a pull request with",
                    ));
                }
                let context = svc
                    .knowledge
                    .project_context(&row.project_id)
                    .await
                    .map_err(|e| e.into_status())?;
                (
                    format!("Pull request: {}", option.title),
                    plans::pull_request_plan(&context, &option, &row.id.to_string()),
                    option.artifacts.clone(),
                )
            }
            _ => (option.title.clone(), option.plan.clone(), Vec::new()),
        };
        let execution = svc
            .execution
            .create_execution(CreateExecutionRequest {
                project_id: row.project_id.clone(),
                proposal_id: row.id.to_string(),
                pattern_id: option.pattern_id.clone(),
                title,
                steps: steps.iter().map(convert::plan_step).collect(),
                adapter: adapter.clone(),
                artifacts: plans::artifact_map(&artifacts),
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
        // Two decisions with the same title and tag yield the same evidence
        // sentence twice; a reason list reads once.
        reasons: {
            let mut seen = std::collections::BTreeSet::new();
            option
                .evidence
                .iter()
                .map(|e| format!("{}: {}", e.label, e.detail))
                .chain(option.pros.iter().cloned())
                .filter(|reason| seen.insert(reason.clone()))
                .collect()
        },
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
    emit_decision(svc, &decision, "created");

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
                "Accepted {}; decision {} recorded; execution {execution_id} created ({adapter} adapter).",
                option.title, decision.id
            )
        },
        false,
        option.evidence.clone(),
    );
    crate::db::messages::insert(&svc.pool, &note)
        .await
        .map_err(db_status)?;

    let row = load(svc, &request.id).await?;
    emit_proposal(svc, &row, "updated");
    Ok(convert::proposal(&row))
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
    let row = load(svc, &request.id).await?;
    emit_proposal(svc, &row, "updated");
    Ok(convert::proposal(&row))
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
