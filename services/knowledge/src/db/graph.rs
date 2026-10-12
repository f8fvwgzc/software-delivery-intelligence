use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use sdi_analyzer::{EdgeDraft, NodeDraft};
use sqlx::{FromRow, PgConnection, PgExecutor, types::Json};
use uuid::Uuid;

use crate::{
    convert::{node_kind, node_kind_to_db, relation_category, relation_category_to_db},
    domain::temporal::{self, EdgeKey},
};

#[derive(Debug, Clone, FromRow)]
pub struct NodeRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub key: String,
    pub label: String,
    pub description: String,
    pub source_path: String,
    pub attributes: Json<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct EdgeRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub relation: String,
    pub category: String,
    pub valid_from: DateTime<Utc>,
    pub valid_to: Option<DateTime<Utc>>,
    pub attributes: Json<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, FromRow)]
struct EdgeKeyRow {
    id: Uuid,
    source_key: String,
    target_key: String,
    relation: String,
}

pub async fn nodes(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
) -> sqlx::Result<Vec<NodeRow>> {
    sqlx::query_as::<_, NodeRow>(
        "SELECT * FROM graph_nodes WHERE workspace_id = $1 AND project_id = $2 ORDER BY kind, key",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_all(exec)
    .await
}

pub async fn get_node(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> sqlx::Result<Option<NodeRow>> {
    sqlx::query_as::<_, NodeRow>("SELECT * FROM graph_nodes WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(exec)
        .await
}

pub async fn edges(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    include_history: bool,
) -> sqlx::Result<Vec<EdgeRow>> {
    sqlx::query_as::<_, EdgeRow>(
        "SELECT * FROM graph_edges WHERE workspace_id = $1 AND project_id = $2 AND ($3 OR valid_to IS NULL) ORDER BY valid_from, relation",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(include_history)
    .fetch_all(exec)
    .await
}

/// Applies a fresh analysis to the stored graph. Nodes are upserted by key
/// and vanished nodes removed; edges are diffed temporally so history stays.
/// Returns `(nodes, valid edges)` after the sync.
pub async fn sync(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    project_id: Uuid,
    nodes: &[NodeDraft],
    edges: &[EdgeDraft],
) -> sqlx::Result<(usize, usize)> {
    let keys: Vec<&str> = nodes.iter().map(|n| n.key.as_str()).collect();
    sqlx::query("DELETE FROM graph_nodes WHERE workspace_id = $1 AND project_id = $2 AND NOT (key = ANY($3))")
        .bind(workspace_id)
        .bind(project_id)
        .bind(&keys)
        .execute(&mut *conn)
        .await?;

    // Nodes are upserted by key in batches; the returned (key, id) pairs
    // map the analyzer's keys onto rows for the edges below.
    let mut ids: HashMap<String, Uuid> = HashMap::with_capacity(nodes.len());
    for batch in nodes.chunks(super::INSERT_BATCH) {
        let new_ids: Vec<Uuid> = batch.iter().map(|_| Uuid::now_v7()).collect();
        let kinds: Vec<String> = batch
            .iter()
            .map(|n| node_kind_to_db(node_kind(n.kind)))
            .collect();
        let node_keys: Vec<&str> = batch.iter().map(|n| n.key.as_str()).collect();
        let labels: Vec<&str> = batch.iter().map(|n| n.label.as_str()).collect();
        let descriptions: Vec<&str> = batch.iter().map(|n| n.description.as_str()).collect();
        let paths: Vec<&str> = batch.iter().map(|n| n.source_path.as_str()).collect();
        let attributes: Vec<serde_json::Value> = batch
            .iter()
            .map(|n| serde_json::to_value(&n.attributes).unwrap_or_default())
            .collect();
        let rows: Vec<(String, Uuid)> = sqlx::query_as(
            "INSERT INTO graph_nodes (id, workspace_id, project_id, kind, key, label, description, source_path, attributes)
             SELECT id, $1, $2, kind, key, label, description, source_path, attributes
               FROM UNNEST($3::uuid[], $4::text[], $5::text[], $6::text[], $7::text[], $8::text[], $9::jsonb[])
                 AS t(id, kind, key, label, description, source_path, attributes)
             ON CONFLICT (project_id, key) DO UPDATE SET
               kind = EXCLUDED.kind, label = EXCLUDED.label, description = EXCLUDED.description,
               source_path = EXCLUDED.source_path, attributes = EXCLUDED.attributes
             RETURNING key, id",
        )
        .bind(workspace_id)
        .bind(project_id)
        .bind(&new_ids)
        .bind(&kinds)
        .bind(&node_keys)
        .bind(&labels)
        .bind(&descriptions)
        .bind(&paths)
        .bind(&attributes)
        .fetch_all(&mut *conn)
        .await?;
        ids.extend(rows);
    }

    let current: Vec<EdgeKeyRow> = sqlx::query_as(
        "SELECT e.id, s.key AS source_key, t.key AS target_key, e.relation
         FROM graph_edges e
         JOIN graph_nodes s ON s.id = e.source_id
         JOIN graph_nodes t ON t.id = e.target_id
         WHERE e.workspace_id = $1 AND e.project_id = $2 AND e.valid_to IS NULL",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_all(&mut *conn)
    .await?;

    let current_keys: Vec<EdgeKey> = current
        .iter()
        .map(|e| EdgeKey {
            source_key: e.source_key.clone(),
            target_key: e.target_key.clone(),
            relation: e.relation.clone(),
        })
        .collect();
    // Edges pointing at unknown nodes are analyzer bugs; dropping them beats failing the scan.
    let wanted: Vec<&EdgeDraft> = edges
        .iter()
        .filter(|e| ids.contains_key(&e.source_key) && ids.contains_key(&e.target_key))
        .collect();
    let wanted_keys: Vec<EdgeKey> = wanted
        .iter()
        .map(|e| EdgeKey {
            source_key: e.source_key.clone(),
            target_key: e.target_key.clone(),
            relation: e.relation.clone(),
        })
        .collect();
    let diff = temporal::diff(&current_keys, &wanted_keys);

    let closed_ids: Vec<Uuid> = current
        .iter()
        .filter(|e| {
            diff.closed.iter().any(|c| {
                c.source_key == e.source_key
                    && c.target_key == e.target_key
                    && c.relation == e.relation
            })
        })
        .map(|e| e.id)
        .collect();
    if !closed_ids.is_empty() {
        sqlx::query(
            "UPDATE graph_edges SET valid_to = now() WHERE workspace_id = $1 AND id = ANY($2)",
        )
        .bind(workspace_id)
        .bind(&closed_ids)
        .execute(&mut *conn)
        .await?;
    }

    let opened: Vec<&EdgeDraft> = wanted
        .iter()
        .copied()
        .filter(|edge| {
            diff.opened.contains(&EdgeKey {
                source_key: edge.source_key.clone(),
                target_key: edge.target_key.clone(),
                relation: edge.relation.clone(),
            })
        })
        .collect();
    for batch in opened.chunks(super::INSERT_BATCH) {
        let new_ids: Vec<Uuid> = batch.iter().map(|_| Uuid::now_v7()).collect();
        let sources: Vec<Uuid> = batch.iter().map(|e| ids[&e.source_key]).collect();
        let targets: Vec<Uuid> = batch.iter().map(|e| ids[&e.target_key]).collect();
        let relations: Vec<&str> = batch.iter().map(|e| e.relation.as_str()).collect();
        let categories: Vec<String> = batch
            .iter()
            .map(|e| relation_category_to_db(relation_category(e.category)))
            .collect();
        let attributes: Vec<serde_json::Value> = batch
            .iter()
            .map(|e| serde_json::to_value(&e.attributes).unwrap_or_default())
            .collect();
        sqlx::query(
            "INSERT INTO graph_edges (id, workspace_id, project_id, source_id, target_id, relation, category, valid_from, attributes)
             SELECT id, $1, $2, source_id, target_id, relation, category, now(), attributes
               FROM UNNEST($3::uuid[], $4::uuid[], $5::uuid[], $6::text[], $7::text[], $8::jsonb[])
                 AS t(id, source_id, target_id, relation, category, attributes)",
        )
        .bind(workspace_id)
        .bind(project_id)
        .bind(&new_ids)
        .bind(&sources)
        .bind(&targets)
        .bind(&relations)
        .bind(&categories)
        .bind(&attributes)
        .execute(&mut *conn)
        .await?;
    }

    Ok((nodes.len(), wanted_keys.len()))
}
