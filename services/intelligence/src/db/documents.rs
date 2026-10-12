use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct DocumentRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
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
        "INSERT INTO documents (id, workspace_id, project_id, kind, title, markdown, commit_sha, facts_used, generated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(row.id)
    .bind(row.workspace_id)
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

pub async fn get(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> sqlx::Result<DocumentRow> {
    sqlx::query_as(
        "SELECT id, workspace_id, project_id, kind, title, markdown, commit_sha, facts_used, generated_at \
         FROM documents WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_one(pool)
    .await
}

/// Markdown bodies are left out: list responses omit them (proto contract).
/// Newest first, at most `limit`, generated strictly before `before`.
pub async fn list(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: &str,
    limit: i64,
    before: Option<DateTime<Utc>>,
) -> sqlx::Result<Vec<DocumentRow>> {
    sqlx::query_as(
        "SELECT id, workspace_id, project_id, kind, title, '' AS markdown, commit_sha, facts_used, generated_at \
         FROM documents WHERE workspace_id = $1 AND project_id = $2 \
           AND ($3::timestamptz IS NULL OR generated_at < $3) \
         ORDER BY generated_at DESC LIMIT $4",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await
}
