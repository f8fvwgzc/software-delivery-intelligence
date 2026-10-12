//! Intelligence area: proposals, negotiation, decisions and documents.

use chrono::{DateTime, Utc};
use sdi_proto::intelligence::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{
    common::{Adapter, PlanStep},
    opt_string, opt_timestamp, proto_enum, timestamp,
};

// ---------------------------------------------------------------------------
// Proposals
// ---------------------------------------------------------------------------

proto_enum! {
    /// What a proposal is about. `modernization` moves a legacy or transitional
    /// project up the maturity axes (CONTRACT.md §6).
    ProposalKind => proto::ProposalKind {
        Deployment, Infrastructure, CiCd, Testing, Security, Documentation, Dependency,
        Modernization
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
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

impl From<ScoreWeights> for proto::ScoreWeights {
    fn from(weights: ScoreWeights) -> Self {
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

/// A generated file an option would add to the repository (CONTRACT.md §14).
/// Content is complete and ready to commit.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    /// Repository-relative path, e.g. `.github/workflows/deploy.yml`.
    pub path: String,
    /// Full file content.
    pub content: String,
    /// `dockerfile`, `yaml`, `hcl`, `bicep`, `json`, `markdown`, `nginx` or `text`.
    pub language: String,
    /// One sentence: why this file exists.
    pub purpose: String,
    /// True when the repository already has a file at this path.
    pub replaces_existing: bool,
}

impl From<proto::Artifact> for Artifact {
    fn from(artifact: proto::Artifact) -> Self {
        Self {
            path: artifact.path,
            content: artifact.content,
            language: artifact.language,
            purpose: artifact.purpose,
            replaces_existing: artifact.replaces_existing,
        }
    }
}

/// One line of an option's cost model. The lines sum to `estimatedMonthlyCostUsd`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CostLine {
    /// What is billed, e.g. `compute`, `database`, `registry`, `networking`, `ci_minutes`.
    pub item: String,
    /// Monthly cost in USD.
    pub monthly_usd: f64,
    /// How the estimate was made.
    pub note: String,
}

impl From<sdi_proto::common::v1::CostLine> for CostLine {
    fn from(line: sdi_proto::common::v1::CostLine) -> Self {
        Self {
            item: line.item,
            monthly_usd: line.monthly_usd,
            note: line.note,
        }
    }
}

/// One ranked option of a proposal. The generated files are not inlined
/// (they can be large); `artifactCount` says how many
/// `GET /proposals/{id}/options/{optionId}/artifacts` returns.
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
    /// Number of generated files; fetch them from the artifacts route.
    pub artifact_count: u32,
    /// Cost model behind `estimatedMonthlyCostUsd`.
    pub cost_breakdown: Vec<CostLine>,
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
            artifact_count: u32::try_from(option.artifacts.len()).unwrap_or(u32::MAX),
            cost_breakdown: option.cost_breakdown.into_iter().map(Into::into).collect(),
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
    /// Page size, newest first (default 50, max 200).
    pub limit: Option<u32>,
    /// Cursor: proposals created strictly before this time.
    pub before: Option<DateTime<Utc>>,
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
    /// Monthly budget in USD; becomes the `budget_monthly_usd` constraint (CONTRACT.md §6).
    /// Unset or 0 means no budget beyond what `.sdi.yml` declares.
    pub budget_monthly_usd: Option<f64>,
}

/// Body of `POST /proposals/{id}/accept`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcceptProposalRequest {
    /// Option to accept. Empty accepts the recommended option.
    pub option_id: Option<String>,
    /// Hand the plan to the execution service straight away.
    pub create_execution: Option<bool>,
    /// Runtime for that execution (CONTRACT.md §5.4). `simulated` by default.
    pub adapter: Option<Adapter>,
}

/// Body of `POST /proposals/{id}/pull-request`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenPullRequestRequest {
    /// Option whose artifacts to commit. Empty uses the recommended option.
    pub option_id: Option<String>,
}

/// Ranking defaults and the reasoner, changed from the settings page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceSettings {
    /// Signal weights every new proposal starts from.
    pub default_weights: Option<ScoreWeights>,
    /// `heuristic` or `claude`. `claude` needs `ANTHROPIC_API_KEY` and falls back otherwise.
    pub reasoner: String,
    /// Model used by the `claude` reasoner.
    pub model: String,
    /// True when `ANTHROPIC_API_KEY` is configured. Read-only.
    pub api_key_configured: bool,
    /// Options above this monthly cost lose ranking unless a budget says otherwise.
    pub cost_ceiling_usd: f64,
    /// `pull_request` webhooks (opened, synchronize) start a review that stops at `proposed` (CONTRACT.md §20). Default off.
    pub auto_review: bool,
}

impl From<proto::IntelligenceSettings> for IntelligenceSettings {
    fn from(settings: proto::IntelligenceSettings) -> Self {
        Self {
            default_weights: settings.default_weights.map(Into::into),
            reasoner: settings.reasoner,
            model: settings.model,
            api_key_configured: settings.api_key_configured,
            cost_ceiling_usd: settings.cost_ceiling_usd,
            auto_review: settings.auto_review,
        }
    }
}

impl From<IntelligenceSettings> for proto::IntelligenceSettings {
    fn from(settings: IntelligenceSettings) -> Self {
        Self {
            default_weights: settings.default_weights.map(Into::into),
            reasoner: settings.reasoner,
            model: settings.model,
            api_key_configured: settings.api_key_configured,
            cost_ceiling_usd: settings.cost_ceiling_usd,
            auto_review: settings.auto_review,
        }
    }
}

/// The `intelligence` part of `PATCH /settings`: every field is optional and
/// unset fields keep their current value. `apiKeyConfigured` is read-only.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceSettingsPatch {
    /// Signal weights every new proposal starts from. Replaced as a whole.
    pub default_weights: Option<ScoreWeights>,
    /// `heuristic` or `claude`.
    pub reasoner: Option<String>,
    /// Model used by the `claude` reasoner.
    pub model: Option<String>,
    /// Options above this monthly cost lose ranking unless a budget says otherwise.
    pub cost_ceiling_usd: Option<f64>,
    /// Whether `pull_request` webhooks start a review.
    pub auto_review: Option<bool>,
}

impl IntelligenceSettingsPatch {
    /// The current settings with the patched fields replaced.
    pub fn apply(self, current: IntelligenceSettings) -> IntelligenceSettings {
        IntelligenceSettings {
            default_weights: self.default_weights.or(current.default_weights),
            reasoner: self.reasoner.unwrap_or(current.reasoner),
            model: self.model.unwrap_or(current.model),
            api_key_configured: current.api_key_configured,
            cost_ceiling_usd: self.cost_ceiling_usd.unwrap_or(current.cost_ceiling_usd),
            auto_review: self.auto_review.unwrap_or(current.auto_review),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_carry_a_count_instead_of_the_artifacts() {
        let option = ProposalOption::from(proto::ProposalOption {
            id: "opt-1".into(),
            artifacts: vec![
                proto::Artifact {
                    path: "Dockerfile".into(),
                    content: "FROM scratch".into(),
                    ..Default::default()
                },
                proto::Artifact {
                    path: ".dockerignore".into(),
                    ..Default::default()
                },
            ],
            cost_breakdown: vec![sdi_proto::common::v1::CostLine {
                item: "compute".into(),
                monthly_usd: 5.0,
                note: String::new(),
            }],
            ..Default::default()
        });
        assert_eq!(option.artifact_count, 2);
        assert_eq!(option.cost_breakdown.len(), 1);
        let json = serde_json::to_value(&option).unwrap();
        assert!(json.get("artifacts").is_none(), "{json}");
        assert_eq!(json["artifactCount"], 2);
        assert_eq!(json["costBreakdown"][0]["monthlyUsd"], 5.0);
    }

    #[test]
    fn modernization_is_a_proposal_kind() {
        let kind: ProposalKind = serde_json::from_str("\"modernization\"").unwrap();
        assert_eq!(kind, ProposalKind::Modernization);
        assert_eq!(i32::from(kind), proto::ProposalKind::Modernization as i32);
    }

    #[test]
    fn intelligence_settings_patch_never_touches_the_api_key_flag() {
        let current = IntelligenceSettings {
            default_weights: None,
            reasoner: "heuristic".into(),
            model: "m".into(),
            api_key_configured: true,
            cost_ceiling_usd: 500.0,
            auto_review: false,
        };
        let patch: IntelligenceSettingsPatch = serde_json::from_str(
            r#"{"reasoner": "claude", "apiKeyConfigured": false, "autoReview": true}"#,
        )
        .unwrap();
        let merged = patch.apply(current);
        assert_eq!(merged.reasoner, "claude");
        assert_eq!(merged.model, "m");
        assert!(merged.api_key_configured);
        assert_eq!(merged.cost_ceiling_usd, 500.0);
        assert!(merged.auto_review);
    }
}
