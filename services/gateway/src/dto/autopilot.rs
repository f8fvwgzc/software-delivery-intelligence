//! Pull request autopilot (CONTRACT.md §23): whether a pull request can be
//! merged and what stands in the way, the merge request, the project's
//! vulnerable dependencies and turning Dependabot on.
//!
//! The fields the feature adds to findings, reviews and pull request
//! summaries live with those shapes in `super::reviews`; its settings in
//! `super::intelligence` and its data control in `super::identity`.

use chrono::{DateTime, Utc};
use sdi_proto::intelligence::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{opt_string, opt_timestamp, timestamp};

// ---------------------------------------------------------------------------
// Merge
// ---------------------------------------------------------------------------

/// One reason a pull request cannot be merged yet.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MergeBlocker {
    /// `not_open` | `draft` | `conflicts` | `mergeability_unknown` | `checks_failing` | `checks_pending` | `changes_requested` | `blocked_by_github` | `no_review` | `review_outdated` | `review_running` | `findings_undecided` | `merge_in_progress`.
    pub code: String,
    /// A sentence that says what to do about it.
    pub message: String,
}

impl From<proto::MergeBlocker> for MergeBlocker {
    fn from(blocker: proto::MergeBlocker) -> Self {
        Self {
            code: blocker.code,
            message: blocker.message,
        }
    }
}

/// Whether a pull request can be merged now: GitHub's state read at
/// `checkedAt` plus SDI's own gate (its review of the head commit).
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MergeReadiness {
    /// Project the pull request belongs to.
    pub project_id: String,
    /// GitHub number.
    pub pr_number: u64,
    /// Title.
    pub title: String,
    /// `open` | `closed` | `merged` | `draft`.
    pub state: String,
    /// Head commit the answer is about; a merge is only ever of this commit.
    pub head_sha: String,
    /// Source branch.
    pub head_ref: String,
    /// Target branch.
    pub base_ref: String,
    /// The head branch lives in another repository.
    pub is_fork: bool,
    /// The author is Dependabot.
    pub is_dependabot: bool,
    /// `mergeable` | `conflicting` | `unknown` (GitHub is still computing it: ask again).
    pub mergeable: String,
    /// GitHub's own word: `clean` | `dirty` | `blocked` | `behind` | `unstable` | `has_hooks` | `draft` | `unknown`.
    pub mergeable_state: String,
    /// Check runs and commit statuses of the head commit, counted together.
    pub checks_total: u32,
    /// Checks that passed.
    pub checks_passed: u32,
    /// Checks that failed.
    pub checks_failed: u32,
    /// Checks still queued or running.
    pub checks_pending: u32,
    /// Names of the failed checks, at most 20.
    pub failed_checks: Vec<String>,
    /// Names of the checks still queued or running, at most 20.
    pub pending_checks: Vec<String>,
    /// Approving reviews by people on GitHub, latest verdict per reviewer.
    pub approvals: u32,
    /// Reviewers on GitHub whose latest verdict requests changes.
    pub changes_requested: u32,
    /// What the repository allows: `merge` | `squash` | `rebase`.
    pub allowed_methods: Vec<String>,
    /// The base branch is protected; GitHub enforces its rules at merge time.
    pub base_protected: bool,
    /// Commits the head is behind its base.
    pub behind_by: u32,
    /// The SDI review of `headSha`; null when there is none.
    pub review_id: Option<String>,
    /// That review's status (`context` | `analyzing` | `proposed` | `published` | `failed`); null when there is none.
    pub review_status: Option<String>,
    /// Findings of severity high or critical nobody decided yet.
    pub undecided_blocking_findings: u32,
    /// What stands in the way, GitHub's reasons first; empty means `canMerge`.
    pub blockers: Vec<MergeBlocker>,
    /// True when nothing blocks the merge.
    pub can_merge: bool,
    /// An open merge execution for this pull request; null when none is running.
    pub merge_execution_id: Option<String>,
    /// When GitHub was read.
    pub checked_at: DateTime<Utc>,
}

impl From<proto::MergeReadiness> for MergeReadiness {
    fn from(r: proto::MergeReadiness) -> Self {
        Self {
            project_id: r.project_id,
            pr_number: r.pr_number,
            title: r.title,
            state: r.state,
            head_sha: r.head_sha,
            head_ref: r.head_ref,
            base_ref: r.base_ref,
            is_fork: r.is_fork,
            is_dependabot: r.is_dependabot,
            mergeable: r.mergeable,
            mergeable_state: r.mergeable_state,
            checks_total: r.checks_total,
            checks_passed: r.checks_passed,
            checks_failed: r.checks_failed,
            checks_pending: r.checks_pending,
            failed_checks: r.failed_checks,
            pending_checks: r.pending_checks,
            approvals: r.approvals,
            changes_requested: r.changes_requested,
            allowed_methods: r.allowed_methods,
            base_protected: r.base_protected,
            behind_by: r.behind_by,
            review_id: opt_string(r.review_id),
            review_status: opt_string(r.review_status),
            undecided_blocking_findings: r.undecided_blocking_findings,
            blockers: r.blockers.into_iter().map(Into::into).collect(),
            can_merge: r.can_merge,
            merge_execution_id: opt_string(r.merge_execution_id),
            checked_at: timestamp(r.checked_at.as_ref()),
        }
    }
}

/// How GitHub merges a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MergeMethod {
    /// A merge commit.
    Merge,
    /// One commit with every change of the pull request.
    Squash,
    /// The commits replayed on top of the base.
    Rebase,
}

impl MergeMethod {
    /// The exact string the intelligence service and GitHub use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }
}

/// Body of `POST /projects/{id}/pull-requests/{number}/merge` (approver).
/// The body may be left out altogether.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MergePullRequestRequest {
    /// Must be one the repository allows (`allowedMethods`); null picks the first allowed of squash, merge, rebase.
    pub method: Option<MergeMethod>,
    /// Delete the head branch after the merge (same-repository branches only). Default false.
    pub delete_branch: Option<bool>,
}

/// Response of `POST /projects/{id}/pull-requests/{number}/merge`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
    /// The execution that merges the pull request: its steps, its approval (the `repo.merge_pull_request` policy) and its outcome are there.
    pub execution_id: String,
    /// The readiness the merge was requested on.
    pub readiness: MergeReadiness,
}

impl From<proto::MergePullRequestResponse> for MergeResult {
    fn from(response: proto::MergePullRequestResponse) -> Self {
        Self {
            execution_id: response.execution_id,
            readiness: response.readiness.unwrap_or_default().into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Vulnerabilities and Dependabot
// ---------------------------------------------------------------------------

/// Query of `GET /projects/{id}/security`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct SecurityQuery {
    /// Ask GitHub (and OSV) again instead of answering from the 5-minute cache.
    pub refresh: Option<bool>,
}

/// One vulnerable dependency.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Vulnerability {
    /// `dependabot` | `osv`.
    pub source: String,
    /// GHSA id when there is one, else the advisory's own id.
    pub id: String,
    /// CVE id; null when the advisory has none.
    pub cve_id: Option<String>,
    /// `npm` | `pip` | `cargo` | `go` | `maven` | `nuget` | `composer` | `rubygems` | `actions` | ...
    pub ecosystem: String,
    /// Package name.
    pub package: String,
    /// The version the manifest names; null when the source does not say.
    pub current_version: Option<String>,
    /// Versions the advisory affects; null when the source does not say.
    pub vulnerable_range: Option<String>,
    /// First version with the fix; null when no fixed version exists yet.
    pub fixed_version: Option<String>,
    /// `critical` | `high` | `medium` | `low` | `unknown`.
    pub severity: String,
    /// What is wrong, in a line.
    pub summary: String,
    /// Manifest that names the package; null when the source does not say.
    pub manifest_path: Option<String>,
    /// `runtime` | `development`; null when the source does not say.
    pub scope: Option<String>,
    /// The advisory or the alert on GitHub; null when there is no page for it.
    pub url: Option<String>,
    /// Number of an open pull request that fixes it; null when there is none.
    pub fix_pull_request: Option<u64>,
    /// When it was detected; null when the source does not say (OSV lookups).
    pub detected_at: Option<DateTime<Utc>>,
}

impl From<proto::Vulnerability> for Vulnerability {
    fn from(v: proto::Vulnerability) -> Self {
        Self {
            source: v.source,
            id: v.id,
            cve_id: opt_string(v.cve_id),
            ecosystem: v.ecosystem,
            package: v.package,
            current_version: opt_string(v.current_version),
            vulnerable_range: opt_string(v.vulnerable_range),
            fixed_version: opt_string(v.fixed_version),
            severity: v.severity,
            summary: v.summary,
            manifest_path: opt_string(v.manifest_path),
            scope: opt_string(v.scope),
            url: opt_string(v.url),
            fix_pull_request: (v.fix_pull_request != 0).then_some(v.fix_pull_request),
            detected_at: opt_timestamp(v.detected_at.as_ref()),
        }
    }
}

/// How many vulnerabilities have one severity.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeverityCount {
    /// `critical` | `high` | `medium` | `low` | `unknown`.
    pub severity: String,
    /// Vulnerabilities of that severity.
    pub count: u32,
}

impl From<proto::SeverityCount> for SeverityCount {
    fn from(count: proto::SeverityCount) -> Self {
        Self {
            severity: count.severity,
            count: count.count,
        }
    }
}

/// A project's vulnerable dependencies and the state of Dependabot on its
/// repository. Computed per request (cached 5 minutes); nothing is stored.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecurityOverview {
    /// Project.
    pub project_id: String,
    /// Where the list came from: `dependabot` (GitHub's open alerts), `osv` (the manifest versions looked up at OSV) or `none` (neither answered).
    pub source: String,
    /// Why it is not Dependabot's list, as a sentence; null when it is.
    pub source_note: Option<String>,
    /// The vulnerabilities, most severe first.
    pub vulnerabilities: Vec<Vulnerability>,
    /// Counts per severity, most severe first.
    pub by_severity: Vec<SeverityCount>,
    /// More exist than were read.
    pub truncated: bool,
    /// A Dependabot configuration file exists on the default branch.
    pub config_present: bool,
    /// Its path (`.github/dependabot.yml`); null when there is none.
    pub config_path: Option<String>,
    /// Ecosystems the configuration names.
    pub config_ecosystems: Vec<String>,
    /// Dependabot alerts: `enabled` | `disabled` | `unknown` (the token may not read the setting).
    pub alerts: String,
    /// Dependabot security updates: `enabled` | `paused` | `disabled` | `unknown`.
    pub security_updates: String,
    /// Open pull requests by Dependabot.
    pub open_dependabot_pull_requests: u32,
    /// An open execution that turns Dependabot on for this project; null when none is running.
    pub configure_execution_id: Option<String>,
    /// When GitHub was read.
    pub checked_at: DateTime<Utc>,
}

impl From<proto::SecurityOverview> for SecurityOverview {
    fn from(o: proto::SecurityOverview) -> Self {
        Self {
            project_id: o.project_id,
            source: o.source,
            source_note: opt_string(o.source_note),
            vulnerabilities: o.vulnerabilities.into_iter().map(Into::into).collect(),
            by_severity: o.by_severity.into_iter().map(Into::into).collect(),
            truncated: o.truncated,
            config_present: o.config_present,
            config_path: opt_string(o.config_path),
            config_ecosystems: o.config_ecosystems,
            alerts: o.alerts,
            security_updates: o.security_updates,
            open_dependabot_pull_requests: o.open_dependabot_pull_requests,
            configure_execution_id: opt_string(o.configure_execution_id),
            checked_at: timestamp(o.checked_at.as_ref()),
        }
    }
}

/// Body of `POST /projects/{id}/security/dependabot` (admin). At least one
/// of the three must be true.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureDependabotRequest {
    /// Turn on Dependabot alerts for the repository.
    pub alerts: Option<bool>,
    /// Turn on Dependabot security updates (pull requests that fix alerts).
    pub security_updates: Option<bool>,
    /// Open a pull request that adds `.github/dependabot.yml`.
    pub config_file: Option<bool>,
}

/// Response of `POST /projects/{id}/security/dependabot`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureDependabotResult {
    /// The execution that turns Dependabot on; changing repository settings pauses it for a timed `repository:admin` grant.
    pub execution_id: String,
}

impl From<proto::ConfigureDependabotResponse> for ConfigureDependabotResult {
    fn from(response: proto::ConfigureDependabotResponse) -> Self {
        Self {
            execution_id: response.execution_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_nulls_what_does_not_exist_and_keeps_the_blockers_in_order() {
        let ready = serde_json::to_value(MergeReadiness::from(proto::MergeReadiness {
            pr_number: 4,
            can_merge: true,
            allowed_methods: vec!["squash".into(), "merge".into()],
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(ready["prNumber"], 4);
        assert_eq!(ready["canMerge"], true);
        assert_eq!(ready["blockers"], serde_json::json!([]));
        assert!(ready["reviewId"].is_null());
        assert!(ready["reviewStatus"].is_null());
        assert!(ready["mergeExecutionId"].is_null());
        assert_eq!(
            ready["checkedAt"], "1970-01-01T00:00:00Z",
            "unset is visible"
        );
        assert_eq!(
            ready["allowedMethods"],
            serde_json::json!(["squash", "merge"])
        );

        let blocked = serde_json::to_value(MergeReadiness::from(proto::MergeReadiness {
            review_id: "r1".into(),
            review_status: "proposed".into(),
            merge_execution_id: "e1".into(),
            blockers: vec![
                proto::MergeBlocker {
                    code: "checks_failing".into(),
                    message: "1 check failed".into(),
                },
                proto::MergeBlocker {
                    code: "merge_in_progress".into(),
                    message: "a merge is already running".into(),
                },
            ],
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(blocked["reviewId"], "r1");
        assert_eq!(blocked["reviewStatus"], "proposed");
        assert_eq!(blocked["mergeExecutionId"], "e1");
        assert_eq!(blocked["blockers"][0]["code"], "checks_failing");
        assert_eq!(
            blocked["blockers"][1]["message"],
            "a merge is already running"
        );
        assert_eq!(blocked["canMerge"], false);
    }

    #[test]
    fn merge_body_takes_only_the_three_methods() {
        let body: MergePullRequestRequest =
            serde_json::from_str(r#"{"method": "rebase", "deleteBranch": true}"#).unwrap();
        assert_eq!(body.method.map(MergeMethod::as_str), Some("rebase"));
        assert_eq!(body.delete_branch, Some(true));
        let empty: MergePullRequestRequest = serde_json::from_str("{}").unwrap();
        assert!(empty.method.is_none() && empty.delete_branch.is_none());
        let null: MergePullRequestRequest =
            serde_json::from_str(r#"{"method": null, "deleteBranch": null}"#).unwrap();
        assert!(null.method.is_none() && null.delete_branch.is_none());
        assert!(
            serde_json::from_str::<MergePullRequestRequest>(r#"{"method": "fast-forward"}"#)
                .is_err()
        );
        for method in [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase] {
            assert_eq!(
                serde_json::to_string(&method).unwrap(),
                format!("\"{}\"", method.as_str())
            );
        }
    }

    #[test]
    fn vulnerability_nulls_what_the_source_does_not_say() {
        let osv = serde_json::to_value(Vulnerability::from(proto::Vulnerability {
            source: "osv".into(),
            id: "GHSA-xxxx".into(),
            package: "lodash".into(),
            severity: "high".into(),
            ..Default::default()
        }))
        .unwrap();
        for field in [
            "cveId",
            "currentVersion",
            "vulnerableRange",
            "fixedVersion",
            "manifestPath",
            "scope",
            "url",
            "fixPullRequest",
            "detectedAt",
        ] {
            assert!(osv[field].is_null(), "{field}: {osv}");
        }
        assert_eq!(osv["package"], "lodash");
        let alert = serde_json::to_value(Vulnerability::from(proto::Vulnerability {
            fix_pull_request: 12,
            fixed_version: "4.17.21".into(),
            scope: "runtime".into(),
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(alert["fixPullRequest"], 12);
        assert_eq!(alert["fixedVersion"], "4.17.21");
        assert_eq!(alert["scope"], "runtime");
    }

    #[test]
    fn overview_nulls_the_note_the_path_and_the_execution_when_empty() {
        let overview = serde_json::to_value(SecurityOverview::from(proto::SecurityOverview {
            project_id: "p1".into(),
            source: "dependabot".into(),
            by_severity: vec![proto::SeverityCount {
                severity: "critical".into(),
                count: 2,
            }],
            ..Default::default()
        }))
        .unwrap();
        assert!(overview["sourceNote"].is_null());
        assert!(overview["configPath"].is_null());
        assert!(overview["configureExecutionId"].is_null());
        assert_eq!(overview["bySeverity"][0]["count"], 2);
        assert_eq!(overview["vulnerabilities"], serde_json::json!([]));
        assert_eq!(overview["openDependabotPullRequests"], 0);
    }
}
