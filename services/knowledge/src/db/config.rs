//! What `.sdi.yml` declared (CONTRACT.md §12). One jsonb document per
//! project: it is only ever read and written whole, and the draft type is
//! already serde-friendly, so a column per key would buy nothing.

use chrono::{DateTime, Utc};
use sdi_analyzer::ProjectConfigDraft;
use sqlx::{FromRow, PgExecutor, types::Json};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct ConfigRow {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub config: Json<ProjectConfigDraft>,
    pub updated_at: DateTime<Utc>,
}

pub async fn upsert(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    config: &ProjectConfigDraft,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO project_config (workspace_id, project_id, config, updated_at) VALUES ($1, $2, $3, now())
         ON CONFLICT (project_id) DO UPDATE SET config = EXCLUDED.config, updated_at = now()",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(Json(config))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn get(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
) -> sqlx::Result<Option<ConfigRow>> {
    sqlx::query_as::<_, ConfigRow>(
        "SELECT * FROM project_config WHERE workspace_id = $1 AND project_id = $2",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_optional(exec)
    .await
}
