use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use crate::{
    db::Tx,
    delivery::model::{AUTH_GITHUB_TOKEN, REGISTRY_GHCR, RegistryInput},
};

#[derive(Debug, Clone, FromRow)]
pub struct RegistryRow {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub host: String,
    pub namespace: String,
    pub auth: String,
    pub username_secret: String,
    pub password_secret: String,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const COLUMNS: &str = "id, name, kind, host, namespace, auth, username_secret, password_secret, \
     is_default, created_at, updated_at";

/// The default first, then by name.
pub async fn list<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
) -> sqlx::Result<Vec<RegistryRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM registries WHERE workspace_id = $1 ORDER BY is_default DESC, name"
    )))
    .bind(workspace_id)
    .fetch_all(exec)
    .await
}

pub async fn get<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    id: Uuid,
) -> sqlx::Result<RegistryRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM registries WHERE workspace_id = $1 AND id = $2"
    )))
    .bind(workspace_id)
    .bind(id)
    .fetch_one(exec)
    .await
}

pub async fn default_of<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
) -> sqlx::Result<Option<RegistryRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM registries WHERE workspace_id = $1 AND is_default"
    )))
    .bind(workspace_id)
    .fetch_optional(exec)
    .await
}

pub async fn count<'e>(exec: impl PgExecutor<'e>, workspace_id: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM registries WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(exec)
        .await
}

/// The workspace's GHCR registry (`ghcr.io/<namespace>`, GitHub token, the
/// default): what CONTRACT.md §22 seeds so nothing has to be configured.
/// A no-op when the workspace already has it or another default.
pub async fn seed_default<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    namespace: &str,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "INSERT INTO registries (id, workspace_id, name, kind, host, namespace, auth, is_default) \
         VALUES ($1, $2, $3, $3, 'ghcr.io', $4, $5, true) ON CONFLICT DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(REGISTRY_GHCR)
    .bind(namespace)
    .bind(AUTH_GITHUB_TOKEN)
    .execute(exec)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Takes the default flag off every registry of the workspace (the caller
/// is about to give it to one).
pub async fn clear_default(tx: &mut Tx, workspace_id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE registries SET is_default = false, updated_at = now() \
         WHERE workspace_id = $1 AND is_default",
    )
    .bind(workspace_id)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

pub async fn insert(
    tx: &mut Tx,
    workspace_id: Uuid,
    id: Uuid,
    input: &RegistryInput,
) -> sqlx::Result<RegistryRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO registries (id, workspace_id, name, kind, host, namespace, auth, username_secret, \
           password_secret, is_default) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING {COLUMNS}"
    )))
    .bind(id)
    .bind(workspace_id)
    .bind(&input.name)
    .bind(&input.kind)
    .bind(&input.host)
    .bind(&input.namespace)
    .bind(&input.auth)
    .bind(&input.username_secret)
    .bind(&input.password_secret)
    .bind(input.is_default)
    .fetch_one(&mut **tx)
    .await
}

pub async fn update(
    tx: &mut Tx,
    workspace_id: Uuid,
    id: Uuid,
    input: &RegistryInput,
) -> sqlx::Result<RegistryRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE registries SET name = $3, kind = $4, host = $5, namespace = $6, auth = $7, \
           username_secret = $8, password_secret = $9, is_default = $10, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2 RETURNING {COLUMNS}"
    )))
    .bind(workspace_id)
    .bind(id)
    .bind(&input.name)
    .bind(&input.kind)
    .bind(&input.host)
    .bind(&input.namespace)
    .bind(&input.auth)
    .bind(&input.username_secret)
    .bind(&input.password_secret)
    .bind(input.is_default)
    .fetch_one(&mut **tx)
    .await
}

/// Environments that name this registry explicitly.
pub async fn used_by<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    id: Uuid,
) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM environments WHERE workspace_id = $1 AND registry_id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_one(exec)
    .await
}

pub async fn delete(tx: &mut Tx, workspace_id: Uuid, id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM registries WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(result.rows_affected() == 1)
}
