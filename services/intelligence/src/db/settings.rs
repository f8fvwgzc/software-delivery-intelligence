//! The single settings row. Reads fall back to the defaults when the row is
//! missing so a fresh schema never blocks ranking.

use sqlx::{FromRow, PgPool, types::Json};

use crate::domain::model::Weights;

pub const DEFAULT_COST_CEILING_USD: f64 = 500.0;
pub const DEFAULT_REASONER: &str = "claude";

#[derive(Debug, Clone, FromRow)]
pub struct SettingsRow {
    pub default_weights: Json<Weights>,
    pub reasoner: String,
    pub model: String,
    pub cost_ceiling_usd: f64,
}

impl SettingsRow {
    pub fn defaults() -> Self {
        Self {
            default_weights: Json(Weights::default()),
            reasoner: DEFAULT_REASONER.to_owned(),
            // Empty means "the ANTHROPIC_MODEL environment default".
            model: String::new(),
            cost_ceiling_usd: DEFAULT_COST_CEILING_USD,
        }
    }
}

pub async fn get(pool: &PgPool) -> sqlx::Result<Option<SettingsRow>> {
    sqlx::query_as(
        "SELECT default_weights, reasoner, model, cost_ceiling_usd FROM settings WHERE id = 1",
    )
    .fetch_optional(pool)
    .await
}

pub async fn upsert(pool: &PgPool, row: &SettingsRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO settings (id, default_weights, reasoner, model, cost_ceiling_usd, updated_at) \
         VALUES (1, $1, $2, $3, $4, now()) \
         ON CONFLICT (id) DO UPDATE SET default_weights = EXCLUDED.default_weights, reasoner = EXCLUDED.reasoner, \
         model = EXCLUDED.model, cost_ceiling_usd = EXCLUDED.cost_ceiling_usd, updated_at = now()",
    )
    .bind(&row.default_weights)
    .bind(&row.reasoner)
    .bind(&row.model)
    .bind(row.cost_ceiling_usd)
    .execute(pool)
    .await?;
    Ok(())
}
