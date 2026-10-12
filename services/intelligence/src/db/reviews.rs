//! Reviews, findings and the review activity log (CONTRACT.md §20).
//! Every function takes the workspace; a review of another workspace is
//! simply not found.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

/// One step of the rail, persisted after every change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRecord {
    pub key: String,
    pub label: String,
    /// "pending" | "running" | "done" | "failed"
    pub status: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub duration_ms: u32,
}

#[derive(Debug, Clone, FromRow)]
pub struct ReviewRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: String,
    pub project_name: String,
    pub repo_full_name: String,
    pub pr_number: i64,
    pub pr_title: String,
    pub pr_author: String,
    pub head_sha: String,
    pub base_ref: String,
    /// "context" | "analyzing" | "proposed" | "published" | "failed"
    pub status: String,
    pub steps: Json<Vec<StepRecord>>,
    pub summary: String,
    pub files_reviewed: i32,
    pub findings_total: i32,
    pub findings_accepted: i32,
    pub reasoner: String,
    pub published_url: String,
    pub error: String,
    pub requested_by: String,
    pub previous_review_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct FindingRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub review_id: Uuid,
    pub position: i32,
    pub category: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub path: String,
    pub line: i32,
    pub end_line: i32,
    pub suggestion: String,
    pub rule: String,
    /// "pending" | "accepted" | "dismissed"
    pub decision: String,
    pub decision_note: String,
    pub decided_by: String,
    pub decided_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ActivityRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub review_id: Uuid,
    pub project_id: String,
    pub project_name: String,
    pub pr_number: i64,
    pub action: String,
    pub actor: String,
    pub detail: String,
    pub at: DateTime<Utc>,
}

const COLUMNS: &str = "id, workspace_id, project_id, project_name, repo_full_name, pr_number, pr_title, pr_author, head_sha, \
    base_ref, status, steps, summary, files_reviewed, findings_total, findings_accepted, reasoner, published_url, error, \
    requested_by, previous_review_id, created_at, updated_at";

/// Summary and steps are left out of list rows (steps stay: the rail needs them).
const LIST_COLUMNS: &str = "id, workspace_id, project_id, project_name, repo_full_name, pr_number, pr_title, pr_author, head_sha, \
    base_ref, status, steps, '' AS summary, files_reviewed, findings_total, findings_accepted, reasoner, published_url, error, \
    requested_by, previous_review_id, created_at, updated_at";

pub async fn insert(pool: &PgPool, row: &ReviewRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO reviews (id, workspace_id, project_id, project_name, repo_full_name, pr_number, pr_title, pr_author, \
         head_sha, base_ref, status, steps, summary, files_reviewed, findings_total, findings_accepted, reasoner, \
         published_url, error, requested_by, previous_review_id, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)",
    )
    .bind(row.id)
    .bind(row.workspace_id)
    .bind(&row.project_id)
    .bind(&row.project_name)
    .bind(&row.repo_full_name)
    .bind(row.pr_number)
    .bind(&row.pr_title)
    .bind(&row.pr_author)
    .bind(&row.head_sha)
    .bind(&row.base_ref)
    .bind(&row.status)
    .bind(&row.steps)
    .bind(&row.summary)
    .bind(row.files_reviewed)
    .bind(row.findings_total)
    .bind(row.findings_accepted)
    .bind(&row.reasoner)
    .bind(&row.published_url)
    .bind(&row.error)
    .bind(&row.requested_by)
    .bind(&row.previous_review_id)
    .bind(row.created_at)
    .bind(row.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> sqlx::Result<ReviewRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM reviews WHERE workspace_id = $1 AND id = $2"
    )))
    .bind(workspace_id)
    .bind(id)
    .fetch_one(pool)
    .await
}

/// The review of this exact head commit, if one exists.
pub async fn find_by_head(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: &str,
    pr_number: i64,
    head_sha: &str,
) -> sqlx::Result<Option<ReviewRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM reviews WHERE workspace_id = $1 AND project_id = $2 AND pr_number = $3 AND head_sha = $4"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .bind(pr_number)
    .bind(head_sha)
    .fetch_optional(pool)
    .await
}

/// The newest review of the pull request (any head commit).
pub async fn latest_for_pr(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: &str,
    pr_number: i64,
) -> sqlx::Result<Option<ReviewRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {LIST_COLUMNS} FROM reviews WHERE workspace_id = $1 AND project_id = $2 AND pr_number = $3 \
         ORDER BY created_at DESC LIMIT 1"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .bind(pr_number)
    .fetch_optional(pool)
    .await
}

/// Newest first; empty `project_id` lists every project. Summaries omitted.
pub async fn list(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: &str,
    limit: i64,
    before: Option<DateTime<Utc>>,
) -> sqlx::Result<Vec<ReviewRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {LIST_COLUMNS} FROM reviews WHERE workspace_id = $1 AND ($2 = '' OR project_id = $2) \
           AND ($3::timestamptz IS NULL OR created_at < $3) \
         ORDER BY created_at DESC LIMIT $4"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Deletes a failed review so it can be started again.
pub async fn delete(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM reviews WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Persists the rail after a step changed.
pub async fn set_steps(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    status: &str,
    steps: &[StepRecord],
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE reviews SET status = $3, steps = $4, updated_at = now() WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(status)
    .bind(Json(steps))
    .execute(pool)
    .await?;
    Ok(())
}

/// The project's display name and repository, learned in the context step.
pub async fn set_project(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    project_name: &str,
    repo_full_name: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE reviews SET project_name = $3, repo_full_name = $4, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(project_name)
    .bind(repo_full_name)
    .execute(pool)
    .await?;
    Ok(())
}

/// What the analyze step learned.
pub async fn set_analysis(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    files_reviewed: i32,
    findings_total: i32,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE reviews SET files_reviewed = $3, findings_total = $4, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(files_reviewed)
    .bind(findings_total)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_summary(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    summary: &str,
    reasoner: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE reviews SET summary = $3, reasoner = $4, updated_at = now() WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(summary)
    .bind(reasoner)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_failed(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    error: &str,
    steps: &[StepRecord],
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE reviews SET status = 'failed', error = $3, steps = $4, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(error)
    .bind(Json(steps))
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_published(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    url: &str,
    steps: &[StepRecord],
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE reviews SET status = 'published', published_url = $3, steps = $4, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(url)
    .bind(Json(steps))
    .execute(pool)
    .await?;
    Ok(())
}

/// Recounts the accepted findings on the review.
pub async fn refresh_accepted(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> sqlx::Result<i32> {
    sqlx::query_scalar(
        "UPDATE reviews SET findings_accepted = \
            (SELECT count(*) FROM findings f WHERE f.workspace_id = $1 AND f.review_id = $2 AND f.decision = 'accepted'), \
            updated_at = now() \
         WHERE workspace_id = $1 AND id = $2 RETURNING findings_accepted",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_one(pool)
    .await
}

// --- Findings --------------------------------------------------------------

pub async fn insert_findings(pool: &PgPool, rows: &[FindingRow]) -> sqlx::Result<()> {
    for row in rows {
        sqlx::query(
            "INSERT INTO findings (id, workspace_id, review_id, position, category, severity, title, detail, path, line, \
             end_line, suggestion, rule, decision, decision_note, decided_by, decided_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        )
        .bind(row.id)
        .bind(row.workspace_id)
        .bind(row.review_id)
        .bind(row.position)
        .bind(&row.category)
        .bind(&row.severity)
        .bind(&row.title)
        .bind(&row.detail)
        .bind(&row.path)
        .bind(row.line)
        .bind(row.end_line)
        .bind(&row.suggestion)
        .bind(&row.rule)
        .bind(&row.decision)
        .bind(&row.decision_note)
        .bind(&row.decided_by)
        .bind(row.decided_at)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn findings_of(
    pool: &PgPool,
    workspace_id: Uuid,
    review_id: Uuid,
) -> sqlx::Result<Vec<FindingRow>> {
    sqlx::query_as(
        "SELECT * FROM findings WHERE workspace_id = $1 AND review_id = $2 ORDER BY position, id",
    )
    .bind(workspace_id)
    .bind(review_id)
    .fetch_all(pool)
    .await
}

pub async fn get_finding(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> sqlx::Result<FindingRow> {
    sqlx::query_as("SELECT * FROM findings WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .fetch_one(pool)
        .await
}

pub async fn decide_finding(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    decision: &str,
    note: &str,
    decided_by: &str,
) -> sqlx::Result<FindingRow> {
    sqlx::query_as(
        "UPDATE findings SET decision = $3, decision_note = $4, decided_by = $5, decided_at = now() \
         WHERE workspace_id = $1 AND id = $2 RETURNING *",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(decision)
    .bind(note)
    .bind(decided_by)
    .fetch_one(pool)
    .await
}

// --- Activity --------------------------------------------------------------

pub async fn insert_activity(pool: &PgPool, row: &ActivityRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO review_activity (id, workspace_id, review_id, project_id, project_name, pr_number, action, actor, detail, at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(row.id)
    .bind(row.workspace_id)
    .bind(row.review_id)
    .bind(&row.project_id)
    .bind(&row.project_name)
    .bind(row.pr_number)
    .bind(&row.action)
    .bind(&row.actor)
    .bind(&row.detail)
    .bind(row.at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Newest first, strictly before the cursor when one is given.
pub async fn list_activity(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: &str,
    limit: i64,
    before: Option<DateTime<Utc>>,
) -> sqlx::Result<Vec<ActivityRow>> {
    sqlx::query_as(
        "SELECT * FROM review_activity WHERE workspace_id = $1 AND ($2 = '' OR project_id = $2) \
           AND ($3::timestamptz IS NULL OR at < $3) \
         ORDER BY at DESC, id DESC LIMIT $4",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await
}
