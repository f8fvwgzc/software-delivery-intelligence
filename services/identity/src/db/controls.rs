use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct ControlsRow {
    pub workspace_id: Uuid,
    pub code_cache: String,
    pub code_cache_ttl_hours: i32,
    pub minimal_chunks: bool,
    pub reasoner: String,
    pub audit_retention_days: i32,
    pub updated_at: DateTime<Utc>,
    /// From `reasoner_keys`; empty when no key is stored.
    pub key_hint: Option<String>,
}

const SELECT: &str = "SELECT c.workspace_id, c.code_cache, c.code_cache_ttl_hours, c.minimal_chunks, c.reasoner, \
     c.audit_retention_days, c.updated_at, k.hint AS key_hint \
     FROM data_controls c LEFT JOIN reasoner_keys k ON k.workspace_id = c.workspace_id";

pub async fn get<'e>(exec: impl PgExecutor<'e>, workspace_id: Uuid) -> sqlx::Result<ControlsRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE c.workspace_id = $1"
    )))
    .bind(workspace_id)
    .fetch_one(exec)
    .await
}

pub async fn update<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    code_cache: &str,
    ttl_hours: i32,
    minimal_chunks: bool,
    reasoner: &str,
    audit_retention_days: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE data_controls SET code_cache = $2, code_cache_ttl_hours = $3, minimal_chunks = $4, \
           reasoner = $5, audit_retention_days = $6, updated_at = now() \
         WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .bind(code_cache)
    .bind(ttl_hours)
    .bind(minimal_chunks)
    .bind(reasoner)
    .bind(audit_retention_days)
    .execute(exec)
    .await
    .map(|_| ())
}

pub async fn set_key<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    sealed_key: &[u8],
    hint: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO reasoner_keys (workspace_id, sealed_key, hint) VALUES ($1, $2, $3) \
         ON CONFLICT (workspace_id) DO UPDATE SET sealed_key = EXCLUDED.sealed_key, \
           hint = EXCLUDED.hint, updated_at = now()",
    )
    .bind(workspace_id)
    .bind(sealed_key)
    .bind(hint)
    .execute(exec)
    .await
    .map(|_| ())
}

pub async fn delete_key<'e>(exec: impl PgExecutor<'e>, workspace_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM reasoner_keys WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(exec)
        .await
        .map(|_| ())
}

pub async fn touch<'e>(exec: impl PgExecutor<'e>, workspace_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE data_controls SET updated_at = now() WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(exec)
        .await
        .map(|_| ())
}

pub async fn get_sealed_key<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
) -> sqlx::Result<Option<Vec<u8>>> {
    sqlx::query_scalar("SELECT sealed_key FROM reasoner_keys WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_optional(exec)
        .await
}
