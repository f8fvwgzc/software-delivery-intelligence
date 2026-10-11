use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct SessionRow {
    pub user_id: Uuid,
    pub oauth_token: Vec<u8>,
    pub expires_at: DateTime<Utc>,
}

const COLUMNS: &str = "user_id, oauth_token, expires_at";

pub async fn insert<'e>(
    exec: impl PgExecutor<'e>,
    user_id: Uuid,
    token_hash: &[u8],
    sealed_oauth_token: &[u8],
    scopes: &str,
    expires_at: DateTime<Utc>,
) -> sqlx::Result<SessionRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO sessions (id, user_id, token_hash, oauth_token, scopes, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING {COLUMNS}"
    )))
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(token_hash)
    .bind(sealed_oauth_token)
    .bind(scopes)
    .bind(expires_at)
    .fetch_one(exec)
    .await
}

/// A live session for the hash: not revoked, not expired. Touches
/// `last_seen_at` on the way.
pub async fn get_live<'e>(
    exec: impl PgExecutor<'e>,
    token_hash: &[u8],
) -> sqlx::Result<Option<SessionRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE sessions SET last_seen_at = now() \
         WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now() \
         RETURNING {COLUMNS}"
    )))
    .bind(token_hash)
    .fetch_optional(exec)
    .await
}

pub async fn revoke<'e>(exec: impl PgExecutor<'e>, token_hash: &[u8]) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE sessions SET revoked_at = now() WHERE token_hash = $1 AND revoked_at IS NULL",
    )
    .bind(token_hash)
    .execute(exec)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// The user's newest live session: its OAuth token is what a personal
/// workspace reads GitHub with.
pub async fn latest_live_for_user<'e>(
    exec: impl PgExecutor<'e>,
    user_id: Uuid,
) -> sqlx::Result<Option<SessionRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM sessions \
         WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now() \
         ORDER BY created_at DESC LIMIT 1"
    )))
    .bind(user_id)
    .fetch_optional(exec)
    .await
}
