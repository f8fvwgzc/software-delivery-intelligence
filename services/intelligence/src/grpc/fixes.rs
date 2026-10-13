//! Fixes (CONTRACT.md §23.2). `DraftFix` asks the workspace's model for the
//! replacement of the lines one finding points at, when the workspace
//! allowed that, and stores the reply as the finding's suggestion.
//! `ApplyFixes` writes the suggestions of the accepted findings into the
//! files at the reviewed commit and hands them to the execution service as
//! one commit on the pull request's branch.
//!
//! The model's reply is data and nothing else: it is cleaned, checked and
//! stored as text. It is never executed and never read as an instruction.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use sdi_common::{grpc::db_status, principal::Principal};
use sdi_proto::{
    identity::v1::{AuditEntry, DataControls},
    intelligence::v1::{ApplyFixesRequest, DraftFixRequest, Finding, Review},
    knowledge::v1::RepoFile,
};
use sqlx::PgPool;
use tonic::{Code, Status};
use uuid::Uuid;

use super::{
    Caller, IntelligenceGrpc,
    autopilot::{self, Liveness, Recorded},
    code_to_model_allowed, parse_id, reasoner_allowed,
    review_runner::{activity, emit},
};
use crate::{
    convert,
    db::{
        autopilot::{self as autopilot_repo, KIND_FIX},
        reviews::{self as repo, FindingRow, ReviewRow, SOURCE_MODEL},
    },
    domain::{
        fixes::{self, FilePatch},
        plans,
    },
    events::EventBus,
    gateway::{IdentityGateway, KnowledgeGateway},
    reasoner::{Reasoner, fixes as reply},
};

/// Fixes one review drafts on its own with `auto_draft_fixes`, at most.
pub(super) const AUTO_DRAFT_LIMIT: usize = 10;
/// Files one fix execution commits, at most. Findings in further files stay
/// available for the next one.
const MAX_FIX_FILES: usize = 100;

/// What the caller reads when one of the two data controls is off.
fn switches_sentence(controls: &DataControls) -> String {
    let reasoner = if reasoner_allowed(controls) {
        "on"
    } else {
        "off"
    };
    let code = if controls.code_to_model.trim().is_empty() {
        "off"
    } else {
        controls.code_to_model.trim()
    };
    format!(
        "drafting a fix sends the flagged lines to your model, and this workspace has not allowed that. Two switches \
         in Workspace, Data and privacy must both be on: the reasoner (`reasoner`, now {reasoner}) and sending \
         changed lines to the model (`code_to_model = changed_lines`, now {code})"
    )
}

/// Everything a draft needs, owned, so the review pipeline can keep one
/// for the whole run. Files read at the head commit are remembered per
/// path: several findings in one file cost one read.
pub(super) struct Drafter {
    pub pool: PgPool,
    pub knowledge: Arc<dyn KnowledgeGateway>,
    pub identity: Arc<dyn IdentityGateway>,
    pub events: EventBus,
    pub reasoner: Arc<dyn Reasoner>,
    pub principal: Principal,
    pub workspace: Uuid,
    files: tokio::sync::Mutex<HashMap<String, RepoFile>>,
}

/// Why a draft did not happen: the status for the caller, and a word for
/// the log (the kind of reason, never its text: a reason can quote a path
/// or a provider).
pub(super) struct DraftError {
    pub status: Status,
    pub kind: &'static str,
}

impl DraftError {
    /// The workspace's day is spent: asking about another finding right
    /// now is pointless.
    pub fn cap_spent(&self) -> bool {
        self.kind == reply::KIND_CAP_SPENT
    }
}

impl From<Status> for DraftError {
    fn from(status: Status) -> Self {
        Self {
            status,
            kind: "not_draftable",
        }
    }
}

fn refused(reason: impl std::fmt::Display) -> Status {
    Status::failed_precondition(format!("no fix was drafted: {reason}"))
}

impl Drafter {
    pub fn new(
        pool: PgPool,
        knowledge: Arc<dyn KnowledgeGateway>,
        identity: Arc<dyn IdentityGateway>,
        events: EventBus,
        reasoner: Arc<dyn Reasoner>,
        principal: Principal,
        workspace: Uuid,
    ) -> Self {
        Self {
            pool,
            knowledge,
            identity,
            events,
            reasoner,
            principal,
            workspace,
            files: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// The file at the review's head commit, as text that can be patched.
    async fn file_at_head(&self, review: &ReviewRow, path: &str) -> Result<RepoFile, Status> {
        if let Some(file) = self.files.lock().await.get(path) {
            return Ok(file.clone());
        }
        let file = self
            .knowledge
            .repo_file_at(&self.principal, &review.project_id, path, &review.head_sha)
            .await
            .map_err(|error| match error.status.code() {
                Code::NotFound => refused(format!(
                    "`{path}` does not exist at the reviewed commit {}",
                    short(&review.head_sha)
                )),
                _ => error.into_status(),
            })?;
        if file.binary {
            return Err(refused(format!("`{path}` is a binary file")));
        }
        if file.truncated {
            return Err(refused(format!(
                "`{path}` is too large to read in full, so its lines cannot be replaced safely"
            )));
        }
        self.files
            .lock()
            .await
            .insert(path.to_owned(), file.clone());
        Ok(file)
    }

    /// Drafts the fix for one finding and stores it. The finding's decision
    /// is not touched: a person still accepts or dismisses it.
    pub async fn draft(
        &self,
        review: &ReviewRow,
        finding: &FindingRow,
    ) -> Result<FindingRow, DraftError> {
        if let Some(reason) = fixes::draft_blocker(finding) {
            return Err(refused(reason).into());
        }
        let file = self.file_at_head(review, &finding.path).await?;
        let language = if file.language.trim().is_empty() {
            fixes::language_of(&finding.path)
        } else {
            file.language.trim()
        };
        let request = fixes::excerpt(finding, language, &file.content)
            .map_err(|error| refused(error.sentence()))?;
        let answer = self
            .reasoner
            .draft_fix(&request)
            .await
            .map_err(|refusal| {
                // The reason only: never the request, never the reply.
                tracing::info!(finding = %finding.id, rule = %finding.rule, reason = refusal.kind(), "draft fix refused");
                DraftError {
                    kind: refusal.kind(),
                    status: refused(refusal.sentence()),
                }
            })?;
        let suggestion =
            reply::accept_reply(&answer, &request.flagged).map_err(|refusal| DraftError {
                kind: refusal.kind(),
                status: refused(refusal.sentence()),
            })?;
        let stored = repo::set_suggestion(
            &self.pool,
            self.workspace,
            finding.id,
            &suggestion,
            SOURCE_MODEL,
        )
        .await
        .map_err(db_status)?
        .ok_or_else(|| refused(fixes::ALREADY_SUGGESTED))?;

        let lines = suggestion.lines().count();
        let detail = format!(
            "{} ({}): {} line{} drafted by {} from {} line{} of context",
            stored.title,
            fixes::location(&stored),
            lines,
            if lines == 1 { "" } else { "s" },
            self.reasoner.name(),
            request.lines_sent(),
            if request.lines_sent() == 1 { "" } else { "s" },
        );
        repo::insert_activity(
            &self.pool,
            &activity(
                review,
                "fix.drafted",
                &self.principal.user_id,
                detail.clone(),
            ),
        )
        .await
        .map_err(db_status)?;
        self.events.emit(
            self.workspace,
            "finding",
            &stored.id.to_string(),
            &review.project_id,
            "updated",
            &stored.decision,
            &[
                ("review_id", review.id.to_string()),
                ("rule", stored.rule.clone()),
                ("suggestion_source", SOURCE_MODEL.to_owned()),
            ],
        );
        self.identity.record_audit(
            &self.principal,
            AuditEntry {
                workspace_id: self.principal.workspace_id.clone(),
                user_id: self.principal.user_id.clone(),
                action: "review.fix_drafted".to_owned(),
                entity: "finding".to_owned(),
                entity_id: stored.id.to_string(),
                detail,
                at: Some(sdi_common::time::to_proto(chrono::Utc::now())),
                ..Default::default()
            },
        );
        Ok(stored)
    }

    /// The review pipeline's own drafts (`auto_draft_fixes`): the findings
    /// the model may be asked about, most severe first (the order the
    /// analysis stored them in), at most [`AUTO_DRAFT_LIMIT`]. A draft that
    /// fails is skipped; a spent token cap ends the round. Never fails the
    /// review. Returns how many fixes were stored.
    pub async fn draft_for_review(&self, review: &ReviewRow, findings: &[FindingRow]) -> usize {
        let mut drafted = 0;
        for finding in findings
            .iter()
            .filter(|f| fixes::drafted_unasked(f))
            .take(AUTO_DRAFT_LIMIT)
        {
            match self.draft(review, finding).await {
                Ok(_) => drafted += 1,
                Err(error) if error.cap_spent() => {
                    tracing::info!(review = %review.id, drafted, "token cap spent; no further fixes drafted for this review");
                    break;
                }
                Err(error) => {
                    tracing::debug!(review = %review.id, finding = %finding.id, reason = error.kind, "fix not drafted");
                }
            }
        }
        drafted
    }
}

fn short(sha: &str) -> String {
    sha.chars().take(12).collect()
}

pub(super) async fn draft(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: DraftFixRequest,
) -> Result<Finding, Status> {
    let id = parse_id("finding_id", &request.finding_id)?;
    let finding = repo::get_finding(&svc.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    let review = repo::get(&svc.pool, caller.workspace, finding.review_id)
        .await
        .map_err(db_status)?;
    // What cannot be drafted at all is refused before anyone is asked anything.
    if let Some(reason) = fixes::draft_blocker(&finding) {
        return Err(refused(reason));
    }
    // The workspace decides whether source lines may leave: both switches,
    // read from identity now. An identity that cannot answer is an error,
    // not a "no".
    let controls = svc
        .identity
        .data_controls(&caller.principal)
        .await
        .map_err(|e| e.into_status())?;
    if !code_to_model_allowed(&controls) {
        return Err(Status::failed_precondition(switches_sentence(&controls)));
    }
    let settings = svc.settings(caller).await?;
    let access = svc.access_from_controls(caller, &settings, &controls).await;
    if !settings.wants_llm() {
        return Err(refused(
            "the reasoner is set to heuristic in Settings, Reasoner; choose the model there first",
        ));
    }
    let reasoner = svc.reasoner_for(caller, &settings, &access);
    if reasoner.name() == "heuristic" {
        return Err(refused(reply::FixRefusal::NoModel.sentence()));
    }
    let drafter = Drafter::new(
        svc.pool.clone(),
        Arc::clone(&svc.knowledge),
        Arc::clone(&svc.identity),
        svc.events.clone(),
        reasoner,
        caller.principal.clone(),
        caller.workspace,
    );
    let stored = drafter
        .draft(&review, &finding)
        .await
        .map_err(|error| error.status)?;
    Ok(convert::finding(&stored))
}

/// One accepted suggestion that did not make it into the commit, for the
/// activity entry.
fn left_out_line(finding: &FindingRow, reason: &str) -> String {
    format!("{} {}: {reason}", finding.rule, fixes::location(finding))
}

pub(super) async fn apply(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: ApplyFixesRequest,
) -> Result<Review, Status> {
    let id = parse_id("review_id", &request.review_id)?;
    let _one_at_a_time = autopilot::exclusive(svc, caller.workspace, &format!("fix:{id}")).await?;
    let review = repo::get(&svc.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    let pr = format!("pull request #{}", review.pr_number);

    // A fix execution for this review that has not ended holds the branch.
    if !review.fix_execution_id.is_empty()
        && let Some(previous) =
            autopilot_repo::by_execution(&svc.pool, caller.workspace, &review.fix_execution_id)
                .await
                .map_err(db_status)?
    {
        match autopilot::liveness(svc, caller, &previous).await {
            Liveness::Open(status) => {
                return Err(Status::failed_precondition(format!(
                    "fix execution {} for this review is still open ({status}); approve, finish or cancel it first",
                    previous.execution_id
                )));
            }
            Liveness::Unknown(status) => return Err(status),
            // Its commit moved the branch: whatever is left of this review
            // was written for a commit that is no longer the head.
            Liveness::Ended(outcome) if outcome == autopilot::SUCCEEDED => {
                return Err(Status::failed_precondition(format!(
                    "execution {} already committed this review's fixes, so {pr} has a new head; review the new \
                     head first",
                    previous.execution_id
                )));
            }
            // Failed, cancelled or gone: nothing was committed.
            Liveness::Ended(_) => {}
        }
    }

    // Read after the check above: an execution that ended without a
    // commit has just given its findings back.
    let findings = repo::findings_of(&svc.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    let ready: Vec<&FindingRow> = findings
        .iter()
        .filter(|f| {
            f.decision == "accepted"
                && !f.suggestion.trim().is_empty()
                // An example is shown with the finding; it is never committed.
                && repo::source_applies(&f.suggestion_source)
                && f.fix_execution_id.is_empty()
        })
        .collect();
    if ready.is_empty() {
        let only_examples = findings.iter().any(|f| {
            f.decision == "accepted"
                && !f.suggestion.trim().is_empty()
                && f.fix_execution_id.is_empty()
        });
        return Err(Status::failed_precondition(if only_examples {
            "no accepted finding with a suggestion is waiting to be committed: the accepted ones carry an \
             example to adapt by hand, which is never committed. Draft a fix for one of them, or change \
             the file yourself"
        } else {
            "no accepted finding with a suggestion is waiting to be committed; accept a finding that has a \
             suggestion first"
        }));
    }

    // GitHub now: the pull request must be open, at the reviewed commit,
    // and on a branch SDI can push to.
    let status = svc
        .knowledge
        .pull_request_status(
            &caller.principal,
            &review.project_id,
            review.pr_number.max(0) as u64,
        )
        .await
        .map_err(|e| e.into_status())?;
    let summary = status.summary.unwrap_or_default();
    if !matches!(summary.state.as_str(), "open" | "draft") {
        return Err(Status::failed_precondition(format!(
            "{pr} is {}; fixes can only be committed to an open pull request",
            if summary.state.is_empty() {
                "not open"
            } else {
                summary.state.as_str()
            }
        )));
    }
    if summary.head_sha != review.head_sha {
        return Err(Status::failed_precondition(format!(
            "{pr} moved to {} since this review of {}; review the new head first",
            short(&summary.head_sha),
            short(&review.head_sha)
        )));
    }
    if summary.is_fork {
        return Err(Status::failed_precondition(format!(
            "{pr} comes from a fork{}, whose branch SDI cannot push to; publish the review so the author can apply \
             the suggestions",
            if summary.head_repo.is_empty() {
                String::new()
            } else {
                format!(" ({})", summary.head_repo)
            }
        )));
    }
    if summary.head_ref.trim().is_empty() {
        return Err(Status::failed_precondition(format!(
            "GitHub did not name the head branch of {pr}; nothing can be committed to it"
        )));
    }
    let repository = if review.repo_full_name.trim().is_empty() {
        svc.knowledge
            .project(&caller.principal, &review.project_id)
            .await
            .map_err(|e| e.into_status())?
            .repo_full_name
    } else {
        review.repo_full_name.clone()
    };

    // Each file once, at the reviewed commit, patched bottom-up.
    let mut by_path: BTreeMap<&str, Vec<&FindingRow>> = BTreeMap::new();
    for finding in ready.iter().copied() {
        by_path
            .entry(finding.path.as_str())
            .or_default()
            .push(finding);
    }
    let mut artifacts: HashMap<String, String> = HashMap::new();
    let mut applied: Vec<&FindingRow> = Vec::new();
    let mut left_out: Vec<String> = Vec::new();
    for (path, in_file) in &by_path {
        let skip = |reason: &str, left_out: &mut Vec<String>| {
            left_out.extend(in_file.iter().map(|f| left_out_line(f, reason)));
        };
        if in_file.iter().any(|f| f.line <= 0) || path.trim().is_empty() {
            skip("does not point at a line", &mut left_out);
            continue;
        }
        if artifacts.len() >= MAX_FIX_FILES {
            skip(
                &format!(
                    "one commit carries at most {MAX_FIX_FILES} files; apply the rest afterwards"
                ),
                &mut left_out,
            );
            continue;
        }
        let file = match svc
            .knowledge
            .repo_file_at(
                &caller.principal,
                &review.project_id,
                path,
                &review.head_sha,
            )
            .await
        {
            Ok(file) => file,
            Err(error) if error.status.code() == Code::NotFound => {
                skip(
                    "the file does not exist at the reviewed commit",
                    &mut left_out,
                );
                continue;
            }
            Err(error) => return Err(error.into_status()),
        };
        if file.binary {
            skip("the file is binary and cannot be patched", &mut left_out);
            continue;
        }
        if file.truncated {
            skip(
                "the file is too large to read in full and cannot be patched",
                &mut left_out,
            );
            continue;
        }
        let FilePatch {
            content,
            applied: ids,
            left_out: skipped,
        } = fixes::patch_file(&file.content, in_file);
        for (id, reason) in skipped {
            if let Some(finding) = in_file.iter().find(|f| f.id == id) {
                left_out.push(left_out_line(finding, &reason.sentence()));
            }
        }
        if ids.is_empty() {
            continue;
        }
        applied.extend(
            ids.iter()
                .filter_map(|id| in_file.iter().find(|f| f.id == *id).copied()),
        );
        artifacts.insert((*path).to_owned(), content);
    }
    if applied.is_empty() {
        return Err(Status::failed_precondition(format!(
            "none of the {} accepted suggestion{} can be committed: {}",
            ready.len(),
            if ready.len() == 1 { "" } else { "s" },
            left_out.join("; ")
        )));
    }

    let message = fixes::commit_message(&applied, review.pr_number, review.id, &review.head_sha);
    let files = artifacts.len();
    let steps = plans::fix_plan(&plans::FixPlan {
        repository: &repository,
        branch: summary.head_ref.trim(),
        expected_head_sha: &review.head_sha,
        message: &message,
        pr_number: review.pr_number.max(0) as u64,
        fixes: applied.len(),
        files,
    });
    let execution_id = autopilot::create_execution(
        svc,
        caller,
        &review.project_id,
        fixes::fix_title(applied.len(), review.pr_number),
        &steps,
        artifacts,
    )
    .await?;

    autopilot::record(
        svc,
        caller,
        Recorded {
            kind: KIND_FIX,
            project_id: &review.project_id,
            pr_number: review.pr_number,
            review_id: Some(review.id),
            head_sha: &review.head_sha,
            execution_id: &execution_id,
        },
    )
    .await?;
    let ids: Vec<Uuid> = applied.iter().map(|f| f.id).collect();
    repo::attach_fix_execution(&svc.pool, caller.workspace, review.id, &ids, &execution_id)
        .await
        .map_err(db_status)?;

    let mut detail = format!(
        "{} fix{} in {} file{}, execution {execution_id}",
        applied.len(),
        if applied.len() == 1 { "" } else { "es" },
        files,
        if files == 1 { "" } else { "s" },
    );
    if !left_out.is_empty() {
        detail.push_str(&format!(
            "; left out {}: {}{}",
            left_out.len(),
            left_out
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join("; "),
            if left_out.len() > 5 { "; ..." } else { "" }
        ));
    }
    repo::insert_activity(
        &svc.pool,
        &activity(
            &review,
            "fixes.requested",
            &caller.principal.user_id,
            detail.clone(),
        ),
    )
    .await
    .map_err(db_status)?;
    svc.audit(
        caller,
        "review.fixes_requested",
        "review",
        &review.id.to_string(),
        detail,
    );

    let review = repo::get(&svc.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    let findings = repo::findings_of(&svc.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    emit(&svc.events, &review, "updated", &review.status, "fixes");
    Ok(convert::review(&review, &findings))
}
