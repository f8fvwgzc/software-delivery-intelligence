use chrono::{DateTime, Utc};
use sdi_analyzer::{DependabotDraft, DependencyDraft};
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct DependencyRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub ecosystem: String,
    pub name: String,
    pub version: String,
    pub manifest_path: String,
    pub scope: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct DependabotRow {
    pub project_id: Uuid,
    pub configured: bool,
    pub config_path: String,
    pub ecosystems: Vec<String>,
    pub open_pull_requests: i32,
    pub synced_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct EcosystemCountRow {
    pub ecosystem: String,
    pub count: i64,
}

pub async fn replace(
    conn: &mut PgConnection,
    project_id: Uuid,
    deps: &[DependencyDraft],
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM dependencies WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    for dep in deps {
        sqlx::query(
            "INSERT INTO dependencies (id, project_id, ecosystem, name, version, manifest_path, scope)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::now_v7())
        .bind(project_id)
        .bind(&dep.ecosystem)
        .bind(&dep.name)
        .bind(&dep.version)
        .bind(&dep.manifest_path)
        .bind(&dep.scope)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn list(exec: impl PgExecutor<'_>, project_id: Uuid) -> sqlx::Result<Vec<DependencyRow>> {
    sqlx::query_as::<_, DependencyRow>(
        "SELECT * FROM dependencies WHERE project_id = $1 ORDER BY ecosystem, scope, name",
    )
    .bind(project_id)
    .fetch_all(exec)
    .await
}

pub async fn by_ecosystem(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
) -> sqlx::Result<Vec<EcosystemCountRow>> {
    sqlx::query_as::<_, EcosystemCountRow>(
        "SELECT ecosystem, count(*) AS count FROM dependencies WHERE project_id = $1 GROUP BY ecosystem ORDER BY count DESC, ecosystem",
    )
    .bind(project_id)
    .fetch_all(exec)
    .await
}

pub async fn upsert_dependabot(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    draft: &DependabotDraft,
    open_pull_requests: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO dependabot (project_id, configured, config_path, ecosystems, open_pull_requests, synced_at)
         VALUES ($1, $2, $3, $4, $5, now())
         ON CONFLICT (project_id) DO UPDATE SET
           configured = EXCLUDED.configured, config_path = EXCLUDED.config_path,
           ecosystems = EXCLUDED.ecosystems, open_pull_requests = EXCLUDED.open_pull_requests, synced_at = now()",
    )
    .bind(project_id)
    .bind(draft.configured)
    .bind(&draft.config_path)
    .bind(&draft.ecosystems)
    .bind(open_pull_requests)
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn get_dependabot(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
) -> sqlx::Result<Option<DependabotRow>> {
    sqlx::query_as::<_, DependabotRow>("SELECT * FROM dependabot WHERE project_id = $1")
        .bind(project_id)
        .fetch_optional(exec)
        .await
}
