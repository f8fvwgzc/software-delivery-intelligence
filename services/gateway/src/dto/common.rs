//! Shapes shared by several areas: plan steps, risk levels, knowledge scope
//! and the error body every failed request returns.

use std::collections::BTreeMap;

use sdi_proto::common::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::proto_enum;

proto_enum! {
    /// How much damage an action can do. Drives the approval policy.
    RiskLevel => proto::RiskLevel { Safe, Medium, High, Blocked }
}

proto_enum! {
    /// Who may reuse a piece of knowledge during retrieval.
    KnowledgeScope => proto::KnowledgeScope { Private, Team, Department, Organization, PublicTemplate }
}

/// One unit of work in a plan. Shared by patterns, proposal options and
/// executions so a plan keeps its shape end to end.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    /// Stable key within the plan, referenced by `dependsOn`.
    pub key: String,
    /// Short human title of the step.
    pub title: String,
    /// What the step does, in plain words.
    pub description: String,
    /// Policy key such as `cloud.create_dev_resource`.
    pub action_type: String,
    /// Tool adapter that would run it: `github`, `docker`, `cloud`, `kubernetes`, `local` or `simulated`.
    pub adapter: String,
    /// Keys of the steps that must succeed first (edges of the execution DAG).
    pub depends_on: Vec<String>,
    /// Permissions the executing identity must hold, e.g. `database:write`.
    pub required_permissions: Vec<String>,
    /// Free-form parameters handed to the adapter.
    pub params: BTreeMap<String, String>,
    /// How success is verified after the action, in plain words.
    pub verification: String,
}

impl From<proto::PlanStep> for PlanStep {
    fn from(step: proto::PlanStep) -> Self {
        Self {
            key: step.key,
            title: step.title,
            description: step.description,
            action_type: step.action_type,
            adapter: step.adapter,
            depends_on: step.depends_on,
            required_permissions: step.required_permissions,
            params: step.params.into_iter().collect(),
            verification: step.verification,
        }
    }
}

impl From<PlanStep> for proto::PlanStep {
    fn from(step: PlanStep) -> Self {
        Self {
            key: step.key,
            title: step.title,
            description: step.description,
            action_type: step.action_type,
            adapter: step.adapter,
            depends_on: step.depends_on,
            required_permissions: step.required_permissions,
            params: step.params.into_iter().collect(),
            verification: step.verification,
        }
    }
}

/// Body of every non-2xx response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorBody {
    /// Stable machine code, e.g. `not_found`, `invalid_argument`, `unavailable`.
    pub code: String,
    /// Human-readable explanation safe to show in the UI.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_strip_the_proto_prefix_and_lowercase() {
        assert_eq!(serde_json::to_string(&RiskLevel::High).unwrap(), "\"high\"");
        assert_eq!(
            serde_json::to_string(&KnowledgeScope::PublicTemplate).unwrap(),
            "\"public_template\""
        );
        assert_eq!(
            serde_json::to_string(&RiskLevel::Unspecified).unwrap(),
            "\"unspecified\""
        );
    }

    #[test]
    fn unknown_proto_values_become_unspecified() {
        assert_eq!(RiskLevel::from(99), RiskLevel::Unspecified);
        assert_eq!(RiskLevel::from(3), RiskLevel::High);
        assert_eq!(i32::from(RiskLevel::High), 3);
    }

    #[test]
    fn enums_deserialize_from_snake_case() {
        let scope: KnowledgeScope = serde_json::from_str("\"public_template\"").unwrap();
        assert_eq!(scope, KnowledgeScope::PublicTemplate);
    }
}
