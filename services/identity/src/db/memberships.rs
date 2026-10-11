use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use super::users::UserRow;

#[derive(Debug, Clone, FromRow)]
pub struct MembershipRow {
    pub github_role: String,
    pub teams: Vec<String>,
    pub derived_role: String,
    pub override_role: Option<String>,
    pub synced_at: DateTime<Utc>,
}

/// A membership with the user it belongs to, which is what `Member` carries.
#[derive(Debug, Clone, FromRow)]
pub struct MemberRow {
    #[sqlx(flatten)]
    pub membership: MembershipRow,
    #[sqlx(flatten)]
    pub user: UserRow,
}

const SELECT: &str = "SELECT m.github_role, m.teams, m.derived_role, m.override_role, m.synced_at, \
     u.id, u.github_host, u.github_id, u.login, u.name, u.avatar_url, u.email, u.created_at \
     FROM memberships m JOIN users u ON u.id = m.user_id";

pub async fn list<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
) -> sqlx::Result<Vec<MemberRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE m.workspace_id = $1 ORDER BY lower(u.login)"
    )))
    .bind(workspace_id)
    .fetch_all(exec)
    .await
}

pub async fn get<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<Option<MemberRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE m.workspace_id = $1 AND m.user_id = $2"
    )))
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(exec)
    .await
}

/// Writes what GitHub says; an existing override is kept (and re-checked
/// against the new derived role by the caller).
pub async fn upsert<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    user_id: Uuid,
    github_role: &str,
    teams: &[String],
    derived_role: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO memberships (workspace_id, user_id, github_role, teams, derived_role, synced_at) \
         VALUES ($1, $2, $3, $4, $5, now()) \
         ON CONFLICT (workspace_id, user_id) DO UPDATE SET \
           github_role = EXCLUDED.github_role, teams = EXCLUDED.teams, \
           derived_role = EXCLUDED.derived_role, synced_at = now()",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(github_role)
    .bind(teams)
    .bind(derived_role)
    .execute(exec)
    .await
    .map(|_| ())
}

pub async fn set_override<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    user_id: Uuid,
    override_role: Option<&str>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE memberships SET override_role = $3 WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(override_role)
    .execute(exec)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Drops everyone a full sync did not see.
pub async fn retain<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    keep_user_ids: &[Uuid],
) -> sqlx::Result<u64> {
    sqlx::query("DELETE FROM memberships WHERE workspace_id = $1 AND NOT (user_id = ANY($2))")
        .bind(workspace_id)
        .bind(keep_user_ids)
        .execute(exec)
        .await
        .map(|r| r.rows_affected())
}

pub async fn last_synced<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
) -> sqlx::Result<Option<DateTime<Utc>>> {
    sqlx::query_scalar("SELECT max(synced_at) FROM memberships WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(exec)
        .await
}
