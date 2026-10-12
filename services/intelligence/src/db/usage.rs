//! Reasoner token ledger (CONTRACT.md §19): one row per workspace and UTC
//! day. The provider checks it before a call and adds to it after one, so
//! the daily cap holds across every proposal, review and diagram.

use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, FromRow)]
pub struct UsageRow {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub calls: i32,
}

impl UsageRow {
    pub fn total(&self) -> u64 {
        u64::try_from(self.input_tokens.max(0) + self.output_tokens.max(0)).unwrap_or(0)
    }
}

pub fn today() -> NaiveDate {
    Utc::now().date_naive()
}

pub async fn today_for(pool: &PgPool, workspace_id: Uuid) -> sqlx::Result<UsageRow> {
    Ok(sqlx::query_as(
        "SELECT input_tokens, output_tokens, calls FROM reasoner_usage WHERE workspace_id = $1 AND day = $2",
    )
    .bind(workspace_id)
    .bind(today())
    .fetch_optional(pool)
    .await?
    .unwrap_or_default())
}

pub async fn add(
    pool: &PgPool,
    workspace_id: Uuid,
    input_tokens: u64,
    output_tokens: u64,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO reasoner_usage (workspace_id, day, input_tokens, output_tokens, calls, updated_at) \
         VALUES ($1, $2, $3, $4, 1, now()) \
         ON CONFLICT (workspace_id, day) DO UPDATE SET \
           input_tokens = reasoner_usage.input_tokens + EXCLUDED.input_tokens, \
           output_tokens = reasoner_usage.output_tokens + EXCLUDED.output_tokens, \
           calls = reasoner_usage.calls + 1, updated_at = now()",
    )
    .bind(workspace_id)
    .bind(today())
    .bind(i64::try_from(input_tokens).unwrap_or(i64::MAX))
    .bind(i64::try_from(output_tokens).unwrap_or(i64::MAX))
    .execute(pool)
    .await?;
    Ok(())
}

/// What the reasoner asks of the ledger. Behind a trait so the provider
/// tests run without Postgres.
#[async_trait]
pub trait UsageLedger: Send + Sync {
    /// Tokens (input + output) the workspace spent today.
    async fn used_today(&self, workspace_id: Uuid) -> u64;
    async fn record(&self, workspace_id: Uuid, input_tokens: u64, output_tokens: u64);
}

pub struct PgUsageLedger {
    pub pool: PgPool,
}

#[async_trait]
impl UsageLedger for PgUsageLedger {
    async fn used_today(&self, workspace_id: Uuid) -> u64 {
        match today_for(&self.pool, workspace_id).await {
            Ok(row) => row.total(),
            Err(error) => {
                // A ledger that cannot be read must not unlock the budget.
                tracing::warn!(%error, "reasoner usage unavailable; treating the cap as reached");
                u64::MAX
            }
        }
    }

    async fn record(&self, workspace_id: Uuid, input_tokens: u64, output_tokens: u64) {
        if let Err(error) = add(&self.pool, workspace_id, input_tokens, output_tokens).await {
            tracing::warn!(%error, "could not record reasoner usage");
        }
    }
}

/// An in-memory ledger for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryLedger {
    pub used: std::sync::Mutex<std::collections::BTreeMap<Uuid, (u64, u64, u32)>>,
}

#[cfg(test)]
#[async_trait]
impl UsageLedger for MemoryLedger {
    async fn used_today(&self, workspace_id: Uuid) -> u64 {
        self.used
            .lock()
            .unwrap()
            .get(&workspace_id)
            .map(|(i, o, _)| i + o)
            .unwrap_or(0)
    }

    async fn record(&self, workspace_id: Uuid, input_tokens: u64, output_tokens: u64) {
        let mut used = self.used.lock().unwrap();
        let entry = used.entry(workspace_id).or_default();
        entry.0 += input_tokens;
        entry.1 += output_tokens;
        entry.2 += 1;
    }
}
