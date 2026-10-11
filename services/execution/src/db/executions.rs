use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor, types::Json};
use uuid::Uuid;

use super::Tx;
use crate::{adapters::Fault, domain::diagnosis::FailureRecord};

#[derive(Debug, Clone, FromRow)]
pub struct ExecutionRow {
    pub id: Uuid,
    pub number: i64,
    pub project_id: String,
    pub project_name: String,
    pub proposal_id: String,
    pub pattern_id: String,
    pub title: String,
    pub status: String,
    pub adapter: String,
    pub identity_id: Uuid,
    pub fault: Option<Json<Fault>>,
    pub failure: Option<Json<FailureRecord>>,
    pub last_checkpoint: Option<String>,
    pub revision: i32,
    /// What the run produced so far ("image", "container_url", ...).
    pub outputs: Json<BTreeMap<String, String>>,
    /// Files a pull-request step commits, keyed by repository path. Never
    /// sent back over gRPC (the proposal already carries them).
    pub artifacts: Json<BTreeMap<String, String>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

const COLUMNS: &str = "id, number, project_id, project_name, proposal_id, pattern_id, title, status, adapter, \
     identity_id, fault, failure, last_checkpoint, revision, outputs, artifacts, created_at, updated_at, \
     started_at, finished_at";

pub struct NewExecution<'a> {
    pub id: Uuid,
    pub project_id: &'a str,
    pub project_name: &'a str,
    pub proposal_id: &'a str,
    pub pattern_id: &'a str,
    pub title: &'a str,
    pub status: &'a str,
    pub adapter: &'a str,
    pub identity_id: Uuid,
    pub artifacts: &'a BTreeMap<String, String>,
}

pub async fn insert(tx: &mut Tx, new: NewExecution<'_>) -> sqlx::Result<ExecutionRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO executions (id, project_id, project_name, proposal_id, pattern_id, title, status, adapter, identity_id, artifacts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING {COLUMNS}"
    )))
    .bind(new.id)
    .bind(new.project_id)
    .bind(new.project_name)
    .bind(new.proposal_id)
    .bind(new.pattern_id)
    .bind(new.title)
    .bind(new.status)
    .bind(new.adapter)
    .bind(new.identity_id)
    .bind(Json(new.artifacts))
    .fetch_one(&mut **tx)
    .await
}

pub async fn get<'e>(exec: impl PgExecutor<'e>, id: Uuid) -> sqlx::Result<ExecutionRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM executions WHERE id = $1"
    )))
    .bind(id)
    .fetch_one(exec)
    .await
}

/// Locks the row for the rest of the transaction so one writer at a time
/// touches an execution and its event sequence.
pub async fn lock(tx: &mut Tx, id: Uuid) -> sqlx::Result<ExecutionRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM executions WHERE id = $1 FOR UPDATE"
    )))
    .bind(id)
    .fetch_one(&mut **tx)
    .await
}

pub async fn list<'e>(
    exec: impl PgExecutor<'e>,
    project_id: &str,
    limit: i64,
) -> sqlx::Result<Vec<ExecutionRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM executions WHERE ($1 = '' OR project_id = $1) ORDER BY number DESC LIMIT $2"
    )))
    .bind(project_id)
    .bind(limit)
    .fetch_all(exec)
    .await
}

pub async fn with_status<'e>(
    exec: impl PgExecutor<'e>,
    statuses: &[&str],
) -> sqlx::Result<Vec<ExecutionRow>> {
    let statuses: Vec<String> = statuses.iter().map(|s| (*s).to_owned()).collect();
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM executions WHERE status = ANY($1) ORDER BY number"
    )))
    .bind(statuses)
    .fetch_all(exec)
    .await
}

/// `started_at` is stamped the first time the run leaves PLANNING;
/// `finished_at` when it reaches a terminal status.
pub async fn set_status(tx: &mut Tx, id: Uuid, status: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE executions SET status = $2, updated_at = now(), \
         started_at = COALESCE(started_at, CASE WHEN $2 <> 'PLANNING' THEN now() END), \
         finished_at = CASE WHEN $2 IN ('SUCCEEDED', 'FAILED', 'CANCELLED') THEN now() ELSE finished_at END \
         WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

pub async fn set_fault(tx: &mut Tx, id: Uuid, fault: Option<&Fault>) -> sqlx::Result<()> {
    sqlx::query("UPDATE executions SET fault = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(fault.map(Json))
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

pub async fn set_failure(
    tx: &mut Tx,
    id: Uuid,
    failure: Option<&FailureRecord>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE executions SET failure = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(failure.map(Json))
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

pub async fn set_checkpoint(tx: &mut Tx, id: Uuid, key: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE executions SET last_checkpoint = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(key)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

/// Adds a step's outputs to the run's. Later steps overwrite earlier keys
/// (a rebuilt image replaces the old `image`).
pub async fn merge_outputs(
    tx: &mut Tx,
    id: Uuid,
    outputs: &BTreeMap<String, String>,
) -> sqlx::Result<()> {
    if outputs.is_empty() {
        return Ok(());
    }
    sqlx::query("UPDATE executions SET outputs = outputs || $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(Json(outputs))
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

pub async fn set_identity(tx: &mut Tx, id: Uuid, identity_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE executions SET identity_id = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(identity_id)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

pub async fn bump_revision(tx: &mut Tx, id: Uuid) -> sqlx::Result<i32> {
    sqlx::query_scalar("UPDATE executions SET revision = revision + 1, updated_at = now() WHERE id = $1 RETURNING revision")
        .bind(id)
        .fetch_one(&mut **tx)
        .await
}

#[derive(Debug, Clone, FromRow)]
pub struct StatsRow {
    pub total: i64,
    pub active: i64,
    pub paused: i64,
    pub succeeded: i64,
    pub failed: i64,
    pub avg_duration_seconds: f64,
    pub steps_executed: i64,
    pub approvals_pending: i64,
    pub failures_recovered: i64,
}

pub async fn stats<'e>(exec: impl PgExecutor<'e>, project_id: &str) -> sqlx::Result<StatsRow> {
    sqlx::query_as(
        "SELECT \
           count(*)::bigint AS total, \
           count(*) FILTER (WHERE e.status IN ('PLANNING','WAITING_FOR_APPROVAL','AUTHORIZED','EXECUTING','VERIFYING','REPLANNING'))::bigint AS active, \
           count(*) FILTER (WHERE e.status IN ('PAUSED','DIAGNOSING'))::bigint AS paused, \
           count(*) FILTER (WHERE e.status = 'SUCCEEDED')::bigint AS succeeded, \
           count(*) FILTER (WHERE e.status = 'FAILED')::bigint AS failed, \
           COALESCE(avg(EXTRACT(EPOCH FROM e.finished_at - e.started_at)) \
             FILTER (WHERE e.status IN ('SUCCEEDED','FAILED') AND e.started_at IS NOT NULL), 0)::float8 AS avg_duration_seconds, \
           COALESCE((SELECT sum(s.attempts) FROM steps s JOIN executions x ON x.id = s.execution_id \
             WHERE $1 = '' OR x.project_id = $1), 0)::bigint AS steps_executed, \
           (SELECT count(*) FROM approvals a JOIN executions x ON x.id = a.execution_id \
             WHERE a.status = 'PENDING' AND ($1 = '' OR x.project_id = $1))::bigint AS approvals_pending, \
           (SELECT count(*) FROM events ev JOIN executions x ON x.id = ev.execution_id \
             WHERE ev.type = 'REPLANNED' AND ($1 = '' OR x.project_id = $1))::bigint AS failures_recovered \
         FROM executions e WHERE $1 = '' OR e.project_id = $1",
    )
    .bind(project_id)
    .fetch_one(exec)
    .await
}
