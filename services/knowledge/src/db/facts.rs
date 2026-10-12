use chrono::{DateTime, Utc};
use sdi_analyzer::FactDraft;
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

use crate::convert::{fact_category, fact_category_to_db, fact_kind, fact_kind_to_db};

#[derive(Debug, Clone, FromRow)]
pub struct FactRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub category: String,
    pub key: String,
    pub label: String,
    pub value: String,
    pub confidence: f64,
    pub source_path: String,
    pub evidence: String,
    pub detected_at: DateTime<Utc>,
}

/// Facts are a snapshot: the new analysis replaces the old one wholesale.
pub async fn replace(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    project_id: Uuid,
    facts: &[FactDraft],
) -> sqlx::Result<usize> {
    sqlx::query("DELETE FROM facts WHERE workspace_id = $1 AND project_id = $2")
        .bind(workspace_id)
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    let mut inserted = 0;
    for batch in facts.chunks(super::INSERT_BATCH) {
        let ids: Vec<Uuid> = batch.iter().map(|_| Uuid::now_v7()).collect();
        let kinds: Vec<String> = batch
            .iter()
            .map(|f| fact_kind_to_db(fact_kind(f.kind)))
            .collect();
        let categories: Vec<String> = batch
            .iter()
            .map(|f| fact_category_to_db(fact_category(f.category)))
            .collect();
        let keys: Vec<&str> = batch.iter().map(|f| f.key.as_str()).collect();
        let labels: Vec<&str> = batch.iter().map(|f| f.label.as_str()).collect();
        let values: Vec<&str> = batch.iter().map(|f| f.value.as_str()).collect();
        let confidences: Vec<f64> = batch.iter().map(|f| f.confidence.clamp(0.0, 1.0)).collect();
        let paths: Vec<&str> = batch.iter().map(|f| f.source_path.as_str()).collect();
        let evidence: Vec<&str> = batch.iter().map(|f| f.evidence.as_str()).collect();
        let result = sqlx::query(
            "INSERT INTO facts (id, workspace_id, project_id, kind, category, key, label, value, confidence, source_path, evidence)
             SELECT id, $1, $2, kind, category, key, label, value, confidence, source_path, evidence
               FROM UNNEST($3::uuid[], $4::text[], $5::text[], $6::text[], $7::text[], $8::text[], $9::float8[], $10::text[], $11::text[])
                 AS t(id, kind, category, key, label, value, confidence, source_path, evidence)
             ON CONFLICT (project_id, key) DO NOTHING",
        )
        .bind(workspace_id)
        .bind(project_id)
        .bind(&ids)
        .bind(&kinds)
        .bind(&categories)
        .bind(&keys)
        .bind(&labels)
        .bind(&values)
        .bind(&confidences)
        .bind(&paths)
        .bind(&evidence)
        .execute(&mut *conn)
        .await?;
        inserted += result.rows_affected() as usize;
    }
    Ok(inserted)
}

pub async fn list(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    kind: Option<&str>,
    category: Option<&str>,
) -> sqlx::Result<Vec<FactRow>> {
    sqlx::query_as::<_, FactRow>(
        "SELECT * FROM facts WHERE workspace_id = $1 AND project_id = $2
           AND ($3::text IS NULL OR kind = $3)
           AND ($4::text IS NULL OR category = $4)
         ORDER BY category, kind, key",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(kind)
    .bind(category)
    .fetch_all(exec)
    .await
}

/// Ids of the facts whose evidence came from any of `paths`, in the list order.
pub async fn ids_for_paths(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    paths: &[String],
) -> sqlx::Result<Vec<Uuid>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM facts WHERE workspace_id = $1 AND project_id = $2 AND source_path = ANY($3) ORDER BY category, kind, key",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(paths)
    .fetch_all(exec)
    .await
}

/// Ids of the facts whose evidence came from `path`, in the list order.
pub async fn ids_for_path(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    path: &str,
) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM facts WHERE workspace_id = $1 AND project_id = $2 AND source_path = $3 ORDER BY category, kind, key",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(path)
    .fetch_all(exec)
    .await
}
