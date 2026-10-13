//! Policy lookup and permission checks. Unknown action types are treated as
//! the most cautious editable default (high risk, human required) so a typo
//! in a plan never becomes an auto-run.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sdi_proto::{common::v1::RiskLevel, execution::v1::PolicyDecision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyRule {
    pub risk: RiskLevel,
    pub decision: PolicyDecision,
}

pub const UNKNOWN_ACTION_RULE: PolicyRule = PolicyRule {
    risk: RiskLevel::High,
    decision: PolicyDecision::Required,
};

pub fn lookup(policies: &HashMap<String, PolicyRule>, action_type: &str) -> PolicyRule {
    policies
        .get(action_type)
        .copied()
        .unwrap_or(UNKNOWN_ACTION_RULE)
}

/// What the gate does with a step before it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Run,
    AskHuman,
    Block,
}

pub fn gate(decision: PolicyDecision) -> Gate {
    match decision {
        PolicyDecision::Auto => Gate::Run,
        PolicyDecision::Approval | PolicyDecision::Required => Gate::AskHuman,
        PolicyDecision::Blocked | PolicyDecision::Unspecified => Gate::Block,
    }
}

/// Whether an operator may change a policy row to `decision`.
/// Non-editable rows never change; editable rows can never be blocked
/// (blocking is a seeded decision, not a knob).
pub fn update_allowed(editable: bool, decision: PolicyDecision) -> Result<(), &'static str> {
    if !editable {
        return Err("policy is not editable");
    }
    match decision {
        PolicyDecision::Auto | PolicyDecision::Approval | PolicyDecision::Required => Ok(()),
        PolicyDecision::Blocked => Err("BLOCKED cannot be chosen for an editable policy"),
        PolicyDecision::Unspecified => Err("decision must be set"),
    }
}

/// The permission that changes repository settings (CONTRACT.md §5.3,
/// §23): known like every `<resource>:<verb>`, held by no seeded identity,
/// so a step that needs it always pauses for a timed grant first.
pub const REPOSITORY_ADMIN: &str = "repository:admin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub permission: String,
    /// `None` is a base-scope grant that never expires.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Required permissions the identity does not hold at `now`. Expired grants
/// count as absent.
pub fn missing_permissions(
    required: &[String],
    grants: &[Grant],
    now: DateTime<Utc>,
) -> Vec<String> {
    required
        .iter()
        .filter(|permission| {
            !grants.iter().any(|grant| {
                grant.permission == **permission && grant.expires_at.is_none_or(|exp| exp > now)
            })
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn rules() -> HashMap<String, PolicyRule> {
        HashMap::from([
            (
                "repo.read".to_string(),
                PolicyRule {
                    risk: RiskLevel::Safe,
                    decision: PolicyDecision::Auto,
                },
            ),
            (
                "deploy.production".to_string(),
                PolicyRule {
                    risk: RiskLevel::High,
                    decision: PolicyDecision::Required,
                },
            ),
        ])
    }

    #[test]
    fn known_action_types_resolve_and_unknown_ones_are_cautious() {
        let rules = rules();
        assert_eq!(lookup(&rules, "repo.read").decision, PolicyDecision::Auto);
        assert_eq!(lookup(&rules, "deploy.production").risk, RiskLevel::High);
        assert_eq!(lookup(&rules, "made.up"), UNKNOWN_ACTION_RULE);
        assert_eq!(gate(lookup(&rules, "made.up").decision), Gate::AskHuman);
    }

    #[test]
    fn gate_maps_every_decision() {
        assert_eq!(gate(PolicyDecision::Auto), Gate::Run);
        assert_eq!(gate(PolicyDecision::Approval), Gate::AskHuman);
        assert_eq!(gate(PolicyDecision::Required), Gate::AskHuman);
        assert_eq!(gate(PolicyDecision::Blocked), Gate::Block);
        assert_eq!(gate(PolicyDecision::Unspecified), Gate::Block);
    }

    #[test]
    fn policy_updates_respect_editable_and_forbid_blocked() {
        assert!(update_allowed(true, PolicyDecision::Auto).is_ok());
        assert!(update_allowed(true, PolicyDecision::Required).is_ok());
        assert!(update_allowed(true, PolicyDecision::Blocked).is_err());
        assert!(update_allowed(true, PolicyDecision::Unspecified).is_err());
        assert!(update_allowed(false, PolicyDecision::Auto).is_err());
    }

    #[test]
    fn expired_grants_are_ignored() {
        let now = Utc::now();
        let grants = vec![
            Grant {
                permission: "repository:read".into(),
                expires_at: None,
            },
            Grant {
                permission: "deploy:staging".into(),
                expires_at: Some(now + Duration::minutes(5)),
            },
            Grant {
                permission: "deploy:production".into(),
                expires_at: Some(now - Duration::minutes(5)),
            },
        ];
        let required = vec![
            "repository:read".to_string(),
            "deploy:staging".to_string(),
            "deploy:production".to_string(),
            "secrets:write".to_string(),
        ];
        assert_eq!(
            missing_permissions(&required, &grants, now),
            vec!["deploy:production".to_string(), "secrets:write".to_string()]
        );
        assert!(missing_permissions(&[], &grants, now).is_empty());
    }
}
