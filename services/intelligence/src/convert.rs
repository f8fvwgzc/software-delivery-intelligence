//! Row and domain record <-> proto message conversions, plus the string
//! encodings the schema uses for proto enums.

use sdi_common::time;
use sdi_proto::{
    common::v1::{CostLine, PlanStep},
    intelligence::v1::{
        Artifact, Constraint, Decision, DecisionScope, DecisionSource, DecisionStatus,
        DeliveryArtifacts, Diagram, DiagramEdge as ProtoDiagramEdge,
        DiagramGroup as ProtoDiagramGroup, DiagramKind, DiagramNode as ProtoDiagramNode,
        DiagramSpec as ProtoDiagramSpec, Document, DocumentKind, Evidence,
        EvidenceKind as ProtoEvidenceKind, Finding, FindingDecision, FindingSeverity,
        IntelligenceSettings, Message, MessageRole, Proposal, ProposalKind, ProposalOption,
        ProposalStatus, Review, ReviewActivity, ReviewStatus, ReviewStep, ScoreBreakdown,
        ScoreWeights,
    },
};

use crate::{
    db::{
        decisions::DecisionRow,
        diagrams::DiagramRow,
        documents::DocumentRow,
        messages::MessageRow,
        proposals::ProposalRow,
        reviews::{ActivityRow, FindingRow, ReviewRow},
        settings::SettingsRow,
    },
    domain::{
        diagrams::model::{DiagramEdge, DiagramGroup, DiagramNode, DiagramSpec},
        model::{
            ArtifactData, Breakdown, ConstraintData, CostLineData, EvidenceItem, EvidenceKind,
            PlanStepData, RankedOption, Weights,
        },
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

pub fn diagram_kind_to_str(kind: DiagramKind) -> String {
    strip(kind.as_str_name(), "DIAGRAM_KIND_")
}

pub fn diagram_kind_from_str(value: &str) -> DiagramKind {
    DiagramKind::from_str_name(&format!("DIAGRAM_KIND_{}", value.to_uppercase()))
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

/// The delivery setup files with their notes (CONTRACT.md §22).
pub fn delivery_artifacts(files: &crate::generators::delivery::DeliveryFiles) -> DeliveryArtifacts {
    DeliveryArtifacts {
        artifacts: files.artifacts.iter().map(artifact).collect(),
        notes: files.notes.clone(),
        required_secrets: files.required_secrets.clone(),
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
    provider: &str,
) -> IntelligenceSettings {
    IntelligenceSettings {
        default_weights: Some(weights(&row.default_weights.0)),
        reasoner: row.reasoner.clone(),
        model: effective_model.clone(),
        api_key_configured,
        cost_ceiling_usd: row.cost_ceiling_usd,
        auto_review: row.auto_review,
        reasoner_provider: provider.to_owned(),
        reasoner_base_url: row.reasoner_base_url.clone(),
        reasoner_model: effective_model,
        reasoner_daily_token_cap: row.daily_token_cap(),
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

pub fn diagram_spec(spec: &DiagramSpec) -> ProtoDiagramSpec {
    ProtoDiagramSpec {
        direction: spec.direction.clone(),
        groups: spec
            .groups
            .iter()
            .map(|g| ProtoDiagramGroup {
                id: g.id.clone(),
                label: g.label.clone(),
                description: g.description.clone(),
                path: g.path.clone(),
            })
            .collect(),
        nodes: spec
            .nodes
            .iter()
            .map(|n| ProtoDiagramNode {
                id: n.id.clone(),
                label: n.label.clone(),
                kind: n.kind.clone(),
                group_id: n.group_id.clone(),
                path: n.path.clone(),
                description: n.description.clone(),
                caption: n.caption.clone(),
            })
            .collect(),
        edges: spec
            .edges
            .iter()
            .map(|e| ProtoDiagramEdge {
                id: e.id.clone(),
                source: e.source.clone(),
                target: e.target.clone(),
                label: e.label.clone(),
                style: e.style.clone(),
                dashed: e.dashed,
                evidence_path: e.evidence_path.clone(),
            })
            .collect(),
    }
}

pub fn diagram_spec_from_proto(spec: &ProtoDiagramSpec) -> DiagramSpec {
    DiagramSpec {
        direction: if spec.direction.trim().is_empty() {
            "LR".to_owned()
        } else {
            spec.direction.trim().to_uppercase()
        },
        groups: spec
            .groups
            .iter()
            .map(|g| DiagramGroup {
                id: g.id.trim().to_owned(),
                label: g.label.trim().to_owned(),
                description: g.description.clone(),
                path: g.path.trim().to_owned(),
            })
            .collect(),
        nodes: spec
            .nodes
            .iter()
            .map(|n| DiagramNode {
                id: n.id.trim().to_owned(),
                label: n.label.trim().to_owned(),
                kind: n.kind.trim().to_lowercase(),
                group_id: n.group_id.trim().to_owned(),
                path: n.path.trim().to_owned(),
                description: n.description.clone(),
                caption: n.caption.trim().to_owned(),
            })
            .collect(),
        edges: spec
            .edges
            .iter()
            .enumerate()
            .map(|(i, e)| DiagramEdge {
                id: if e.id.trim().is_empty() {
                    format!("e{}", i + 1)
                } else {
                    e.id.trim().to_owned()
                },
                source: e.source.trim().to_owned(),
                target: e.target.trim().to_owned(),
                label: e.label.trim().to_owned(),
                style: if e.style.trim().is_empty() {
                    "call".to_owned()
                } else {
                    e.style.trim().to_lowercase()
                },
                dashed: e.dashed,
                evidence_path: e.evidence_path.trim().to_owned(),
            })
            .collect(),
    }
}

pub fn diagram(row: &DiagramRow) -> Diagram {
    Diagram {
        id: row.id.to_string(),
        project_id: row.project_id.clone(),
        kind: diagram_kind_from_str(&row.kind) as i32,
        title: row.title.clone(),
        explanation: row.explanation.clone(),
        spec: Some(diagram_spec(&row.spec.0)),
        mermaid: row.mermaid.clone(),
        source: row.source.clone(),
        commit_sha: row.commit_sha.clone(),
        revision: row.revision.max(0) as u32,
        reasoner: row.reasoner.clone(),
        created_at: Some(time::to_proto(row.created_at)),
        updated_at: Some(time::to_proto(row.updated_at)),
    }
}

// --- Code review (CONTRACT.md §20) ----------------------------------------

pub fn review_status_from_str(value: &str) -> ReviewStatus {
    match value {
        "context" => ReviewStatus::Context,
        "analyzing" => ReviewStatus::Analyzing,
        "proposed" => ReviewStatus::Proposed,
        "published" => ReviewStatus::Published,
        "failed" => ReviewStatus::Failed,
        _ => ReviewStatus::Unspecified,
    }
}

pub fn severity_from_str(value: &str) -> FindingSeverity {
    match value {
        "info" => FindingSeverity::Info,
        "low" => FindingSeverity::Low,
        "medium" => FindingSeverity::Medium,
        "high" => FindingSeverity::High,
        "critical" => FindingSeverity::Critical,
        _ => FindingSeverity::Unspecified,
    }
}

pub fn decision_from_str(value: &str) -> FindingDecision {
    match value {
        "pending" => FindingDecision::Pending,
        "accepted" => FindingDecision::Accepted,
        "dismissed" => FindingDecision::Dismissed,
        _ => FindingDecision::Unspecified,
    }
}

pub fn finding(row: &FindingRow) -> Finding {
    Finding {
        id: row.id.to_string(),
        review_id: row.review_id.to_string(),
        category: row.category.clone(),
        severity: severity_from_str(&row.severity) as i32,
        title: row.title.clone(),
        detail: row.detail.clone(),
        path: row.path.clone(),
        line: row.line.max(0) as u32,
        end_line: row.end_line.max(0) as u32,
        suggestion: row.suggestion.clone(),
        rule: row.rule.clone(),
        decision: decision_from_str(&row.decision) as i32,
        decision_note: row.decision_note.clone(),
        decided_by: row.decided_by.clone(),
        decided_at: time::opt_to_proto(row.decided_at),
    }
}

/// `findings` is empty for list responses (summary omitted there too).
pub fn review(row: &ReviewRow, findings: &[FindingRow]) -> Review {
    Review {
        id: row.id.to_string(),
        project_id: row.project_id.clone(),
        project_name: row.project_name.clone(),
        pr_number: row.pr_number.max(0) as u64,
        pr_title: row.pr_title.clone(),
        pr_author: row.pr_author.clone(),
        head_sha: row.head_sha.clone(),
        status: review_status_from_str(&row.status) as i32,
        steps: row
            .steps
            .0
            .iter()
            .map(|s| ReviewStep {
                key: s.key.clone(),
                label: s.label.clone(),
                status: s.status.clone(),
                detail: s.detail.clone(),
                duration_ms: s.duration_ms,
            })
            .collect(),
        summary: row.summary.clone(),
        findings: findings.iter().map(finding).collect(),
        files_reviewed: row.files_reviewed.max(0) as u32,
        findings_total: row.findings_total.max(0) as u32,
        findings_accepted: row.findings_accepted.max(0) as u32,
        reasoner: row.reasoner.clone(),
        published_url: row.published_url.clone(),
        error: row.error.clone(),
        requested_by: row.requested_by.clone(),
        created_at: Some(time::to_proto(row.created_at)),
        updated_at: Some(time::to_proto(row.updated_at)),
    }
}

pub fn review_activity(row: &ActivityRow) -> ReviewActivity {
    ReviewActivity {
        id: row.id.to_string(),
        review_id: row.review_id.to_string(),
        project_id: row.project_id.clone(),
        project_name: row.project_name.clone(),
        pr_number: row.pr_number.max(0) as u64,
        action: row.action.clone(),
        actor: row.actor.clone(),
        detail: row.detail.clone(),
        at: Some(time::to_proto(row.at)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagram_spec_round_trips_through_proto() {
        let spec = DiagramSpec {
            direction: "TB".into(),
            groups: vec![DiagramGroup {
                id: "g".into(),
                label: "G".into(),
                description: String::new(),
                path: "src".into(),
            }],
            nodes: vec![DiagramNode {
                id: "a".into(),
                label: "A".into(),
                kind: "service".into(),
                group_id: "g".into(),
                path: "src/main.rs".into(),
                description: "d".into(),
                caption: "axum".into(),
            }],
            edges: vec![DiagramEdge {
                id: "e1".into(),
                source: "a".into(),
                target: "a".into(),
                label: "calls".into(),
                style: "call".into(),
                dashed: true,
                evidence_path: String::new(),
            }],
        };
        assert_eq!(diagram_spec_from_proto(&diagram_spec(&spec)), spec);
        let mut loose = diagram_spec(&spec);
        loose.direction = " lr ".into();
        loose.edges[0].id.clear();
        loose.edges[0].style.clear();
        let back = diagram_spec_from_proto(&loose);
        assert_eq!(back.direction, "LR");
        assert_eq!(back.edges[0].id, "e1");
        assert_eq!(back.edges[0].style, "call");
        assert_eq!(diagram_kind_to_str(DiagramKind::DbSchema), "db_schema");
        assert_eq!(diagram_kind_from_str("high_level"), DiagramKind::HighLevel);
    }

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
