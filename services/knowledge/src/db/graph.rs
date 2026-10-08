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

pub async fn nodes(exec: impl PgExecutor<'_>, project_id: Uuid) -> sqlx::Result<Vec<NodeRow>> {
    sqlx::query_as::<_, NodeRow>(
        "SELECT * FROM graph_nodes WHERE project_id = $1 ORDER BY kind, key",
    )
    .bind(project_id)
    .fetch_all(exec)
    .await
}

pub async fn get_node(exec: impl PgExecutor<'_>, id: Uuid) -> sqlx::Result<Option<NodeRow>> {
    sqlx::query_as::<_, NodeRow>("SELECT * FROM graph_nodes WHERE id = $1")
        .bind(id)
        .fetch_optional(exec)
        .await
}

pub async fn edges(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    include_history: bool,
) -> sqlx::Result<Vec<EdgeRow>> {
    sqlx::query_as::<_, EdgeRow>(
        "SELECT * FROM graph_edges WHERE project_id = $1 AND ($2 OR valid_to IS NULL) ORDER BY valid_from, relation",
    )
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
    project_id: Uuid,
    nodes: &[NodeDraft],
    edges: &[EdgeDraft],
) -> sqlx::Result<(usize, usize)> {
    let keys: Vec<&str> = nodes.iter().map(|n| n.key.as_str()).collect();
    sqlx::query("DELETE FROM graph_nodes WHERE project_id = $1 AND NOT (key = ANY($2))")
        .bind(project_id)
        .bind(&keys)
        .execute(&mut *conn)
        .await?;

    let mut ids: HashMap<&str, Uuid> = HashMap::new();
    for node in nodes {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO graph_nodes (id, project_id, kind, key, label, description, source_path, attributes)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (project_id, key) DO UPDATE SET
               kind = EXCLUDED.kind, label = EXCLUDED.label, description = EXCLUDED.description,
               source_path = EXCLUDED.source_path, attributes = EXCLUDED.attributes
             RETURNING id",
        )
        .bind(Uuid::now_v7())
        .bind(project_id)
        .bind(node_kind_to_db(node_kind(node.kind)))
        .bind(&node.key)
        .bind(&node.label)
        .bind(&node.description)
        .bind(&node.source_path)
        .bind(Json(&node.attributes))
        .fetch_one(&mut *conn)
        .await?;
        ids.insert(node.key.as_str(), id);
    }

    let current: Vec<EdgeKeyRow> = sqlx::query_as(
        "SELECT e.id, s.key AS source_key, t.key AS target_key, e.relation
         FROM graph_edges e
         JOIN graph_nodes s ON s.id = e.source_id
         JOIN graph_nodes t ON t.id = e.target_id
         WHERE e.project_id = $1 AND e.valid_to IS NULL",
    )
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
        .filter(|e| {
            ids.contains_key(e.source_key.as_str()) && ids.contains_key(e.target_key.as_str())
        })
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
        sqlx::query("UPDATE graph_edges SET valid_to = now() WHERE id = ANY($1)")
            .bind(&closed_ids)
            .execute(&mut *conn)
            .await?;
    }

    for edge in wanted {
        let key = EdgeKey {
            source_key: edge.source_key.clone(),
            target_key: edge.target_key.clone(),
            relation: edge.relation.clone(),
        };
        if !diff.opened.contains(&key) {
            continue;
        }
        sqlx::query(
            "INSERT INTO graph_edges (id, project_id, source_id, target_id, relation, category, valid_from, attributes)
             VALUES ($1, $2, $3, $4, $5, $6, now(), $7)",
        )
        .bind(Uuid::now_v7())
        .bind(project_id)
        .bind(ids[edge.source_key.as_str()])
        .bind(ids[edge.target_key.as_str()])
        .bind(&edge.relation)
        .bind(relation_category_to_db(relation_category(edge.category)))
        .bind(Json(&edge.attributes))
        .execute(&mut *conn)
        .await?;
    }

    Ok((nodes.len(), wanted_keys.len()))
}
