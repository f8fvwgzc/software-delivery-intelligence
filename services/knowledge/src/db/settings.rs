//! Scanner limits, one row per workspace, seeded lazily with the defaults
//! the first time a workspace reads them. The scanner reads the row when a
//! scan starts, so a change applies to the next scan and never to one in
//! flight.

use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct SettingsRow {
    pub scan_max_files: i32,
    pub scan_max_file_bytes: i32,
    pub scan_concurrency: i32,
    pub pipeline_runs_limit: i32,
    pub cache_github: bool,
}

impl Default for SettingsRow {
    /// The values the table seeds; used when the row cannot be read so a
    /// scan still runs with sane limits.
    fn default() -> Self {
        Self {
            scan_max_files: 150,
            scan_max_file_bytes: 200_000,
            scan_concurrency: 6,
            pipeline_runs_limit: 50,
            cache_github: true,
        }
    }
}

const COLUMNS: &str =
    "scan_max_files, scan_max_file_bytes, scan_concurrency, pipeline_runs_limit, cache_github";

/// The workspace's settings, creating the row with the defaults when it is
/// read for the first time (one statement, so concurrent first reads agree).
pub async fn get(exec: impl PgExecutor<'_>, workspace_id: Uuid) -> sqlx::Result<SettingsRow> {
    sqlx::query_as::<_, SettingsRow>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO workspace_settings (workspace_id) VALUES ($1)
         ON CONFLICT (workspace_id) DO UPDATE SET workspace_id = EXCLUDED.workspace_id
         RETURNING {COLUMNS}"
    )))
    .bind(workspace_id)
    .fetch_one(exec)
    .await
}

pub async fn update(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    settings: &SettingsRow,
) -> sqlx::Result<SettingsRow> {
    sqlx::query_as::<_, SettingsRow>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO workspace_settings (workspace_id, {COLUMNS}, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, now())
         ON CONFLICT (workspace_id) DO UPDATE SET
           scan_max_files = EXCLUDED.scan_max_files, scan_max_file_bytes = EXCLUDED.scan_max_file_bytes,
           scan_concurrency = EXCLUDED.scan_concurrency, pipeline_runs_limit = EXCLUDED.pipeline_runs_limit,
           cache_github = EXCLUDED.cache_github, updated_at = now()
         RETURNING {COLUMNS}"
    )))
    .bind(workspace_id)
    .bind(settings.scan_max_files)
    .bind(settings.scan_max_file_bytes)
    .bind(settings.scan_concurrency)
    .bind(settings.pipeline_runs_limit)
    .bind(settings.cache_github)
    .fetch_one(exec)
    .await
}
