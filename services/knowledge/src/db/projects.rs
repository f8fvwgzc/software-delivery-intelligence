use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct ProjectRow {
    pub id: Uuid,
    pub github_repo_id: Option<i64>,
    pub name: String,
    pub repo_full_name: String,
    pub repo_url: String,
    pub default_branch: String,
    pub is_private: bool,
    pub description: String,
    pub primary_language: String,
    pub status: String,
    pub mode: String,
    pub last_scan_id: Option<Uuid>,
    pub last_scanned_at: Option<DateTime<Utc>>,
    pub tldr: String,
    pub stack: Vec<String>,
    pub runs_synced_at: Option<DateTime<Utc>>,
    pub deployments_synced_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Delivery maturity 0–100; 0 until the first successful scan.
    pub maturity_score: i32,
    /// True when the last scan found a `.sdi.yml`.
    pub has_config: bool,
    /// Computed: unanswered questions, so list views need no second query.
    pub open_questions: i64,
}

#[derive(Debug, Clone)]
pub struct NewProject {
    pub github_repo_id: Option<i64>,
    pub name: String,
    pub repo_full_name: String,
    pub repo_url: String,
    pub default_branch: String,
    pub is_private: bool,
    pub description: String,
    pub primary_language: String,
}

const SELECT: &str = "SELECT p.*, (SELECT count(*) FROM questions q WHERE q.project_id = p.id AND q.answered_at IS NULL) AS open_questions FROM projects p";

pub async fn insert(exec: impl PgExecutor<'_>, new: &NewProject) -> sqlx::Result<ProjectRow> {
    let id = Uuid::now_v7();
    sqlx::query_as::<_, ProjectRow>(
        "INSERT INTO projects (id, github_repo_id, name, repo_full_name, repo_url, default_branch, is_private, description, primary_language, status, mode)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'pending', 'observe')
         RETURNING *, 0::bigint AS open_questions",
    )
    .bind(id)
    .bind(new.github_repo_id)
    .bind(&new.name)
    .bind(&new.repo_full_name)
    .bind(&new.repo_url)
    .bind(&new.default_branch)
    .bind(new.is_private)
    .bind(&new.description)
    .bind(&new.primary_language)
    .fetch_one(exec)
    .await
}

pub async fn list(exec: impl PgExecutor<'_>) -> sqlx::Result<Vec<ProjectRow>> {
    sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!(
        "{SELECT} ORDER BY p.created_at DESC"
    )))
    .fetch_all(exec)
    .await
}

pub async fn get(exec: impl PgExecutor<'_>, id: Uuid) -> sqlx::Result<ProjectRow> {
    sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("{SELECT} WHERE p.id = $1")))
        .bind(id)
        .fetch_one(exec)
        .await
}

pub async fn get_by_repo(
    exec: impl PgExecutor<'_>,
    repo_full_name: &str,
) -> sqlx::Result<Option<ProjectRow>> {
    sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE lower(p.repo_full_name) = lower($1)"
    )))
    .bind(repo_full_name)
    .fetch_optional(exec)
    .await
}

/// Returns whether a row was deleted. Everything else cascades.
pub async fn delete(exec: impl PgExecutor<'_>, id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(exec)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn set_mode(exec: impl PgExecutor<'_>, id: Uuid, mode: &str) -> sqlx::Result<ProjectRow> {
    sqlx::query_as::<_, ProjectRow>(
        "UPDATE projects SET mode = $2, updated_at = now() WHERE id = $1
         RETURNING *, (SELECT count(*) FROM questions q WHERE q.project_id = projects.id AND q.answered_at IS NULL) AS open_questions",
    )
    .bind(id)
    .bind(mode)
    .fetch_one(exec)
    .await
}

pub async fn set_status(exec: impl PgExecutor<'_>, id: Uuid, status: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE projects SET status = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(exec)
        .await?;
    Ok(())
}

/// Refreshes what the repository metadata said at scan time.
pub async fn set_metadata(
    exec: impl PgExecutor<'_>,
    id: Uuid,
    description: &str,
    default_branch: &str,
    primary_language: &str,
    is_private: bool,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE projects SET description = $2, default_branch = $3, primary_language = $4, is_private = $5, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(description)
    .bind(default_branch)
    .bind(primary_language)
    .bind(is_private)
    .execute(exec)
    .await?;
    Ok(())
}

/// What a successful scan leaves on the project row.
#[derive(Debug, Clone, Default)]
pub struct ScanOutcome<'a> {
    pub tldr: &'a str,
    pub stack: &'a [String],
    pub maturity_score: u32,
    pub has_config: bool,
}

pub async fn finish_scan(
    exec: impl PgExecutor<'_>,
    id: Uuid,
    scan_id: Uuid,
    outcome: &ScanOutcome<'_>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE projects SET status = 'ready', last_scan_id = $2, last_scanned_at = now(), tldr = $3, stack = $4,
                maturity_score = $5, has_config = $6, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(scan_id)
    .bind(outcome.tldr)
    .bind(outcome.stack)
    .bind(outcome.maturity_score.min(100) as i32)
    .bind(outcome.has_config)
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn set_runs_synced(exec: impl PgExecutor<'_>, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE projects SET runs_synced_at = now() WHERE id = $1")
        .bind(id)
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn set_deployments_synced(exec: impl PgExecutor<'_>, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE projects SET deployments_synced_at = now() WHERE id = $1")
        .bind(id)
        .execute(exec)
        .await?;
    Ok(())
}
