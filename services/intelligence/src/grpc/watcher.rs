//! The pull request watcher (CONTRACT.md §23.1): a background task that
//! looks at the open pull requests of every workspace that switched
//! `auto_review` on, starts the reviews that are missing and, where the
//! workspace asked for it, requests the merge of Dependabot's pull requests
//! that are ready. It works with a plain token: nothing here needs a webhook.
//!
//! Bounds: one look at a time across replicas (a Postgres advisory lock),
//! at most [`MAX_REVIEWS_PER_LOOK`] reviews and [`MAX_MERGES_PER_LOOK`]
//! merge requests started per workspace per look, and a workspace whose
//! look fails is left until the next one with the reason in its state row.

use std::{collections::HashSet, time::Duration};

use futures::StreamExt;

use sdi_common::principal::Principal;
use sdi_proto::{
    intelligence::v1::StartReviewRequest,
    knowledge::v1::{Project, PullRequestSummary},
};
use tonic::{Code, Status};
use uuid::Uuid;

use super::{
    Caller, IntelligenceGrpc, MergeCheckKey, locked,
    merge::{self, MergeAsk},
    reviews,
};
use crate::{
    db::{autopilot as autopilot_repo, reviews as review_repo, settings as settings_repo},
    domain::merge as gate,
};

/// The activity log's actor for what the watcher starts.
pub(crate) const ACTOR: &str = "watcher";
pub(crate) const MAX_REVIEWS_PER_LOOK: usize = 5;
pub(crate) const MAX_MERGES_PER_LOOK: usize = 2;
/// Open pull requests read per project per look.
pub(crate) const PULL_REQUESTS_PER_PROJECT: u32 = 50;
/// One workspace may not hold the others up for longer than this.
const WORKSPACE_BUDGET: Duration = Duration::from_secs(120);
/// Workspaces looked at side by side. Each look is mostly waiting on
/// GitHub through knowledge, so a few at once shorten a round a lot; more
/// than a few would only queue behind knowledge's own limits.
const WORKSPACES_AT_ONCE: usize = 8;

/// What one look did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Look {
    /// False when another replica was looking: nothing was done.
    pub ran: bool,
    pub workspaces: usize,
    pub reviews_started: usize,
    pub merges_requested: usize,
    /// Workspaces whose look failed, with the reason that was recorded.
    pub failed: Vec<(Uuid, String)>,
}

/// What one workspace's look did.
#[derive(Debug, Default)]
struct Seen {
    reviews_started: usize,
    /// Reviews that could not be started. Bounded like the ones that
    /// could, so a look never turns into one failing call per pull request.
    reviews_failed: usize,
    merges_requested: usize,
    /// Problems with single projects or pull requests: the look went on.
    problems: Vec<String>,
}

/// A failure that is about one project or one pull request (it is gone,
/// it cannot be reviewed) rather than about the workspace's access to
/// knowledge or GitHub. The first kind is noted and the look goes on; the
/// second ends the workspace's look.
fn local_failure(status: &Status) -> bool {
    matches!(
        status.code(),
        Code::NotFound | Code::FailedPrecondition | Code::InvalidArgument
    )
}

fn problem(what: &str, status: &Status) -> String {
    format!("{what}: {}", status.message())
}

async fn look_at_project(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    project: &Project,
    settings: &settings_repo::SettingsRow,
    seen: &mut Seen,
) -> Result<(), Status> {
    let pull_requests = svc
        .knowledge
        .list_pull_requests(
            &caller.principal,
            &project.id,
            "open",
            true,
            PULL_REQUESTS_PER_PROJECT,
        )
        .await
        .map_err(|e| e.into_status())?;
    // Drafts are not reviewed and not merged; a head without a commit
    // cannot be either.
    let open: Vec<&PullRequestSummary> = pull_requests
        .iter()
        .filter(|pr| pr.state == "open" && pr.number > 0 && !pr.head_sha.trim().is_empty())
        .collect();
    if open.is_empty() {
        return Ok(());
    }
    let numbers: Vec<i64> = open
        .iter()
        .map(|pr| i64::try_from(pr.number).unwrap_or(i64::MAX))
        .collect();

    // --- Reviews: one per head that has none ------------------------------
    let reviewed: HashSet<(i64, String)> =
        review_repo::reviewed_heads(&svc.pool, caller.workspace, &project.id, &numbers)
            .await
            .map_err(sdi_common::grpc::db_status)?
            .into_iter()
            .collect();
    for (pr, number) in open.iter().zip(&numbers) {
        if seen.reviews_started >= MAX_REVIEWS_PER_LOOK
            || seen.reviews_failed >= MAX_REVIEWS_PER_LOOK
        {
            break;
        }
        if reviewed.contains(&(*number, pr.head_sha.clone())) {
            continue;
        }
        match reviews::start_as(
            svc,
            caller,
            StartReviewRequest {
                project_id: project.id.clone(),
                pr_number: pr.number,
            },
            ACTOR,
            // A review that failed for this head stays failed until a
            // person asks again: the watcher does not retry it every look.
            false,
        )
        .await
        {
            Ok(started) => {
                if started.created {
                    seen.reviews_started += 1;
                    tracing::info!(workspace = %caller.workspace, project = %project.id, pr = pr.number, review = %started.review.id, "watcher started a review");
                }
            }
            Err(status) if local_failure(&status) => {
                seen.reviews_failed += 1;
                seen.problems.push(problem(
                    &format!("{} #{}", project.name, pr.number),
                    &status,
                ));
            }
            Err(status) => return Err(status),
        }
    }

    // --- Auto-merge: Dependabot's pull requests that are ready -------------
    if !settings.merges_dependabot() || seen.merges_requested >= MAX_MERGES_PER_LOOK {
        return Ok(());
    }
    let dependabot: Vec<(&PullRequestSummary, i64)> = open
        .iter()
        .zip(&numbers)
        .filter(|(pr, _)| pr.is_dependabot)
        .map(|(pr, number)| (*pr, *number))
        .collect();
    if dependabot.is_empty() {
        return Ok(());
    }
    let dependabot_numbers: Vec<i64> = dependabot.iter().map(|(_, n)| *n).collect();
    // Once per head, whatever became of the execution: a merge a person
    // rejected is not asked for again until the pull request moves.
    let requested: HashSet<(i64, String)> = autopilot_repo::merge_requested_heads(
        &svc.pool,
        caller.workspace,
        &project.id,
        &dependabot_numbers,
    )
    .await
    .map_err(sdi_common::grpc::db_status)?
    .into_iter()
    .collect();
    for (pr, number) in dependabot {
        if seen.merges_requested >= MAX_MERGES_PER_LOOK {
            break;
        }
        if requested.contains(&(number, pr.head_sha.clone())) {
            continue;
        }
        let key: MergeCheckKey = (
            caller.workspace,
            project.id.clone(),
            pr.number,
            pr.head_sha.clone(),
        );
        if locked(&svc.merge_recheck).get(&key).is_some() {
            continue;
        }
        // SDI's own gate first: it costs GitHub nothing.
        let (own, _) =
            merge::gate(svc, caller, &project.id, pr.number, &pr.head_sha, settings).await?;
        if !gate::gate_blockers(&pr.head_sha, &own).is_empty() {
            continue;
        }
        // A person may be asking for this very merge right now: whoever
        // holds the turn creates the execution, and the other steps back.
        let Ok(_one_at_a_time) = super::autopilot::exclusive(
            svc,
            caller.workspace,
            &merge::merge_lock(&project.id, pr.number),
        )
        .await
        else {
            continue;
        };
        let ready = match merge::compute(svc, caller, &project.id, pr.number, settings).await {
            Ok(ready) => ready,
            Err(status) if local_failure(&status) => {
                locked(&svc.merge_recheck).insert(key, ());
                seen.problems.push(problem(
                    &format!("{} #{}", project.name, pr.number),
                    &status,
                ));
                continue;
            }
            Err(status) => return Err(status),
        };
        if !ready.blockers.is_empty() {
            // Not ready: leave this head alone for a while instead of
            // asking GitHub about it on every look.
            locked(&svc.merge_recheck).insert(key, ());
            continue;
        }
        match merge::request_merge(
            svc,
            caller,
            MergeAsk {
                project_id: &project.id,
                pr_number: pr.number,
                method: "",
                delete_branch: false,
                actor: ACTOR,
            },
            ready,
        )
        .await
        {
            Ok((execution_id, _)) => {
                seen.merges_requested += 1;
                tracing::info!(workspace = %caller.workspace, project = %project.id, pr = pr.number, execution = %execution_id, "watcher requested a merge");
            }
            // It was ready and still could not be asked for (no merge
            // method the repository allows, say): the same wait applies.
            Err(status) if local_failure(&status) => {
                locked(&svc.merge_recheck).insert(key, ());
                seen.problems.push(problem(
                    &format!("merge of {} #{}", project.name, pr.number),
                    &status,
                ));
            }
            Err(status) => return Err(status),
        }
    }
    Ok(())
}

async fn look_at_workspace(svc: &IntelligenceGrpc, caller: &Caller) -> Result<Seen, Status> {
    let settings = svc.settings(caller).await?;
    // The switch may have been turned off since the list was read.
    if !settings.auto_review {
        return Ok(Seen::default());
    }
    let projects = svc
        .knowledge
        .list_projects(&caller.principal)
        .await
        .map_err(|e| e.into_status())?;
    let mut seen = Seen::default();
    for project in &projects {
        match look_at_project(svc, caller, project, &settings, &mut seen).await {
            Ok(()) => {}
            Err(status) if local_failure(&status) => {
                seen.problems.push(problem(&project.name, &status))
            }
            Err(status) => return Err(status),
        }
    }
    Ok(seen)
}

/// The sentence kept in `pr_watch_last_error`: short enough for a settings page.
fn error_text(problems: &[String]) -> String {
    let Some(first) = problems.first() else {
        return String::new();
    };
    let mut text: String = first.chars().take(400).collect();
    if problems.len() > 1 {
        text.push_str(&format!(" (and {} more)", problems.len() - 1));
    }
    text
}

/// One advisory lock per schema: replicas of one deployment share it, two
/// deployments in one database do not get in each other's way.
pub(crate) const LOCK_SQL: &str = "SELECT pg_try_advisory_lock(hashtextextended('sdi.intelligence.pr_watcher:' || current_schema(), 0))";
pub(crate) const UNLOCK_SQL: &str = "SELECT pg_advisory_unlock(hashtextextended('sdi.intelligence.pr_watcher:' || current_schema(), 0))";

/// One look. Callable directly (the tests do): it does not sleep.
pub(crate) async fn tick(svc: &IntelligenceGrpc) -> Look {
    // The lock belongs to a session, so it gets a connection of its own
    // that is closed rather than returned to the pool: whatever happens to
    // this task, the lock goes with the connection.
    let mut lock = match svc.pool.acquire().await {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(%error, "pull request watcher: no database connection; look skipped");
            return Look::default();
        }
    };
    lock.close_on_drop();
    match sqlx::query_scalar::<_, bool>(LOCK_SQL)
        .fetch_one(&mut *lock)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            tracing::debug!("pull request watcher: another replica is looking; look skipped");
            return Look::default();
        }
        Err(error) => {
            tracing::warn!(%error, "pull request watcher: could not take the lock; look skipped");
            return Look::default();
        }
    }

    let mut look = Look {
        ran: true,
        ..Look::default()
    };
    let workspaces = match settings_repo::auto_review_workspaces(&svc.pool).await {
        Ok(workspaces) => workspaces,
        Err(error) => {
            tracing::warn!(%error, "pull request watcher: could not list the workspaces to watch");
            Vec::new()
        }
    };
    // Workspaces are independent, so several are looked at side by side;
    // each still gets its own budget and its own recorded outcome.
    let outcomes: Vec<(Uuid, usize, usize, String)> = futures::stream::iter(workspaces)
        .map(|workspace| async move {
            let caller = Caller {
                principal: Principal::system(workspace.to_string()),
                workspace,
            };
            let outcome = match tokio::time::timeout(
                WORKSPACE_BUDGET,
                look_at_workspace(svc, &caller),
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(_) => Err(Status::deadline_exceeded(format!(
                    "the look took longer than {} seconds",
                    WORKSPACE_BUDGET.as_secs()
                ))),
            };
            let (reviews, merges, error) = match outcome {
                Ok(seen) => (
                    seen.reviews_started,
                    seen.merges_requested,
                    error_text(&seen.problems),
                ),
                Err(status) => (0, 0, error_text(&[status.message().to_owned()])),
            };
            if !error.is_empty() {
                tracing::warn!(%workspace, %error, "pull request watcher: look failed");
            }
            if let Err(db_error) =
                autopilot_repo::record_watch(&svc.pool, workspace, &error).await
            {
                tracing::warn!(%workspace, error = %db_error, "pull request watcher: could not record the look");
            }
            (workspace, reviews, merges, error)
        })
        .buffer_unordered(WORKSPACES_AT_ONCE)
        .collect()
        .await;
    for (workspace, reviews, merges, error) in outcomes {
        look.workspaces += 1;
        look.reviews_started += reviews;
        look.merges_requested += merges;
        if !error.is_empty() {
            look.failed.push((workspace, error));
        }
    }
    // Side by side means "in no particular order": keep the report stable.
    look.failed.sort();

    if let Err(error) = sqlx::query_scalar::<_, bool>(UNLOCK_SQL)
        .fetch_one(&mut *lock)
        .await
    {
        tracing::debug!(%error, "pull request watcher: unlock failed; closing the connection releases the lock");
    }
    if let Err(error) = lock.close().await {
        tracing::debug!(%error, "pull request watcher: lock connection did not close cleanly");
    }
    look
}

/// Starts the watcher: one look every `seconds`, until shutdown. `0` means
/// the operator turned it off, and nothing is spawned.
pub(crate) fn spawn(svc: IntelligenceGrpc, seconds: u32) -> Option<tokio::task::JoinHandle<()>> {
    if seconds == 0 {
        tracing::info!("SDI_PR_WATCH_SECONDS=0: the pull request watcher is off");
        return None;
    }
    let period = Duration::from_secs(u64::from(seconds));
    Some(sdi_common::tasks::spawn("pr-watcher", async move {
        // The first look waits one period: the peers may still be starting.
        let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                () = sdi_common::shutdown::cancelled() => return,
            }
            // A look in flight is dropped at shutdown; its lock goes with
            // its connection.
            tokio::select! {
                look = tick(&svc) => {
                    if look.ran && (look.reviews_started > 0 || look.merges_requested > 0 || !look.failed.is_empty()) {
                        tracing::info!(
                            workspaces = look.workspaces,
                            reviews_started = look.reviews_started,
                            merges_requested = look.merges_requested,
                            failed = look.failed.len(),
                            "pull request watcher: look done"
                        );
                    }
                }
                () = sdi_common::shutdown::cancelled() => return,
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recorded_error_is_one_short_sentence() {
        assert_eq!(error_text(&[]), "");
        assert_eq!(error_text(&["api #3: gone".into()]), "api #3: gone");
        assert_eq!(
            error_text(&["api: gone".into(), "web: gone".into(), "x".into()]),
            "api: gone (and 2 more)"
        );
        assert_eq!(error_text(&["e".repeat(1_000)]).chars().count(), 400);
    }

    #[test]
    fn only_project_scoped_failures_let_the_look_go_on() {
        for code in [
            Code::NotFound,
            Code::FailedPrecondition,
            Code::InvalidArgument,
        ] {
            assert!(local_failure(&Status::new(code, "x")), "{code:?}");
        }
        for code in [
            Code::Unavailable,
            Code::DeadlineExceeded,
            Code::PermissionDenied,
            Code::Unauthenticated,
            Code::ResourceExhausted,
            Code::Internal,
        ] {
            assert!(!local_failure(&Status::new(code, "x")), "{code:?}");
        }
    }
}
