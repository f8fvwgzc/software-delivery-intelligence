use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgExecutor, types::Json};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stage {
    pub key: String,
    pub label: String,
    /// Lowercase `StageStatus` name.
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct ScanRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub status: String,
    pub commit_sha: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub files_total: i32,
    pub files_analyzed: i32,
    pub error: String,
    pub stages: Json<Vec<Stage>>,
    pub facts_found: i32,
    pub nodes_found: i32,
    pub edges_found: i32,
    pub created_at: DateTime<Utc>,
}

pub async fn insert(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    stages: &[Stage],
) -> sqlx::Result<ScanRow> {
    sqlx::query_as::<_, ScanRow>(
        "INSERT INTO scans (id, project_id, status, stages) VALUES ($1, $2, 'queued', $3) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(project_id)
    .bind(Json(stages))
    .fetch_one(exec)
    .await
}

pub async fn get(exec: impl PgExecutor<'_>, id: Uuid) -> sqlx::Result<ScanRow> {
    sqlx::query_as::<_, ScanRow>("SELECT * FROM scans WHERE id = $1")
        .bind(id)
        .fetch_one(exec)
        .await
}

pub async fn list(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    limit: i64,
) -> sqlx::Result<Vec<ScanRow>> {
    sqlx::query_as::<_, ScanRow>(
        "SELECT * FROM scans WHERE project_id = $1 ORDER BY created_at DESC LIMIT $2",
    )
    .bind(project_id)
    .bind(limit)
    .fetch_all(exec)
    .await
}

pub async fn active_exists(exec: impl PgExecutor<'_>, project_id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM scans WHERE project_id = $1 AND status IN ('queued', 'running'))",
    )
    .bind(project_id)
    .fetch_one(exec)
    .await
}

pub async fn set_running(exec: impl PgExecutor<'_>, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE scans SET status = 'running', started_at = now() WHERE id = $1")
        .bind(id)
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn set_commit(exec: impl PgExecutor<'_>, id: Uuid, commit_sha: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE scans SET commit_sha = $2 WHERE id = $1")
        .bind(id)
        .bind(commit_sha)
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn set_stages(exec: impl PgExecutor<'_>, id: Uuid, stages: &[Stage]) -> sqlx::Result<()> {
    sqlx::query("UPDATE scans SET stages = $2 WHERE id = $1")
        .bind(id)
        .bind(Json(stages))
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn set_files(
    exec: impl PgExecutor<'_>,
    id: Uuid,
    total: i32,
    analyzed: i32,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE scans SET files_total = $2, files_analyzed = $3 WHERE id = $1")
        .bind(id)
        .bind(total)
        .bind(analyzed)
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn set_counts(
    exec: impl PgExecutor<'_>,
    id: Uuid,
    facts: i32,
    nodes: i32,
    edges: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE scans SET facts_found = $2, nodes_found = $3, edges_found = $4 WHERE id = $1",
    )
    .bind(id)
    .bind(facts)
    .bind(nodes)
    .bind(edges)
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn set_succeeded(
    exec: impl PgExecutor<'_>,
    id: Uuid,
    stages: &[Stage],
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE scans SET status = 'succeeded', finished_at = now(), stages = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(Json(stages))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn set_failed(
    exec: impl PgExecutor<'_>,
    id: Uuid,
    error: &str,
    stages: &[Stage],
) -> sqlx::Result<()> {
    sqlx::query("UPDATE scans SET status = 'failed', finished_at = now(), error = $2, stages = $3 WHERE id = $1")
        .bind(id)
        .bind(error)
        .bind(Json(stages))
        .execute(exec)
        .await?;
    Ok(())
}

/// Fails every queued or running scan; called once at startup because a scan
/// only lives inside the process that started it.
pub async fn fail_interrupted(exec: impl PgExecutor<'_>) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "UPDATE scans SET status = 'failed', finished_at = now(), error = 'interrupted by service restart'
         WHERE status IN ('queued', 'running')",
    )
    .execute(exec)
    .await?;
    Ok(result.rows_affected())
}
