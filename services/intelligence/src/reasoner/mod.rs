//! Turns the templated reply into prose. The heuristic reasoner returns the
//! template as is; the Claude reasoner rewrites it and must never add facts
//! that are not in the structured context. Ranking is never delegated.

mod claude;

use std::sync::Arc;

use async_trait::async_trait;

pub use claude::ClaudeReasoner;

use crate::config::Settings;

#[async_trait]
pub trait Reasoner: Send + Sync {
    /// "heuristic" | "claude", stored on the proposal.
    fn name(&self) -> &'static str;
    /// `context` is the JSON the reply may draw on; `draft` is the templated text.
    async fn write_reply(&self, draft: &str, context: &serde_json::Value) -> String;
}

pub struct HeuristicReasoner;

#[async_trait]
impl Reasoner for HeuristicReasoner {
    fn name(&self) -> &'static str {
        "heuristic"
    }

    async fn write_reply(&self, draft: &str, _context: &serde_json::Value) -> String {
        draft.to_owned()
    }
}

pub fn from_settings(settings: &Settings) -> Arc<dyn Reasoner> {
    match &settings.anthropic_api_key {
        Some(key) => {
            tracing::info!(model = %settings.anthropic_model, "claude reasoner enabled");
            Arc::new(ClaudeReasoner::new(
                key.clone(),
                settings.anthropic_model.clone(),
            ))
        }
        None => Arc::new(HeuristicReasoner),
    }
}
