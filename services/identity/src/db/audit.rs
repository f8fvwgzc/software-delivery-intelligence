use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct AuditRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: String,
    pub user_login: String,
    pub action: String,
    pub entity: String,
    pub entity_id: String,
    pub detail: String,
    pub request_id: String,
    pub ip: String,
    pub at: DateTime<Utc>,
}

const COLUMNS: &str =
    "id, workspace_id, user_id, user_login, action, entity, entity_id, detail, request_id, ip, at";

pub async fn insert<'e>(exec: impl PgExecutor<'e>, row: &AuditRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO audit (id, workspace_id, user_id, user_login, action, entity, entity_id, detail, request_id, ip, at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(row.id)
    .bind(row.workspace_id)
    .bind(&row.user_id)
    .bind(&row.user_login)
    .bind(&row.action)
    .bind(&row.entity)
    .bind(&row.entity_id)
    .bind(&row.detail)
    .bind(&row.request_id)
    .bind(&row.ip)
    .bind(row.at)
    .execute(exec)
    .await
    .map(|_| ())
}

/// Newest first; `before` is an exclusive cursor, `action_prefix` a
/// `starts-with` filter (empty matches all).
pub async fn list<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    action_prefix: &str,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> sqlx::Result<Vec<AuditRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM audit \
         WHERE workspace_id = $1 AND ($2 = '' OR action LIKE $2 || '%') \
           AND ($3::timestamptz IS NULL OR at < $3) \
         ORDER BY at DESC, id DESC LIMIT $4"
    )))
    .bind(workspace_id)
    .bind(action_prefix)
    .bind(before)
    .bind(limit)
    .fetch_all(exec)
    .await
}

/// Applies every workspace's `audit_retention_days` (0 keeps forever).
pub async fn prune<'e>(exec: impl PgExecutor<'e>) -> sqlx::Result<u64> {
    sqlx::query(
        "DELETE FROM audit a USING data_controls c \
         WHERE c.workspace_id = a.workspace_id AND c.audit_retention_days > 0 \
           AND a.at < now() - make_interval(days => c.audit_retention_days)",
    )
    .execute(exec)
    .await
    .map(|r| r.rows_affected())
}
