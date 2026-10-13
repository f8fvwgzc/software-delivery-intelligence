//! What the pull request autopilot keeps (CONTRACT.md §23): the executions
//! it asked for (commit fixes, merge, turn Dependabot on) and what the
//! watcher last saw in each workspace. Both are tenant tables: every
//! function takes the workspace, and the export and the delete of a
//! workspace walk them like every other table.

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

pub const KIND_FIX: &str = "fix";
pub const KIND_MERGE: &str = "merge";
pub const KIND_DEPENDABOT: &str = "dependabot";

#[derive(Debug, Clone, FromRow)]
pub struct AutopilotExecutionRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: String,
    /// "fix" | "merge" | "dependabot"
    pub kind: String,
    /// 0 when the execution is not about one pull request.
    pub pr_number: i64,
    pub review_id: Option<Uuid>,
    pub head_sha: String,
    pub execution_id: String,
    pub requested_by: String,
    /// "" while it may still run; "succeeded" | "failed" | "cancelled" | "gone".
    pub outcome: String,
    pub created_at: DateTime<Utc>,
}

impl AutopilotExecutionRow {
    /// The execution service already told us this one ended.
    pub fn settled(&self) -> bool {
        !self.outcome.is_empty()
    }
}

pub async fn insert(pool: &PgPool, row: &AutopilotExecutionRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO autopilot_executions (id, workspace_id, project_id, kind, pr_number, review_id, head_sha, \
         execution_id, requested_by, outcome, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(row.id)
    .bind(row.workspace_id)
    .bind(&row.project_id)
    .bind(&row.kind)
    .bind(row.pr_number)
    .bind(row.review_id)
    .bind(&row.head_sha)
    .bind(&row.execution_id)
    .bind(&row.requested_by)
    .bind(&row.outcome)
    .bind(row.created_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// The newest execution of `kind` for the pull request (`pr_number` 0 for
/// the project-level Dependabot setup), whatever head it was asked for.
pub async fn latest(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: &str,
    kind: &str,
    pr_number: i64,
) -> sqlx::Result<Option<AutopilotExecutionRow>> {
    sqlx::query_as(
        "SELECT * FROM autopilot_executions \
         WHERE workspace_id = $1 AND project_id = $2 AND kind = $3 AND pr_number = $4 \
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(kind)
    .bind(pr_number)
    .fetch_optional(pool)
    .await
}

/// The row of one recorded execution, by the execution service's id.
pub async fn by_execution(
    pool: &PgPool,
    workspace_id: Uuid,
    execution_id: &str,
) -> sqlx::Result<Option<AutopilotExecutionRow>> {
    sqlx::query_as(
        "SELECT * FROM autopilot_executions WHERE workspace_id = $1 AND execution_id = $2 \
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(workspace_id)
    .bind(execution_id)
    .fetch_optional(pool)
    .await
}

/// Head commits of these pull requests that already have a merge
/// execution, whatever became of it: the watcher asks once per head.
pub async fn merge_requested_heads(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: &str,
    pr_numbers: &[i64],
) -> sqlx::Result<Vec<(i64, String)>> {
    if pr_numbers.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as(
        "SELECT pr_number, head_sha FROM autopilot_executions \
         WHERE workspace_id = $1 AND project_id = $2 AND kind = 'merge' AND pr_number = ANY($3)",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(pr_numbers)
    .fetch_all(pool)
    .await
}

/// Remembers how an execution ended so nobody has to ask again.
pub async fn set_outcome(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    outcome: &str,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE autopilot_executions SET outcome = $3 WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .bind(outcome)
        .execute(pool)
        .await?;
    Ok(())
}

// --- Watcher state -----------------------------------------------------------

#[derive(Debug, Clone, Default, FromRow)]
pub struct WatchStateRow {
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_error: String,
}

pub async fn watch_state(pool: &PgPool, workspace_id: Uuid) -> sqlx::Result<WatchStateRow> {
    Ok(
        sqlx::query_as(
            "SELECT last_run_at, last_error FROM pr_watch_state WHERE workspace_id = $1",
        )
        .bind(workspace_id)
        .fetch_optional(pool)
        .await?
        .unwrap_or_default(),
    )
}

/// Records one look at a workspace: when, and why it failed ('' = it worked).
pub async fn record_watch(pool: &PgPool, workspace_id: Uuid, error: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO pr_watch_state (workspace_id, last_run_at, last_error, updated_at) \
         VALUES ($1, now(), $2, now()) \
         ON CONFLICT (workspace_id) DO UPDATE SET last_run_at = now(), last_error = EXCLUDED.last_error, \
         updated_at = now()",
    )
    .bind(workspace_id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}
