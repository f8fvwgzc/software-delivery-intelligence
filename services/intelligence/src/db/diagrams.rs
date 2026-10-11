use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

use crate::domain::diagrams::DiagramSpec;

#[derive(Debug, Clone, FromRow)]
pub struct DiagramRow {
    pub id: Uuid,
    pub project_id: String,
    pub kind: String,
    pub title: String,
    pub explanation: String,
    pub spec: Json<DiagramSpec>,
    pub mermaid: String,
    /// "generated" | "edited" | "mermaid"
    pub source: String,
    pub commit_sha: String,
    pub revision: i32,
    pub reasoner: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn insert(pool: &PgPool, row: &DiagramRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO diagrams (id, project_id, kind, title, explanation, spec, mermaid, source, commit_sha, revision, reasoner, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(row.id)
    .bind(&row.project_id)
    .bind(&row.kind)
    .bind(&row.title)
    .bind(&row.explanation)
    .bind(&row.spec)
    .bind(&row.mermaid)
    .bind(&row.source)
    .bind(&row.commit_sha)
    .bind(row.revision)
    .bind(&row.reasoner)
    .bind(row.created_at)
    .bind(row.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &PgPool, id: Uuid) -> sqlx::Result<DiagramRow> {
    sqlx::query_as(
        "SELECT id, project_id, kind, title, explanation, spec, mermaid, source, commit_sha, revision, reasoner, \
         created_at, updated_at FROM diagrams WHERE id = $1",
    )
        .bind(id)
        .fetch_one(pool)
        .await
}

/// Spec and Mermaid are left out: list responses omit them (proto contract).
pub async fn list(pool: &PgPool, project_id: &str) -> sqlx::Result<Vec<DiagramRow>> {
    sqlx::query_as(
        "SELECT id, project_id, kind, title, explanation, '{}'::jsonb AS spec, '' AS mermaid, source, commit_sha, \
         revision, reasoner, created_at, updated_at \
         FROM diagrams WHERE project_id = $1 ORDER BY created_at DESC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

/// Writes the editable fields back (title, spec, mermaid, source, revision, updated_at).
pub async fn update(pool: &PgPool, row: &DiagramRow) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE diagrams SET title = $2, spec = $3, mermaid = $4, source = $5, revision = $6, updated_at = $7 \
         WHERE id = $1",
    )
    .bind(row.id)
    .bind(&row.title)
    .bind(&row.spec)
    .bind(&row.mermaid)
    .bind(&row.source)
    .bind(row.revision)
    .bind(row.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Whether a row was deleted.
pub async fn delete(pool: &PgPool, id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM diagrams WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
