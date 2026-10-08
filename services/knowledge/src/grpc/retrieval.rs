use std::collections::HashMap;

use sdi_common::{embed::embed, grpc::db_status};
use sdi_proto::knowledge::v1::*;
use tonic::Status;

use super::{KnowledgeState, limit, opt_id};
use crate::{
    convert::{self, knowledge_kind_from_db, knowledge_kind_to_db},
    db::{self, chunks::NewChunk},
    domain::similarity,
};

pub(super) async fn search_knowledge(
    state: &KnowledgeState,
    req: SearchKnowledgeRequest,
) -> Result<SearchKnowledgeResponse, Status> {
    let query = req.query.trim();
    if query.is_empty() {
        return Err(Status::invalid_argument("query is required"));
    }
    let project_id = opt_id(&req.project_id, "project id")?;
    let kinds: Vec<String> = req
        .kinds
        .iter()
        .filter_map(|k| KnowledgeKind::try_from(*k).ok())
        .filter(|k| *k != KnowledgeKind::Unspecified)
        .map(knowledge_kind_to_db)
        .collect();
    let hits = db::chunks::search(
        &state.pool,
        &embed(query),
        project_id,
        &kinds,
        limit(req.limit, 20, 100),
    )
    .await
    .map_err(db_status)?;
    Ok(SearchKnowledgeResponse {
        hits: hits
            .into_iter()
            .map(|h| KnowledgeHit {
                id: h.id.to_string(),
                kind: knowledge_kind_from_db(&h.kind),
                project_id: h.project_id.map(|id| id.to_string()).unwrap_or_default(),
                project_name: h.project_name,
                ref_id: h.ref_id,
                title: h.title,
                path: h.path,
                snippet: h.snippet,
                score: h.score.clamp(0.0, 1.0),
            })
            .collect(),
    })
}

pub(super) async fn index_knowledge(
    state: &KnowledgeState,
    req: IndexKnowledgeRequest,
) -> Result<IndexKnowledgeResponse, Status> {
    let kind = match KnowledgeKind::try_from(req.kind) {
        Ok(KnowledgeKind::Unspecified) | Err(_) => {
            return Err(Status::invalid_argument("kind is required"));
        }
        Ok(kind) => kind,
    };
    if req.content.trim().is_empty() {
        return Err(Status::invalid_argument("content is required"));
    }
    let project_id = opt_id(&req.project_id, "project id")?;
    if let Some(id) = project_id {
        state.project(&id.to_string()).await?;
    }
    let chunk = NewChunk {
        kind: knowledge_kind_to_db(kind),
        project_id,
        ref_id: req.ref_id,
        title: req.title,
        path: req.path,
        embedding: embed(&req.content),
        content: req.content,
    };
    let id = db::chunks::insert(&state.pool, &chunk)
        .await
        .map_err(db_status)?;
    Ok(IndexKnowledgeResponse { id: id.to_string() })
}

pub(super) async fn list_similar_projects(
    state: &KnowledgeState,
    req: ListSimilarProjectsRequest,
) -> Result<ListSimilarProjectsResponse, Status> {
    let project = state.project(&req.project_id).await?;
    let own = db::profiles::get(&state.pool, project.id)
        .await
        .map_err(db_status)?
        .ok_or_else(|| Status::failed_precondition("project has not been scanned yet"))?
        .to_profile();

    let projects: HashMap<_, _> = db::projects::list(&state.pool)
        .await
        .map_err(db_status)?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let mut similar: Vec<SimilarProject> = db::profiles::list(&state.pool)
        .await
        .map_err(db_status)?
        .into_iter()
        .filter(|row| row.project_id != project.id)
        .filter_map(|row| {
            let other = projects.get(&row.project_id)?.clone();
            let profile = row.to_profile();
            let b = similarity::compare(&own, &profile);
            Some(SimilarProject {
                project: Some(convert::project(other)),
                similarity: Some(SimilarityBreakdown {
                    architecture: b.architecture,
                    iac: b.iac,
                    deployment: b.deployment,
                    cloud: b.cloud,
                    database: b.database,
                    overall: b.overall,
                }),
                shared_technologies: similarity::shared_technologies(
                    &own.technologies,
                    &profile.technologies,
                ),
            })
        })
        .collect();
    similar.sort_by(|a, b| {
        let overall = |s: &SimilarProject| s.similarity.as_ref().map_or(0.0, |x| x.overall);
        overall(b)
            .partial_cmp(&overall(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    similar.truncate(limit(req.limit, 5, 50) as usize);
    Ok(ListSimilarProjectsResponse { projects: similar })
}
