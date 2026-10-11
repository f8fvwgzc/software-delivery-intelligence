use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

pub const KIND_PERSONAL: &str = "PERSONAL";
pub const KIND_ORGANIZATION: &str = "ORGANIZATION";

/// A workspace joined with its installation (when it has one) and its
/// member count.
#[derive(Debug, Clone, FromRow)]
pub struct WorkspaceRow {
    pub id: Uuid,
    pub kind: String,
    pub github_host: String,
    pub slug: String,
    pub name: String,
    pub avatar_url: String,
    pub owner_user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub installation_id: Option<i64>,
    pub api_base_url: Option<String>,
    pub repository_selection: Option<String>,
    pub installation_removed_at: Option<DateTime<Utc>>,
    pub member_count: i64,
}

const SELECT: &str = "SELECT w.id, w.kind, w.github_host, w.slug, w.name, w.avatar_url, w.owner_user_id, \
     w.created_at, i.installation_id, i.api_base_url, i.repository_selection, \
     i.removed_at AS installation_removed_at, \
     (SELECT count(*) FROM memberships m WHERE m.workspace_id = w.id) AS member_count \
     FROM workspaces w LEFT JOIN installations i ON i.workspace_id = w.id";

pub async fn get<'e>(exec: impl PgExecutor<'e>, id: Uuid) -> sqlx::Result<WorkspaceRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("{SELECT} WHERE w.id = $1")))
        .bind(id)
        .fetch_one(exec)
        .await
}

/// Personal first, then organisations by creation: the first row is the
/// default workspace of a request that names none.
pub async fn list_for_user<'e>(
    exec: impl PgExecutor<'e>,
    user_id: Uuid,
) -> sqlx::Result<Vec<WorkspaceRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE w.owner_user_id = $1 \
         OR w.id IN (SELECT workspace_id FROM memberships WHERE user_id = $1) \
         ORDER BY (w.kind = 'PERSONAL') DESC, w.created_at"
    )))
    .bind(user_id)
    .fetch_all(exec)
    .await
}

/// The user's own workspace, created on first login.
pub async fn ensure_personal<'e>(
    exec: impl PgExecutor<'e>,
    owner_user_id: Uuid,
    github_host: &str,
    slug: &str,
    name: &str,
    avatar_url: &str,
) -> sqlx::Result<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO workspaces (id, kind, github_host, slug, name, avatar_url, owner_user_id) \
         VALUES ($1, 'PERSONAL', $2, $3, $4, $5, $6) \
         ON CONFLICT (github_host, kind, slug) DO UPDATE SET \
           name = EXCLUDED.name, avatar_url = EXCLUDED.avatar_url, owner_user_id = EXCLUDED.owner_user_id \
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(github_host)
    .bind(slug)
    .bind(name)
    .bind(avatar_url)
    .bind(owner_user_id)
    .fetch_one(exec)
    .await
}

pub struct NewInstallation<'a> {
    pub installation_id: i64,
    pub kind: &'a str,
    pub github_host: &'a str,
    pub api_base_url: &'a str,
    pub account_login: &'a str,
    pub account_type: &'a str,
    pub avatar_url: &'a str,
    /// Empty keeps the stored value (webhooks that do not carry it).
    pub repository_selection: &'a str,
    pub installed_by_user_id: Option<Uuid>,
    /// Owner of the workspace for an app installed on a user account.
    pub owner_user_id: Option<Uuid>,
}

/// Creates or refreshes the workspace of an installation and the
/// installation row itself (a re-install after an uninstall revives it).
pub async fn upsert_installation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    install: NewInstallation<'_>,
) -> sqlx::Result<Uuid> {
    let workspace_id: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (id, kind, github_host, slug, name, avatar_url, owner_user_id) \
         VALUES ($1, $2, $3, $4, $4, $5, $6) \
         ON CONFLICT (github_host, kind, slug) DO UPDATE SET \
           avatar_url = CASE WHEN EXCLUDED.avatar_url = '' THEN workspaces.avatar_url ELSE EXCLUDED.avatar_url END, \
           owner_user_id = COALESCE(workspaces.owner_user_id, EXCLUDED.owner_user_id) \
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(install.kind)
    .bind(install.github_host)
    .bind(install.account_login)
    .bind(install.avatar_url)
    .bind(install.owner_user_id)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO installations (installation_id, workspace_id, account_login, account_type, github_host, \
           api_base_url, repository_selection, installed_by_user_id) \
         VALUES ($1, $2, $3, $4, $5, $6, COALESCE(NULLIF($7, ''), 'selected'), $8) \
         ON CONFLICT (installation_id) DO UPDATE SET \
           workspace_id = EXCLUDED.workspace_id, account_login = EXCLUDED.account_login, \
           account_type = EXCLUDED.account_type, api_base_url = EXCLUDED.api_base_url, \
           repository_selection = COALESCE(NULLIF($7, ''), installations.repository_selection), \
           installed_by_user_id = COALESCE(EXCLUDED.installed_by_user_id, installations.installed_by_user_id), \
           removed_at = NULL, updated_at = now()",
    )
    .bind(install.installation_id)
    .bind(workspace_id)
    .bind(install.account_login)
    .bind(install.account_type)
    .bind(install.github_host)
    .bind(install.api_base_url)
    .bind(install.repository_selection)
    .bind(install.installed_by_user_id)
    .execute(&mut **tx)
    .await?;
    Ok(workspace_id)
}

pub async fn remove_installation<'e>(
    exec: impl PgExecutor<'e>,
    installation_id: i64,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE installations SET removed_at = now(), updated_at = now() \
         WHERE installation_id = $1 AND removed_at IS NULL",
    )
    .bind(installation_id)
    .execute(exec)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn ensure_controls<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO data_controls (workspace_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(workspace_id)
        .execute(exec)
        .await
        .map(|_| ())
}
