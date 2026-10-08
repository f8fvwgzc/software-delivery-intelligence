use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct DocumentRow {
    pub id: Uuid,
    pub project_id: String,
    pub kind: String,
    pub title: String,
    pub markdown: String,
    pub commit_sha: String,
    pub facts_used: i32,
    pub generated_at: DateTime<Utc>,
}

pub async fn insert(pool: &PgPool, row: &DocumentRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO documents (id, project_id, kind, title, markdown, commit_sha, facts_used, generated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(row.id)
    .bind(&row.project_id)
    .bind(&row.kind)
    .bind(&row.title)
    .bind(&row.markdown)
    .bind(&row.commit_sha)
    .bind(row.facts_used)
    .bind(row.generated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &PgPool, id: Uuid) -> sqlx::Result<DocumentRow> {
    sqlx::query_as(
        "SELECT id, project_id, kind, title, markdown, commit_sha, facts_used, generated_at FROM documents WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

/// Markdown bodies are left out: list responses omit them (proto contract).
pub async fn list(pool: &PgPool, project_id: &str) -> sqlx::Result<Vec<DocumentRow>> {
    sqlx::query_as(
        "SELECT id, project_id, kind, title, '' AS markdown, commit_sha, facts_used, generated_at \
         FROM documents WHERE project_id = $1 ORDER BY generated_at DESC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}
