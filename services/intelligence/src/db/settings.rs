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
    /// Reviews ask the model to draft fixes on their own (CONTRACT.md §23.2).
    pub auto_draft_fixes: bool,
    /// Whose pull requests the watcher asks to merge: "off" | "dependabot".
    pub auto_merge: String,
    /// A merge needs an SDI review of the current head (CONTRACT.md §23.3).
    pub merge_requires_review: bool,
}

pub const AUTO_MERGE_OFF: &str = "off";
pub const AUTO_MERGE_DEPENDABOT: &str = "dependabot";

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
            auto_draft_fixes: false,
            auto_merge: AUTO_MERGE_OFF.to_owned(),
            merge_requires_review: true,
        }
    }

    /// Whether the watcher may ask to merge Dependabot's pull requests.
    pub fn merges_dependabot(&self) -> bool {
        self.auto_merge == AUTO_MERGE_DEPENDABOT
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
         reasoner_daily_token_cap, auto_draft_fixes, auto_merge, merge_requires_review \
         FROM settings WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
}

pub async fn upsert(pool: &PgPool, workspace_id: Uuid, row: &SettingsRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO settings (workspace_id, default_weights, reasoner, model, cost_ceiling_usd, auto_review, \
         reasoner_provider, reasoner_base_url, reasoner_daily_token_cap, auto_draft_fixes, auto_merge, \
         merge_requires_review, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now()) \
         ON CONFLICT (workspace_id) DO UPDATE SET default_weights = EXCLUDED.default_weights, \
         reasoner = EXCLUDED.reasoner, model = EXCLUDED.model, cost_ceiling_usd = EXCLUDED.cost_ceiling_usd, \
         auto_review = EXCLUDED.auto_review, reasoner_provider = EXCLUDED.reasoner_provider, \
         reasoner_base_url = EXCLUDED.reasoner_base_url, \
         reasoner_daily_token_cap = EXCLUDED.reasoner_daily_token_cap, \
         auto_draft_fixes = EXCLUDED.auto_draft_fixes, auto_merge = EXCLUDED.auto_merge, \
         merge_requires_review = EXCLUDED.merge_requires_review, updated_at = now()",
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
    .bind(row.auto_draft_fixes)
    .bind(&row.auto_merge)
    .bind(row.merge_requires_review)
    .execute(pool)
    .await?;
    Ok(())
}

/// Every workspace that switched `auto_review` on, in a stable order: the
/// ones the pull request watcher looks at (CONTRACT.md §23.1). This is the
/// one query of the schema that is not scoped to a workspace: it only
/// answers which workspaces asked to be watched.
/// Workspaces the watcher looks at, the one it looked at longest ago
/// first (never looked at before all others), so a look that is cut short
/// starves nobody: whoever was left out comes first next time.
pub async fn auto_review_workspaces(pool: &PgPool) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT s.workspace_id FROM settings s \
         LEFT JOIN pr_watch_state w ON w.workspace_id = s.workspace_id \
         WHERE s.auto_review \
         ORDER BY w.last_run_at ASC NULLS FIRST, s.workspace_id",
    )
    .fetch_all(pool)
    .await
}
