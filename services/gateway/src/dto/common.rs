//! Shapes shared by several areas: plan steps, risk levels, knowledge scope,
//! execution adapters, platform events and the error body every failed
//! request returns.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sdi_proto::common::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{opt_string, proto_enum, timestamp};

/// Paging of a time-ordered list (CONTRACT.md §21): `limit` (default 50)
/// and a `before` cursor.
#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// Page size, newest first (default 50, max 200).
    pub limit: Option<u32>,
    /// Cursor: items created strictly before this time.
    pub before: Option<DateTime<Utc>>,
}

proto_enum! {
    /// How much damage an action can do. Drives the approval policy.
    RiskLevel => proto::RiskLevel { Safe, Medium, High, Blocked }
}

proto_enum! {
    /// Who may reuse a piece of knowledge during retrieval.
    KnowledgeScope => proto::KnowledgeScope { Private, Team, Department, Organization, PublicTemplate }
}

/// Runtime that runs a whole execution (CONTRACT.md §5.4). The proto carries
/// it as a free string; the API narrows it to the adapters that exist so a
/// typo is a 400 here instead of an `INVALID_ARGUMENT` three hops later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Adapter {
    /// Timed dry run that never touches anything. The default.
    #[default]
    Simulated,
    /// Real local execution on the host's Docker daemon.
    Docker,
    /// Writes to the connected repository (branch, commit, pull request).
    Github,
}

impl Adapter {
    /// The exact string the execution service matches on.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Simulated => "simulated",
            Self::Docker => "docker",
            Self::Github => "github",
        }
    }
}

/// A change notification from one of the services, as streamed by
/// `GET /events/stream`. The web app invalidates its query caches by
/// `entity` (CONTRACT.md §8, "Live events").
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlatformEvent {
    /// `project`, `scan`, `proposal`, `decision`, `document`, `execution`, `approval`, `pattern`,
    /// or, for continuous delivery, `release`, `deployment` or `environment`.
    pub entity: String,
    /// Id of the entity that changed.
    pub entity_id: String,
    /// Project the entity belongs to; null for entities not tied to one.
    pub project_id: Option<String>,
    /// `created`, `updated`, `deleted` or `progress`.
    pub action: String,
    /// Entity status after the change, in the entity's own vocabulary (e.g. `scanning`).
    pub status: String,
    /// When it happened.
    pub at: DateTime<Utc>,
    /// Extra data.
    pub data: BTreeMap<String, String>,
}

impl From<proto::PlatformEvent> for PlatformEvent {
    fn from(event: proto::PlatformEvent) -> Self {
        Self {
            entity: event.entity,
            entity_id: event.entity_id,
            project_id: opt_string(event.project_id),
            action: event.action,
            status: event.status,
            at: timestamp(event.at.as_ref()),
            data: event.data.into_iter().collect(),
        }
    }
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

    #[test]
    fn adapter_round_trips_as_the_service_string() {
        let adapter: Adapter = serde_json::from_str("\"github\"").unwrap();
        assert_eq!(adapter, Adapter::Github);
        assert_eq!(adapter.as_str(), "github");
        assert_eq!(Adapter::default(), Adapter::Simulated);
        assert!(serde_json::from_str::<Adapter>("\"cloud\"").is_err());
    }

    #[test]
    fn platform_event_nulls_an_empty_project_id() {
        let event = PlatformEvent::from(proto::PlatformEvent {
            entity: "pattern".into(),
            entity_id: "p1".into(),
            project_id: String::new(),
            action: "updated".into(),
            status: String::new(),
            at: None,
            data: Default::default(),
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["projectId"], serde_json::Value::Null);
        assert_eq!(json["entityId"], "p1");
        assert_eq!(json["at"], "1970-01-01T00:00:00Z");
    }
}
