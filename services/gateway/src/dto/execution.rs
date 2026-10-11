//! Execution area: executions, steps, events, approvals, policies, identities.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sdi_proto::execution::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{
    common::{Adapter, PlanStep, RiskLevel},
    opt_string, opt_timestamp, proto_enum, timestamp,
};

// ---------------------------------------------------------------------------
// Executions
// ---------------------------------------------------------------------------

proto_enum! {
    /// State of the execution state machine (CONTRACT.md §7).
    ExecutionStatus => proto::ExecutionStatus {
        Planning, WaitingForApproval, Authorized, Executing, Verifying, Paused,
        Diagnosing, Replanning, Succeeded, Failed, Cancelled
    }
}

proto_enum! {
    /// State of one step.
    StepStatus => proto::StepStatus {
        Pending, WaitingApproval, Ready, Running, Verifying, Succeeded, Failed, Blocked, Skipped, Cancelled
    }
}

proto_enum! {
    /// What the policy says to do with an action before it runs.
    PolicyDecision => proto::PolicyDecision { Auto, Approval, Required, Blocked }
}

/// One step of an execution.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Step id.
    pub id: String,
    /// Stable key within the plan.
    pub key: String,
    /// Position in the plan.
    pub position: u32,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Policy key, e.g. `deploy.staging`.
    pub action_type: String,
    /// Adapter that runs it.
    pub adapter: String,
    /// Keys of the steps it depends on.
    pub depends_on: Vec<String>,
    /// Status.
    pub status: StepStatus,
    /// Risk of the action.
    pub risk: RiskLevel,
    /// Policy decision applied.
    pub policy_decision: PolicyDecision,
    /// Permissions the identity must hold.
    pub required_permissions: Vec<String>,
    /// Approval gating this step, if any.
    pub approval_id: Option<String>,
    /// Adapter parameters.
    pub params: BTreeMap<String, String>,
    /// How success is verified.
    pub verification: String,
    /// Tool output of the last attempt.
    pub output: String,
    /// True once the result was verified and checkpointed.
    pub checkpointed: bool,
    /// Attempts so far.
    pub attempts: u32,
    /// When the last attempt started.
    pub started_at: Option<DateTime<Utc>>,
    /// When the last attempt finished.
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<proto::Step> for Step {
    fn from(step: proto::Step) -> Self {
        Self {
            id: step.id,
            key: step.key,
            position: step.position,
            title: step.title,
            description: step.description,
            action_type: step.action_type,
            adapter: step.adapter,
            depends_on: step.depends_on,
            status: step.status.into(),
            risk: step.risk.into(),
            policy_decision: step.policy_decision.into(),
            required_permissions: step.required_permissions,
            approval_id: opt_string(step.approval_id),
            params: step.params.into_iter().collect(),
            verification: step.verification,
            output: step.output,
            checkpointed: step.checkpointed,
            attempts: step.attempts,
            started_at: opt_timestamp(step.started_at.as_ref()),
            finished_at: opt_timestamp(step.finished_at.as_ref()),
        }
    }
}

/// A way out of a failure.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemediationOption {
    /// Option id, passed to `POST /executions/{id}/resolve`.
    pub id: String,
    /// Short label.
    pub label: String,
    /// Description.
    pub description: String,
    /// What choosing it changes in the plan.
    pub effect: String,
    /// True for the suggested option.
    pub recommended: bool,
}

impl From<proto::RemediationOption> for RemediationOption {
    fn from(option: proto::RemediationOption) -> Self {
        Self {
            id: option.id,
            label: option.label,
            description: option.description,
            effect: option.effect,
            recommended: option.recommended,
        }
    }
}

/// Why an execution is paused.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    /// Step that failed.
    pub step_key: String,
    /// Failure code (CONTRACT.md §5.5), e.g. `PERMISSION_DENIED`.
    pub code: String,
    /// Message.
    pub message: String,
    /// Raw tool or provider output.
    pub raw_output: String,
    /// Diagnosis in plain words.
    pub diagnosis: String,
    /// Remediation options.
    pub options: Vec<RemediationOption>,
    /// Step keys that cannot run until this is resolved.
    pub blocked_steps: Vec<String>,
    /// Permissions the identity was missing, for `PERMISSION_DENIED`.
    pub missing_permissions: Vec<String>,
    /// When it happened.
    pub occurred_at: DateTime<Utc>,
}

impl From<proto::Failure> for Failure {
    fn from(failure: proto::Failure) -> Self {
        Self {
            step_key: failure.step_key,
            code: failure.code,
            message: failure.message,
            raw_output: failure.raw_output,
            diagnosis: failure.diagnosis,
            options: failure.options.into_iter().map(Into::into).collect(),
            blocked_steps: failure.blocked_steps,
            missing_permissions: failure.missing_permissions,
            occurred_at: timestamp(failure.occurred_at.as_ref()),
        }
    }
}

/// A plan being run as a DAG, one action at a time.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    /// Execution id.
    pub id: String,
    /// Human-friendly sequence number.
    pub number: u64,
    /// Project it belongs to.
    pub project_id: String,
    /// Project display name.
    pub project_name: String,
    /// Proposal it came from, if any.
    pub proposal_id: Option<String>,
    /// Pattern it came from, if any.
    pub pattern_id: Option<String>,
    /// Title.
    pub title: String,
    /// Status.
    pub status: ExecutionStatus,
    /// Adapter in use; `simulated` is a dry run.
    pub adapter: String,
    /// Identity actions run as.
    pub identity_id: String,
    /// Steps in plan order.
    pub steps: Vec<Step>,
    /// Present while paused on a failure.
    pub failure: Option<Failure>,
    /// Key of the last checkpointed step.
    pub last_checkpoint: Option<String>,
    /// Bumped on every replan.
    pub revision: u32,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
    /// When it started running.
    pub started_at: Option<DateTime<Utc>>,
    /// When it reached a terminal state.
    pub finished_at: Option<DateTime<Utc>>,
    /// Things the run produced: `pull_request_url`, `image`, `container_url`, `workdir`.
    pub outputs: BTreeMap<String, String>,
}

impl From<proto::Execution> for Execution {
    fn from(execution: proto::Execution) -> Self {
        Self {
            id: execution.id,
            number: execution.number,
            project_id: execution.project_id,
            project_name: execution.project_name,
            proposal_id: opt_string(execution.proposal_id),
            pattern_id: opt_string(execution.pattern_id),
            title: execution.title,
            status: execution.status.into(),
            adapter: execution.adapter,
            identity_id: execution.identity_id,
            steps: execution.steps.into_iter().map(Into::into).collect(),
            failure: execution.failure.map(Into::into),
            last_checkpoint: opt_string(execution.last_checkpoint),
            revision: execution.revision,
            created_at: timestamp(execution.created_at.as_ref()),
            updated_at: timestamp(execution.updated_at.as_ref()),
            started_at: opt_timestamp(execution.started_at.as_ref()),
            finished_at: opt_timestamp(execution.finished_at.as_ref()),
            outputs: execution.outputs.into_iter().collect(),
        }
    }
}

/// Query of `GET /executions`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListExecutionsQuery {
    /// Only executions of this project. Empty lists every project.
    pub project_id: Option<String>,
    /// Maximum number, newest first. 0 uses the service default.
    pub limit: Option<u32>,
}

/// Body of `POST /executions`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateExecutionRequest {
    /// Project to run against.
    pub project_id: String,
    /// Proposal the plan came from, if any.
    pub proposal_id: Option<String>,
    /// Pattern the plan came from, if any.
    pub pattern_id: Option<String>,
    /// Title.
    pub title: String,
    /// Plan steps.
    pub steps: Vec<PlanStep>,
    /// Runtime for the whole run (CONTRACT.md §5.4). `simulated` by default.
    pub adapter: Option<Adapter>,
    /// Files a `repo.open_pull_request` step commits, keyed by repository path.
    pub artifacts: Option<BTreeMap<String, String>>,
}

/// Dry-run only: make one step fail on purpose.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FaultInjection {
    /// Step to fail.
    pub step_key: String,
    /// Failure code to raise (CONTRACT.md §5.5).
    pub code: String,
}

impl From<FaultInjection> for proto::FaultInjection {
    fn from(fault: FaultInjection) -> Self {
        Self {
            step_key: fault.step_key,
            code: fault.code,
        }
    }
}

/// Body of `POST /executions/{id}/start`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartExecutionRequest {
    /// Fault to inject during the dry run, if any.
    pub fault: Option<FaultInjection>,
}

/// Body of `POST /executions/{id}/resolve`. One of `optionId` or `instruction` is required.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveFailureRequest {
    /// One of the failure's remediation option ids.
    pub option_id: Option<String>,
    /// Free-text instruction instead of an option.
    pub instruction: Option<String>,
}

/// Body of `POST /executions/{id}/cancel`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelExecutionRequest {
    /// Why it was cancelled.
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

proto_enum! {
    /// What an execution event records.
    EventType => proto::EventType {
        StateChanged, PolicyChecked, PermissionChecked, PermissionDenied, ApprovalRequested,
        ApprovalDecided, StepStarted, StepSucceeded, StepFailed, StepBlocked, Checkpoint,
        Diagnosis, Replanned, Log
    }
}

/// One entry of the execution event log.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEvent {
    /// Monotonic per execution; use it to resume a watch.
    pub seq: u64,
    /// Execution it belongs to.
    pub execution_id: String,
    /// Event type.
    pub r#type: EventType,
    /// Step involved, if any.
    pub step_key: Option<String>,
    /// Message.
    pub message: String,
    /// Execution status after this event.
    pub status: ExecutionStatus,
    /// Extra data.
    pub data: BTreeMap<String, String>,
    /// When it happened.
    pub at: DateTime<Utc>,
}

impl From<proto::ExecutionEvent> for ExecutionEvent {
    fn from(event: proto::ExecutionEvent) -> Self {
        Self {
            seq: event.seq,
            execution_id: event.execution_id,
            r#type: event.r#type.into(),
            step_key: opt_string(event.step_key),
            message: event.message,
            status: event.status.into(),
            data: event.data.into_iter().collect(),
            at: timestamp(event.at.as_ref()),
        }
    }
}

/// Query of `GET /executions/{id}/events`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListEventsQuery {
    /// Only events with a greater sequence number.
    pub after_seq: Option<u64>,
    /// Maximum number of events. 0 uses the service default.
    pub limit: Option<u32>,
}

/// Query of `GET /executions/{id}/events/stream`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct WatchEventsQuery {
    /// Only events with a greater sequence number.
    pub after_seq: Option<u64>,
}

// ---------------------------------------------------------------------------
// Approvals
// ---------------------------------------------------------------------------

proto_enum! {
    /// State of an approval request.
    ApprovalStatus => proto::ApprovalStatus { Pending, Approved, Rejected, Expired }
}

/// A human decision requested before a step runs.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    /// Approval id.
    pub id: String,
    /// Execution it gates.
    pub execution_id: String,
    /// Execution sequence number.
    pub execution_number: u64,
    /// Execution title.
    pub execution_title: String,
    /// Project id.
    pub project_id: String,
    /// Project display name.
    pub project_name: String,
    /// Step key.
    pub step_key: String,
    /// Step title.
    pub step_title: String,
    /// Policy key of the action.
    pub action_type: String,
    /// Risk of the action.
    pub risk: RiskLevel,
    /// Policy decision that required the approval.
    pub policy_decision: PolicyDecision,
    /// Why a human is needed.
    pub reason: String,
    /// Permissions the step needs.
    pub required_permissions: Vec<String>,
    /// Status.
    pub status: ApprovalStatus,
    /// Who decided.
    pub decided_by: Option<String>,
    /// Note left with the decision.
    pub note: Option<String>,
    /// When it was requested.
    pub requested_at: DateTime<Utc>,
    /// When it was decided.
    pub decided_at: Option<DateTime<Utc>>,
}

impl From<proto::Approval> for Approval {
    fn from(approval: proto::Approval) -> Self {
        Self {
            id: approval.id,
            execution_id: approval.execution_id,
            execution_number: approval.execution_number,
            execution_title: approval.execution_title,
            project_id: approval.project_id,
            project_name: approval.project_name,
            step_key: approval.step_key,
            step_title: approval.step_title,
            action_type: approval.action_type,
            risk: approval.risk.into(),
            policy_decision: approval.policy_decision.into(),
            reason: approval.reason,
            required_permissions: approval.required_permissions,
            status: approval.status.into(),
            decided_by: opt_string(approval.decided_by),
            note: opt_string(approval.note),
            requested_at: timestamp(approval.requested_at.as_ref()),
            decided_at: opt_timestamp(approval.decided_at.as_ref()),
        }
    }
}

/// Query of `GET /approvals`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListApprovalsQuery {
    /// Only approvals in this status. Empty lists every status.
    pub status: Option<ApprovalStatus>,
    /// Only approvals of this execution.
    pub execution_id: Option<String>,
}

/// Body of `POST /approvals/{id}/decide`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecideApprovalRequest {
    /// True approves, false rejects.
    pub approve: bool,
    /// Optional note.
    pub note: Option<String>,
    /// Who decided.
    pub decided_by: Option<String>,
}

// ---------------------------------------------------------------------------
// Policy and identity
// ---------------------------------------------------------------------------

/// The decision applied to one action type.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    /// Stable key, e.g. `deploy.production`.
    pub action_type: String,
    /// Label.
    pub label: String,
    /// Description.
    pub description: String,
    /// Risk level.
    pub risk: RiskLevel,
    /// Current decision.
    pub decision: PolicyDecision,
    /// False for defaults that must not be relaxed.
    pub editable: bool,
    /// When it last changed.
    pub updated_at: Option<DateTime<Utc>>,
}

impl From<proto::Policy> for Policy {
    fn from(policy: proto::Policy) -> Self {
        Self {
            action_type: policy.action_type,
            label: policy.label,
            description: policy.description,
            risk: policy.risk.into(),
            decision: policy.decision.into(),
            editable: policy.editable,
            updated_at: opt_timestamp(policy.updated_at.as_ref()),
        }
    }
}

/// Body of `PATCH /policies/{actionType}`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePolicyRequest {
    /// New decision.
    pub decision: PolicyDecision,
}

/// One permission an identity holds.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionGrant {
    /// Permission, e.g. `database:write`.
    pub permission: String,
    /// Expiry; null for the identity's base scope.
    pub expires_at: Option<DateTime<Utc>>,
}

impl From<proto::PermissionGrant> for PermissionGrant {
    fn from(grant: proto::PermissionGrant) -> Self {
        Self {
            permission: grant.permission,
            expires_at: opt_timestamp(grant.expires_at.as_ref()),
        }
    }
}

/// The scoped identity actions run as. Never an administrator.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    /// Identity id.
    pub id: String,
    /// Name.
    pub name: String,
    /// `simulated`, `github` or `cloud`.
    pub kind: String,
    /// Description.
    pub description: String,
    /// Permissions currently held.
    pub grants: Vec<PermissionGrant>,
}

impl From<proto::Identity> for Identity {
    fn from(identity: proto::Identity) -> Self {
        Self {
            id: identity.id,
            name: identity.name,
            kind: identity.kind,
            description: identity.description,
            grants: identity.grants.into_iter().map(Into::into).collect(),
        }
    }
}

/// Body of `POST /identities/{id}/grants`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrantPermissionRequest {
    /// Permissions to grant.
    pub permissions: Vec<String>,
    /// Lifetime in minutes. Empty or 0 uses the default of 30.
    pub ttl_minutes: Option<u32>,
}

/// Query of `GET /delivery/stats`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DeliveryStatsQuery {
    /// Only this project. Empty aggregates every project.
    pub project_id: Option<String>,
}

/// Counters of the execution store.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryStats {
    /// Executions in total.
    pub executions_total: u32,
    /// Executions currently running or waiting.
    pub executions_active: u32,
    /// Executions paused on a failure.
    pub executions_paused: u32,
    /// Succeeded executions.
    pub executions_succeeded: u32,
    /// Failed executions.
    pub executions_failed: u32,
    /// 0.0 – 1.0 over finished executions.
    pub success_rate: f64,
    /// Mean duration in seconds.
    pub avg_duration_seconds: u32,
    /// Approvals waiting for a decision.
    pub approvals_pending: u32,
    /// Steps executed in total.
    pub steps_executed: u32,
    /// Failures resolved by replanning.
    pub failures_recovered: u32,
}

impl From<proto::DeliveryStats> for DeliveryStats {
    fn from(stats: proto::DeliveryStats) -> Self {
        Self {
            executions_total: stats.executions_total,
            executions_active: stats.executions_active,
            executions_paused: stats.executions_paused,
            executions_succeeded: stats.executions_succeeded,
            executions_failed: stats.executions_failed,
            success_rate: stats.success_rate,
            avg_duration_seconds: stats.avg_duration_seconds,
            approvals_pending: stats.approvals_pending,
            steps_executed: stats.steps_executed,
            failures_recovered: stats.failures_recovered,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_status_matches_the_contract_spelling() {
        assert_eq!(
            serde_json::to_string(&ExecutionStatus::WaitingForApproval).unwrap(),
            "\"waiting_for_approval\""
        );
        assert_eq!(
            ExecutionStatus::from(2),
            ExecutionStatus::WaitingForApproval
        );
    }

    #[test]
    fn event_serialises_with_type_and_rfc3339_timestamp() {
        let event = ExecutionEvent::from(proto::ExecutionEvent {
            seq: 3,
            execution_id: "e1".into(),
            r#type: proto::EventType::Checkpoint as i32,
            step_key: String::new(),
            message: "ok".into(),
            status: proto::ExecutionStatus::Executing as i32,
            data: Default::default(),
            at: Some(prost_types::Timestamp {
                seconds: 0,
                nanos: 0,
            }),
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "checkpoint");
        assert_eq!(json["stepKey"], serde_json::Value::Null);
        assert_eq!(json["at"], "1970-01-01T00:00:00Z");
        assert_eq!(json["executionId"], "e1");
    }
}
