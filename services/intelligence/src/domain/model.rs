//! Domain records that are persisted as JSON inside a proposal row. They
//! mirror the proto messages but derive serde, which generated types do not.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    SemanticSimilarity,
    Policy,
    Recency,
    Success,
    ArchitectureFit,
    Cost,
    Security,
}

impl Signal {
    pub const ALL: [Signal; 7] = [
        Signal::SemanticSimilarity,
        Signal::Policy,
        Signal::Recency,
        Signal::Success,
        Signal::ArchitectureFit,
        Signal::Cost,
        Signal::Security,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Signal::SemanticSimilarity => "semantic_similarity",
            Signal::Policy => "policy",
            Signal::Recency => "recency",
            Signal::Success => "success",
            Signal::ArchitectureFit => "architecture_fit",
            Signal::Cost => "cost",
            Signal::Security => "security",
        }
    }
}

/// CONTRACT.md §6 weights. Always kept normalised to a sum of 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weights {
    pub semantic_similarity: f64,
    pub policy: f64,
    pub recency: f64,
    pub success: f64,
    pub architecture_fit: f64,
    pub cost: f64,
    pub security: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            semantic_similarity: 0.20,
            policy: 0.20,
            recency: 0.10,
            success: 0.15,
            architecture_fit: 0.15,
            cost: 0.10,
            security: 0.10,
        }
    }
}

impl Weights {
    pub fn get(&self, signal: Signal) -> f64 {
        match signal {
            Signal::SemanticSimilarity => self.semantic_similarity,
            Signal::Policy => self.policy,
            Signal::Recency => self.recency,
            Signal::Success => self.success,
            Signal::ArchitectureFit => self.architecture_fit,
            Signal::Cost => self.cost,
            Signal::Security => self.security,
        }
    }

    fn slot(&mut self, signal: Signal) -> &mut f64 {
        match signal {
            Signal::SemanticSimilarity => &mut self.semantic_similarity,
            Signal::Policy => &mut self.policy,
            Signal::Recency => &mut self.recency,
            Signal::Success => &mut self.success,
            Signal::ArchitectureFit => &mut self.architecture_fit,
            Signal::Cost => &mut self.cost,
            Signal::Security => &mut self.security,
        }
    }

    /// Adds `delta` to one weight and renormalises so the sum stays 1.
    pub fn shift(&mut self, signal: Signal, delta: f64) {
        let slot = self.slot(signal);
        *slot = (*slot + delta).max(0.0);
        self.normalise();
    }

    pub fn normalise(&mut self) {
        let total: f64 = Signal::ALL.iter().map(|s| self.get(*s)).sum();
        if total <= f64::EPSILON {
            *self = Self::default();
            return;
        }
        for signal in Signal::ALL {
            *self.slot(signal) /= total;
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Breakdown {
    pub semantic_similarity: f64,
    pub policy: f64,
    pub recency: f64,
    pub success: f64,
    pub architecture_fit: f64,
    pub cost: f64,
    pub security: f64,
    pub overall: f64,
}

impl Breakdown {
    pub fn get(&self, signal: Signal) -> f64 {
        match signal {
            Signal::SemanticSimilarity => self.semantic_similarity,
            Signal::Policy => self.policy,
            Signal::Recency => self.recency,
            Signal::Success => self.success,
            Signal::ArchitectureFit => self.architecture_fit,
            Signal::Cost => self.cost,
            Signal::Security => self.security,
        }
    }

    pub fn weighted(&self, weights: &Weights) -> f64 {
        Signal::ALL
            .iter()
            .map(|s| weights.get(*s) * self.get(*s))
            .sum::<f64>()
            .clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Fact,
    Pattern,
    Decision,
    DeliveryHistory,
    SimilarProject,
    Policy,
    Answer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub kind: EvidenceKind,
    pub ref_id: String,
    pub label: String,
    pub detail: String,
}

impl EvidenceItem {
    pub fn new(
        kind: EvidenceKind,
        ref_id: impl Into<String>,
        label: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            ref_id: ref_id.into(),
            label: label.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStepData {
    pub key: String,
    pub title: String,
    pub description: String,
    pub action_type: String,
    pub adapter: String,
    pub depends_on: Vec<String>,
    pub required_permissions: Vec<String>,
    pub params: BTreeMap<String, String>,
    pub verification: String,
}

/// A generated file, complete and ready to commit (CONTRACT.md §14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactData {
    pub path: String,
    pub content: String,
    /// "dockerfile" | "yaml" | "hcl" | "bicep" | "json" | "markdown" | "nginx" | "text"
    pub language: String,
    pub purpose: String,
    pub replaces_existing: bool,
}

/// One line of an option's monthly cost model. The lines sum to
/// `RankedOption::estimated_monthly_cost_usd`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostLineData {
    pub item: String,
    pub monthly_usd: f64,
    pub note: String,
}

impl CostLineData {
    pub fn new(item: impl Into<String>, monthly_usd: f64, note: impl Into<String>) -> Self {
        Self {
            item: item.into(),
            monthly_usd,
            note: note.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankedOption {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub pattern_id: String,
    pub rank: u32,
    pub score: Breakdown,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
    pub evidence: Vec<EvidenceItem>,
    pub estimated_monthly_cost_usd: f64,
    pub unmet_requirements: Vec<String>,
    pub plan: Vec<PlanStepData>,
    /// Pattern tags (`cloud:*`, `runtime:*`, `iac:*`) so accepting the option
    /// can record a decision without refetching the pattern.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Generated files. Stored in the proposal row but stripped from list
    /// responses, so deserialising without them must work.
    #[serde(default)]
    pub artifacts: Vec<ArtifactData>,
    #[serde(default)]
    pub cost_breakdown: Vec<CostLineData>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstraintData {
    pub key: String,
    pub description: String,
    #[serde(default)]
    pub source_message_id: String,
}

/// Constraint key prefix of the monthly budget (CONTRACT.md §6 Budget).
pub const BUDGET_KEY: &str = "budget_monthly_usd";
/// Constraint key prefix of a negotiated generator parameter: `param:<name>:<value>`.
pub const PARAM_KEY: &str = "param";

impl ConstraintData {
    /// `require:x` / `prefer:x` / `exclude:x` - the term after the colon.
    pub fn split(&self) -> (&str, &str) {
        self.key
            .split_once(':')
            .unwrap_or(("require", self.key.as_str()))
    }

    pub fn budget(monthly_usd: f64) -> Self {
        Self {
            key: format!("{BUDGET_KEY}:{}", fmt_usd(monthly_usd)),
            description: format!("budget under ${}/month", fmt_usd(monthly_usd)),
            source_message_id: String::new(),
        }
    }

    pub fn param(name: &str, value: &str) -> Self {
        Self {
            key: format!("{PARAM_KEY}:{name}:{value}"),
            description: format!("{name} {value}"),
            source_message_id: String::new(),
        }
    }

    /// The budget this constraint carries, if it is one.
    pub fn as_budget(&self) -> Option<f64> {
        let (kind, value) = self.split();
        (kind == BUDGET_KEY).then(|| value.parse().ok()).flatten()
    }

    /// `(name, value)` when this is a negotiated generator parameter.
    pub fn as_param(&self) -> Option<(&str, &str)> {
        let (kind, rest) = self.split();
        (kind == PARAM_KEY).then(|| rest.split_once(':')).flatten()
    }

    /// Two constraints of the same family (budget, or the same parameter)
    /// replace each other instead of piling up.
    pub fn same_family(&self, other: &ConstraintData) -> bool {
        match (self.as_param(), other.as_param()) {
            (Some((a, _)), Some((b, _))) => a == b,
            _ => self.as_budget().is_some() && other.as_budget().is_some(),
        }
    }
}

/// Budgets print as integers when they are whole ("15", not "15.0").
pub fn fmt_usd(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.2}")
    }
}

/// The budget active on a proposal, if any constraint sets one.
pub fn budget_of(constraints: &[ConstraintData]) -> Option<f64> {
    constraints.iter().find_map(ConstraintData::as_budget)
}

/// Negotiated generator parameters (`region`, `port`, `runtime`), last one wins.
pub fn params_of(constraints: &[ConstraintData]) -> BTreeMap<String, String> {
    constraints
        .iter()
        .filter_map(ConstraintData::as_param)
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_sum_to_one() {
        let w = Weights::default();
        let total: f64 = Signal::ALL.iter().map(|s| w.get(*s)).sum();
        assert!((total - 1.0).abs() < 1e-9);
    }

    #[test]
    fn budget_and_param_constraints_round_trip() {
        let budget = ConstraintData::budget(15.0);
        assert_eq!(budget.key, "budget_monthly_usd:15");
        assert_eq!(budget.as_budget(), Some(15.0));
        assert_eq!(ConstraintData::budget(12.5).key, "budget_monthly_usd:12.50");
        let region = ConstraintData::param("region", "westeurope");
        assert_eq!(region.as_param(), Some(("region", "westeurope")));
        assert!(region.same_family(&ConstraintData::param("region", "eastus")));
        assert!(!region.same_family(&ConstraintData::param("port", "3000")));
        assert!(budget.same_family(&ConstraintData::budget(20.0)));
        assert!(!budget.same_family(&region));
        let all = vec![budget, region];
        assert_eq!(budget_of(&all), Some(15.0));
        assert_eq!(
            params_of(&all).get("region").map(String::as_str),
            Some("westeurope")
        );
    }

    #[test]
    fn shift_renormalises() {
        let mut w = Weights::default();
        w.shift(Signal::Cost, 0.15);
        let total: f64 = Signal::ALL.iter().map(|s| w.get(*s)).sum();
        assert!((total - 1.0).abs() < 1e-9);
        assert!(w.cost > Weights::default().cost);
        assert!(w.policy < Weights::default().policy);
    }
}
