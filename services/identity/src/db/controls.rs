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
    /// `off` | `changed_lines` (CONTRACT.md §19, §23).
    pub code_to_model: String,
    pub updated_at: DateTime<Utc>,
    /// From `reasoner_keys`; empty when no key is stored.
    pub key_hint: Option<String>,
}

const SELECT: &str = "SELECT c.workspace_id, c.code_cache, c.code_cache_ttl_hours, c.minimal_chunks, c.reasoner, \
     c.audit_retention_days, c.code_to_model, c.updated_at, k.hint AS key_hint \
     FROM data_controls c LEFT JOIN reasoner_keys k ON k.workspace_id = c.workspace_id";

pub async fn get<'e>(exec: impl PgExecutor<'e>, workspace_id: Uuid) -> sqlx::Result<ControlsRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE c.workspace_id = $1"
    )))
    .bind(workspace_id)
    .fetch_one(exec)
    .await
}

/// The writable columns of one row, as `UpdateDataControls` validated them.
#[derive(Debug, Clone, Copy)]
pub struct ControlsUpdate<'a> {
    pub code_cache: &'a str,
    pub ttl_hours: i32,
    pub minimal_chunks: bool,
    pub reasoner: &'a str,
    pub audit_retention_days: i32,
    /// `None` when the caller did not say: the stored value stays (the
    /// column is newer than the callers that send the whole row).
    pub code_to_model: Option<&'a str>,
}

pub async fn update<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    update: ControlsUpdate<'_>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE data_controls SET code_cache = $2, code_cache_ttl_hours = $3, minimal_chunks = $4, \
           reasoner = $5, audit_retention_days = $6, \
           code_to_model = COALESCE($7, code_to_model), updated_at = now() \
         WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .bind(update.code_cache)
    .bind(update.ttl_hours)
    .bind(update.minimal_chunks)
    .bind(update.reasoner)
    .bind(update.audit_retention_days)
    .bind(update.code_to_model)
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
