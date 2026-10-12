use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use crate::db::Tx;

#[derive(Debug, Clone, FromRow)]
pub struct RunRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: String,
    pub environment_id: Uuid,
    pub environment_name: String,
    pub release_id: Uuid,
    pub release_version: String,
    pub kind: String,
    pub status: String,
    pub execution_id: Uuid,
    pub external_url: String,
    pub previous_release_id: Option<Uuid>,
    pub started_by: String,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

const COLUMNS: &str = "id, workspace_id, project_id, environment_id, environment_name, release_id, release_version, \
     kind, status, execution_id, external_url, previous_release_id, started_by, created_at, finished_at";

pub struct NewRun<'a> {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: &'a str,
    pub environment_id: Uuid,
    pub environment_name: &'a str,
    pub release_id: Uuid,
    pub release_version: &'a str,
    pub kind: &'a str,
    pub status: &'a str,
    pub execution_id: Uuid,
    pub previous_release_id: Option<Uuid>,
    pub started_by: &'a str,
    pub reason: &'a str,
}

pub async fn insert(tx: &mut Tx, new: NewRun<'_>) -> sqlx::Result<RunRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO deployment_runs (id, workspace_id, project_id, environment_id, environment_name, \
           release_id, release_version, kind, status, execution_id, previous_release_id, started_by, reason) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) RETURNING {COLUMNS}"
    )))
    .bind(new.id)
    .bind(new.workspace_id)
    .bind(new.project_id)
    .bind(new.environment_id)
    .bind(new.environment_name)
    .bind(new.release_id)
    .bind(new.release_version)
    .bind(new.kind)
    .bind(new.status)
    .bind(new.execution_id)
    .bind(new.previous_release_id)
    .bind(new.started_by)
    .bind(new.reason)
    .fetch_one(&mut **tx)
    .await
}

pub async fn get<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    id: Uuid,
) -> sqlx::Result<RunRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM deployment_runs WHERE workspace_id = $1 AND id = $2"
    )))
    .bind(workspace_id)
    .bind(id)
    .fetch_one(exec)
    .await
}

pub async fn by_execution<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    execution_id: Uuid,
) -> sqlx::Result<Option<RunRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM deployment_runs WHERE workspace_id = $1 AND execution_id = $2"
    )))
    .bind(workspace_id)
    .bind(execution_id)
    .fetch_optional(exec)
    .await
}

/// The run an execution carries, locked for the projection.
pub async fn lock_by_execution(
    tx: &mut Tx,
    workspace_id: Uuid,
    execution_id: Uuid,
) -> sqlx::Result<Option<RunRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM deployment_runs WHERE workspace_id = $1 AND execution_id = $2 FOR UPDATE"
    )))
    .bind(workspace_id)
    .bind(execution_id)
    .fetch_optional(&mut **tx)
    .await
}

/// Newest first; `environment_id` None lists the whole project; `before`
/// is the cursor (created strictly before it).
pub async fn list<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    project_id: &str,
    environment_id: Option<Uuid>,
    limit: i64,
    before: Option<DateTime<Utc>>,
) -> sqlx::Result<Vec<RunRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM deployment_runs WHERE workspace_id = $1 AND project_id = $2 \
           AND ($3::uuid IS NULL OR environment_id = $3) \
           AND ($5::timestamptz IS NULL OR created_at < $5) \
         ORDER BY created_at DESC, id DESC LIMIT $4"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .bind(environment_id)
    .bind(limit)
    .bind(before)
    .fetch_all(exec)
    .await
}

/// The deployment to an environment that has not ended yet, if any: still
/// running, waiting for an approval, or paused on a failure.
pub async fn open_for_environment<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    environment_id: Uuid,
) -> sqlx::Result<Option<RunRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM deployment_runs \
         WHERE workspace_id = $1 AND environment_id = $2 AND finished_at IS NULL \
         ORDER BY created_at DESC, id DESC LIMIT 1"
    )))
    .bind(workspace_id)
    .bind(environment_id)
    .fetch_optional(exec)
    .await
}

/// Writes what the projection derived. `finished` stamps the end once.
pub async fn project(
    tx: &mut Tx,
    workspace_id: Uuid,
    id: Uuid,
    status: &str,
    external_url: &str,
    finished: bool,
) -> sqlx::Result<RunRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE deployment_runs SET status = $3, external_url = $4, \
           finished_at = CASE WHEN $5 THEN COALESCE(finished_at, now()) ELSE NULL END \
         WHERE workspace_id = $1 AND id = $2 RETURNING {COLUMNS}"
    )))
    .bind(workspace_id)
    .bind(id)
    .bind(status)
    .bind(external_url)
    .bind(finished)
    .fetch_one(&mut **tx)
    .await
}

/// The latest run of each environment of a project.
pub async fn latest_per_environment<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    project_id: &str,
) -> sqlx::Result<Vec<RunRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT DISTINCT ON (environment_id) {COLUMNS} FROM deployment_runs \
         WHERE workspace_id = $1 AND project_id = $2 \
         ORDER BY environment_id, created_at DESC, id DESC"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .fetch_all(exec)
    .await
}

pub async fn latest_for_environment<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    environment_id: Uuid,
) -> sqlx::Result<Option<RunRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM deployment_runs WHERE workspace_id = $1 AND environment_id = $2 \
         ORDER BY created_at DESC, id DESC LIMIT 1"
    )))
    .bind(workspace_id)
    .bind(environment_id)
    .fetch_optional(exec)
    .await
}

/// `(environment, release)` for every release that ever succeeded in an
/// environment of the project: what a later stage may take as a candidate.
pub async fn succeeded_pairs<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    project_id: &str,
) -> sqlx::Result<Vec<(Uuid, Uuid)>> {
    sqlx::query_as(
        "SELECT DISTINCT environment_id, release_id FROM deployment_runs \
         WHERE workspace_id = $1 AND project_id = $2 AND status = 'SUCCEEDED'",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_all(exec)
    .await
}

pub async fn has_succeeded<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    environment_id: Uuid,
    release_id: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM deployment_runs WHERE workspace_id = $1 AND environment_id = $2 \
           AND release_id = $3 AND status = 'SUCCEEDED')",
    )
    .bind(workspace_id)
    .bind(environment_id)
    .bind(release_id)
    .fetch_one(exec)
    .await
}

/// The release of the most recent successful deployment to the environment
/// that is not `other_than`: what a rollback goes back to.
pub async fn last_successful_release<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    environment_id: Uuid,
    other_than: Option<Uuid>,
) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT release_id FROM deployment_runs \
         WHERE workspace_id = $1 AND environment_id = $2 AND status = 'SUCCEEDED' \
           AND ($3::uuid IS NULL OR release_id <> $3) \
         ORDER BY finished_at DESC NULLS LAST, created_at DESC, id DESC LIMIT 1",
    )
    .bind(workspace_id)
    .bind(environment_id)
    .bind(other_than)
    .fetch_optional(exec)
    .await
}

#[derive(Debug, Clone, FromRow)]
pub struct StatsRow {
    /// Runs started in the last 30 days.
    pub deployments: i64,
    /// Runs that finished in the last 30 days as SUCCEEDED or FAILED.
    pub finished: i64,
    pub succeeded: i64,
}

pub async fn stats<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    project_id: &str,
) -> sqlx::Result<StatsRow> {
    sqlx::query_as(
        "SELECT \
           count(*) FILTER (WHERE created_at > now() - interval '30 days')::bigint AS deployments, \
           count(*) FILTER (WHERE finished_at > now() - interval '30 days' \
             AND status IN ('SUCCEEDED', 'FAILED'))::bigint AS finished, \
           count(*) FILTER (WHERE finished_at > now() - interval '30 days' \
             AND status = 'SUCCEEDED')::bigint AS succeeded \
         FROM deployment_runs WHERE workspace_id = $1 AND project_id = $2",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_one(exec)
    .await
}

/// Median seconds from a release being created to its first successful
/// promotion into `last_environment`, over the last 30 days.
pub async fn median_lead_time<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    project_id: &str,
    last_environment: Uuid,
) -> sqlx::Result<Option<f64>> {
    sqlx::query_scalar(
        "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY lead)::float8 FROM ( \
           SELECT EXTRACT(EPOCH FROM min(d.finished_at) - r.created_at) AS lead \
           FROM deployment_runs d JOIN releases r ON r.id = d.release_id \
           WHERE d.workspace_id = $1 AND d.project_id = $2 AND d.environment_id = $3 \
             AND d.kind = 'promote' AND d.status = 'SUCCEEDED' \
             AND d.finished_at > now() - interval '30 days' \
           GROUP BY r.id, r.created_at) leads",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(last_environment)
    .fetch_one(exec)
    .await
}
