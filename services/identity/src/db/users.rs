use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub github_host: String,
    pub github_id: i64,
    pub login: String,
    pub name: String,
    pub avatar_url: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

const COLUMNS: &str = "id, github_host, github_id, login, name, avatar_url, email, created_at";

/// Profile fields GitHub reports; the row is created or refreshed.
pub struct Profile<'a> {
    pub github_host: &'a str,
    pub github_id: i64,
    pub login: &'a str,
    pub name: &'a str,
    pub avatar_url: &'a str,
    /// `None` keeps the stored address (org listings do not carry emails).
    pub email: Option<&'a str>,
}

pub async fn upsert<'e>(exec: impl PgExecutor<'e>, profile: Profile<'_>) -> sqlx::Result<UserRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO users (id, github_host, github_id, login, name, avatar_url, email) \
         VALUES ($1, $2, $3, $4, $5, $6, COALESCE($7, '')) \
         ON CONFLICT (github_host, github_id) DO UPDATE SET \
           login = EXCLUDED.login, name = EXCLUDED.name, avatar_url = EXCLUDED.avatar_url, \
           email = COALESCE($7, users.email), updated_at = now() \
         RETURNING {COLUMNS}"
    )))
    .bind(Uuid::now_v7())
    .bind(profile.github_host)
    .bind(profile.github_id)
    .bind(profile.login)
    .bind(profile.name)
    .bind(profile.avatar_url)
    .bind(profile.email)
    .fetch_one(exec)
    .await
}

pub async fn get<'e>(exec: impl PgExecutor<'e>, id: Uuid) -> sqlx::Result<UserRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM users WHERE id = $1"
    )))
    .bind(id)
    .fetch_one(exec)
    .await
}

/// What the rest of the platform may show of a user: never the email.
#[derive(Debug, Clone, FromRow)]
pub struct UserRefRow {
    pub id: Uuid,
    pub login: String,
    pub name: String,
    pub avatar_url: String,
    /// Belongs to the workspace now (a member, or its owner).
    pub member: bool,
}

/// The users among `ids` this workspace may name: its members, its owner,
/// and anyone who appears in its own audit trail (so history keeps its names
/// after someone leaves). Ids outside those three are not returned.
pub async fn resolve<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> sqlx::Result<Vec<UserRefRow>> {
    sqlx::query_as(
        "SELECT id, login, name, avatar_url, member FROM ( \
           SELECT u.id, u.login, u.name, u.avatar_url, \
             (EXISTS (SELECT 1 FROM memberships m WHERE m.workspace_id = $1 AND m.user_id = u.id) \
              OR EXISTS (SELECT 1 FROM workspaces w WHERE w.id = $1 AND w.owner_user_id = u.id)) AS member \
           FROM users u WHERE u.id = ANY($2)) known \
         WHERE member OR EXISTS ( \
           SELECT 1 FROM audit a WHERE a.workspace_id = $1 AND a.user_id = known.id::text) \
         ORDER BY lower(login)",
    )
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(exec)
    .await
}
