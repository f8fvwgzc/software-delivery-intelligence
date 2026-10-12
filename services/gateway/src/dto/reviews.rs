//! Code review (CONTRACT.md §20): pull requests as the knowledge service
//! lists them, reviews with their rail and findings, and the activity log.

use chrono::{DateTime, Utc};
use sdi_proto::{intelligence::v1 as intelligence, knowledge::v1 as knowledge};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{opt_string, opt_timestamp, proto_enum, timestamp};

proto_enum! {
    /// Where a review is in its pipeline.
    ReviewStatus => intelligence::ReviewStatus { Context, Analyzing, Proposed, Published, Failed }
}

proto_enum! {
    /// How much a finding matters.
    FindingSeverity => intelligence::FindingSeverity { Info, Low, Medium, High, Critical }
}

proto_enum! {
    /// The human's verdict on a finding.
    FindingDecision => intelligence::FindingDecision { Pending, Accepted, Dismissed }
}

/// A pull request as GitHub lists it, with the id of its latest SDI review.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestSummary {
    /// Project the pull request belongs to.
    pub project_id: String,
    /// GitHub number.
    pub number: u64,
    /// Title.
    pub title: String,
    /// Author login.
    pub author: String,
    /// Author avatar.
    pub author_avatar_url: String,
    /// `open` | `closed` | `merged` | `draft`.
    pub state: String,
    /// Target branch.
    pub base_ref: String,
    /// Source branch.
    pub head_ref: String,
    /// Head commit; a review is identified by it.
    pub head_sha: String,
    /// Files changed, as GitHub counts them.
    pub changed_files: u32,
    /// Lines added.
    pub additions: u32,
    /// Lines removed.
    pub deletions: u32,
    /// Link to the pull request on GitHub.
    pub html_url: String,
    /// When it was opened.
    pub created_at: Option<DateTime<Utc>>,
    /// When it last changed.
    pub updated_at: Option<DateTime<Utc>>,
    /// The latest SDI review of this pull request, if any.
    pub latest_review_id: Option<String>,
}

impl From<knowledge::PullRequestSummary> for PullRequestSummary {
    fn from(pr: knowledge::PullRequestSummary) -> Self {
        Self {
            project_id: pr.project_id,
            number: pr.number,
            title: pr.title,
            author: pr.author,
            author_avatar_url: pr.author_avatar_url,
            state: pr.state,
            base_ref: pr.base_ref,
            head_ref: pr.head_ref,
            head_sha: pr.head_sha,
            changed_files: pr.changed_files,
            additions: pr.additions,
            deletions: pr.deletions,
            html_url: pr.html_url,
            created_at: opt_timestamp(pr.created_at.as_ref()),
            updated_at: opt_timestamp(pr.updated_at.as_ref()),
            latest_review_id: opt_string(pr.latest_review_id),
        }
    }
}

/// One `@@` section of a unified diff.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    /// First line of the hunk in the old file.
    pub old_start: u32,
    /// Lines of the hunk in the old file.
    pub old_lines: u32,
    /// First line of the hunk in the new file.
    pub new_start: u32,
    /// Lines of the hunk in the new file.
    pub new_lines: u32,
    /// Diff lines including their leading ` `, `+` or `-`.
    pub lines: Vec<String>,
}

impl From<knowledge::DiffHunk> for DiffHunk {
    fn from(hunk: knowledge::DiffHunk) -> Self {
        Self {
            old_start: hunk.old_start,
            old_lines: hunk.old_lines,
            new_start: hunk.new_start,
            new_lines: hunk.new_lines,
            lines: hunk.lines,
        }
    }
}

/// A file the pull request changes, with its bounded diff.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    /// Path in the head commit.
    pub path: String,
    /// Path before a rename; null otherwise.
    pub previous_path: Option<String>,
    /// `added` | `modified` | `removed` | `renamed`.
    pub status: String,
    /// Lines added.
    pub additions: u32,
    /// Lines removed.
    pub deletions: u32,
    /// Language inferred from the path.
    pub language: String,
    /// Diff hunks; empty when `truncated`.
    pub hunks: Vec<DiffHunk>,
    /// The patch was larger than the limit (300 KB across the pull request) and was not read.
    pub truncated: bool,
}

impl From<knowledge::ChangedFile> for ChangedFile {
    fn from(file: knowledge::ChangedFile) -> Self {
        Self {
            path: file.path,
            previous_path: opt_string(file.previous_path),
            status: file.status,
            additions: file.additions,
            deletions: file.deletions,
            language: file.language,
            hunks: file.hunks.into_iter().map(Into::into).collect(),
            truncated: file.truncated,
        }
    }
}

/// One pull request with its files, diff and knowledge links.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    /// The summary.
    pub summary: PullRequestSummary,
    /// Description (Markdown) as written on GitHub.
    pub body: String,
    /// Changed files, at most 200.
    pub files: Vec<ChangedFile>,
    /// Facts whose evidence lives in a changed file.
    pub touched_fact_ids: Vec<String>,
    /// Graph node ids the change reaches (impact walk from every changed path, at most 60).
    pub impacted_node_ids: Vec<String>,
}

impl From<knowledge::PullRequest> for PullRequest {
    fn from(pr: knowledge::PullRequest) -> Self {
        Self {
            summary: pr.summary.unwrap_or_default().into(),
            body: pr.body,
            files: pr.files.into_iter().map(Into::into).collect(),
            touched_fact_ids: pr.touched_fact_ids,
            impacted_node_ids: pr.impacted_node_ids,
        }
    }
}

/// Query of `GET /projects/{id}/pull-requests`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListPullRequestsQuery {
    /// `open` (default), `closed` or `all`.
    pub state: Option<String>,
    /// Re-sync from GitHub before answering.
    pub refresh: Option<bool>,
    /// Page size, most recently updated first (default 50, max 200).
    pub limit: Option<u32>,
}

/// Response of `GET /projects/{id}/pull-requests`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestsResponse {
    /// The pull requests.
    pub pull_requests: Vec<PullRequestSummary>,
    /// When the list was last synced from GitHub; null before the first sync.
    pub synced_at: Option<DateTime<Utc>>,
}

/// One analyzer finding.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Finding id.
    pub id: String,
    /// Review it belongs to.
    pub review_id: String,
    /// `secrets` | `security` | `dependencies` | `docker` | `ci` | `iac` | `tests` | `docs` | `correctness` | `maturity`.
    pub category: String,
    /// Severity.
    pub severity: FindingSeverity,
    /// Short title.
    pub title: String,
    /// Markdown: the evidence and the fix.
    pub detail: String,
    /// File the finding points at.
    pub path: String,
    /// Line in the new file; 0 for a file-level finding.
    pub line: u32,
    /// Last line of the range; equals `line` for a single line.
    pub end_line: u32,
    /// Replacement for the referenced lines, when the fix is mechanical; becomes a GitHub suggestion.
    pub suggestion: Option<String>,
    /// Rule id, e.g. `secrets.aws_key`.
    pub rule: String,
    /// The human's verdict.
    pub decision: FindingDecision,
    /// Note left with the decision.
    pub decision_note: Option<String>,
    /// Who decided.
    pub decided_by: Option<String>,
    /// When.
    pub decided_at: Option<DateTime<Utc>>,
}

impl From<intelligence::Finding> for Finding {
    fn from(f: intelligence::Finding) -> Self {
        Self {
            id: f.id,
            review_id: f.review_id,
            category: f.category,
            severity: f.severity.into(),
            title: f.title,
            detail: f.detail,
            path: f.path,
            line: f.line,
            end_line: f.end_line,
            suggestion: opt_string(f.suggestion),
            rule: f.rule,
            decision: f.decision.into(),
            decision_note: opt_string(f.decision_note),
            decided_by: opt_string(f.decided_by),
            decided_at: opt_timestamp(f.decided_at.as_ref()),
        }
    }
}

/// One step of the review rail.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewStep {
    /// `context` | `analyze` | `findings` | `summary` | `publish`.
    pub key: String,
    /// Label.
    pub label: String,
    /// `pending` | `running` | `done` | `failed`.
    pub status: String,
    /// What the step found or why it failed.
    pub detail: String,
    /// Wall time of the step.
    pub duration_ms: u32,
}

impl From<intelligence::ReviewStep> for ReviewStep {
    fn from(s: intelligence::ReviewStep) -> Self {
        Self {
            key: s.key,
            label: s.label,
            status: s.status,
            detail: s.detail,
            duration_ms: s.duration_ms,
        }
    }
}

/// A review of one pull request at one head commit.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    /// Review id.
    pub id: String,
    /// Project.
    pub project_id: String,
    /// Project display name.
    pub project_name: String,
    /// Pull request number.
    pub pr_number: u64,
    /// Pull request title.
    pub pr_title: String,
    /// Pull request author.
    pub pr_author: String,
    /// Head commit reviewed.
    pub head_sha: String,
    /// Where the review is.
    pub status: ReviewStatus,
    /// The rail.
    pub steps: Vec<ReviewStep>,
    /// Markdown walkthrough. `null` in list responses.
    pub summary: Option<String>,
    /// Findings. Empty in list responses.
    pub findings: Vec<Finding>,
    /// Files the review read.
    pub files_reviewed: u32,
    /// Findings proposed.
    pub findings_total: u32,
    /// Findings accepted so far.
    pub findings_accepted: u32,
    /// `heuristic` | `claude`.
    pub reasoner: String,
    /// GitHub review URL once published.
    pub published_url: Option<String>,
    /// Why it failed, when it did.
    pub error: Option<String>,
    /// Who started it (`system` for webhooks).
    pub requested_by: String,
    /// When it was started.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<intelligence::Review> for Review {
    fn from(r: intelligence::Review) -> Self {
        Self {
            id: r.id,
            project_id: r.project_id,
            project_name: r.project_name,
            pr_number: r.pr_number,
            pr_title: r.pr_title,
            pr_author: r.pr_author,
            head_sha: r.head_sha,
            status: r.status.into(),
            steps: r.steps.into_iter().map(Into::into).collect(),
            summary: opt_string(r.summary),
            findings: r.findings.into_iter().map(Into::into).collect(),
            files_reviewed: r.files_reviewed,
            findings_total: r.findings_total,
            findings_accepted: r.findings_accepted,
            reasoner: r.reasoner,
            published_url: opt_string(r.published_url),
            error: opt_string(r.error),
            requested_by: r.requested_by,
            created_at: timestamp(r.created_at.as_ref()),
            updated_at: timestamp(r.updated_at.as_ref()),
        }
    }
}

/// Query of `GET /reviews`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListReviewsQuery {
    /// Only reviews of this project. Empty lists every project.
    pub project_id: Option<String>,
    /// Page size, newest first (default 50, max 200).
    pub limit: Option<u32>,
}

/// Body of `POST /findings/{id}/decide`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecideFindingRequest {
    /// `accepted` or `dismissed`.
    pub decision: FindingDecision,
    /// Why, in a sentence.
    #[serde(default)]
    pub note: Option<String>,
}

/// Body of `POST /reviews/{id}/publish`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublishReviewRequest {
    /// `comment` (default), `request_changes` or `approve`.
    #[serde(default)]
    pub event: Option<String>,
}

/// One entry of the review activity log.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewActivity {
    /// Entry id.
    pub id: String,
    /// Review.
    pub review_id: String,
    /// Project.
    pub project_id: String,
    /// Project display name.
    pub project_name: String,
    /// Pull request number.
    pub pr_number: u64,
    /// `review.started` | `review.proposed` | `finding.accepted` | `finding.dismissed` | `review.published` | `review.failed`.
    pub action: String,
    /// Who did it (`system` for the pipeline).
    pub actor: String,
    /// What happened, in a line.
    pub detail: String,
    /// When.
    pub at: DateTime<Utc>,
}

impl From<intelligence::ReviewActivity> for ReviewActivity {
    fn from(a: intelligence::ReviewActivity) -> Self {
        Self {
            id: a.id,
            review_id: a.review_id,
            project_id: a.project_id,
            project_name: a.project_name,
            pr_number: a.pr_number,
            action: a.action,
            actor: a.actor,
            detail: a.detail,
            at: timestamp(a.at.as_ref()),
        }
    }
}

/// Query of `GET /activity`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListActivityQuery {
    /// Only activity of this project. Empty lists every project.
    pub project_id: Option<String>,
    /// Cursor: entries strictly before this time.
    pub before: Option<DateTime<Utc>>,
    /// Page size, newest first (default 50, max 200).
    pub limit: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_shape_hides_summary_and_findings() {
        let listed = Review::from(intelligence::Review {
            id: "r1".into(),
            status: intelligence::ReviewStatus::Proposed as i32,
            steps: vec![intelligence::ReviewStep {
                key: "context".into(),
                status: "done".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        assert_eq!(listed.status, ReviewStatus::Proposed);
        assert!(listed.summary.is_none());
        assert!(listed.findings.is_empty());
        assert!(listed.published_url.is_none());
        let json = serde_json::to_value(&listed).unwrap();
        assert_eq!(json["status"], "proposed");
        assert!(json["summary"].is_null());
        assert_eq!(json["steps"][0]["key"], "context");
        assert!(json.get("prNumber").is_some());
    }

    #[test]
    fn decisions_and_severities_are_lowercase() {
        let body: DecideFindingRequest =
            serde_json::from_str(r#"{"decision": "accepted"}"#).unwrap();
        assert_eq!(body.decision, FindingDecision::Accepted);
        assert_eq!(
            i32::from(body.decision),
            intelligence::FindingDecision::Accepted as i32
        );
        let finding = Finding::from(intelligence::Finding {
            severity: intelligence::FindingSeverity::High as i32,
            suggestion: String::new(),
            ..Default::default()
        });
        assert_eq!(serde_json::to_value(&finding).unwrap()["severity"], "high");
        assert!(finding.suggestion.is_none());
    }

    #[test]
    fn pull_request_summary_leaves_the_review_id_null_until_filled() {
        let summary = PullRequestSummary::from(knowledge::PullRequestSummary {
            number: 3,
            state: "draft".into(),
            ..Default::default()
        });
        assert!(summary.latest_review_id.is_none());
        assert_eq!(serde_json::to_value(&summary).unwrap()["state"], "draft");
        let file = ChangedFile::from(knowledge::ChangedFile {
            path: "a".into(),
            hunks: vec![knowledge::DiffHunk {
                lines: vec!["+x".into()],
                ..Default::default()
            }],
            ..Default::default()
        });
        assert!(file.previous_path.is_none());
        assert_eq!(file.hunks[0].lines, vec!["+x"]);
    }
}
