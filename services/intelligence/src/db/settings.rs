//! One settings row per workspace. Reads fall back to the defaults when the
//! row is missing so a fresh workspace never blocks ranking.

use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

use crate::domain::model::Weights;

pub const DEFAULT_COST_CEILING_USD: f64 = 500.0;
pub const DEFAULT_REASONER: &str = "claude";

#[derive(Debug, Clone, FromRow)]
pub struct SettingsRow {
    pub default_weights: Json<Weights>,
    /// "heuristic" | "claude". "claude" only takes effect when the
    /// workspace's data controls allow the reasoner and a key exists.
    pub reasoner: String,
    pub model: String,
    pub cost_ceiling_usd: f64,
    /// Webhooks (`pull_request` opened/synchronize) start a review (CONTRACT.md §20).
    pub auto_review: bool,
}

impl SettingsRow {
    pub fn defaults() -> Self {
        Self {
            default_weights: Json(Weights::default()),
            reasoner: DEFAULT_REASONER.to_owned(),
            // Empty means "the workspace key's model, else ANTHROPIC_MODEL".
            model: String::new(),
            cost_ceiling_usd: DEFAULT_COST_CEILING_USD,
            auto_review: false,
        }
    }
}

pub async fn get(pool: &PgPool, workspace_id: Uuid) -> sqlx::Result<Option<SettingsRow>> {
    sqlx::query_as(
        "SELECT default_weights, reasoner, model, cost_ceiling_usd, auto_review FROM settings WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
}

pub async fn upsert(pool: &PgPool, workspace_id: Uuid, row: &SettingsRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO settings (workspace_id, default_weights, reasoner, model, cost_ceiling_usd, auto_review, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, now()) \
         ON CONFLICT (workspace_id) DO UPDATE SET default_weights = EXCLUDED.default_weights, \
         reasoner = EXCLUDED.reasoner, model = EXCLUDED.model, cost_ceiling_usd = EXCLUDED.cost_ceiling_usd, \
         auto_review = EXCLUDED.auto_review, updated_at = now()",
    )
    .bind(workspace_id)
    .bind(&row.default_weights)
    .bind(&row.reasoner)
    .bind(&row.model)
    .bind(row.cost_ceiling_usd)
    .bind(row.auto_review)
    .execute(pool)
    .await?;
    Ok(())
}
