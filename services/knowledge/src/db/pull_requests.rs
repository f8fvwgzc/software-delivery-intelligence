//! Pull request caches (CONTRACT.md §20): the summaries GitHub lists for a
//! project and, per head commit, the changed files with their diff hunks.
//!
//! Summaries are metadata and live until the project is disconnected. The
//! file rows hold code and follow the data controls (§19): [`get_files`]
//! treats an expired row as a miss, [`sweep_expired`] reclaims them with the
//! code cache, and [`purge`] drops them on demand. A `code_cache = none`
//! workspace never writes a file row at all.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgExecutor, types::Json};
use uuid::Uuid;

use crate::domain::diff::Hunk;

#[derive(Debug, Clone, FromRow)]
pub struct PullRequestRow {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub number: i64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub author_avatar_url: String,
    /// "open" | "closed" | "merged" | "draft"
    pub state: String,
    pub base_ref: String,
    pub head_ref: String,
    pub head_sha: String,
    pub changed_files: i32,
    pub additions: i32,
    pub deletions: i32,
    pub html_url: String,
    pub pr_created_at: Option<DateTime<Utc>>,
    pub pr_updated_at: Option<DateTime<Utc>>,
    pub synced_at: DateTime<Utc>,
}

/// What one GitHub pull request becomes in the cache.
#[derive(Debug, Clone, Default)]
pub struct NewPullRequest {
    pub number: i64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub author_avatar_url: String,
    pub state: String,
    pub base_ref: String,
    pub head_ref: String,
    pub head_sha: String,
    pub changed_files: i32,
    pub additions: i32,
    pub deletions: i32,
    pub html_url: String,
    pub pr_created_at: Option<DateTime<Utc>>,
    pub pr_updated_at: Option<DateTime<Utc>>,
}

/// One changed file as stored; `hunks` are empty when `truncated`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: String,
    #[serde(default)]
    pub previous_path: String,
    /// "added" | "modified" | "removed" | "renamed"
    pub status: String,
    #[serde(default)]
    pub additions: u32,
    #[serde(default)]
    pub deletions: u32,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub hunks: Vec<Hunk>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct FilesRow {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub number: i64,
    pub head_sha: String,
    pub files: Json<Vec<FileRecord>>,
    /// The file list itself was cut at the limit.
    pub truncated: bool,
    pub fetched_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// The states a list filter stands for.
pub fn states_for_filter(filter: &str) -> &'static [&'static str] {
    match filter {
        "closed" => &["closed", "merged"],
        "all" => &["open", "draft", "closed", "merged"],
        _ => &["open", "draft"],
    }
}

/// Upserts the fetched summaries. `synced_at` moves on every row touched.
pub async fn upsert_all(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    project_id: Uuid,
    pulls: &[NewPullRequest],
) -> sqlx::Result<()> {
    for pull in pulls {
        upsert(&mut *conn, workspace_id, project_id, pull).await?;
    }
    Ok(())
}

pub async fn upsert(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    pull: &NewPullRequest,
) -> sqlx::Result<PullRequestRow> {
    sqlx::query_as::<_, PullRequestRow>(
        "INSERT INTO pull_requests (workspace_id, project_id, number, title, body, author, author_avatar_url, state,
            base_ref, head_ref, head_sha, changed_files, additions, deletions, html_url, pr_created_at, pr_updated_at, synced_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, now())
         ON CONFLICT (project_id, number) DO UPDATE SET
           title = EXCLUDED.title, body = EXCLUDED.body, author = EXCLUDED.author,
           author_avatar_url = EXCLUDED.author_avatar_url, state = EXCLUDED.state, base_ref = EXCLUDED.base_ref,
           head_ref = EXCLUDED.head_ref, head_sha = EXCLUDED.head_sha, changed_files = EXCLUDED.changed_files,
           additions = EXCLUDED.additions, deletions = EXCLUDED.deletions, html_url = EXCLUDED.html_url,
           pr_created_at = EXCLUDED.pr_created_at, pr_updated_at = EXCLUDED.pr_updated_at, synced_at = now()
         RETURNING *",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(pull.number)
    .bind(&pull.title)
    .bind(&pull.body)
    .bind(&pull.author)
    .bind(&pull.author_avatar_url)
    .bind(&pull.state)
    .bind(&pull.base_ref)
    .bind(&pull.head_ref)
    .bind(&pull.head_sha)
    .bind(pull.changed_files)
    .bind(pull.additions)
    .bind(pull.deletions)
    .bind(&pull.html_url)
    .bind(pull.pr_created_at)
    .bind(pull.pr_updated_at)
    .fetch_one(exec)
    .await
}

/// After a complete sync of the open set: whatever is still cached as open
/// but was not listed has been closed meanwhile. Returns rows changed.
pub async fn close_missing_open(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    still_open: &[i64],
) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "UPDATE pull_requests SET state = 'closed', synced_at = now()
          WHERE workspace_id = $1 AND project_id = $2 AND state IN ('open', 'draft') AND NOT (number = ANY($3))",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(still_open)
    .execute(exec)
    .await?;
    Ok(result.rows_affected())
}

/// Summaries of the project in one of `states`, most recently updated first.
pub async fn list(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    states: &[&str],
    limit: i64,
) -> sqlx::Result<Vec<PullRequestRow>> {
    let states: Vec<String> = states.iter().map(|s| (*s).to_owned()).collect();
    sqlx::query_as::<_, PullRequestRow>(
        "SELECT * FROM pull_requests WHERE workspace_id = $1 AND project_id = $2 AND state = ANY($3)
         ORDER BY pr_updated_at DESC NULLS LAST, number DESC LIMIT $4",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(&states)
    .bind(limit)
    .fetch_all(exec)
    .await
}

pub async fn get(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    number: i64,
) -> sqlx::Result<Option<PullRequestRow>> {
    sqlx::query_as::<_, PullRequestRow>(
        "SELECT * FROM pull_requests WHERE workspace_id = $1 AND project_id = $2 AND number = $3",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(number)
    .fetch_optional(exec)
    .await
}

/// When the project's summaries were last synced, if ever.
pub async fn last_synced(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
) -> sqlx::Result<Option<DateTime<Utc>>> {
    sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        "SELECT max(synced_at) FROM pull_requests WHERE workspace_id = $1 AND project_id = $2",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_one(exec)
    .await
}

/// The cached files at `head_sha`, unless expired.
pub async fn get_files(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    number: i64,
    head_sha: &str,
) -> sqlx::Result<Option<FilesRow>> {
    sqlx::query_as::<_, FilesRow>(
        "SELECT * FROM pull_request_files
          WHERE workspace_id = $1 AND project_id = $2 AND number = $3 AND head_sha = $4
            AND (expires_at IS NULL OR expires_at > now())",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(number)
    .bind(head_sha)
    .fetch_optional(exec)
    .await
}

/// What [`upsert_files`] stores for one head commit.
#[derive(Debug, Clone)]
pub struct NewFiles<'a> {
    pub number: i64,
    pub head_sha: &'a str,
    pub files: &'a [FileRecord],
    /// The file list was cut at the limit.
    pub truncated: bool,
    /// `None` for a persistent cache.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Stores the files at `head_sha`; older head commits of the same pull
/// request are dropped, so a PR holds at most its latest diff.
pub async fn upsert_files(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    project_id: Uuid,
    new: &NewFiles<'_>,
) -> sqlx::Result<FilesRow> {
    let NewFiles {
        number,
        head_sha,
        files,
        truncated,
        expires_at,
    } = *new;
    sqlx::query(
        "DELETE FROM pull_request_files WHERE workspace_id = $1 AND project_id = $2 AND number = $3 AND head_sha <> $4",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(number)
    .bind(head_sha)
    .execute(&mut *conn)
    .await?;
    sqlx::query_as::<_, FilesRow>(
        "INSERT INTO pull_request_files (workspace_id, project_id, number, head_sha, files, truncated, fetched_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, now(), $7)
         ON CONFLICT (project_id, number, head_sha) DO UPDATE SET
           files = EXCLUDED.files, truncated = EXCLUDED.truncated, fetched_at = now(), expires_at = EXCLUDED.expires_at
         RETURNING *",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(number)
    .bind(head_sha)
    .bind(Json(files))
    .bind(truncated)
    .bind(expires_at)
    .fetch_one(&mut *conn)
    .await
}

/// Expired diff rows of every workspace; the sweeper's job.
pub async fn sweep_expired(exec: impl PgExecutor<'_>) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM pull_request_files WHERE expires_at IS NOT NULL AND expires_at <= now()",
    )
    .execute(exec)
    .await?;
    Ok(result.rows_affected())
}

/// Drops the diff cache of one project, or of the whole workspace.
pub async fn purge(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM pull_request_files WHERE workspace_id = $1 AND ($2::uuid IS NULL OR project_id = $2)",
    )
    .bind(workspace_id)
    .bind(project_id)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_map_to_states() {
        assert_eq!(states_for_filter("open"), &["open", "draft"]);
        assert_eq!(states_for_filter(""), &["open", "draft"]);
        assert_eq!(states_for_filter("closed"), &["closed", "merged"]);
        assert_eq!(states_for_filter("all").len(), 4);
    }

    #[test]
    fn file_records_round_trip_as_json() {
        let record = FileRecord {
            path: "src/main.rs".into(),
            status: "modified".into(),
            additions: 1,
            hunks: vec![Hunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 2,
                lines: vec![" a".into(), "+b".into()],
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&record).unwrap();
        let back: FileRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, record);
        let sparse: FileRecord = serde_json::from_str(r#"{"path":"x","status":"added"}"#).unwrap();
        assert!(sparse.hunks.is_empty());
        assert!(!sparse.truncated);
    }
}
