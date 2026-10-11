use std::collections::HashMap;

use sdi_common::{grpc::db_status, principal::Role};
use sdi_proto::knowledge::v1::*;
use tonic::Status;

use super::{KnowledgeState, parse_id};
use crate::{
    auth::Caller,
    convert::{self, fact_category_to_db, fact_kind_to_db},
    db,
    domain::impact,
};

pub(super) async fn list_facts(
    state: &KnowledgeState,
    caller: &Caller,
    req: ListFactsRequest,
) -> Result<ListFactsResponse, Status> {
    let project_id = parse_id(&req.project_id, "project id")?;
    let kind = FactKind::try_from(req.kind)
        .ok()
        .filter(|k| *k != FactKind::Unspecified)
        .map(fact_kind_to_db);
    let category = FactCategory::try_from(req.category)
        .ok()
        .filter(|c| *c != FactCategory::Unspecified)
        .map(fact_category_to_db);
    let facts = db::facts::list(
        &state.pool,
        caller.workspace_id,
        project_id,
        kind.as_deref(),
        category.as_deref(),
    )
    .await
    .map_err(db_status)?;
    Ok(ListFactsResponse {
        facts: facts.into_iter().map(convert::fact).collect(),
    })
}

pub(super) async fn list_questions(
    state: &KnowledgeState,
    caller: &Caller,
    req: ListQuestionsRequest,
) -> Result<ListQuestionsResponse, Status> {
    let project_id = parse_id(&req.project_id, "project id")?;
    let questions =
        db::questions::list(&state.pool, caller.workspace_id, project_id, req.only_open)
            .await
            .map_err(db_status)?;
    Ok(ListQuestionsResponse {
        questions: questions.into_iter().map(convert::question).collect(),
    })
}

pub(super) async fn answer_question(
    state: &KnowledgeState,
    caller: &Caller,
    req: AnswerQuestionRequest,
) -> Result<Question, Status> {
    caller.require(Role::Member)?;
    let ws = caller.workspace_id;
    let id = parse_id(&req.id, "question id")?;
    let answer = req.answer.trim();
    if answer.is_empty() {
        return Err(Status::invalid_argument("answer is required"));
    }
    let question = db::questions::get(&state.pool, ws, id)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => Status::not_found("question not found"),
            other => db_status(other),
        })?;
    if !question.allow_free_text && !question.options.iter().any(|o| o == answer) {
        return Err(Status::invalid_argument(
            "answer must be one of the offered options",
        ));
    }
    let answered = db::questions::answer(&state.pool, ws, id, answer)
        .await
        .map_err(db_status)?;
    // `open_questions` on the project changed; list views refresh on this.
    if let Ok(project) = state
        .project(caller, &answered.project_id.to_string())
        .await
    {
        state
            .events
            .project(ws, project.id, "updated", &project.status);
    }
    Ok(convert::question(answered))
}

pub(super) async fn get_understanding(
    state: &KnowledgeState,
    caller: &Caller,
    req: GetUnderstandingRequest,
) -> Result<Understanding, Status> {
    let ws = caller.workspace_id;
    let project = state.project(caller, &req.project_id).await?;
    let maturity = db::maturity::get(&state.pool, ws, project.id)
        .await
        .map_err(db_status)?
        .map(convert::maturity);
    db::understanding::get(&state.pool, ws, project.id)
        .await
        .map_err(db_status)?
        .map(|row| convert::understanding(row, maturity))
        .ok_or_else(|| Status::not_found("understanding not generated yet; run a scan"))
}

/// The report the last successful scan stored; nothing is computed on read.
pub(super) async fn get_maturity(
    state: &KnowledgeState,
    caller: &Caller,
    req: GetMaturityRequest,
) -> Result<MaturityScore, Status> {
    let project = state.project(caller, &req.project_id).await?;
    db::maturity::get(&state.pool, caller.workspace_id, project.id)
        .await
        .map_err(db_status)?
        .map(convert::maturity)
        .ok_or_else(|| Status::not_found("maturity not computed yet; run a scan"))
}

pub(super) async fn get_graph(
    state: &KnowledgeState,
    caller: &Caller,
    req: GetGraphRequest,
) -> Result<Graph, Status> {
    let ws = caller.workspace_id;
    let project_id = parse_id(&req.project_id, "project id")?;
    let nodes = db::graph::nodes(&state.pool, ws, project_id)
        .await
        .map_err(db_status)?;
    let edges = db::graph::edges(&state.pool, ws, project_id, req.include_history)
        .await
        .map_err(db_status)?;
    Ok(Graph {
        nodes: nodes.into_iter().map(convert::node).collect(),
        edges: edges.into_iter().map(convert::edge).collect(),
    })
}

pub(super) async fn analyze_impact(
    state: &KnowledgeState,
    caller: &Caller,
    req: AnalyzeImpactRequest,
) -> Result<ImpactAnalysis, Status> {
    let ws = caller.workspace_id;
    let project_id = parse_id(&req.project_id, "project id")?;
    if req.node_id.trim().is_empty() && req.path.trim().is_empty() {
        return Err(Status::invalid_argument("node_id or path is required"));
    }
    let nodes = db::graph::nodes(&state.pool, ws, project_id)
        .await
        .map_err(db_status)?;
    let by_id: HashMap<String, _> = nodes.into_iter().map(|n| (n.id.to_string(), n)).collect();

    let origin_id = if !req.node_id.trim().is_empty() {
        let id = parse_id(&req.node_id, "node id")?.to_string();
        if !by_id.contains_key(&id) {
            return Err(Status::not_found("node not found in this project"));
        }
        id
    } else {
        let paths: Vec<(String, String)> = by_id
            .values()
            .map(|n| (n.id.to_string(), n.source_path.clone()))
            .collect();
        impact::origin_for_path(&paths, &req.path)
            .map(str::to_owned)
            .ok_or_else(|| Status::not_found("no graph node covers that path"))?
    };

    let edges: Vec<impact::Edge> = db::graph::edges(&state.pool, ws, project_id, false)
        .await
        .map_err(db_status)?
        .into_iter()
        .map(|e| impact::Edge {
            source: e.source_id.to_string(),
            target: e.target_id.to_string(),
            relation: e.relation,
            category: e.category,
        })
        .collect();
    let max_depth = if req.max_depth == 0 {
        impact::DEFAULT_MAX_DEPTH
    } else {
        req.max_depth.min(10)
    };
    let impacted = impact::analyze(&origin_id, &edges, max_depth);
    let kinds: HashMap<String, String> = by_id
        .iter()
        .map(|(id, n)| (id.clone(), n.kind.clone()))
        .collect();
    let origin = by_id[&origin_id].clone();
    let summary = impact::summary(&origin.label, &impacted, &kinds);

    Ok(ImpactAnalysis {
        origin: Some(convert::node(origin)),
        impacted: impacted
            .into_iter()
            .filter_map(|n| {
                by_id.get(&n.id).map(|row| ImpactedNode {
                    node: Some(convert::node(row.clone())),
                    distance: n.distance,
                    via: n.via,
                })
            })
            .collect(),
        summary,
    })
}

pub(super) async fn list_dependencies(
    state: &KnowledgeState,
    caller: &Caller,
    req: ListDependenciesRequest,
) -> Result<ListDependenciesResponse, Status> {
    let ws = caller.workspace_id;
    let project_id = parse_id(&req.project_id, "project id")?;
    let dependencies = db::dependencies::list(&state.pool, ws, project_id)
        .await
        .map_err(db_status)?;
    let by_ecosystem = db::dependencies::by_ecosystem(&state.pool, ws, project_id)
        .await
        .map_err(db_status)?;
    let dependabot = db::dependencies::get_dependabot(&state.pool, ws, project_id)
        .await
        .map_err(db_status)?;
    Ok(ListDependenciesResponse {
        dependencies: dependencies.into_iter().map(convert::dependency).collect(),
        by_ecosystem: by_ecosystem
            .into_iter()
            .map(|c| EcosystemCount {
                ecosystem: c.ecosystem,
                count: c.count.max(0) as u32,
            })
            .collect(),
        dependabot: Some(dependabot_status(dependabot)),
    })
}

pub(super) fn dependabot_status(row: Option<db::dependencies::DependabotRow>) -> DependabotStatus {
    row.map(|d| DependabotStatus {
        configured: d.configured,
        config_path: d.config_path,
        ecosystems: d.ecosystems,
        open_pull_requests: d.open_pull_requests.max(0) as u32,
    })
    .unwrap_or_default()
}
