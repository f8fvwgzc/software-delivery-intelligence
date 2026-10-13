//! Turns the templated reply into prose. The heuristic reasoner returns the
//! template as is; the LLM reasoner (any provider, CONTRACT.md §19)
//! rewrites it and must never add facts that are not in the structured
//! context. Ranking is never delegated.

pub mod budget;
pub mod diagrams;
pub mod fixes;
mod provider;

use async_trait::async_trait;

pub(crate) use provider::without_body;
pub use provider::{CapReached, LlmReasoner, Provider};

/// The one request shape every provider receives: a system prompt, the
/// structured context (trimmed to the token budget before it is sent),
/// the instruction that follows it, and the reply cap.
#[derive(Debug, Clone)]
pub struct ReasonerRequest {
    pub system: &'static str,
    pub context_json: serde_json::Value,
    pub instruction: String,
    pub max_tokens: u32,
    /// Ask for a JSON object where the protocol can (OpenAI-compatible
    /// `response_format`); the prompt asks for it everywhere.
    pub json: bool,
}

#[async_trait]
pub trait Reasoner: Send + Sync {
    /// "heuristic" | "claude" | "llm", stored on the proposal.
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

    /// Asks the model for the replacement of a finding's lines
    /// (CONTRACT.md §23.2). Returns the reply as the model wrote it; the
    /// caller cleans and checks it ([`fixes::accept_reply`]) before anything
    /// is stored. The heuristic has no model to ask.
    async fn draft_fix(&self, _request: &fixes::FixRequest) -> Result<String, fixes::FixRefusal> {
        Err(fixes::FixRefusal::NoModel)
    }
}

/// The system prompt of the negotiation reply rewrite.
pub const REPLY_SYSTEM_PROMPT: &str = "You rewrite a delivery assistant's templated reply into clear, natural prose for an \
engineer. You are given a JSON context (ranked options with scores, evidence, the effects of the user's message, \
any recorded decision) and the templated draft. Rules: use only facts, numbers, option names and evidence present \
in the context or the draft; never invent services, costs, scores, technologies or history; keep every score and \
evidence reference that the draft cites; if the recommendation changed, say so explicitly with the before and after \
scores; keep it under 250 words; plain text or light Markdown, no preamble.";

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
