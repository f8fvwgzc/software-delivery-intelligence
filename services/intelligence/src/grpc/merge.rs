//! Merge (CONTRACT.md §23.3). `GetMergeReadiness` reads GitHub now (through
//! knowledge) and adds SDI's own gate; `MergePullRequest` refuses unless
//! nothing blocks, then hands a `verify_mergeable → merge → delete_branch`
//! plan to the execution service. The merge itself is an execution: its
//! policy decides whether a person approves it.

use std::collections::HashMap;

use chrono::Utc;
use sdi_common::grpc::db_status;
use sdi_proto::{
    intelligence::v1::{
        GetMergeReadinessRequest, MergeBlocker, MergePullRequestRequest, MergePullRequestResponse,
        MergeReadiness,
    },
    knowledge::v1::PullRequestStatus,
};
use tonic::Status;
use uuid::Uuid;

use super::{
    Caller, IntelligenceGrpc,
    autopilot::{self, Liveness, Recorded},
    require,
    review_runner::emit,
};
use crate::{
    db::{
        autopilot::{self as autopilot_repo, KIND_MERGE},
        reviews::{self as repo, ActivityRow, ReviewRow},
        settings::SettingsRow,
    },
    domain::{
        merge::{self, Blocker, Gate, HeadReview, MethodError},
        plans,
    },
};

/// SDI's side of the gate for one pull request at one head commit, from
/// our own tables (and the execution service while a merge execution has
/// not been seen to end). Also returns the review of that head, if any.
pub(super) async fn gate(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    project_id: &str,
    pr_number: u64,
    head_sha: &str,
    settings: &SettingsRow,
) -> Result<(Gate, Option<ReviewRow>), Status> {
    let number = i64::try_from(pr_number).unwrap_or(i64::MAX);
    let newest = repo::latest_for_pr(&svc.pool, caller.workspace, project_id, number)
        .await
        .map_err(db_status)?;
    let at_head = if head_sha.is_empty() {
        None
    } else {
        repo::find_by_head(&svc.pool, caller.workspace, project_id, number, head_sha)
            .await
            .map_err(db_status)?
    };
    let head_review = match &at_head {
        Some(review) => Some(HeadReview {
            status: review.status.clone(),
            error: review.error.clone(),
            undecided_blocking: u32::try_from(
                repo::undecided_blocking(&svc.pool, caller.workspace, review.id)
                    .await
                    .map_err(db_status)?,
            )
            .unwrap_or(u32::MAX),
        }),
        None => None,
    };
    let mut gate = Gate {
        requires_review: settings.merge_requires_review,
        newest_review_head: newest.map(|r| r.head_sha),
        head_review,
        open_merge_execution: None,
        merge_execution_unconfirmed: false,
    };
    if let Some(latest) =
        autopilot_repo::latest(&svc.pool, caller.workspace, project_id, KIND_MERGE, number)
            .await
            .map_err(db_status)?
    {
        match autopilot::liveness(svc, caller, &latest).await {
            Liveness::Open(_) => gate.open_merge_execution = Some(latest.execution_id),
            // Not knowing is not a yes: the merge stays blocked, and says why.
            Liveness::Unknown(_) => {
                gate.open_merge_execution = Some(latest.execution_id);
                gate.merge_execution_unconfirmed = true;
            }
            Liveness::Ended(_) => {}
        }
    }
    Ok((gate, at_head))
}

/// What a readiness check found: the answer for the caller, and what a
/// merge request built on it needs.
pub(super) struct Readiness {
    pub proto: MergeReadiness,
    pub status: PullRequestStatus,
    pub head_review: Option<ReviewRow>,
    pub blockers: Vec<Blocker>,
}

fn to_proto(
    project_id: &str,
    pr_number: u64,
    status: &PullRequestStatus,
    gate: &Gate,
    head_review: Option<&ReviewRow>,
    blockers: &[Blocker],
) -> MergeReadiness {
    let summary = status.summary.clone().unwrap_or_default();
    let checks = status.checks.clone().unwrap_or_default();
    MergeReadiness {
        project_id: project_id.to_owned(),
        pr_number,
        title: summary.title,
        state: summary.state,
        head_sha: summary.head_sha,
        head_ref: summary.head_ref,
        base_ref: summary.base_ref,
        is_fork: summary.is_fork,
        is_dependabot: summary.is_dependabot,
        mergeable: status.mergeable.clone(),
        mergeable_state: status.mergeable_state.clone(),
        checks_total: checks.total,
        checks_passed: checks.passed,
        checks_failed: checks.failed,
        checks_pending: checks.pending,
        failed_checks: checks.failed_names,
        pending_checks: checks.pending_names,
        approvals: status.approvals,
        changes_requested: status.changes_requested,
        allowed_methods: status.allowed_methods.clone(),
        base_protected: status.base_protected,
        behind_by: status.behind_by,
        review_id: head_review.map(|r| r.id.to_string()).unwrap_or_default(),
        review_status: head_review.map(|r| r.status.clone()).unwrap_or_default(),
        undecided_blocking_findings: gate
            .head_review
            .as_ref()
            .map(|r| r.undecided_blocking)
            .unwrap_or(0),
        blockers: blockers
            .iter()
            .map(|b| MergeBlocker {
                code: b.code.to_owned(),
                message: b.message.clone(),
            })
            .collect(),
        can_merge: blockers.is_empty(),
        merge_execution_id: gate.open_merge_execution.clone().unwrap_or_default(),
        checked_at: status
            .checked_at
            .or_else(|| Some(sdi_common::time::to_proto(Utc::now()))),
    }
}

/// GitHub's view of the pull request plus the gate.
pub(super) async fn compute(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    project_id: &str,
    pr_number: u64,
    settings: &SettingsRow,
) -> Result<Readiness, Status> {
    let status = svc
        .knowledge
        .pull_request_status(&caller.principal, project_id, pr_number)
        .await
        .map_err(|e| e.into_status())?;
    let head_sha = status
        .summary
        .as_ref()
        .map(|s| s.head_sha.clone())
        .unwrap_or_default();
    let (gate, head_review) = gate(svc, caller, project_id, pr_number, &head_sha, settings).await?;
    let blockers = merge::blockers(&status, &gate);
    Ok(Readiness {
        proto: to_proto(
            project_id,
            pr_number,
            &status,
            &gate,
            head_review.as_ref(),
            &blockers,
        ),
        status,
        head_review,
        blockers,
    })
}

pub(super) async fn readiness(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: GetMergeReadinessRequest,
) -> Result<MergeReadiness, Status> {
    let project_id = require("project_id", &request.project_id)?;
    if request.pr_number == 0 {
        return Err(Status::invalid_argument("pr_number is required"));
    }
    let settings = svc.settings(caller).await?;
    Ok(
        compute(svc, caller, &project_id, request.pr_number, &settings)
            .await?
            .proto,
    )
}

/// What was asked for: which pull request, how, and who is on record.
pub(super) struct MergeAsk<'a> {
    pub project_id: &'a str,
    pub pr_number: u64,
    /// "" picks the first method the repository allows.
    pub method: &'a str,
    pub delete_branch: bool,
    /// The activity log's actor: the user, or `watcher`.
    pub actor: &'a str,
}

/// Asks for the merge of a pull request whose readiness has no blocker:
/// one execution, recorded, announced. Shared by `MergePullRequest` and the
/// watcher's auto-merge, which differ only in who the caller is.
pub(super) async fn request_merge(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    asked: MergeAsk<'_>,
    mut ready: Readiness,
) -> Result<(String, MergeReadiness), Status> {
    let MergeAsk {
        project_id,
        pr_number,
        method,
        delete_branch,
        actor,
    } = asked;
    if let Some(first) = ready.blockers.first() {
        return Err(Status::failed_precondition(first.message.clone()));
    }
    let method =
        merge::pick_method(method, &ready.status.allowed_methods).map_err(|error| match error {
            MethodError::Unknown(_) => Status::invalid_argument(error.sentence()),
            _ => Status::failed_precondition(error.sentence()),
        })?;
    let summary = ready.status.summary.clone().unwrap_or_default();
    if summary.head_sha.trim().is_empty() {
        return Err(Status::failed_precondition(
            "GitHub did not name the head commit of this pull request; nothing can be merged safely",
        ));
    }
    let project = svc
        .knowledge
        .project(&caller.principal, project_id)
        .await
        .map_err(|e| e.into_status())?;
    // A branch in another repository is the author's: never deleted.
    let same_repository = !summary.is_fork
        && (summary.head_repo.trim().is_empty()
            || summary
                .head_repo
                .trim()
                .eq_ignore_ascii_case(project.repo_full_name.trim()));
    let branch = (delete_branch && same_repository && !summary.head_ref.trim().is_empty())
        .then(|| summary.head_ref.trim().to_owned());
    let steps = plans::merge_plan(&plans::MergePlan {
        repository: &project.repo_full_name,
        pr_number,
        expected_head_sha: &summary.head_sha,
        method: &method,
        delete_branch: branch.as_deref(),
    });
    let execution_id = autopilot::create_execution(
        svc,
        caller,
        project_id,
        merge::merge_title(pr_number, &summary.title),
        &steps,
        HashMap::new(),
    )
    .await?;
    let number = i64::try_from(pr_number).unwrap_or(i64::MAX);
    autopilot::record(
        svc,
        caller,
        Recorded {
            kind: KIND_MERGE,
            project_id,
            pr_number: number,
            review_id: ready.head_review.as_ref().map(|r| r.id),
            head_sha: &summary.head_sha,
            execution_id: &execution_id,
        },
    )
    .await?;

    let detail = format!(
        "{method} merge of {} into {}, execution {execution_id}{}",
        summary.head_sha.chars().take(12).collect::<String>(),
        if summary.base_ref.is_empty() {
            "its base"
        } else {
            summary.base_ref.as_str()
        },
        match (&branch, delete_branch) {
            (Some(branch), _) => format!("; {branch} is deleted afterwards"),
            (None, true) => "; the branch is kept (it lives in another repository)".to_owned(),
            (None, false) => String::new(),
        }
    );
    // The activity log is keyed by review; a merge without one (the gate is
    // off) is still worth a line.
    let review = ready.head_review.as_ref();
    repo::insert_activity(
        &svc.pool,
        &ActivityRow {
            id: Uuid::now_v7(),
            workspace_id: caller.workspace,
            review_id: review.map(|r| r.id).unwrap_or_else(Uuid::nil),
            project_id: project_id.to_owned(),
            project_name: review
                .map(|r| r.project_name.clone())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| project.name.clone()),
            pr_number: number,
            action: "merge.requested".to_owned(),
            actor: actor.to_owned(),
            detail: detail.clone(),
            at: Utc::now(),
        },
    )
    .await
    .map_err(db_status)?;
    svc.audit(
        caller,
        "pull_request.merge_requested",
        "pull_request",
        &format!("{project_id}#{pr_number}"),
        detail,
    );
    if let Some(review) = review {
        emit(&svc.events, review, "updated", &review.status, "merge");
    }

    // The readiness as it is now: this execution stands in the way of another.
    let blocker = Blocker {
        code: "merge_in_progress",
        message: format!(
            "Merge execution {execution_id} for this pull request is still open; approve, finish or cancel it first."
        ),
    };
    ready.proto.blockers = vec![MergeBlocker {
        code: blocker.code.to_owned(),
        message: blocker.message,
    }];
    ready.proto.can_merge = false;
    ready.proto.merge_execution_id = execution_id.clone();
    Ok((execution_id, ready.proto))
}

/// The key under which merge requests for one pull request take turns
/// (people and the watcher alike).
pub(super) fn merge_lock(project_id: &str, pr_number: u64) -> String {
    format!("merge:{project_id}:{pr_number}")
}

pub(super) async fn merge(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: MergePullRequestRequest,
) -> Result<MergePullRequestResponse, Status> {
    let project_id = require("project_id", &request.project_id)?;
    if request.pr_number == 0 {
        return Err(Status::invalid_argument("pr_number is required"));
    }
    // A method that is not a method is the request's fault, whatever the
    // pull request looks like.
    let method = request.method.trim().to_ascii_lowercase();
    if !method.is_empty() && !merge::METHODS.contains(&method.as_str()) {
        return Err(Status::invalid_argument(
            MethodError::Unknown(method).sentence(),
        ));
    }
    let _one_at_a_time = autopilot::exclusive(
        svc,
        caller.workspace,
        &merge_lock(&project_id, request.pr_number),
    )
    .await?;
    let settings = svc.settings(caller).await?;
    let ready = compute(svc, caller, &project_id, request.pr_number, &settings).await?;
    let (execution_id, readiness) = request_merge(
        svc,
        caller,
        MergeAsk {
            project_id: &project_id,
            pr_number: request.pr_number,
            method: &method,
            delete_branch: request.delete_branch,
            actor: &caller.principal.user_id,
        },
        ready,
    )
    .await?;
    Ok(MergePullRequestResponse {
        execution_id,
        readiness: Some(readiness),
    })
}
