//! Scanner limits. A single row (`id = 1`) seeded by the migration; the
//! scanner reads it when a scan starts, so a change applies to the next scan
//! and never to one in flight.

use sqlx::{FromRow, PgExecutor};

#[derive(Debug, Clone, FromRow)]
pub struct SettingsRow {
    pub scan_max_files: i32,
    pub scan_max_file_bytes: i32,
    pub scan_concurrency: i32,
    pub pipeline_runs_limit: i32,
    pub cache_github: bool,
}

impl Default for SettingsRow {
    /// The values the migration seeds; used when the row cannot be read so a
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

pub async fn get(exec: impl PgExecutor<'_>) -> sqlx::Result<SettingsRow> {
    sqlx::query_as::<_, SettingsRow>(
        "SELECT scan_max_files, scan_max_file_bytes, scan_concurrency, pipeline_runs_limit, cache_github
         FROM settings WHERE id = 1",
    )
    .fetch_one(exec)
    .await
}

pub async fn update(
    exec: impl PgExecutor<'_>,
    settings: &SettingsRow,
) -> sqlx::Result<SettingsRow> {
    sqlx::query_as::<_, SettingsRow>(
        "UPDATE settings SET scan_max_files = $1, scan_max_file_bytes = $2, scan_concurrency = $3,
                pipeline_runs_limit = $4, cache_github = $5, updated_at = now()
         WHERE id = 1
         RETURNING scan_max_files, scan_max_file_bytes, scan_concurrency, pipeline_runs_limit, cache_github",
    )
    .bind(settings.scan_max_files)
    .bind(settings.scan_max_file_bytes)
    .bind(settings.scan_concurrency)
    .bind(settings.pipeline_runs_limit)
    .bind(settings.cache_github)
    .fetch_one(exec)
    .await
}
