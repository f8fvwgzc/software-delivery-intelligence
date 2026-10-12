//! One settings row per workspace. Reads fall back to the defaults when the
//! row is missing so a fresh workspace never blocks ranking.

use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

use crate::domain::model::Weights;

pub const DEFAULT_COST_CEILING_USD: f64 = 500.0;
/// "claude" means "the configured provider when the workspace allows it"
/// (the spelling the web app and existing rows use); "llm" is its alias.
pub const DEFAULT_REASONER: &str = "claude";
pub const DEFAULT_DAILY_TOKEN_CAP: i64 = 200_000;

#[derive(Debug, Clone, FromRow)]
pub struct SettingsRow {
    pub default_weights: Json<Weights>,
    /// "heuristic" | "claude" (alias "llm"). The LLM only takes effect when
    /// the workspace's data controls allow the reasoner and a key exists.
    pub reasoner: String,
    pub model: String,
    pub cost_ceiling_usd: f64,
    /// Webhooks (`pull_request` opened/synchronize) start a review (CONTRACT.md §20).
    pub auto_review: bool,
    /// "anthropic" | "openai_compatible" | "" (environment / default).
    pub reasoner_provider: String,
    /// OpenAI-compatible endpoint; empty means the environment or the public API.
    pub reasoner_base_url: String,
    /// Tokens per UTC day before the reasoner falls back to the heuristic.
    pub reasoner_daily_token_cap: i64,
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
            reasoner_provider: String::new(),
            reasoner_base_url: String::new(),
            reasoner_daily_token_cap: DEFAULT_DAILY_TOKEN_CAP,
        }
    }

    /// Whether the settings ask for the LLM at all.
    pub fn wants_llm(&self) -> bool {
        matches!(self.reasoner.trim(), "llm" | "claude")
    }

    pub fn daily_token_cap(&self) -> u64 {
        u64::try_from(self.reasoner_daily_token_cap).unwrap_or(0)
    }
}

pub async fn get(pool: &PgPool, workspace_id: Uuid) -> sqlx::Result<Option<SettingsRow>> {
    sqlx::query_as(
        "SELECT default_weights, reasoner, model, cost_ceiling_usd, auto_review, reasoner_provider, reasoner_base_url, \
         reasoner_daily_token_cap FROM settings WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
}

pub async fn upsert(pool: &PgPool, workspace_id: Uuid, row: &SettingsRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO settings (workspace_id, default_weights, reasoner, model, cost_ceiling_usd, auto_review, \
         reasoner_provider, reasoner_base_url, reasoner_daily_token_cap, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now()) \
         ON CONFLICT (workspace_id) DO UPDATE SET default_weights = EXCLUDED.default_weights, \
         reasoner = EXCLUDED.reasoner, model = EXCLUDED.model, cost_ceiling_usd = EXCLUDED.cost_ceiling_usd, \
         auto_review = EXCLUDED.auto_review, reasoner_provider = EXCLUDED.reasoner_provider, \
         reasoner_base_url = EXCLUDED.reasoner_base_url, \
         reasoner_daily_token_cap = EXCLUDED.reasoner_daily_token_cap, updated_at = now()",
    )
    .bind(workspace_id)
    .bind(&row.default_weights)
    .bind(&row.reasoner)
    .bind(&row.model)
    .bind(row.cost_ceiling_usd)
    .bind(row.auto_review)
    .bind(&row.reasoner_provider)
    .bind(&row.reasoner_base_url)
    .bind(row.reasoner_daily_token_cap)
    .execute(pool)
    .await?;
    Ok(())
}
