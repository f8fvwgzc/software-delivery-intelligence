use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use super::Tx;

/// An approval joined with the execution and step it gates, which is what
/// the proto message carries.
#[derive(Debug, Clone, FromRow)]
pub struct ApprovalRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub execution_id: Uuid,
    pub step_key: String,
    pub reason: String,
    pub status: String,
    pub decided_by: String,
    pub note: String,
    pub requested_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub execution_number: i64,
    pub execution_title: String,
    pub project_id: String,
    pub project_name: String,
    pub step_title: String,
    pub action_type: String,
    pub risk: String,
    pub policy_decision: String,
    pub required_permissions: Vec<String>,
}

const SELECT: &str = "SELECT a.id, a.workspace_id, a.execution_id, a.step_key, a.reason, a.status, a.decided_by, \
     a.note, a.requested_at, a.decided_at, e.number AS execution_number, e.title AS execution_title, \
     e.project_id, e.project_name, s.title AS step_title, s.action_type, s.risk, s.policy_decision, \
     s.required_permissions \
     FROM approvals a \
     JOIN executions e ON e.id = a.execution_id \
     JOIN steps s ON s.execution_id = a.execution_id AND s.key = a.step_key";

#[allow(clippy::too_many_arguments)]
pub async fn insert(
    tx: &mut Tx,
    workspace_id: Uuid,
    id: Uuid,
    execution_id: Uuid,
    revision: i32,
    step_key: &str,
    reason: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO approvals (id, workspace_id, execution_id, revision, step_key, reason, status) \
         VALUES ($1, $2, $3, $4, $5, $6, 'PENDING')",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(execution_id)
    .bind(revision)
    .bind(step_key)
    .bind(reason)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

pub async fn get<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    id: Uuid,
) -> sqlx::Result<ApprovalRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE a.workspace_id = $1 AND a.id = $2"
    )))
    .bind(workspace_id)
    .bind(id)
    .fetch_one(exec)
    .await
}

/// `status` empty lists every status; `execution_id` None lists every execution.
pub async fn list<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    status: &str,
    execution_id: Option<Uuid>,
) -> sqlx::Result<Vec<ApprovalRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE a.workspace_id = $1 AND ($2 = '' OR a.status = $2) \
           AND ($3::uuid IS NULL OR a.execution_id = $3) \
         ORDER BY a.requested_at DESC"
    )))
    .bind(workspace_id)
    .bind(status)
    .bind(execution_id)
    .fetch_all(exec)
    .await
}

/// Decides a PENDING approval. Returns false when it was already decided
/// or expired, so the caller can answer FAILED_PRECONDITION.
pub async fn decide(
    tx: &mut Tx,
    workspace_id: Uuid,
    id: Uuid,
    approve: bool,
    decided_by: &str,
    note: &str,
) -> sqlx::Result<bool> {
    let status = if approve { "APPROVED" } else { "REJECTED" };
    let result = sqlx::query(
        "UPDATE approvals SET status = $3, decided_by = $4, note = $5, decided_at = now() \
         WHERE workspace_id = $1 AND id = $2 AND status = 'PENDING'",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(status)
    .bind(decided_by)
    .bind(note)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn expire_pending(
    tx: &mut Tx,
    workspace_id: Uuid,
    execution_id: Uuid,
) -> sqlx::Result<u64> {
    sqlx::query(
        "UPDATE approvals SET status = 'EXPIRED', decided_at = now() \
         WHERE workspace_id = $1 AND execution_id = $2 AND status = 'PENDING'",
    )
    .bind(workspace_id)
    .bind(execution_id)
    .execute(&mut **tx)
    .await
    .map(|r| r.rows_affected())
}
