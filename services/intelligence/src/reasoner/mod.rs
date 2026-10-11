//! Turns the templated reply into prose. The heuristic reasoner returns the
//! template as is; the Claude reasoner rewrites it and must never add facts
//! that are not in the structured context. Ranking is never delegated.

mod claude;
pub mod diagrams;

use async_trait::async_trait;

pub use claude::ClaudeReasoner;

#[async_trait]
pub trait Reasoner: Send + Sync {
    /// "heuristic" | "claude", stored on the proposal.
    fn name(&self) -> &'static str;
    /// `context` is the JSON the reply may draw on; `draft` is the templated text.
    async fn write_reply(&self, draft: &str, context: &serde_json::Value) -> String;

    /// Refines a generated diagram (CONTRACT.md §16): the model may write the
    /// explanation and relabel or regroup nodes, nothing else. Returns the
    /// raw JSON reply, or `None` when the reasoner does not do this (the
    /// heuristic) or the call failed; the caller validates what it applies.
    async fn refine_diagram(&self, _request: &diagrams::RefineRequest) -> Option<String> {
        None
    }
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
