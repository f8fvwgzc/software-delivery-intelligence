use chrono::{DateTime, Utc};
use sdi_analyzer::{DependabotDraft, DependencyDraft};
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct DependencyRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub ecosystem: String,
    pub name: String,
    pub version: String,
    pub manifest_path: String,
    pub scope: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct DependabotRow {
    pub workspace_id: Uuid,
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
    workspace_id: Uuid,
    project_id: Uuid,
    deps: &[DependencyDraft],
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM dependencies WHERE workspace_id = $1 AND project_id = $2")
        .bind(workspace_id)
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    for batch in deps.chunks(super::INSERT_BATCH) {
        let ids: Vec<Uuid> = batch.iter().map(|_| Uuid::now_v7()).collect();
        let ecosystems: Vec<&str> = batch.iter().map(|d| d.ecosystem.as_str()).collect();
        let names: Vec<&str> = batch.iter().map(|d| d.name.as_str()).collect();
        let versions: Vec<&str> = batch.iter().map(|d| d.version.as_str()).collect();
        let manifests: Vec<&str> = batch.iter().map(|d| d.manifest_path.as_str()).collect();
        let scopes: Vec<&str> = batch.iter().map(|d| d.scope.as_str()).collect();
        sqlx::query(
            "INSERT INTO dependencies (id, workspace_id, project_id, ecosystem, name, version, manifest_path, scope)
             SELECT id, $1, $2, ecosystem, name, version, manifest_path, scope
               FROM UNNEST($3::uuid[], $4::text[], $5::text[], $6::text[], $7::text[], $8::text[])
                 AS t(id, ecosystem, name, version, manifest_path, scope)",
        )
        .bind(workspace_id)
        .bind(project_id)
        .bind(&ids)
        .bind(&ecosystems)
        .bind(&names)
        .bind(&versions)
        .bind(&manifests)
        .bind(&scopes)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn list(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
) -> sqlx::Result<Vec<DependencyRow>> {
    sqlx::query_as::<_, DependencyRow>(
        "SELECT * FROM dependencies WHERE workspace_id = $1 AND project_id = $2 ORDER BY ecosystem, scope, name",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_all(exec)
    .await
}

pub async fn by_ecosystem(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
) -> sqlx::Result<Vec<EcosystemCountRow>> {
    sqlx::query_as::<_, EcosystemCountRow>(
        "SELECT ecosystem, count(*) AS count FROM dependencies WHERE workspace_id = $1 AND project_id = $2
         GROUP BY ecosystem ORDER BY count DESC, ecosystem",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_all(exec)
    .await
}

pub async fn upsert_dependabot(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    draft: &DependabotDraft,
    open_pull_requests: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO dependabot (workspace_id, project_id, configured, config_path, ecosystems, open_pull_requests, synced_at)
         VALUES ($1, $2, $3, $4, $5, $6, now())
         ON CONFLICT (project_id) DO UPDATE SET
           configured = EXCLUDED.configured, config_path = EXCLUDED.config_path,
           ecosystems = EXCLUDED.ecosystems, open_pull_requests = EXCLUDED.open_pull_requests, synced_at = now()",
    )
    .bind(workspace_id)
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
    workspace_id: Uuid,
    project_id: Uuid,
) -> sqlx::Result<Option<DependabotRow>> {
    sqlx::query_as::<_, DependabotRow>(
        "SELECT * FROM dependabot WHERE workspace_id = $1 AND project_id = $2",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_optional(exec)
    .await
}
