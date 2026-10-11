//! Row and domain record <-> proto message conversions, plus the string
//! encodings the schema uses for proto enums.

use sdi_common::time;
use sdi_proto::{
    common::v1::{CostLine, PlanStep},
    intelligence::v1::{
        Artifact, Constraint, Decision, DecisionScope, DecisionSource, DecisionStatus, Document,
        DocumentKind, Evidence, EvidenceKind as ProtoEvidenceKind, IntelligenceSettings, Message,
        MessageRole, Proposal, ProposalKind, ProposalOption, ProposalStatus, ScoreBreakdown,
        ScoreWeights,
    },
};

use crate::{
    db::{
        decisions::DecisionRow, documents::DocumentRow, messages::MessageRow,
        proposals::ProposalRow, settings::SettingsRow,
    },
    domain::model::{
        ArtifactData, Breakdown, ConstraintData, CostLineData, EvidenceItem, EvidenceKind,
        PlanStepData, RankedOption, Weights,
    },
};

/// Lowercase enum name without its proto prefix, e.g. `PROPOSAL_KIND_CI_CD` -> `ci_cd`.
fn strip(name: &str, prefix: &str) -> String {
    name.trim_start_matches(prefix).to_lowercase()
}

pub fn kind_to_str(kind: ProposalKind) -> String {
    strip(kind.as_str_name(), "PROPOSAL_KIND_")
}

pub fn kind_from_str(value: &str) -> ProposalKind {
    ProposalKind::from_str_name(&format!("PROPOSAL_KIND_{}", value.to_uppercase()))
        .unwrap_or_default()
}

pub fn status_from_str(value: &str) -> ProposalStatus {
    ProposalStatus::from_str_name(&format!("PROPOSAL_STATUS_{}", value.to_uppercase()))
        .unwrap_or_default()
}

pub fn scope_to_str(scope: DecisionScope) -> String {
    strip(scope.as_str_name(), "DECISION_SCOPE_")
}

pub fn scope_from_str(value: &str) -> DecisionScope {
    DecisionScope::from_str_name(&format!("DECISION_SCOPE_{}", value.to_uppercase()))
        .unwrap_or_default()
}

pub fn source_to_str(source: DecisionSource) -> String {
    strip(source.as_str_name(), "DECISION_SOURCE_")
}

pub fn source_from_str(value: &str) -> DecisionSource {
    DecisionSource::from_str_name(&format!("DECISION_SOURCE_{}", value.to_uppercase()))
        .unwrap_or_default()
}

pub fn decision_status_from_str(value: &str) -> DecisionStatus {
    DecisionStatus::from_str_name(&format!("DECISION_STATUS_{}", value.to_uppercase()))
        .unwrap_or_default()
}

pub fn document_kind_to_str(kind: DocumentKind) -> String {
    strip(kind.as_str_name(), "DOCUMENT_KIND_")
}

pub fn document_kind_from_str(value: &str) -> DocumentKind {
    DocumentKind::from_str_name(&format!("DOCUMENT_KIND_{}", value.to_uppercase()))
        .unwrap_or_default()
}

pub fn role_from_str(value: &str) -> MessageRole {
    MessageRole::from_str_name(&format!("MESSAGE_ROLE_{}", value.to_uppercase()))
        .unwrap_or_default()
}

pub fn evidence_kind(kind: EvidenceKind) -> ProtoEvidenceKind {
    match kind {
        EvidenceKind::Fact => ProtoEvidenceKind::Fact,
        EvidenceKind::Pattern => ProtoEvidenceKind::Pattern,
        EvidenceKind::Decision => ProtoEvidenceKind::Decision,
        EvidenceKind::DeliveryHistory => ProtoEvidenceKind::DeliveryHistory,
        EvidenceKind::SimilarProject => ProtoEvidenceKind::SimilarProject,
        EvidenceKind::Policy => ProtoEvidenceKind::Policy,
        EvidenceKind::Answer => ProtoEvidenceKind::Answer,
    }
}

pub fn evidence(item: &EvidenceItem) -> Evidence {
    Evidence {
        kind: evidence_kind(item.kind) as i32,
        ref_id: item.ref_id.clone(),
        label: item.label.clone(),
        detail: item.detail.clone(),
    }
}

pub fn plan_step(step: &PlanStepData) -> PlanStep {
    PlanStep {
        key: step.key.clone(),
        title: step.title.clone(),
        description: step.description.clone(),
        action_type: step.action_type.clone(),
        adapter: step.adapter.clone(),
        depends_on: step.depends_on.clone(),
        required_permissions: step.required_permissions.clone(),
        params: step
            .params
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        verification: step.verification.clone(),
    }
}

pub fn breakdown(score: &Breakdown) -> ScoreBreakdown {
    ScoreBreakdown {
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

pub fn weights(weights: &Weights) -> ScoreWeights {
    ScoreWeights {
        semantic_similarity: weights.semantic_similarity,
        policy: weights.policy,
        recency: weights.recency,
        success: weights.success,
        architecture_fit: weights.architecture_fit,
        cost: weights.cost,
        security: weights.security,
    }
}

pub fn weights_from_proto(weights: &ScoreWeights) -> Weights {
    Weights {
        semantic_similarity: weights.semantic_similarity,
        policy: weights.policy,
        recency: weights.recency,
        success: weights.success,
        architecture_fit: weights.architecture_fit,
        cost: weights.cost,
        security: weights.security,
    }
}

pub fn artifact(a: &ArtifactData) -> Artifact {
    Artifact {
        path: a.path.clone(),
        content: a.content.clone(),
        language: a.language.clone(),
        purpose: a.purpose.clone(),
        replaces_existing: a.replaces_existing,
    }
}

pub fn cost_line(line: &CostLineData) -> CostLine {
    CostLine {
        item: line.item.clone(),
        monthly_usd: line.monthly_usd,
        note: line.note.clone(),
    }
}

pub fn settings(
    row: &SettingsRow,
    api_key_configured: bool,
    effective_model: String,
) -> IntelligenceSettings {
    IntelligenceSettings {
        default_weights: Some(weights(&row.default_weights.0)),
        reasoner: row.reasoner.clone(),
        model: effective_model,
        api_key_configured,
        cost_ceiling_usd: row.cost_ceiling_usd,
    }
}

pub fn option(option: &RankedOption) -> ProposalOption {
    ProposalOption {
        id: option.id.clone(),
        title: option.title.clone(),
        summary: option.summary.clone(),
        pattern_id: option.pattern_id.clone(),
        rank: option.rank,
        score: Some(breakdown(&option.score)),
        pros: option.pros.clone(),
        cons: option.cons.clone(),
        evidence: option.evidence.iter().map(evidence).collect(),
        estimated_monthly_cost_usd: option.estimated_monthly_cost_usd,
        unmet_requirements: option.unmet_requirements.clone(),
        plan: option.plan.iter().map(plan_step).collect(),
        artifacts: option.artifacts.iter().map(artifact).collect(),
        cost_breakdown: option.cost_breakdown.iter().map(cost_line).collect(),
    }
}

pub fn constraint(c: &ConstraintData) -> Constraint {
    Constraint {
        key: c.key.clone(),
        description: c.description.clone(),
        source_message_id: c.source_message_id.clone(),
    }
}

pub fn proposal(row: &ProposalRow) -> Proposal {
    Proposal {
        id: row.id.to_string(),
        project_id: row.project_id.clone(),
        kind: kind_from_str(&row.kind) as i32,
        status: status_from_str(&row.status) as i32,
        title: row.title.clone(),
        summary: row.summary.clone(),
        prompt: row.prompt.clone(),
        recommended_option_id: row.recommended_option_id.clone(),
        options: row.options.0.iter().map(option).collect(),
        constraints: row.constraints.0.iter().map(constraint).collect(),
        weights: Some(weights(&row.weights.0)),
        revision: row.revision.max(0) as u32,
        reasoner: row.reasoner.clone(),
        accepted_option_id: row.accepted_option_id.clone(),
        execution_id: row.execution_id.clone(),
        created_at: Some(time::to_proto(row.created_at)),
        updated_at: Some(time::to_proto(row.updated_at)),
    }
}

pub fn message(row: &MessageRow) -> Message {
    Message {
        id: row.id.to_string(),
        proposal_id: row.proposal_id.to_string(),
        role: role_from_str(&row.role) as i32,
        content: row.content.clone(),
        created_at: Some(time::to_proto(row.created_at)),
        changed_recommendation: row.changed_recommendation,
        references: row.references.0.iter().map(evidence).collect(),
    }
}

pub fn decision(row: &DecisionRow) -> Decision {
    Decision {
        id: row.id.to_string(),
        project_id: row.project_id.clone(),
        scope: scope_from_str(&row.scope) as i32,
        status: decision_status_from_str(&row.status) as i32,
        source: source_from_str(&row.source) as i32,
        title: row.title.clone(),
        statement: row.statement.clone(),
        reasons: row.reasons.clone(),
        approved_by: row.approved_by.clone(),
        proposal_id: row.proposal_id.clone(),
        tags: row.tags.clone(),
        created_at: Some(time::to_proto(row.created_at)),
        superseded_at: time::opt_to_proto(row.superseded_at),
        superseded_by: row.superseded_by.clone(),
    }
}

pub fn document(row: &DocumentRow) -> Document {
    Document {
        id: row.id.to_string(),
        project_id: row.project_id.clone(),
        kind: document_kind_from_str(&row.kind) as i32,
        title: row.title.clone(),
        markdown: row.markdown.clone(),
        commit_sha: row.commit_sha.clone(),
        facts_used: row.facts_used.max(0) as u32,
        generated_at: Some(time::to_proto(row.generated_at)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_strings_round_trip() {
        assert_eq!(kind_to_str(ProposalKind::CiCd), "ci_cd");
        assert_eq!(kind_from_str("ci_cd"), ProposalKind::CiCd);
        assert_eq!(kind_from_str("nope"), ProposalKind::Unspecified);
        assert_eq!(scope_to_str(DecisionScope::Organization), "organization");
        assert_eq!(
            source_from_str("proposal_accepted"),
            DecisionSource::ProposalAccepted
        );
        assert_eq!(
            document_kind_from_str("deployment_guide"),
            DocumentKind::DeploymentGuide
        );
        assert_eq!(status_from_str("negotiating"), ProposalStatus::Negotiating);
        assert_eq!(role_from_str("assistant"), MessageRole::Assistant);
    }
}
