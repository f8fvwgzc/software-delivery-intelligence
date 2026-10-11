use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor, types::Json};
use uuid::Uuid;

use super::Tx;

#[derive(Debug, Clone, FromRow)]
pub struct StepRow {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub key: String,
    pub position: i32,
    pub title: String,
    pub description: String,
    pub action_type: String,
    pub adapter: String,
    pub depends_on: Vec<String>,
    pub status: String,
    pub risk: String,
    pub policy_decision: String,
    pub required_permissions: Vec<String>,
    pub approval_id: Option<Uuid>,
    pub params: Json<BTreeMap<String, String>>,
    pub verification: String,
    pub output: String,
    pub checkpointed: bool,
    pub attempts: i32,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

pub async fn insert(tx: &mut Tx, step: &StepRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO steps (id, execution_id, key, position, title, description, action_type, adapter, \
         depends_on, status, risk, policy_decision, required_permissions, params, verification) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(step.id)
    .bind(step.execution_id)
    .bind(&step.key)
    .bind(step.position)
    .bind(&step.title)
    .bind(&step.description)
    .bind(&step.action_type)
    .bind(&step.adapter)
    .bind(&step.depends_on)
    .bind(&step.status)
    .bind(&step.risk)
    .bind(&step.policy_decision)
    .bind(&step.required_permissions)
    .bind(&step.params)
    .bind(&step.verification)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

pub async fn list_for<'e>(
    exec: impl PgExecutor<'e>,
    execution_id: Uuid,
) -> sqlx::Result<Vec<StepRow>> {
    sqlx::query_as(
        "SELECT id, execution_id, key, position, title, description, action_type, adapter, depends_on, \
         status, risk, policy_decision, required_permissions, approval_id, params, verification, output, \
         checkpointed, attempts, started_at, finished_at FROM steps WHERE execution_id = $1 ORDER BY position",
    )
    .bind(execution_id)
    .fetch_all(exec)
    .await
}

pub async fn list_for_many<'e>(
    exec: impl PgExecutor<'e>,
    execution_ids: &[Uuid],
) -> sqlx::Result<Vec<StepRow>> {
    sqlx::query_as(
        "SELECT id, execution_id, key, position, title, description, action_type, adapter, depends_on, \
         status, risk, policy_decision, required_permissions, approval_id, params, verification, output, \
         checkpointed, attempts, started_at, finished_at FROM steps WHERE execution_id = ANY($1) \
         ORDER BY execution_id, position",
    )
    .bind(execution_ids)
    .fetch_all(exec)
    .await
}

pub async fn set_status(tx: &mut Tx, id: Uuid, status: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE steps SET status = $2 WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

/// Moves every step of `execution_id` whose key is listed to `status`.
pub async fn set_status_for_keys(
    tx: &mut Tx,
    execution_id: Uuid,
    keys: &[String],
    status: &str,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE steps SET status = $3 WHERE execution_id = $1 AND key = ANY($2)")
        .bind(execution_id)
        .bind(keys)
        .bind(status)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

/// Cancels whatever has not reached a terminal status yet.
pub async fn cancel_open(tx: &mut Tx, execution_id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE steps SET status = 'CANCELLED', finished_at = COALESCE(finished_at, now()) \
         WHERE execution_id = $1 AND status NOT IN ('SUCCEEDED', 'SKIPPED', 'CANCELLED')",
    )
    .bind(execution_id)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

/// Give-up: the failed step keeps its FAILED status as the record of what
/// went wrong; everything that never ran is cancelled.
pub async fn abandon_open(tx: &mut Tx, execution_id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE steps SET status = 'CANCELLED', finished_at = COALESCE(finished_at, now()) \
         WHERE execution_id = $1 AND status NOT IN ('SUCCEEDED', 'SKIPPED', 'CANCELLED', 'FAILED')",
    )
    .bind(execution_id)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

pub async fn set_approval(
    tx: &mut Tx,
    id: Uuid,
    approval_id: Option<Uuid>,
    status: &str,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE steps SET approval_id = $2, status = $3 WHERE id = $1")
        .bind(id)
        .bind(approval_id)
        .bind(status)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

pub async fn start_attempt(tx: &mut Tx, id: Uuid) -> sqlx::Result<i32> {
    sqlx::query_scalar(
        "UPDATE steps SET status = 'RUNNING', attempts = attempts + 1, output = '', \
         started_at = now(), finished_at = NULL WHERE id = $1 RETURNING attempts",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await
}

pub async fn finish(
    tx: &mut Tx,
    id: Uuid,
    status: &str,
    output: &str,
    checkpointed: bool,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE steps SET status = $2, output = $3, checkpointed = $4, finished_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(output)
    .bind(checkpointed)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

/// Puts a failed step back in line with its (possibly patched) params.
pub async fn reset_for_retry(
    tx: &mut Tx,
    id: Uuid,
    params: &BTreeMap<String, String>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE steps SET status = 'PENDING', approval_id = NULL, params = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(Json(params))
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

/// Steps parked on an approval go back to PENDING when a replan expires it.
pub async fn release_waiting(tx: &mut Tx, execution_id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE steps SET status = 'PENDING', approval_id = NULL \
         WHERE execution_id = $1 AND status = 'WAITING_APPROVAL'",
    )
    .bind(execution_id)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}
