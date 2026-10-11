//! Code browser caches (CONTRACT.md §15): the recursive tree and single
//! files at the scanned commit. Rows are keyed by `(project, commit)` so a
//! rescan at a new commit never serves stale content, and [`prune`] keeps
//! only the latest commit's rows once that scan succeeded.

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
    pub project_id: Uuid,
    pub commit_sha: String,
    pub entries: Json<Vec<TreeEntryRecord>>,
    pub truncated: bool,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct FileRow {
    pub project_id: Uuid,
    pub commit_sha: String,
    pub path: String,
    pub content: String,
    pub binary: bool,
    pub truncated: bool,
    pub size: i64,
    pub language: String,
    pub fetched_at: DateTime<Utc>,
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

pub async fn get_tree(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    commit_sha: &str,
) -> sqlx::Result<Option<TreeRow>> {
    sqlx::query_as::<_, TreeRow>(
        "SELECT * FROM repo_trees WHERE project_id = $1 AND commit_sha = $2",
    )
    .bind(project_id)
    .bind(commit_sha)
    .fetch_optional(exec)
    .await
}

pub async fn upsert_tree(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    commit_sha: &str,
    entries: &[TreeEntryRecord],
    truncated: bool,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO repo_trees (project_id, commit_sha, entries, truncated, fetched_at)
         VALUES ($1, $2, $3, $4, now())
         ON CONFLICT (project_id, commit_sha) DO UPDATE SET
           entries = EXCLUDED.entries, truncated = EXCLUDED.truncated, fetched_at = now()",
    )
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
    project_id: Uuid,
    commit_sha: &str,
    path: &str,
) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar::<_, i64>(
        "SELECT (e->>'size')::bigint
           FROM repo_trees t, jsonb_array_elements(t.entries) e
          WHERE t.project_id = $1 AND t.commit_sha = $2 AND e->>'path' = $3
          LIMIT 1",
    )
    .bind(project_id)
    .bind(commit_sha)
    .bind(path)
    .fetch_optional(exec)
    .await
}

pub async fn get_file(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    commit_sha: &str,
    path: &str,
) -> sqlx::Result<Option<FileRow>> {
    sqlx::query_as::<_, FileRow>(
        "SELECT * FROM repo_files WHERE project_id = $1 AND commit_sha = $2 AND path = $3",
    )
    .bind(project_id)
    .bind(commit_sha)
    .bind(path)
    .fetch_optional(exec)
    .await
}

pub async fn upsert_file(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    file: &NewFile<'_>,
) -> sqlx::Result<FileRow> {
    sqlx::query_as::<_, FileRow>(
        "INSERT INTO repo_files (project_id, commit_sha, path, content, \"binary\", truncated, size, language, fetched_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
         ON CONFLICT (project_id, commit_sha, path) DO UPDATE SET
           content = EXCLUDED.content, \"binary\" = EXCLUDED.\"binary\", truncated = EXCLUDED.truncated,
           size = EXCLUDED.size, language = EXCLUDED.language, fetched_at = now()
         RETURNING *",
    )
    .bind(project_id)
    .bind(file.commit_sha)
    .bind(file.path)
    .bind(file.content)
    .bind(file.binary)
    .bind(file.truncated)
    .bind(i64::try_from(file.size).unwrap_or(i64::MAX))
    .bind(file.language)
    .fetch_one(exec)
    .await
}

/// Drops every cached row of the project that is not at `commit_sha`, so
/// the caches hold at most the latest scanned commit. Returns rows removed.
pub async fn prune(
    conn: &mut PgConnection,
    project_id: Uuid,
    commit_sha: &str,
) -> sqlx::Result<u64> {
    let trees = sqlx::query("DELETE FROM repo_trees WHERE project_id = $1 AND commit_sha <> $2")
        .bind(project_id)
        .bind(commit_sha)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    let files = sqlx::query("DELETE FROM repo_files WHERE project_id = $1 AND commit_sha <> $2")
        .bind(project_id)
        .bind(commit_sha)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    Ok(trees + files)
}
