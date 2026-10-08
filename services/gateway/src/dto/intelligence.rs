//! Intelligence area: proposals, negotiation, decisions and documents.

use chrono::{DateTime, Utc};
use sdi_proto::intelligence::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{common::PlanStep, opt_string, opt_timestamp, proto_enum, timestamp};

// ---------------------------------------------------------------------------
// Proposals
// ---------------------------------------------------------------------------

proto_enum! {
    /// What a proposal is about.
    ProposalKind => proto::ProposalKind {
        Deployment, Infrastructure, CiCd, Testing, Security, Documentation, Dependency
    }
}

proto_enum! {
    /// Lifecycle of a proposal.
    ProposalStatus => proto::ProposalStatus { Proposed, Negotiating, Accepted, Rejected, Superseded }
}

proto_enum! {
    /// Where a piece of evidence points.
    EvidenceKind => proto::EvidenceKind {
        Fact, Pattern, Decision, DeliveryHistory, SimilarProject, Policy, Answer
    }
}

/// Ranking signals, each 0.0 – 1.0; `overall` is the weighted sum.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScoreBreakdown {
    /// Cosine similarity between project profile and pattern.
    pub semantic_similarity: f64,
    /// Agreement with active decisions.
    pub policy: f64,
    /// Decay since the pattern was last used.
    pub recency: f64,
    /// Pattern success rate.
    pub success: f64,
    /// Share of pattern requirements the project meets.
    pub architecture_fit: f64,
    /// Cheaper is higher.
    pub cost: f64,
    /// Pattern security score.
    pub security: f64,
    /// Weighted sum.
    pub overall: f64,
}

impl From<proto::ScoreBreakdown> for ScoreBreakdown {
    fn from(score: proto::ScoreBreakdown) -> Self {
        Self {
            semantic_similarity: score.semantic_similarity,
            policy: score.policy,
            recency: score.recency,
            success: score.success,
            architecture_fit: score.architecture_fit,
            cost: score.cost,
            security: score.security,
            overall: score.overall,
        }
    }
}

/// Relative weight of each signal. Negotiation changes these.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScoreWeights {
    /// Weight of semantic similarity.
    pub semantic_similarity: f64,
    /// Weight of policy agreement.
    pub policy: f64,
    /// Weight of recency.
    pub recency: f64,
    /// Weight of success rate.
    pub success: f64,
    /// Weight of architecture fit.
    pub architecture_fit: f64,
    /// Weight of cost.
    pub cost: f64,
    /// Weight of security.
    pub security: f64,
}

impl From<proto::ScoreWeights> for ScoreWeights {
    fn from(weights: proto::ScoreWeights) -> Self {
        Self {
            semantic_similarity: weights.semantic_similarity,
            policy: weights.policy,
            recency: weights.recency,
            success: weights.success,
            architecture_fit: weights.architecture_fit,
            cost: weights.cost,
            security: weights.security,
        }
    }
}

/// Why the engine believes something.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    /// Kind of object referenced.
    pub kind: EvidenceKind,
    /// Id of the referenced object.
    pub ref_id: String,
    /// Short label.
    pub label: String,
    /// Detail text.
    pub detail: String,
}

impl From<proto::Evidence> for Evidence {
    fn from(evidence: proto::Evidence) -> Self {
        Self {
            kind: evidence.kind.into(),
            ref_id: evidence.ref_id,
            label: evidence.label,
            detail: evidence.detail,
        }
    }
}

/// One ranked option of a proposal.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProposalOption {
    /// Option id.
    pub id: String,
    /// Title.
    pub title: String,
    /// Summary.
    pub summary: String,
    /// Pattern the option is built from.
    pub pattern_id: String,
    /// 1 is the best ranked option.
    pub rank: u32,
    /// Score breakdown.
    pub score: Option<ScoreBreakdown>,
    /// Advantages.
    pub pros: Vec<String>,
    /// Drawbacks.
    pub cons: Vec<String>,
    /// Evidence behind the ranking.
    pub evidence: Vec<Evidence>,
    /// Estimated monthly cost in USD.
    pub estimated_monthly_cost_usd: f64,
    /// Pattern requirements this project does not meet yet.
    pub unmet_requirements: Vec<String>,
    /// Plan that would be executed.
    pub plan: Vec<PlanStep>,
}

impl From<proto::ProposalOption> for ProposalOption {
    fn from(option: proto::ProposalOption) -> Self {
        Self {
            id: option.id,
            title: option.title,
            summary: option.summary,
            pattern_id: option.pattern_id,
            rank: option.rank,
            score: option.score.map(Into::into),
            pros: option.pros,
            cons: option.cons,
            evidence: option.evidence.into_iter().map(Into::into).collect(),
            estimated_monthly_cost_usd: option.estimated_monthly_cost_usd,
            unmet_requirements: option.unmet_requirements,
            plan: option.plan.into_iter().map(Into::into).collect(),
        }
    }
}

/// A condition agreed during negotiation.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Constraint {
    /// Stable key.
    pub key: String,
    /// Plain-words description.
    pub description: String,
    /// Message that introduced it.
    pub source_message_id: String,
}

impl From<proto::Constraint> for Constraint {
    fn from(constraint: proto::Constraint) -> Self {
        Self {
            key: constraint.key,
            description: constraint.description,
            source_message_id: constraint.source_message_id,
        }
    }
}

/// A ranked, evidence-backed set of options for one project.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    /// Proposal id.
    pub id: String,
    /// Project it belongs to.
    pub project_id: String,
    /// Kind.
    pub kind: ProposalKind,
    /// Status.
    pub status: ProposalStatus,
    /// Title.
    pub title: String,
    /// Summary.
    pub summary: String,
    /// What the user asked for, if anything.
    pub prompt: Option<String>,
    /// Currently recommended option.
    pub recommended_option_id: String,
    /// Options, best rank first.
    pub options: Vec<ProposalOption>,
    /// Constraints agreed so far.
    pub constraints: Vec<Constraint>,
    /// Current signal weights.
    pub weights: Option<ScoreWeights>,
    /// Bumped every time negotiation re-ranks the options.
    pub revision: u32,
    /// `heuristic` or `claude`.
    pub reasoner: String,
    /// Option that was accepted, if any.
    pub accepted_option_id: Option<String>,
    /// Execution created from the accepted option, if any.
    pub execution_id: Option<String>,
    /// When the proposal was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<proto::Proposal> for Proposal {
    fn from(proposal: proto::Proposal) -> Self {
        Self {
            id: proposal.id,
            project_id: proposal.project_id,
            kind: proposal.kind.into(),
            status: proposal.status.into(),
            title: proposal.title,
            summary: proposal.summary,
            prompt: opt_string(proposal.prompt),
            recommended_option_id: proposal.recommended_option_id,
            options: proposal.options.into_iter().map(Into::into).collect(),
            constraints: proposal.constraints.into_iter().map(Into::into).collect(),
            weights: proposal.weights.map(Into::into),
            revision: proposal.revision,
            reasoner: proposal.reasoner,
            accepted_option_id: opt_string(proposal.accepted_option_id),
            execution_id: opt_string(proposal.execution_id),
            created_at: timestamp(proposal.created_at.as_ref()),
            updated_at: timestamp(proposal.updated_at.as_ref()),
        }
    }
}

/// Query of `GET /proposals`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListProposalsQuery {
    /// Only proposals of this project. Empty lists every project.
    pub project_id: Option<String>,
}

/// Body of `POST /proposals`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateProposalRequest {
    /// Project to propose for.
    pub project_id: String,
    /// What to propose.
    pub kind: ProposalKind,
    /// Free-text request, if any.
    pub prompt: Option<String>,
}

/// Body of `POST /proposals/{id}/accept`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcceptProposalRequest {
    /// Option to accept. Empty accepts the recommended option.
    pub option_id: Option<String>,
    /// Hand the plan to the execution service straight away.
    pub create_execution: Option<bool>,
}

/// Body of `POST /proposals/{id}/reject`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RejectProposalRequest {
    /// Why it was rejected.
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Negotiation
// ---------------------------------------------------------------------------

proto_enum! {
    /// Who wrote a negotiation message.
    MessageRole => proto::MessageRole { User, Assistant, System }
}

/// One turn of a negotiation.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// Message id.
    pub id: String,
    /// Proposal it belongs to.
    pub proposal_id: String,
    /// Author role.
    pub role: MessageRole,
    /// Text.
    pub content: String,
    /// When it was written.
    pub created_at: DateTime<Utc>,
    /// True when this assistant turn switched the recommended option.
    pub changed_recommendation: bool,
    /// Evidence the reply cites.
    pub references: Vec<Evidence>,
}

impl From<proto::Message> for Message {
    fn from(message: proto::Message) -> Self {
        Self {
            id: message.id,
            proposal_id: message.proposal_id,
            role: message.role.into(),
            content: message.content,
            created_at: timestamp(message.created_at.as_ref()),
            changed_recommendation: message.changed_recommendation,
            references: message.references.into_iter().map(Into::into).collect(),
        }
    }
}

/// Body of `POST /proposals/{id}/messages`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NegotiateRequest {
    /// The user's message.
    pub content: String,
}

/// Response of `POST /proposals/{id}/messages`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NegotiateResponse {
    /// Proposal after the turn, possibly re-ranked.
    pub proposal: Option<Proposal>,
    /// The stored user message.
    pub user_message: Option<Message>,
    /// The assistant reply.
    pub reply: Option<Message>,
    /// Set when the turn recorded a new decision.
    pub recorded_decision: Option<Decision>,
}

impl From<proto::NegotiateResponse> for NegotiateResponse {
    fn from(response: proto::NegotiateResponse) -> Self {
        Self {
            proposal: response.proposal.map(Into::into),
            user_message: response.user_message.map(Into::into),
            reply: response.reply.map(Into::into),
            recorded_decision: response.recorded_decision.map(Into::into),
        }
    }
}

// ---------------------------------------------------------------------------
// Decisions
// ---------------------------------------------------------------------------

proto_enum! {
    /// How widely a decision applies.
    DecisionScope => proto::DecisionScope { Project, Team, Department, Organization }
}

proto_enum! {
    /// Whether a decision is still in force.
    DecisionStatus => proto::DecisionStatus { Active, Superseded }
}

proto_enum! {
    /// How a decision was recorded.
    DecisionSource => proto::DecisionSource { Manual, Negotiation, ProposalAccepted, QuestionAnswered }
}

/// A remembered decision with provenance.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    /// Decision id.
    pub id: String,
    /// Project, when tied to one.
    pub project_id: Option<String>,
    /// Scope.
    pub scope: DecisionScope,
    /// Status.
    pub status: DecisionStatus,
    /// Source.
    pub source: DecisionSource,
    /// Title.
    pub title: String,
    /// The decision itself.
    pub statement: String,
    /// Reasons given.
    pub reasons: Vec<String>,
    /// Who approved it.
    pub approved_by: String,
    /// Proposal it came from, if any.
    pub proposal_id: Option<String>,
    /// Machine tags the ranker matches on, e.g. `cloud:azure`.
    pub tags: Vec<String>,
    /// When it was recorded.
    pub created_at: DateTime<Utc>,
    /// When it was superseded.
    pub superseded_at: Option<DateTime<Utc>>,
    /// Decision that replaced it.
    pub superseded_by: Option<String>,
}

impl From<proto::Decision> for Decision {
    fn from(decision: proto::Decision) -> Self {
        Self {
            id: decision.id,
            project_id: opt_string(decision.project_id),
            scope: decision.scope.into(),
            status: decision.status.into(),
            source: decision.source.into(),
            title: decision.title,
            statement: decision.statement,
            reasons: decision.reasons,
            approved_by: decision.approved_by,
            proposal_id: opt_string(decision.proposal_id),
            tags: decision.tags,
            created_at: timestamp(decision.created_at.as_ref()),
            superseded_at: opt_timestamp(decision.superseded_at.as_ref()),
            superseded_by: opt_string(decision.superseded_by),
        }
    }
}

/// Query of `GET /decisions`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListDecisionsQuery {
    /// Only decisions of this project. Empty lists every decision.
    pub project_id: Option<String>,
    /// Also return superseded decisions.
    pub include_superseded: Option<bool>,
}

/// Body of `POST /decisions`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDecisionRequest {
    /// Project, when tied to one.
    pub project_id: Option<String>,
    /// Scope.
    pub scope: DecisionScope,
    /// Title.
    pub title: String,
    /// The decision itself.
    pub statement: String,
    /// Reasons.
    #[serde(default)]
    pub reasons: Vec<String>,
    /// Who approved it.
    pub approved_by: Option<String>,
    /// Machine tags, e.g. `cloud:azure`.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Body of `POST /decisions/{id}/supersede`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SupersedeDecisionRequest {
    /// Decision that replaces it, if any.
    pub superseded_by: Option<String>,
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

proto_enum! {
    /// What a generated document is.
    DocumentKind => proto::DocumentKind {
        Readme, Architecture, DeploymentGuide, Runbook, Adr, UatPlan, ExecutiveReport, EngineeringReport
    }
}

/// A document generated from the knowledge model.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    /// Document id.
    pub id: String,
    /// Project it belongs to.
    pub project_id: String,
    /// Kind.
    pub kind: DocumentKind,
    /// Title.
    pub title: String,
    /// Markdown body. Empty in list responses.
    pub markdown: String,
    /// Commit it was generated from.
    pub commit_sha: String,
    /// Number of facts used.
    pub facts_used: u32,
    /// When it was generated.
    pub generated_at: DateTime<Utc>,
}

impl From<proto::Document> for Document {
    fn from(document: proto::Document) -> Self {
        Self {
            id: document.id,
            project_id: document.project_id,
            kind: document.kind.into(),
            title: document.title,
            markdown: document.markdown,
            commit_sha: document.commit_sha,
            facts_used: document.facts_used,
            generated_at: timestamp(document.generated_at.as_ref()),
        }
    }
}

/// Body of `POST /projects/{id}/documents`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateDocumentRequest {
    /// Kind of document to generate.
    pub kind: DocumentKind,
}

/// Counters of the intelligence store.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceStats {
    /// Proposals in total.
    pub proposals: u32,
    /// Proposals still open.
    pub proposals_open: u32,
    /// Accepted proposals.
    pub proposals_accepted: u32,
    /// Active decisions.
    pub decisions_active: u32,
    /// Generated documents.
    pub documents: u32,
    /// Negotiation messages.
    pub messages: u32,
}

impl From<proto::IntelligenceStats> for IntelligenceStats {
    fn from(stats: proto::IntelligenceStats) -> Self {
        Self {
            proposals: stats.proposals,
            proposals_open: stats.proposals_open,
            proposals_accepted: stats.proposals_accepted,
            decisions_active: stats.decisions_active,
            documents: stats.documents,
            messages: stats.messages,
        }
    }
}
