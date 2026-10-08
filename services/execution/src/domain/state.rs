//! The state machines of CONTRACT.md §7 as data. Every status write in the
//! service goes through `execution_transition` / `step_transition`, so an
//! illegal move surfaces as FAILED_PRECONDITION instead of a corrupt row.

use sdi_proto::execution::v1::{ExecutionStatus, StepStatus};
use thiserror::Error;

use ExecutionStatus as E;
use StepStatus as S;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
#[error("illegal transition {from} -> {to}")]
pub struct IllegalTransition {
    pub from: &'static str,
    pub to: &'static str,
}

pub const EXECUTION_TERMINAL: [ExecutionStatus; 3] = [E::Succeeded, E::Failed, E::Cancelled];

pub fn is_terminal(status: ExecutionStatus) -> bool {
    EXECUTION_TERMINAL.contains(&status)
}

/// Where an execution may go from `from`. Cancel is allowed from every
/// non-terminal state; the rest mirrors the contract diagram.
pub fn execution_targets(from: ExecutionStatus) -> &'static [ExecutionStatus] {
    match from {
        E::Planning => &[E::Authorized, E::WaitingForApproval, E::Cancelled],
        E::WaitingForApproval => &[E::Authorized, E::Cancelled],
        E::Authorized => &[
            E::WaitingForApproval,
            E::Executing,
            E::Paused,
            E::Succeeded,
            E::Cancelled,
        ],
        E::Executing => &[E::Verifying, E::Paused, E::Cancelled],
        E::Verifying => &[E::Authorized, E::Succeeded, E::Paused, E::Cancelled],
        E::Paused => &[E::Diagnosing, E::Cancelled],
        E::Diagnosing => &[E::Replanning, E::Failed, E::Cancelled],
        E::Replanning => &[E::Authorized, E::WaitingForApproval, E::Cancelled],
        E::Succeeded | E::Failed | E::Cancelled | E::Unspecified => &[],
    }
}

pub fn execution_transition(
    from: ExecutionStatus,
    to: ExecutionStatus,
) -> Result<ExecutionStatus, IllegalTransition> {
    if execution_targets(from).contains(&to) {
        Ok(to)
    } else {
        Err(IllegalTransition {
            from: from.as_str_name(),
            to: to.as_str_name(),
        })
    }
}

pub fn step_targets(from: StepStatus) -> &'static [StepStatus] {
    match from {
        S::Pending => &[S::Ready, S::Blocked, S::Skipped, S::Cancelled],
        // READY -> FAILED is the permission / policy gate refusing the step.
        S::Ready => &[
            S::WaitingApproval,
            S::Running,
            S::Failed,
            S::Skipped,
            S::Cancelled,
        ],
        // -> PENDING when its approval expires on a replan.
        S::WaitingApproval => &[S::Running, S::Pending, S::Cancelled],
        S::Running => &[S::Verifying, S::Failed, S::Cancelled],
        S::Verifying => &[S::Succeeded, S::Failed, S::Cancelled],
        // A failed step is retried (PENDING) or skipped by a remediation.
        S::Failed => &[S::Pending, S::Skipped, S::Cancelled],
        // -> SKIPPED when the whole blocked branch is left out by a remediation.
        S::Blocked => &[S::Pending, S::Skipped, S::Cancelled],
        S::Succeeded | S::Skipped | S::Cancelled | S::Unspecified => &[],
    }
}

pub fn step_transition(from: StepStatus, to: StepStatus) -> Result<StepStatus, IllegalTransition> {
    if step_targets(from).contains(&to) {
        Ok(to)
    } else {
        Err(IllegalTransition {
            from: from.as_str_name(),
            to: to.as_str_name(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_EXECUTION: [ExecutionStatus; 12] = [
        E::Unspecified,
        E::Planning,
        E::WaitingForApproval,
        E::Authorized,
        E::Executing,
        E::Verifying,
        E::Paused,
        E::Diagnosing,
        E::Replanning,
        E::Succeeded,
        E::Failed,
        E::Cancelled,
    ];

    const ALL_STEP: [StepStatus; 11] = [
        S::Unspecified,
        S::Pending,
        S::WaitingApproval,
        S::Ready,
        S::Running,
        S::Verifying,
        S::Succeeded,
        S::Failed,
        S::Blocked,
        S::Skipped,
        S::Cancelled,
    ];

    #[test]
    fn every_execution_pair_is_either_listed_or_rejected() {
        for from in ALL_EXECUTION {
            for to in ALL_EXECUTION {
                let allowed = execution_targets(from).contains(&to);
                let result = execution_transition(from, to);
                assert_eq!(result.is_ok(), allowed, "{from:?} -> {to:?}");
                if let Err(err) = result {
                    assert_eq!(err.from, from.as_str_name());
                    assert_eq!(err.to, to.as_str_name());
                }
            }
        }
    }

    #[test]
    fn every_step_pair_is_either_listed_or_rejected() {
        for from in ALL_STEP {
            for to in ALL_STEP {
                let allowed = step_targets(from).contains(&to);
                assert_eq!(
                    step_transition(from, to).is_ok(),
                    allowed,
                    "{from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn terminal_states_have_no_exits_and_no_self_loops() {
        for status in ALL_EXECUTION {
            assert!(
                !execution_targets(status).contains(&status),
                "{status:?} loops"
            );
            if is_terminal(status) || status == E::Unspecified {
                assert!(execution_targets(status).is_empty(), "{status:?} has exits");
            } else {
                assert!(
                    execution_targets(status).contains(&E::Cancelled),
                    "{status:?} cannot be cancelled"
                );
            }
        }
        for status in [S::Succeeded, S::Skipped, S::Cancelled, S::Unspecified] {
            assert!(step_targets(status).is_empty());
        }
    }

    #[test]
    fn the_happy_path_is_legal_and_the_shortcuts_are_not() {
        let path = [
            E::Planning,
            E::Authorized,
            E::WaitingForApproval,
            E::Authorized,
            E::Executing,
            E::Verifying,
            E::Authorized,
            E::Executing,
            E::Paused,
            E::Diagnosing,
            E::Replanning,
            E::Authorized,
            E::Executing,
            E::Verifying,
            E::Succeeded,
        ];
        for pair in path.windows(2) {
            assert!(execution_transition(pair[0], pair[1]).is_ok(), "{pair:?}");
        }
        assert!(execution_transition(E::Planning, E::Executing).is_err());
        assert!(execution_transition(E::Executing, E::Succeeded).is_err());
        assert!(execution_transition(E::Paused, E::Replanning).is_err());
        assert!(execution_transition(E::Diagnosing, E::Authorized).is_err());
        assert!(execution_transition(E::Succeeded, E::Cancelled).is_err());
        assert!(step_transition(S::Pending, S::Running).is_err());
        assert!(step_transition(S::Succeeded, S::Pending).is_err());
    }
}
