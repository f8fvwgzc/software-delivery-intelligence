use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};

use super::Tx;
use crate::{convert, domain::policy::PolicyRule};

#[derive(Debug, Clone, FromRow)]
pub struct PolicyRow {
    pub action_type: String,
    pub label: String,
    pub description: String,
    pub risk: String,
    pub decision: String,
    pub editable: bool,
    pub updated_at: DateTime<Utc>,
}

pub async fn list<'e>(exec: impl PgExecutor<'e>) -> sqlx::Result<Vec<PolicyRow>> {
    sqlx::query_as(
        "SELECT action_type, label, description, risk, decision, editable, updated_at \
         FROM policies ORDER BY risk, action_type",
    )
    .fetch_all(exec)
    .await
}

pub async fn get<'e>(exec: impl PgExecutor<'e>, action_type: &str) -> sqlx::Result<PolicyRow> {
    sqlx::query_as(
        "SELECT action_type, label, description, risk, decision, editable, updated_at \
         FROM policies WHERE action_type = $1",
    )
    .bind(action_type)
    .fetch_one(exec)
    .await
}

/// The lookup table the domain uses to decorate steps.
pub async fn rules<'e>(exec: impl PgExecutor<'e>) -> sqlx::Result<HashMap<String, PolicyRule>> {
    Ok(list(exec)
        .await?
        .into_iter()
        .map(|row| {
            (
                row.action_type,
                PolicyRule {
                    risk: convert::risk_from_db(&row.risk),
                    decision: convert::decision_from_db(&row.decision),
                },
            )
        })
        .collect())
}

pub async fn update(tx: &mut Tx, action_type: &str, decision: &str) -> sqlx::Result<PolicyRow> {
    sqlx::query_as(
        "UPDATE policies SET decision = $2, updated_at = now() WHERE action_type = $1 \
         RETURNING action_type, label, description, risk, decision, editable, updated_at",
    )
    .bind(action_type)
    .bind(decision)
    .fetch_one(&mut **tx)
    .await
}
