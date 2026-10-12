use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor, types::Json};
use uuid::Uuid;

use crate::{db::Tx, delivery::model::EnvironmentInput};

#[derive(Debug, Clone, FromRow)]
pub struct EnvironmentRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: String,
    pub name: String,
    pub position: i32,
    pub kind: String,
    pub config: Json<BTreeMap<String, String>>,
    pub url: String,
    pub health_path: String,
    pub registry_id: Option<Uuid>,
    pub protected: bool,
    pub auto_promote: bool,
    pub source: String,
    pub current_release_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const COLUMNS: &str = "id, workspace_id, project_id, name, position, kind, config, url, health_path, registry_id, \
     protected, auto_promote, source, current_release_id, created_at, updated_at";

/// A project's stages in promotion order.
pub async fn list_for_project<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    project_id: &str,
) -> sqlx::Result<Vec<EnvironmentRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM environments WHERE workspace_id = $1 AND project_id = $2 \
         ORDER BY position, name"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .fetch_all(exec)
    .await
}

pub async fn get<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    id: Uuid,
) -> sqlx::Result<EnvironmentRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM environments WHERE workspace_id = $1 AND id = $2"
    )))
    .bind(workspace_id)
    .bind(id)
    .fetch_one(exec)
    .await
}

/// Locks the environment for the rest of the transaction: one writer at a
/// time decides whether a deployment to it may start.
pub async fn lock(tx: &mut Tx, workspace_id: Uuid, id: Uuid) -> sqlx::Result<EnvironmentRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM environments WHERE workspace_id = $1 AND id = $2 FOR UPDATE"
    )))
    .bind(workspace_id)
    .bind(id)
    .fetch_one(&mut **tx)
    .await
}

pub async fn by_name<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    project_id: &str,
    name: &str,
) -> sqlx::Result<Option<EnvironmentRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM environments WHERE workspace_id = $1 AND project_id = $2 AND name = $3"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .bind(name)
    .fetch_optional(exec)
    .await
}

pub async fn insert(
    tx: &mut Tx,
    workspace_id: Uuid,
    id: Uuid,
    project_id: &str,
    input: &EnvironmentInput,
    source: &str,
) -> sqlx::Result<EnvironmentRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO environments (id, workspace_id, project_id, name, position, kind, config, url, \
           health_path, registry_id, protected, auto_promote, source) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) RETURNING {COLUMNS}"
    )))
    .bind(id)
    .bind(workspace_id)
    .bind(project_id)
    .bind(&input.name)
    .bind(input.position)
    .bind(&input.kind)
    .bind(Json(&input.config))
    .bind(&input.url)
    .bind(&input.health_path)
    .bind(input.registry_id)
    .bind(input.protected)
    .bind(input.auto_promote)
    .bind(source)
    .fetch_one(&mut **tx)
    .await
}

/// Replaces everything an editor (or `.sdi.yml`) decides; what runs in the
/// environment (`current_release_id`) and its history stay.
pub async fn update(
    tx: &mut Tx,
    workspace_id: Uuid,
    id: Uuid,
    input: &EnvironmentInput,
    source: &str,
) -> sqlx::Result<EnvironmentRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE environments SET name = $3, position = $4, kind = $5, config = $6, url = $7, \
           health_path = $8, registry_id = $9, protected = $10, auto_promote = $11, source = $12, \
           updated_at = now() \
         WHERE workspace_id = $1 AND id = $2 RETURNING {COLUMNS}"
    )))
    .bind(workspace_id)
    .bind(id)
    .bind(&input.name)
    .bind(input.position)
    .bind(&input.kind)
    .bind(Json(&input.config))
    .bind(&input.url)
    .bind(&input.health_path)
    .bind(input.registry_id)
    .bind(input.protected)
    .bind(input.auto_promote)
    .bind(source)
    .fetch_one(&mut **tx)
    .await
}

pub async fn delete(tx: &mut Tx, workspace_id: Uuid, id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM environments WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// The config-managed environments of a project that `.sdi.yml` no longer
/// declares.
pub async fn config_rows_not_in(
    tx: &mut Tx,
    workspace_id: Uuid,
    project_id: &str,
    keep: &[String],
) -> sqlx::Result<Vec<EnvironmentRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM environments \
         WHERE workspace_id = $1 AND project_id = $2 AND source = 'config' AND NOT (name = ANY($3)) \
         ORDER BY position, name FOR UPDATE"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .bind(keep)
    .fetch_all(&mut **tx)
    .await
}

/// A release now runs in the environment.
pub async fn set_current_release(
    tx: &mut Tx,
    workspace_id: Uuid,
    id: Uuid,
    release_id: Uuid,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE environments SET current_release_id = $3, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(release_id)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

/// `(release, environment name)` for every environment currently running
/// one of the releases, in promotion order: `Release.deployed_to`.
pub async fn running<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    release_ids: &[Uuid],
) -> sqlx::Result<Vec<(Uuid, String)>> {
    sqlx::query_as(
        "SELECT current_release_id, name FROM environments \
         WHERE workspace_id = $1 AND current_release_id = ANY($2) ORDER BY position, name",
    )
    .bind(workspace_id)
    .bind(release_ids)
    .fetch_all(exec)
    .await
}
