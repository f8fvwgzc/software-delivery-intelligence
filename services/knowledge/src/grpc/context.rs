use sdi_common::grpc::db_status;
use sdi_proto::knowledge::v1::*;
use tonic::Status;

use super::{KnowledgeState, delivery, knowledge};
use crate::{convert, db};

/// Everything the proposal engine needs, in one round trip. Parts that a
/// scan has not produced yet are simply absent.
pub(super) async fn get_project_context(
    state: &KnowledgeState,
    req: GetProjectContextRequest,
) -> Result<ProjectContext, Status> {
    let project = state.project(&req.project_id).await?;
    let id = project.id;
    let pool = &state.pool;

    let (understanding, facts, questions, nodes, edges, dependabot, profile) = tokio::try_join!(
        db::understanding::get(pool, id),
        db::facts::list(pool, id, None, None),
        db::questions::list(pool, id, false),
        db::graph::nodes(pool, id),
        db::graph::edges(pool, id, false),
        db::dependencies::get_dependabot(pool, id),
        db::profiles::get(pool, id),
    )
    .map_err(db_status)?;
    let pipeline_stats = delivery::pipeline_stats(pool, id).await?;
    let deployment_stats = delivery::deployment_stats(pool, id).await?;

    let (technologies, capabilities, profile_text) = profile
        .map(|p| (p.technologies, p.capabilities, p.profile_text))
        .unwrap_or_default();

    Ok(ProjectContext {
        project: Some(convert::project(project)),
        understanding: understanding.map(convert::understanding),
        facts: facts.into_iter().map(convert::fact).collect(),
        questions: questions.into_iter().map(convert::question).collect(),
        graph: Some(Graph {
            nodes: nodes.into_iter().map(convert::node).collect(),
            edges: edges.into_iter().map(convert::edge).collect(),
        }),
        dependabot: Some(knowledge::dependabot_status(dependabot)),
        pipeline_stats: Some(pipeline_stats),
        deployment_stats: Some(deployment_stats),
        technologies,
        capabilities,
        profile_text,
    })
}

pub(super) async fn get_knowledge_stats(
    state: &KnowledgeState,
    _req: GetKnowledgeStatsRequest,
) -> Result<KnowledgeStats, Status> {
    let c = db::stats::counts(&state.pool).await.map_err(db_status)?;
    let n = |v: i64| v.max(0) as u32;
    Ok(KnowledgeStats {
        projects: n(c.projects),
        facts: n(c.facts),
        nodes: n(c.nodes),
        edges: n(c.edges),
        patterns: n(c.patterns),
        chunks: n(c.chunks),
        open_questions: n(c.open_questions),
    })
}
