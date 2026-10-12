//! Workspace secrets (CONTRACT.md §22). The value column is sealed; no
//! query here ever selects it except `get_sealed`, the read behind
//! `ResolveSecret`.

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

/// What a listing returns: everything but the value.
#[derive(Debug, Clone, FromRow)]
pub struct SecretRow {
    pub name: String,
    pub hint: String,
    pub description: String,
    pub updated_by: String,
    pub updated_at: DateTime<Utc>,
}

const COLUMNS: &str = "name, hint, description, updated_by, updated_at";

pub async fn list<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
) -> sqlx::Result<Vec<SecretRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM secrets WHERE workspace_id = $1 ORDER BY name"
    )))
    .bind(workspace_id)
    .fetch_all(exec)
    .await
}

pub async fn count<'e>(exec: impl PgExecutor<'e>, workspace_id: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM secrets WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(exec)
        .await
}

pub async fn exists<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    name: &str,
) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM secrets WHERE workspace_id = $1 AND name = $2)",
    )
    .bind(workspace_id)
    .bind(name)
    .fetch_one(exec)
    .await
}

/// Stores or replaces a secret and returns its public description.
pub async fn upsert<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    name: &str,
    sealed_value: &[u8],
    hint: &str,
    description: &str,
    updated_by: &str,
) -> sqlx::Result<SecretRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO secrets (workspace_id, name, sealed_value, hint, description, updated_by) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (workspace_id, name) DO UPDATE SET sealed_value = EXCLUDED.sealed_value, \
           hint = EXCLUDED.hint, description = EXCLUDED.description, \
           updated_by = EXCLUDED.updated_by, updated_at = now() \
         RETURNING {COLUMNS}"
    )))
    .bind(workspace_id)
    .bind(name)
    .bind(sealed_value)
    .bind(hint)
    .bind(description)
    .bind(updated_by)
    .fetch_one(exec)
    .await
}

/// True when a row was removed.
pub async fn delete<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    name: &str,
) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM secrets WHERE workspace_id = $1 AND name = $2")
        .bind(workspace_id)
        .bind(name)
        .execute(exec)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// The sealed value, for `ResolveSecret` only.
pub async fn get_sealed<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    name: &str,
) -> sqlx::Result<Option<Vec<u8>>> {
    sqlx::query_scalar("SELECT sealed_value FROM secrets WHERE workspace_id = $1 AND name = $2")
        .bind(workspace_id)
        .bind(name)
        .fetch_optional(exec)
        .await
}
