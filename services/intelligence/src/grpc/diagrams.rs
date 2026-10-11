//! Diagram RPCs (CONTRACT.md §16): generation from the knowledge graph,
//! tree and key files; user edits of title, spec or Mermaid; deletion.

use std::collections::BTreeMap;

use chrono::Utc;
use sdi_common::grpc::db_status;
use sdi_proto::intelligence::v1::{
    DeleteDiagramRequest, DeleteDiagramResponse, Diagram, DiagramKind, GenerateDiagramRequest,
    GetDiagramRequest, ListDiagramsRequest, ListDiagramsResponse, UpdateDiagramRequest,
};
use sqlx::types::Json;
use tonic::Status;
use uuid::Uuid;

use super::{IntelligenceGrpc, parse_id, require};
use crate::{
    convert,
    db::diagrams::{self as repo, DiagramRow},
    domain::diagrams::{
        self, DiagramInput, DiagramSpec, Generated, MAX_FILE_BYTES, MAX_KEY_FILES, follow_imports,
        key_files, mermaid,
    },
    reasoner,
};

pub(super) async fn generate(
    svc: &IntelligenceGrpc,
    request: GenerateDiagramRequest,
) -> Result<Diagram, Status> {
    let project_id = require("project_id", &request.project_id)?;
    let kind = request.kind();
    if kind == DiagramKind::Unspecified {
        return Err(Status::invalid_argument("kind is required"));
    }
    let prompt = request.prompt.trim().to_owned();

    let mut context = svc
        .knowledge
        .project_context(&project_id)
        .await
        .map_err(|e| e.into_status())?;
    if context.graph.is_none() {
        // Older knowledge builds leave the graph out of the context.
        context.graph = svc.knowledge.graph(&project_id, false).await.ok();
    }
    let commit_sha = context
        .understanding
        .as_ref()
        .map(|u| u.commit_sha.clone())
        .unwrap_or_default();
    let now = Utc::now();

    let (generated, mermaid, source, reasoner_name) = if kind == DiagramKind::Custom
        && looks_like_mermaid(&prompt)
    {
        // Imported Mermaid: the source is authoritative, the spec stays empty.
        mermaid::check(&prompt).map_err(|e| Status::invalid_argument(format!("mermaid: {e}")))?;
        let name = context
            .project
            .as_ref()
            .map(|p| p.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "project".to_owned());
        let generated = Generated {
            title: format!("{name} imported diagram"),
            explanation: imported_explanation(&name, &prompt),
            spec: DiagramSpec::default(),
        };
        (generated, prompt, "mermaid", "heuristic")
    } else {
        let tree = match svc.knowledge.repo_tree(&project_id).await {
            Ok(tree) => tree.entries,
            Err(error) => {
                tracing::warn!(%error, "repository tree unavailable; building the diagram from the graph alone");
                Vec::new()
            }
        };
        let files = read_key_files(svc, &project_id, kind, &tree, &context, &prompt).await;
        let input = DiagramInput {
            context: &context,
            tree: &tree,
            files: &files,
            prompt: &prompt,
        };
        let mut generated = diagrams::generate(kind, &input)
            .ok_or_else(|| Status::invalid_argument("kind is required"))?;
        let settings = svc.settings().await?;
        let reasoner = svc.reasoner_for(&settings);
        let refined = reasoner::diagrams::refine(reasoner.as_ref(), kind, &mut generated).await;
        let mermaid = mermaid::render_kind(kind, &generated.spec, &project_id);
        (
            generated,
            mermaid,
            "generated",
            if refined { "claude" } else { "heuristic" },
        )
    };

    let row = DiagramRow {
        id: Uuid::now_v7(),
        project_id,
        kind: convert::diagram_kind_to_str(kind),
        title: generated.title,
        explanation: generated.explanation,
        spec: Json(generated.spec),
        mermaid,
        source: source.to_owned(),
        commit_sha,
        revision: 1,
        reasoner: reasoner_name.to_owned(),
        created_at: now,
        updated_at: now,
    };
    repo::insert(&svc.pool, &row).await.map_err(db_status)?;
    svc.events.emit(
        "diagram",
        &row.id.to_string(),
        &row.project_id,
        "created",
        "",
        &[("kind", row.kind.clone()), ("title", row.title.clone())],
    );
    Ok(convert::diagram(&row))
}

/// The key files for a kind, fetched concurrently: first the structural
/// sample (`key_files`), then, for the kinds that read the import graph,
/// the files that sample imports most (`follow_imports`) up to
/// `MAX_KEY_FILES`. Unreadable or binary files are skipped and every
/// excerpt is cut at `MAX_FILE_BYTES`.
async fn read_key_files(
    svc: &IntelligenceGrpc,
    project_id: &str,
    kind: DiagramKind,
    tree: &[sdi_proto::knowledge::v1::RepoTreeEntry],
    context: &sdi_proto::knowledge::v1::ProjectContext,
    prompt: &str,
) -> BTreeMap<String, String> {
    let mut files = fetch_files(svc, project_id, key_files(kind, tree)).await;
    let follows = matches!(
        kind,
        DiagramKind::Architecture
            | DiagramKind::Custom
            | DiagramKind::DataFlow
            | DiagramKind::Sequence
            | DiagramKind::HighLevel
    );
    if follows && files.len() < MAX_KEY_FILES {
        let more = {
            let input = DiagramInput {
                context,
                tree,
                files: &files,
                prompt,
            };
            follow_imports(&input, MAX_KEY_FILES - files.len())
        };
        files.extend(fetch_files(svc, project_id, more).await);
    }
    tracing::debug!(
        kind = ?kind,
        count = files.len(),
        paths = ?files.keys().collect::<Vec<_>>(),
        "key files sampled"
    );
    files
}

async fn fetch_files(
    svc: &IntelligenceGrpc,
    project_id: &str,
    paths: Vec<String>,
) -> BTreeMap<String, String> {
    let fetched = futures::future::join_all(
        paths
            .iter()
            .map(|path| svc.knowledge.repo_file(project_id, path)),
    )
    .await;
    let mut files = BTreeMap::new();
    for (path, result) in paths.into_iter().zip(fetched) {
        match result {
            Ok(file) if !file.binary && !file.content.is_empty() => {
                files.insert(path, truncate(file.content));
            }
            Ok(_) => {}
            Err(error) => tracing::debug!(%error, path, "key file unavailable"),
        }
    }
    files
}

fn truncate(mut content: String) -> String {
    if content.len() > MAX_FILE_BYTES {
        let mut cut = MAX_FILE_BYTES;
        while !content.is_char_boundary(cut) {
            cut -= 1;
        }
        content.truncate(cut);
    }
    content
}

pub(crate) fn looks_like_mermaid(text: &str) -> bool {
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("%%") && *l != "---");
    first.is_some_and(|l| {
        l.starts_with("flowchart")
            || l.starts_with("graph ")
            || l.starts_with("sequenceDiagram")
            || l.starts_with("classDiagram")
            || l.starts_with("stateDiagram")
            || l.starts_with("erDiagram")
            || l.starts_with("gantt")
            || l.starts_with("mindmap")
            || l.starts_with("journey")
            || l.starts_with("gitGraph")
            || l.starts_with("timeline")
            || l.starts_with("pie")
            || l.starts_with("C4")
    })
}

fn imported_explanation(name: &str, source: &str) -> String {
    let lines = source.lines().filter(|l| !l.trim().is_empty()).count();
    let header = source
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("%%") && *l != "---")
        .unwrap_or("diagram");
    format!(
        "This custom diagram of {name} was imported from Mermaid source rather than generated from the knowledge graph, \
         so the platform has not verified it against the repository: nodes do not link to the code browser, edges carry no \
         evidence files and nothing on it is checked against the scanned commit. The source is a {} diagram of {lines} \
         non-empty lines and is authoritative: the Mermaid tab renders it as written and the editorial canvas has no spec \
         to draw. Edit the Mermaid to change it, or generate an architecture, data-flow or deployment diagram to get a map \
         whose components cite the files that implement them and whose inferred relationships are shown dashed.",
        header.split_whitespace().next().unwrap_or("Mermaid")
    )
}

pub(super) async fn list(
    svc: &IntelligenceGrpc,
    request: ListDiagramsRequest,
) -> Result<ListDiagramsResponse, Status> {
    let project_id = require("project_id", &request.project_id)?;
    let rows = repo::list(&svc.pool, &project_id)
        .await
        .map_err(db_status)?;
    Ok(ListDiagramsResponse {
        diagrams: rows
            .iter()
            .map(|row| {
                let mut diagram = convert::diagram(row);
                // Omitted in list responses (proto contract).
                diagram.spec = None;
                diagram.mermaid.clear();
                diagram
            })
            .collect(),
    })
}

pub(super) async fn get(
    svc: &IntelligenceGrpc,
    request: GetDiagramRequest,
) -> Result<Diagram, Status> {
    let id = parse_id("id", &request.id)?;
    let row = repo::get(&svc.pool, id).await.map_err(db_status)?;
    Ok(convert::diagram(&row))
}

/// Title, spec or Mermaid: a spec edit regenerates the Mermaid
/// (`source = "edited"`); a Mermaid edit makes the source authoritative and
/// keeps the spec (`source = "mermaid"`). Every edit bumps the revision.
pub(super) async fn update(
    svc: &IntelligenceGrpc,
    request: UpdateDiagramRequest,
) -> Result<Diagram, Status> {
    let id = parse_id("id", &request.id)?;
    let title = request.title.trim();
    let mermaid_source = request.mermaid.trim();
    if title.is_empty() && request.spec.is_none() && mermaid_source.is_empty() {
        return Err(Status::invalid_argument(
            "nothing to update: set title, spec or mermaid",
        ));
    }
    let mut row = repo::get(&svc.pool, id).await.map_err(db_status)?;
    if !title.is_empty() {
        row.title = title.to_owned();
    }
    if let Some(spec) = &request.spec {
        let spec = convert::diagram_spec_from_proto(spec);
        spec.validate()
            .map_err(|e| Status::invalid_argument(format!("spec: {e}")))?;
        if spec.nodes.len() > diagrams::model::MAX_NODES
            || spec.edges.len() > diagrams::model::MAX_EDGES
        {
            return Err(Status::invalid_argument(format!(
                "spec: at most {} nodes and {} edges",
                diagrams::model::MAX_NODES,
                diagrams::model::MAX_EDGES
            )));
        }
        row.mermaid = mermaid::render_kind(
            convert::diagram_kind_from_str(&row.kind),
            &spec,
            &row.project_id,
        );
        row.spec = Json(spec);
        row.source = "edited".to_owned();
    }
    if !mermaid_source.is_empty() {
        mermaid::check(mermaid_source)
            .map_err(|e| Status::invalid_argument(format!("mermaid: {e}")))?;
        row.mermaid = mermaid_source.to_owned();
        row.source = "mermaid".to_owned();
    }
    row.revision += 1;
    row.updated_at = Utc::now();
    repo::update(&svc.pool, &row).await.map_err(db_status)?;
    svc.events.emit(
        "diagram",
        &row.id.to_string(),
        &row.project_id,
        "updated",
        &row.source,
        &[
            ("kind", row.kind.clone()),
            ("title", row.title.clone()),
            ("revision", row.revision.to_string()),
        ],
    );
    Ok(convert::diagram(&row))
}

pub(super) async fn delete(
    svc: &IntelligenceGrpc,
    request: DeleteDiagramRequest,
) -> Result<DeleteDiagramResponse, Status> {
    let id = parse_id("id", &request.id)?;
    let row = repo::get(&svc.pool, id).await.map_err(db_status)?;
    if !repo::delete(&svc.pool, id).await.map_err(db_status)? {
        return Err(Status::not_found("diagram not found"));
    }
    svc.events.emit(
        "diagram",
        &row.id.to_string(),
        &row.project_id,
        "deleted",
        "",
        &[("kind", row.kind.clone()), ("title", row.title.clone())],
    );
    Ok(DeleteDiagramResponse {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mermaid_detection_and_truncation() {
        assert!(looks_like_mermaid("flowchart LR\n a --> b"));
        assert!(looks_like_mermaid("%% comment\nsequenceDiagram\n A->>B: x"));
        assert!(!looks_like_mermaid("show me the payment flow"));
        assert!(!looks_like_mermaid(""));
        let long = "é".repeat(MAX_FILE_BYTES);
        let cut = truncate(long);
        assert!(cut.len() <= MAX_FILE_BYTES);
        assert!(cut.is_char_boundary(cut.len()));
        assert_eq!(truncate("short".into()), "short");
        let words = diagrams::word_count(&imported_explanation("x", "flowchart LR\n a --> b"));
        assert!((60..=150).contains(&words), "{words}");
    }
}
