//! Rows <-> proto messages, and the enum <-> column-text mapping. Columns
//! hold the proto name without its prefix ("PLANNING"), which keeps psql
//! output readable and the mapping lossless.

use std::collections::HashMap;

use sdi_common::time;
use sdi_proto::{
    common::v1::RiskLevel,
    execution::v1::{
        Approval, ApprovalStatus, EventType, Execution, ExecutionEvent, ExecutionStatus, Failure,
        Identity, PermissionGrant, Policy, PolicyDecision, RemediationOption, Step, StepStatus,
    },
};

use crate::{
    db::{
        approvals::ApprovalRow, events::EventRow, executions::ExecutionRow, identities::GrantRow,
        identities::IdentityRow, policies::PolicyRow, steps::StepRow,
    },
    domain::diagnosis::{self, FailureRecord},
};

fn strip<'a>(name: &'a str, prefix: &str) -> &'a str {
    name.strip_prefix(prefix).unwrap_or(name)
}

pub fn status_to_db(status: ExecutionStatus) -> &'static str {
    strip(status.as_str_name(), "EXECUTION_STATUS_")
}

pub fn status_from_db(text: &str) -> ExecutionStatus {
    ExecutionStatus::from_str_name(&format!("EXECUTION_STATUS_{text}")).unwrap_or_default()
}

pub fn step_status_to_db(status: StepStatus) -> &'static str {
    strip(status.as_str_name(), "STEP_STATUS_")
}

pub fn step_status_from_db(text: &str) -> StepStatus {
    StepStatus::from_str_name(&format!("STEP_STATUS_{text}")).unwrap_or_default()
}

pub fn decision_to_db(decision: PolicyDecision) -> &'static str {
    strip(decision.as_str_name(), "POLICY_DECISION_")
}

pub fn decision_from_db(text: &str) -> PolicyDecision {
    PolicyDecision::from_str_name(&format!("POLICY_DECISION_{text}")).unwrap_or_default()
}

pub fn risk_to_db(risk: RiskLevel) -> &'static str {
    strip(risk.as_str_name(), "RISK_LEVEL_")
}

pub fn risk_from_db(text: &str) -> RiskLevel {
    RiskLevel::from_str_name(&format!("RISK_LEVEL_{text}")).unwrap_or_default()
}

pub fn approval_status_to_db(status: ApprovalStatus) -> &'static str {
    strip(status.as_str_name(), "APPROVAL_STATUS_")
}

pub fn approval_status_from_db(text: &str) -> ApprovalStatus {
    ApprovalStatus::from_str_name(&format!("APPROVAL_STATUS_{text}")).unwrap_or_default()
}

pub fn event_type_to_db(kind: EventType) -> &'static str {
    strip(kind.as_str_name(), "EVENT_TYPE_")
}

pub fn event_type_from_db(text: &str) -> EventType {
    EventType::from_str_name(&format!("EVENT_TYPE_{text}")).unwrap_or_default()
}

pub fn step_to_proto(row: StepRow) -> Step {
    Step {
        id: row.id.to_string(),
        key: row.key,
        position: row.position.max(0) as u32,
        title: row.title,
        description: row.description,
        action_type: row.action_type,
        adapter: row.adapter,
        depends_on: row.depends_on,
        status: step_status_from_db(&row.status) as i32,
        risk: risk_from_db(&row.risk) as i32,
        policy_decision: decision_from_db(&row.policy_decision) as i32,
        required_permissions: row.required_permissions,
        approval_id: row.approval_id.map(|id| id.to_string()).unwrap_or_default(),
        params: row.params.0.into_iter().collect(),
        verification: row.verification,
        output: row.output,
        checkpointed: row.checkpointed,
        attempts: row.attempts.max(0) as u32,
        started_at: time::opt_to_proto(row.started_at),
        finished_at: time::opt_to_proto(row.finished_at),
    }
}

pub fn option_to_proto(option: diagnosis::RemediationOption) -> RemediationOption {
    RemediationOption {
        id: option.id,
        label: option.label,
        description: option.description,
        effect: option.effect,
        recommended: option.recommended,
    }
}

pub fn failure_to_proto(record: FailureRecord) -> Failure {
    Failure {
        step_key: record.step_key,
        code: record.code,
        message: record.message,
        raw_output: record.raw_output,
        diagnosis: record.diagnosis,
        options: record.options.into_iter().map(option_to_proto).collect(),
        blocked_steps: record.blocked_steps,
        missing_permissions: record.missing_permissions,
        occurred_at: Some(time::to_proto(record.occurred_at)),
    }
}

/// `include_failure` is false for list responses (CONTRACT.md: the failure
/// body is only sent on Get).
pub fn execution_to_proto(
    row: ExecutionRow,
    steps: Vec<StepRow>,
    include_failure: bool,
) -> Execution {
    Execution {
        id: row.id.to_string(),
        number: row.number.max(0) as u64,
        project_id: row.project_id,
        project_name: row.project_name,
        proposal_id: row.proposal_id,
        pattern_id: row.pattern_id,
        title: row.title,
        status: status_from_db(&row.status) as i32,
        adapter: row.adapter,
        identity_id: row.identity_id.to_string(),
        steps: steps.into_iter().map(step_to_proto).collect(),
        failure: if include_failure {
            row.failure.map(|json| failure_to_proto(json.0))
        } else {
            None
        },
        last_checkpoint: row.last_checkpoint.unwrap_or_default(),
        revision: row.revision.max(0) as u32,
        created_at: Some(time::to_proto(row.created_at)),
        updated_at: Some(time::to_proto(row.updated_at)),
        started_at: time::opt_to_proto(row.started_at),
        finished_at: time::opt_to_proto(row.finished_at),
    }
}

pub fn event_to_proto(row: EventRow) -> ExecutionEvent {
    ExecutionEvent {
        seq: row.seq.max(0) as u64,
        execution_id: row.execution_id.to_string(),
        r#type: event_type_from_db(&row.r#type) as i32,
        step_key: row.step_key,
        message: row.message,
        status: status_from_db(&row.status) as i32,
        data: row.data.0.into_iter().collect(),
        at: Some(time::to_proto(row.at)),
    }
}

pub fn approval_to_proto(row: ApprovalRow) -> Approval {
    Approval {
        id: row.id.to_string(),
        execution_id: row.execution_id.to_string(),
        execution_number: row.execution_number.max(0) as u64,
        execution_title: row.execution_title,
        project_id: row.project_id,
        project_name: row.project_name,
        step_key: row.step_key,
        step_title: row.step_title,
        action_type: row.action_type,
        risk: risk_from_db(&row.risk) as i32,
        policy_decision: decision_from_db(&row.policy_decision) as i32,
        reason: row.reason,
        required_permissions: row.required_permissions,
        status: approval_status_from_db(&row.status) as i32,
        decided_by: row.decided_by,
        note: row.note,
        requested_at: Some(time::to_proto(row.requested_at)),
        decided_at: time::opt_to_proto(row.decided_at),
    }
}

pub fn policy_to_proto(row: PolicyRow) -> Policy {
    Policy {
        action_type: row.action_type,
        label: row.label,
        description: row.description,
        risk: risk_from_db(&row.risk) as i32,
        decision: decision_from_db(&row.decision) as i32,
        editable: row.editable,
        updated_at: Some(time::to_proto(row.updated_at)),
    }
}

pub fn identity_to_proto(row: IdentityRow, grants: Vec<GrantRow>) -> Identity {
    Identity {
        id: row.id.to_string(),
        name: row.name,
        kind: row.kind,
        description: row.description,
        grants: grants
            .into_iter()
            .map(|grant| PermissionGrant {
                permission: grant.permission,
                expires_at: time::opt_to_proto(grant.expires_at),
            })
            .collect(),
    }
}

/// Groups grants by identity for the list response.
pub fn identities_to_proto(identities: Vec<IdentityRow>, grants: Vec<GrantRow>) -> Vec<Identity> {
    let mut by_identity: HashMap<uuid::Uuid, Vec<GrantRow>> = HashMap::new();
    for grant in grants {
        by_identity
            .entry(grant.identity_id)
            .or_default()
            .push(grant);
    }
    identities
        .into_iter()
        .map(|identity| {
            let grants = by_identity.remove(&identity.id).unwrap_or_default();
            identity_to_proto(identity, grants)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_text_round_trips() {
        for status in [
            ExecutionStatus::Planning,
            ExecutionStatus::WaitingForApproval,
            ExecutionStatus::Succeeded,
        ] {
            assert_eq!(status_from_db(status_to_db(status)), status);
        }
        assert_eq!(
            status_to_db(ExecutionStatus::WaitingForApproval),
            "WAITING_FOR_APPROVAL"
        );
        assert_eq!(
            step_status_from_db(step_status_to_db(StepStatus::WaitingApproval)),
            StepStatus::WaitingApproval
        );
        assert_eq!(decision_from_db("REQUIRED"), PolicyDecision::Required);
        assert_eq!(risk_from_db("HIGH"), RiskLevel::High);
        assert_eq!(approval_status_to_db(ApprovalStatus::Pending), "PENDING");
        assert_eq!(event_type_from_db("STEP_BLOCKED"), EventType::StepBlocked);
        assert_eq!(status_from_db("garbage"), ExecutionStatus::Unspecified);
    }
}
