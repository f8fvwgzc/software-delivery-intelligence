//! Per-project review memory (CONTRACT.md §20): for every rule, how many of
//! its findings the project accepted or dismissed. `DecideFinding` writes
//! it; the rule engine reads it before every analysis so a rule the team
//! keeps dismissing stops shouting and one they keep accepting moves up.

use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct LearningRow {
    pub rule: String,
    pub accepted: i32,
    pub dismissed: i32,
}

/// Records one verdict. `previous` is the finding's decision before this
/// one (`pending`, `accepted` or `dismissed`): changing one's mind moves
/// the count from one column to the other instead of counting twice.
pub async fn record(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: &str,
    rule: &str,
    previous: &str,
    decision: &str,
    note: &str,
) -> sqlx::Result<LearningRow> {
    let accepted_delta = i32::from(decision == "accepted") - i32::from(previous == "accepted");
    let dismissed_delta = i32::from(decision == "dismissed") - i32::from(previous == "dismissed");
    sqlx::query_as(
        "INSERT INTO review_learnings (workspace_id, project_id, rule, accepted, dismissed, last_note, updated_at) \
         VALUES ($1, $2, $3, GREATEST($4, 0), GREATEST($5, 0), $6, now()) \
         ON CONFLICT (workspace_id, project_id, rule) DO UPDATE SET \
           accepted = GREATEST(review_learnings.accepted + $4, 0), \
           dismissed = GREATEST(review_learnings.dismissed + $5, 0), \
           last_note = CASE WHEN $6 = '' THEN review_learnings.last_note ELSE $6 END, \
           updated_at = now() \
         RETURNING rule, accepted, dismissed",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(rule)
    .bind(accepted_delta)
    .bind(dismissed_delta)
    .bind(note)
    .fetch_one(pool)
    .await
}

/// Everything the project taught, by rule.
pub async fn for_project(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: &str,
) -> sqlx::Result<Vec<LearningRow>> {
    sqlx::query_as(
        "SELECT rule, accepted, dismissed FROM review_learnings \
         WHERE workspace_id = $1 AND project_id = $2 ORDER BY rule",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
}
