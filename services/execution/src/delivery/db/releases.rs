use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use crate::db::Tx;

#[derive(Debug, Clone, FromRow)]
pub struct ReleaseRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: String,
    pub version: String,
    pub commit_sha: String,
    pub image: String,
    pub tag: String,
    pub digest: String,
    pub source: String,
    pub pipeline_run_id: String,
    pub notes: String,
    pub verified: bool,
    pub created_by: String,
    /// `owner/name` the release was cut from (not part of the API).
    pub repo_full_name: String,
    pub project_name: String,
    pub created_at: DateTime<Utc>,
}

const COLUMNS: &str = "id, workspace_id, project_id, version, commit_sha, image, tag, digest, source, \
     pipeline_run_id, notes, verified, created_by, repo_full_name, project_name, created_at";

pub struct NewRelease<'a> {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: &'a str,
    pub version: &'a str,
    pub commit_sha: &'a str,
    pub image: &'a str,
    pub tag: &'a str,
    pub digest: &'a str,
    pub source: &'a str,
    pub pipeline_run_id: &'a str,
    pub notes: &'a str,
    pub created_by: &'a str,
    pub repo_full_name: &'a str,
    pub project_name: &'a str,
}

/// Inserts unless the project already has a release with this tag
/// (`None`): `CreateRelease` is idempotent on (project, tag), and two
/// webhooks for the same build must not make two releases.
pub async fn insert<'e>(
    exec: impl PgExecutor<'e>,
    new: NewRelease<'_>,
) -> sqlx::Result<Option<ReleaseRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO releases (id, workspace_id, project_id, version, commit_sha, image, tag, digest, \
           source, pipeline_run_id, notes, created_by, repo_full_name, project_name) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
         ON CONFLICT (workspace_id, project_id, tag) DO NOTHING RETURNING {COLUMNS}"
    )))
    .bind(new.id)
    .bind(new.workspace_id)
    .bind(new.project_id)
    .bind(new.version)
    .bind(new.commit_sha)
    .bind(new.image)
    .bind(new.tag)
    .bind(new.digest)
    .bind(new.source)
    .bind(new.pipeline_run_id)
    .bind(new.notes)
    .bind(new.created_by)
    .bind(new.repo_full_name)
    .bind(new.project_name)
    .fetch_optional(exec)
    .await
}

pub async fn get<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    id: Uuid,
) -> sqlx::Result<ReleaseRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM releases WHERE workspace_id = $1 AND id = $2"
    )))
    .bind(workspace_id)
    .bind(id)
    .fetch_one(exec)
    .await
}

pub async fn by_tag<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    project_id: &str,
    tag: &str,
) -> sqlx::Result<Option<ReleaseRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM releases WHERE workspace_id = $1 AND project_id = $2 AND tag = $3"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .bind(tag)
    .fetch_optional(exec)
    .await
}

/// Newest first; `before` is the cursor (created strictly before it).
pub async fn list<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    project_id: &str,
    limit: i64,
    before: Option<DateTime<Utc>>,
) -> sqlx::Result<Vec<ReleaseRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM releases WHERE workspace_id = $1 AND project_id = $2 \
           AND ($4::timestamptz IS NULL OR created_at < $4) \
         ORDER BY created_at DESC, id DESC LIMIT $3"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .bind(limit)
    .bind(before)
    .fetch_all(exec)
    .await
}

/// Several releases of the workspace by id (the current ones of a pipeline).
pub async fn by_ids<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> sqlx::Result<Vec<ReleaseRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM releases WHERE workspace_id = $1 AND id = ANY($2)"
    )))
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(exec)
    .await
}

/// The newest release of the project: what the next one's notes start from.
pub async fn latest<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    project_id: &str,
) -> sqlx::Result<Option<ReleaseRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM releases WHERE workspace_id = $1 AND project_id = $2 \
         ORDER BY created_at DESC, id DESC LIMIT 1"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .fetch_optional(exec)
    .await
}

/// Records the digest a later event learned for an existing release (the
/// package webhook after the workflow webhook). A recorded digest is never
/// replaced: a release is one immutable image.
pub async fn fill_digest<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    id: Uuid,
    digest: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE releases SET digest = $3 WHERE workspace_id = $1 AND id = $2 AND digest = ''",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(digest)
    .execute(exec)
    .await
    .map(|_| ())
}

/// The registry confirmed the manifest: the release is verified and, when
/// it had no digest yet, gets the one the registry reported. True when the
/// row changed.
pub async fn mark_verified(
    tx: &mut Tx,
    workspace_id: Uuid,
    id: Uuid,
    digest: &str,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE releases SET verified = true, digest = CASE WHEN digest = '' THEN $3 ELSE digest END \
         WHERE workspace_id = $1 AND id = $2 AND (NOT verified OR (digest = '' AND $3 <> ''))",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(digest)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected() == 1)
}
