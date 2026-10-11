//! Code browser caches (CONTRACT.md §15): the recursive tree and single
//! files at the scanned commit. Rows are keyed by `(project, commit)` so a
//! rescan at a new commit never serves stale content, and [`prune`] keeps
//! only the latest commit's rows once that scan succeeded.
//!
//! Data controls (§19): a file row may carry `expires_at` (`code_cache =
//! session`); [`get_file`] treats an expired row as a miss and
//! [`sweep_expired`] deletes them in the background. `code_cache = none`
//! never writes a file row at all; [`purge`] drops the caches on demand.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgExecutor, types::Json};
use uuid::Uuid;

/// One entry of the stored tree. Name and language are derived from the
/// path on read, so a better language table applies to cached trees too.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeEntryRecord {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, FromRow)]
pub struct TreeRow {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub commit_sha: String,
    pub entries: Json<Vec<TreeEntryRecord>>,
    pub truncated: bool,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct FileRow {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub commit_sha: String,
    pub path: String,
    pub content: String,
    pub binary: bool,
    pub truncated: bool,
    pub size: i64,
    pub language: String,
    pub fetched_at: DateTime<Utc>,
    /// `None` keeps the row until the next scan or a purge.
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewFile<'a> {
    pub commit_sha: &'a str,
    pub path: &'a str,
    pub content: &'a str,
    pub binary: bool,
    pub truncated: bool,
    pub size: u64,
    pub language: &'a str,
}

/// What [`purge`] removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Purged {
    pub files: u64,
    pub trees: u64,
}

pub async fn get_tree(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    commit_sha: &str,
) -> sqlx::Result<Option<TreeRow>> {
    sqlx::query_as::<_, TreeRow>(
        "SELECT * FROM repo_trees WHERE workspace_id = $1 AND project_id = $2 AND commit_sha = $3",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(commit_sha)
    .fetch_optional(exec)
    .await
}

pub async fn upsert_tree(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    commit_sha: &str,
    entries: &[TreeEntryRecord],
    truncated: bool,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO repo_trees (workspace_id, project_id, commit_sha, entries, truncated, fetched_at)
         VALUES ($1, $2, $3, $4, $5, now())
         ON CONFLICT (project_id, commit_sha) DO UPDATE SET
           entries = EXCLUDED.entries, truncated = EXCLUDED.truncated, fetched_at = now()",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(commit_sha)
    .bind(Json(entries))
    .bind(truncated)
    .execute(exec)
    .await?;
    Ok(())
}

/// Size GitHub reported for `path` in the cached tree, when the tree is
/// cached and lists the file. Cheaper than fetching the tree to size one
/// truncated file.
pub async fn tree_entry_size(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    commit_sha: &str,
    path: &str,
) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar::<_, i64>(
        "SELECT (e->>'size')::bigint
           FROM repo_trees t, jsonb_array_elements(t.entries) e
          WHERE t.workspace_id = $1 AND t.project_id = $2 AND t.commit_sha = $3 AND e->>'path' = $4
          LIMIT 1",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(commit_sha)
    .bind(path)
    .fetch_optional(exec)
    .await
}

/// A cached file that has not expired. An expired row the sweeper has not
/// reached yet is a miss, never served.
pub async fn get_file(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    commit_sha: &str,
    path: &str,
) -> sqlx::Result<Option<FileRow>> {
    sqlx::query_as::<_, FileRow>(
        "SELECT * FROM repo_files
          WHERE workspace_id = $1 AND project_id = $2 AND commit_sha = $3 AND path = $4
            AND (expires_at IS NULL OR expires_at > now())",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(commit_sha)
    .bind(path)
    .fetch_optional(exec)
    .await
}

/// Stores a file; `expires_at` is `None` for a persistent cache.
pub async fn upsert_file(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    file: &NewFile<'_>,
    expires_at: Option<DateTime<Utc>>,
) -> sqlx::Result<FileRow> {
    sqlx::query_as::<_, FileRow>(
        "INSERT INTO repo_files (workspace_id, project_id, commit_sha, path, content, \"binary\", truncated, size, language, fetched_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now(), $10)
         ON CONFLICT (project_id, commit_sha, path) DO UPDATE SET
           content = EXCLUDED.content, \"binary\" = EXCLUDED.\"binary\", truncated = EXCLUDED.truncated,
           size = EXCLUDED.size, language = EXCLUDED.language, fetched_at = now(), expires_at = EXCLUDED.expires_at
         RETURNING *",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(file.commit_sha)
    .bind(file.path)
    .bind(file.content)
    .bind(file.binary)
    .bind(file.truncated)
    .bind(i64::try_from(file.size).unwrap_or(i64::MAX))
    .bind(file.language)
    .bind(expires_at)
    .fetch_one(exec)
    .await
}

/// Drops every cached row of the project that is not at `commit_sha`, so
/// the caches hold at most the latest scanned commit. Returns rows removed.
pub async fn prune(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    project_id: Uuid,
    commit_sha: &str,
) -> sqlx::Result<u64> {
    let trees = sqlx::query(
        "DELETE FROM repo_trees WHERE workspace_id = $1 AND project_id = $2 AND commit_sha <> $3",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(commit_sha)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    let files = sqlx::query(
        "DELETE FROM repo_files WHERE workspace_id = $1 AND project_id = $2 AND commit_sha <> $3",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(commit_sha)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(trees + files)
}

/// Drops the file cache (and the trees) of one project, or of the whole
/// workspace when `project_id` is `None`.
pub async fn purge(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
) -> sqlx::Result<Purged> {
    let files = sqlx::query(
        "DELETE FROM repo_files WHERE workspace_id = $1 AND ($2::uuid IS NULL OR project_id = $2)",
    )
    .bind(workspace_id)
    .bind(project_id)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    let trees = sqlx::query(
        "DELETE FROM repo_trees WHERE workspace_id = $1 AND ($2::uuid IS NULL OR project_id = $2)",
    )
    .bind(workspace_id)
    .bind(project_id)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(Purged { files, trees })
}

/// Deletes every expired file row of every workspace; the background
/// sweeper's job, so it is the one code-cache function without a workspace.
pub async fn sweep_expired(exec: impl PgExecutor<'_>) -> sqlx::Result<u64> {
    let result =
        sqlx::query("DELETE FROM repo_files WHERE expires_at IS NOT NULL AND expires_at <= now()")
            .execute(exec)
            .await?;
    Ok(result.rows_affected())
}
