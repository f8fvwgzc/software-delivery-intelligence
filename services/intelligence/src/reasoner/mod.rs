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

    /// Rewrites the templated review walkthrough (CONTRACT.md §20) from
    /// the structured context only: file paths, counters, findings and
    /// impacted components, never diff lines. The heuristic keeps the draft.
    async fn write_review_summary(&self, draft: &str, _context: &serde_json::Value) -> String {
        draft.to_owned()
    }
}

/// The system prompt of the review rewrite; public so a test can pin it.
pub const REVIEW_SYSTEM_PROMPT: &str = "You rewrite a templated pull request walkthrough into clear prose for the \
reviewer. You are given a JSON context (the pull request's title and description, the changed files grouped by area \
with line counters, the analyzer findings with rule ids, the impacted components from the dependency graph, and a list \
of things to verify) and the templated draft. Keep the five sections and their headings exactly: Intent, What changed, \
Impacted components, Risk, What to verify. Use only files, numbers, findings, rules and components present in the \
context; never invent code, behaviour, files or risks; keep every rule id the draft cites in parentheses; keep the risk \
level word of the draft. Prefer short paragraphs over bullet walls; under 350 words; Markdown only, no preamble.";

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
