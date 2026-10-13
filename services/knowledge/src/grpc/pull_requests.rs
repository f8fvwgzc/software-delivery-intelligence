//! Pull requests (CONTRACT.md §20): the review's input. `ListPullRequests`
//! serves the per-project summary cache and re-syncs it from GitHub on
//! `refresh` (or when nothing is cached yet); `GetPullRequest` always reads
//! the PR itself (its head moves) and the changed files from the diff cache
//! keyed by head commit, fetching and parsing them on a miss.
//!
//! The list is polled: the watcher (§23.1) asks for every connected
//! project's open pull requests about once a minute. Each page is a
//! conditional request, so an unchanged list is a `304` that costs no rate
//! limit, and [`SyncMarks`] remembers which listing the cache already
//! reflects, so an unchanged list writes nothing either.
//!
//! The diff is bounded: at most [`MAX_FILES`] files and [`MAX_DIFF_BYTES`]
//! of patch text in total; a file beyond the byte budget (or one GitHub
//! serves without a patch) keeps its counters and is flagged `truncated`.
//! Diffs are code, so the cache follows the workspace's `code_cache` mode
//! (§19). Every read of a PR is audited as `pr.read`.

use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::Mutex,
    time::Duration,
};

use chrono::{DateTime, Utc};
use sdi_common::{cache::TtlCache, grpc::db_status, time::opt_to_proto};
use sdi_proto::knowledge::v1::*;
use tonic::Status;
use uuid::Uuid;

use super::{KnowledgeState, github_status, limit};
use crate::{
    auth::Caller,
    convert,
    db::{
        self,
        projects::ProjectRow,
        pull_requests::{FileRecord, NewFiles, NewPullRequest},
    },
    domain::{diff, impact},
    github::{Pull, PullFile, split_full_name},
};

/// Most changed files one review reads.
pub const MAX_FILES: usize = 200;
/// Most patch bytes one review reads, across every file.
pub const MAX_DIFF_BYTES: usize = 300 * 1024;
/// Most impacted graph nodes reported, nearest first.
pub const MAX_IMPACTED: usize = 60;
/// Pull requests listed per call when the request says nothing.
const DEFAULT_LIST: u32 = 50;
const MAX_LIST: u32 = 200;
/// Projects whose last applied listing is remembered at once.
pub const SYNC_MARKS_CAPACITY: usize = 8192;
/// A mark nobody confirmed for this long is dropped; the next sync of that
/// project then writes once, which is all forgetting costs.
const SYNC_MARKS_TTL: Duration = Duration::from_secs(60 * 60);

/// The listing a project's cached summaries were last written from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SyncMark {
    filter: &'static str,
    limit: usize,
    fingerprint: u64,
    /// When GitHub last confirmed it (a write, or an identical listing).
    confirmed_at: DateTime<Utc>,
}

/// Per project, which GitHub listing the `pull_requests` rows reflect.
///
/// A sync whose listing is the very one already applied has nothing to
/// write: the rows are a function of the listing. Only the *last* applied
/// listing is remembered, so a sync with another filter or limit in
/// between (which may have changed rows) always makes the next one write
/// again. The marks are per process and never stored: a restart, or
/// another replica, costs one ordinary rewrite.
pub struct SyncMarks {
    marks: Mutex<TtlCache<(Uuid, Uuid), SyncMark>>,
}

impl Default for SyncMarks {
    fn default() -> Self {
        Self {
            marks: Mutex::new(TtlCache::new(SYNC_MARKS_CAPACITY, SYNC_MARKS_TTL)),
        }
    }
}

impl SyncMarks {
    fn lock(&self) -> std::sync::MutexGuard<'_, TtlCache<(Uuid, Uuid), SyncMark>> {
        self.marks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether the project's rows were last written from exactly this listing.
    fn applied(&self, project: (Uuid, Uuid), filter: &str, limit: usize, fingerprint: u64) -> bool {
        self.lock().get(&project).is_some_and(|mark| {
            mark.filter == filter && mark.limit == limit && mark.fingerprint == fingerprint
        })
    }

    /// Records the listing the rows now reflect (or still do).
    fn confirm(&self, project: (Uuid, Uuid), filter: &'static str, limit: usize, fingerprint: u64) {
        self.lock().insert(
            project,
            SyncMark {
                filter,
                limit,
                fingerprint,
                confirmed_at: Utc::now(),
            },
        );
    }

    /// When GitHub last confirmed what the project's rows hold, if this
    /// process knows.
    fn confirmed_at(&self, project: (Uuid, Uuid)) -> Option<DateTime<Utc>> {
        self.lock().get(&project).map(|mark| mark.confirmed_at)
    }

    /// Forgets every mark of the workspace (its data was deleted).
    pub fn forget_workspace(&self, workspace_id: Uuid) {
        self.lock().retain(|(ws, _), _| *ws != workspace_id);
    }

    /// Marks held right now (diagnostics, tests).
    pub fn held(&self) -> usize {
        self.lock().len()
    }
}

pub(super) async fn list_pull_requests(
    state: &KnowledgeState,
    caller: &Caller,
    req: ListPullRequestsRequest,
) -> Result<ListPullRequestsResponse, Status> {
    let ws = caller.workspace_id;
    let project = state.project(caller, &req.project_id).await?;
    let filter = match req.state.trim() {
        "" | "open" => "open",
        "closed" => "closed",
        "all" => "all",
        _ => {
            return Err(Status::invalid_argument(
                "state must be \"open\", \"closed\" or \"all\"",
            ));
        }
    };
    let states = db::pull_requests::states_for_filter(filter);
    let limit = limit(req.limit, DEFAULT_LIST, MAX_LIST);

    let mut rows = db::pull_requests::list(&state.pool, ws, project.id, states, limit)
        .await
        .map_err(db_status)?;
    if req.refresh || rows.is_empty() {
        let wrote = sync(
            state,
            caller,
            &project,
            filter,
            limit as usize,
            !rows.is_empty(),
        )
        .await?;
        if wrote {
            rows = db::pull_requests::list(&state.pool, ws, project.id, states, limit)
                .await
                .map_err(db_status)?;
        }
    }
    // The rows carry when they were last written; an unchanged list is
    // confirmed without a write, and that confirmation is the newer truth.
    let written_at = db::pull_requests::last_synced(&state.pool, ws, project.id)
        .await
        .map_err(db_status)?;
    let synced_at = written_at.max(state.pull_sync.confirmed_at((ws, project.id)));
    Ok(ListPullRequestsResponse {
        pull_requests: rows
            .into_iter()
            .map(convert::pull_request_summary)
            .collect(),
        synced_at: opt_to_proto(synced_at),
    })
}

/// Pulls up to `limit` summaries in `filter` from GitHub into the cache. A
/// complete listing of the open set also closes whatever the cache still
/// held as open but GitHub no longer listed.
///
/// Returns whether anything was written. A listing identical to the one
/// the rows were last written from (the usual answer to a poll: GitHub
/// said `304`) leaves the database alone; `have_rows` is what the caller
/// found cached, and a cache that is empty while GitHub lists something is
/// rewritten whatever the mark says.
async fn sync(
    state: &KnowledgeState,
    caller: &Caller,
    project: &ProjectRow,
    filter: &'static str,
    limit: usize,
    have_rows: bool,
) -> Result<bool, Status> {
    let (owner, name) = repo_parts(project)?;
    let github = state.github(caller).await?;
    let listing = github
        .pulls_listing(owner, name, filter, limit)
        .await
        .map_err(github_status)?;
    let pulls = listing.pulls;
    // The rows depend on the listing and on which repository is "ours"
    // (that decides `is_fork`), so both identify what would be written.
    let fingerprint = {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        listing.fingerprint.hash(&mut hasher);
        project
            .repo_full_name
            .to_ascii_lowercase()
            .hash(&mut hasher);
        hasher.finish()
    };
    let key = (project.workspace_id, project.id);
    if state.pull_sync.applied(key, filter, limit, fingerprint) && (have_rows || pulls.is_empty()) {
        state.pull_sync.confirm(key, filter, limit, fingerprint);
        tracing::debug!(
            project = %project.id,
            filter,
            not_modified = listing.not_modified,
            "pull requests unchanged, cache left alone"
        );
        return Ok(false);
    }
    let new_rows: Vec<NewPullRequest> = pulls
        .iter()
        .map(|pull| new_pull_request(&project.repo_full_name, pull))
        .collect();
    let mut conn = state.pool.acquire().await.map_err(db_status)?;
    db::pull_requests::upsert_all(&mut conn, project.workspace_id, project.id, &new_rows)
        .await
        .map_err(db_status)?;
    let complete = pulls.len() < limit;
    if complete && filter != "closed" {
        let still_open: Vec<i64> = pulls
            .iter()
            .filter(|p| matches!(p.platform_state(), "open" | "draft"))
            .map(|p| p.number as i64)
            .collect();
        db::pull_requests::close_missing_open(
            &mut *conn,
            project.workspace_id,
            project.id,
            &still_open,
        )
        .await
        .map_err(db_status)?;
    }
    // Only now: a failed write must leave the next sync to try again.
    state.pull_sync.confirm(key, filter, limit, fingerprint);
    tracing::debug!(project = %project.id, filter, count = pulls.len(), "pull requests synced");
    Ok(true)
}

pub(super) async fn get_pull_request(
    state: &KnowledgeState,
    caller: &Caller,
    req: GetPullRequestRequest,
) -> Result<PullRequest, Status> {
    let ws = caller.workspace_id;
    if req.number == 0 {
        return Err(Status::invalid_argument("number is required"));
    }
    let project = state.project(caller, &req.project_id).await?;
    let (owner, name) = repo_parts(&project)?;
    let github = state.github(caller).await?;

    // The PR itself is always re-read: its head commit decides which diff applies.
    let pull = github
        .pull(owner, name, req.number)
        .await
        .map_err(github_status)?;
    let row = db::pull_requests::upsert(
        &state.pool,
        ws,
        project.id,
        &new_pull_request(&project.repo_full_name, &pull),
    )
    .await
    .map_err(db_status)?;

    let cached =
        db::pull_requests::get_files(&state.pool, ws, project.id, row.number, &row.head_sha)
            .await
            .map_err(db_status)?;
    let (files, list_truncated) = match cached {
        Some(files) => (files.files.0, files.truncated),
        None => {
            let raw = github
                .pull_files(owner, name, req.number, MAX_FILES)
                .await
                .map_err(github_status)?;
            let (records, list_truncated) = bound_files(raw);
            let controls = state.controls.get(caller).await;
            if controls.stores_files() {
                let mut conn = state.pool.acquire().await.map_err(db_status)?;
                db::pull_requests::upsert_files(
                    &mut conn,
                    ws,
                    project.id,
                    &NewFiles {
                        number: row.number,
                        head_sha: &row.head_sha,
                        files: &records,
                        truncated: list_truncated,
                        expires_at: controls.file_expiry(Utc::now()),
                    },
                )
                .await
                .map_err(db_status)?;
            }
            (records, list_truncated)
        }
    };

    let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let touched_fact_ids = db::facts::ids_for_paths(&state.pool, ws, project.id, &paths)
        .await
        .map_err(db_status)?;
    let impacted_node_ids = impacted_nodes(state, caller, &project, &paths).await?;

    state.identity.audit(
        caller,
        "pr.read",
        "pull_request",
        &row.number.to_string(),
        &format!(
            "{}#{}@{} files={} (project {})",
            project.repo_full_name,
            row.number,
            row.head_sha,
            files.len(),
            project.id
        ),
    );

    let mut summary = convert::pull_request_summary(row.clone());
    if list_truncated {
        // The counter stays GitHub's; the files below are the first MAX_FILES.
        summary.changed_files = summary.changed_files.max(files.len() as u32);
    }
    Ok(PullRequest {
        summary: Some(summary),
        body: row.body,
        files: files.into_iter().map(convert::changed_file).collect(),
        touched_fact_ids: touched_fact_ids
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        impacted_node_ids,
    })
}

/// Applies the file and byte limits: files past [`MAX_FILES`] are dropped
/// (the list is flagged), and a patch that does not fit the remaining byte
/// budget, or that GitHub withheld, leaves a file with counters but no
/// hunks and `truncated = true`.
pub(crate) fn bound_files(raw: Vec<PullFile>) -> (Vec<FileRecord>, bool) {
    let list_truncated = raw.len() > MAX_FILES;
    let mut budget = MAX_DIFF_BYTES;
    let records = raw
        .into_iter()
        .take(MAX_FILES)
        .map(|file| {
            let (hunks, truncated) = match file.patch.as_deref() {
                Some(patch) if patch.len() <= budget => {
                    budget -= patch.len();
                    (diff::parse_patch(patch), false)
                }
                Some(_) => (Vec::new(), true),
                None => (Vec::new(), file.additions + file.deletions > 0),
            };
            FileRecord {
                language: sdi_analyzer::language_for_path(&file.filename).to_owned(),
                previous_path: file.previous_filename.unwrap_or_default(),
                status: normalise_status(&file.status).to_owned(),
                additions: file.additions,
                deletions: file.deletions,
                hunks,
                truncated,
                path: file.filename,
            }
        })
        .collect();
    (records, list_truncated)
}

/// GitHub's seven statuses folded onto the four the contract names.
fn normalise_status(status: &str) -> &'static str {
    match status {
        "added" | "copied" => "added",
        "removed" => "removed",
        "renamed" => "renamed",
        _ => "modified",
    }
}

/// The graph nodes a change to these paths reaches: for every path, the
/// node covering it and the nodes the impact walk finds from it, deduped
/// in that order and cut at [`MAX_IMPACTED`]. A project without a graph
/// yields nothing.
async fn impacted_nodes(
    state: &KnowledgeState,
    caller: &Caller,
    project: &ProjectRow,
    paths: &[String],
) -> Result<Vec<String>, Status> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let ws = caller.workspace_id;
    let nodes = db::graph::nodes(&state.pool, ws, project.id)
        .await
        .map_err(db_status)?;
    if nodes.is_empty() {
        return Ok(Vec::new());
    }
    let by_path: Vec<(String, String)> = nodes
        .iter()
        .map(|n| (n.id.to_string(), n.source_path.clone()))
        .collect();
    let edges: Vec<impact::Edge> = db::graph::edges(&state.pool, ws, project.id, false)
        .await
        .map_err(db_status)?
        .into_iter()
        .map(|e| impact::Edge {
            source: e.source_id.to_string(),
            target: e.target_id.to_string(),
            relation: e.relation,
            category: e.category,
        })
        .collect();
    Ok(impacted_from_paths(&by_path, &edges, paths))
}

/// The pure half of [`impacted_nodes`].
pub(crate) fn impacted_from_paths(
    nodes: &[(String, String)],
    edges: &[impact::Edge],
    paths: &[String],
) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    let mut walked: HashMap<String, Vec<String>> = HashMap::new();
    for path in paths {
        let Some(origin) = impact::origin_for_path(nodes, path) else {
            continue;
        };
        let reached = walked.entry(origin.to_owned()).or_insert_with(|| {
            let mut ids = vec![origin.to_owned()];
            ids.extend(
                impact::analyze(origin, edges, impact::DEFAULT_MAX_DEPTH)
                    .into_iter()
                    .map(|n| n.id),
            );
            ids
        });
        for id in reached {
            if out.len() >= MAX_IMPACTED {
                return out;
            }
            if seen.insert(id.clone()) {
                out.push(id.clone());
            }
        }
    }
    out
}

/// What a GitHub pull request becomes in the cache. `repository` is the
/// project's own `owner/name` and decides `is_fork`: a head branch that
/// lives in any other repository, or in one that no longer exists, is a
/// fork's. "The project's repository" is the name the project was
/// connected under or the one GitHub gives the base branch now, so a
/// repository that was renamed or transferred since does not turn every
/// one of its own branches into a fork.
pub(super) fn new_pull_request(repository: &str, pull: &Pull) -> NewPullRequest {
    let repo_of = |end: &Option<crate::github::PullRef>| {
        end.as_ref()
            .and_then(|end| end.repo.as_ref())
            .map(|repo| repo.full_name.trim().to_owned())
            .filter(|full_name| !full_name.is_empty())
    };
    let head_repo = repo_of(&pull.head);
    let base_repo = repo_of(&pull.base);
    let is_fork = head_repo.as_deref().is_none_or(|head| {
        !head.eq_ignore_ascii_case(repository.trim())
            && !base_repo
                .as_deref()
                .is_some_and(|base| head.eq_ignore_ascii_case(base))
    });
    NewPullRequest {
        head_repo: head_repo.unwrap_or_default(),
        is_fork,
        is_bot: pull.user.as_ref().is_some_and(|user| user.is_bot()),
        is_dependabot: pull.user.as_ref().is_some_and(|user| user.is_dependabot()),
        number: pull.number as i64,
        title: pull.title.clone(),
        body: pull.body.clone().unwrap_or_default(),
        author: pull
            .user
            .as_ref()
            .map(|u| u.login.clone())
            .unwrap_or_default(),
        author_avatar_url: pull
            .user
            .as_ref()
            .map(|u| u.avatar_url.clone())
            .unwrap_or_default(),
        state: pull.platform_state().to_owned(),
        base_ref: pull
            .base
            .as_ref()
            .map(|b| b.reference.clone())
            .unwrap_or_default(),
        head_ref: pull
            .head
            .as_ref()
            .map(|h| h.reference.clone())
            .unwrap_or_default(),
        head_sha: pull
            .head
            .as_ref()
            .map(|h| h.sha.clone())
            .unwrap_or_default(),
        changed_files: pull.changed_files as i32,
        additions: pull.additions as i32,
        deletions: pull.deletions as i32,
        html_url: pull.html_url.clone(),
        pr_created_at: pull.created_at,
        pr_updated_at: pull.updated_at,
    }
}

pub(super) fn repo_parts(project: &ProjectRow) -> Result<(&str, &str), Status> {
    split_full_name(&project.repo_full_name).ok_or_else(|| {
        Status::internal(format!(
            "project has an invalid repository name: {}",
            project.repo_full_name
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, patch: Option<String>, additions: u32) -> PullFile {
        PullFile {
            filename: name.into(),
            status: "modified".into(),
            additions,
            deletions: 0,
            patch,
            ..Default::default()
        }
    }

    #[test]
    fn files_are_bounded_by_count_and_bytes() {
        let big = format!("@@ -1 +1 @@\n+{}", "x".repeat(MAX_DIFF_BYTES));
        let mut raw = vec![
            file("a.rs", Some("@@ -1 +1,2 @@\n a\n+b".into()), 1),
            file("big.rs", Some(big), 1),
            file("bin.png", None, 0),
            file("huge.min.js", None, 900),
        ];
        for i in 0..MAX_FILES {
            raw.push(file(&format!("f{i}.rs"), Some("@@ -1 +1 @@\n+x".into()), 1));
        }
        let (records, list_truncated) = bound_files(raw);
        assert!(list_truncated);
        assert_eq!(records.len(), MAX_FILES);
        assert_eq!(records[0].hunks.len(), 1);
        assert!(!records[0].truncated);
        assert_eq!(records[0].language, "rust");
        // Over the byte budget: counters kept, hunks dropped, flagged.
        assert!(records[1].truncated);
        assert!(records[1].hunks.is_empty());
        assert_eq!(records[1].additions, 1);
        // No patch and no changes (binary): not truncated.
        assert!(!records[2].truncated);
        // No patch but changes (GitHub withheld a large diff): truncated.
        assert!(records[3].truncated);
        // The budget was spent by the big file only if it fit; it did not, so
        // the small ones after it still parse.
        assert_eq!(records[4].hunks.len(), 1);
    }

    #[test]
    fn statuses_fold_onto_the_contract_set() {
        assert_eq!(normalise_status("copied"), "added");
        assert_eq!(normalise_status("changed"), "modified");
        assert_eq!(normalise_status("renamed"), "renamed");
        assert_eq!(normalise_status("removed"), "removed");
    }

    #[test]
    fn impacted_nodes_are_deduped_in_walk_order_and_capped() {
        let nodes = vec![
            ("api".to_owned(), "services/api".to_owned()),
            ("web".to_owned(), "web".to_owned()),
            ("db".to_owned(), String::new()),
        ];
        let edges = vec![
            impact::Edge {
                source: "web".into(),
                target: "api".into(),
                relation: "DEPENDS_ON".into(),
                category: "infrastructure".into(),
            },
            impact::Edge {
                source: "api".into(),
                target: "db".into(),
                relation: "DEPENDS_ON".into(),
                category: "infrastructure".into(),
            },
        ];
        let paths = vec![
            "services/api/src/main.rs".to_owned(),
            "services/api/Cargo.toml".to_owned(),
            "web/src/App.tsx".to_owned(),
        ];
        let ids = impacted_from_paths(&nodes, &edges, &paths);
        assert_eq!(ids, vec!["api", "db", "web"]);
        assert!(impacted_from_paths(&[], &edges, &paths).is_empty());
    }

    #[test]
    fn pull_state_and_summary_mapping() {
        let mut pull = Pull {
            number: 7,
            title: "Add CI".into(),
            state: "open".into(),
            draft: true,
            ..Default::default()
        };
        assert_eq!(pull.platform_state(), "draft");
        pull.draft = false;
        assert_eq!(pull.platform_state(), "open");
        pull.state = "closed".into();
        assert_eq!(pull.platform_state(), "closed");
        pull.merged_at = Some(Utc::now());
        assert_eq!(pull.platform_state(), "merged");
        let row = new_pull_request("acme/api", &pull);
        assert_eq!(row.number, 7);
        assert_eq!(row.state, "merged");
        assert_eq!(row.author, "");
        // The single-PR payload says `merged` outright.
        let merged = Pull {
            state: "closed".into(),
            merged: true,
            ..Default::default()
        };
        assert_eq!(merged.platform_state(), "merged");
    }

    fn pull_from(json: serde_json::Value) -> Pull {
        serde_json::from_value(json).expect("a pull request payload")
    }

    #[test]
    fn origin_flags_come_from_the_head_repository_and_the_author() {
        // A branch of the project's own repository, opened by a person.
        let own = new_pull_request(
            "acme/api",
            &pull_from(serde_json::json!({
                "number": 1,
                "user": {"login": "ana", "type": "User"},
                "head": {"ref": "feat/x", "sha": "a1", "repo": {"full_name": "Acme/API"}},
            })),
        );
        assert_eq!(own.head_repo, "Acme/API");
        assert!(
            !own.is_fork,
            "repository names compare without case, as GitHub's do"
        );
        assert!(!own.is_bot && !own.is_dependabot);

        // A fork.
        let fork = new_pull_request(
            "acme/api",
            &pull_from(serde_json::json!({
                "number": 2,
                "user": {"login": "bo", "type": "User"},
                "head": {"ref": "patch-1", "sha": "b2", "repo": {"full_name": "bo/api"}},
            })),
        );
        assert_eq!(fork.head_repo, "bo/api");
        assert!(fork.is_fork);

        // A fork that was deleted: GitHub sends `"repo": null`.
        let gone = new_pull_request(
            "acme/api",
            &pull_from(serde_json::json!({
                "number": 3,
                "user": {"login": "bo", "type": "User"},
                "head": {"ref": "patch-2", "sha": "c3", "repo": null},
            })),
        );
        assert_eq!(gone.head_repo, "");
        assert!(gone.is_fork);

        // The repository was renamed after it was connected: GitHub names
        // both ends by the new name, and a branch of the repository itself
        // is still not a fork. A fork of the renamed repository still is.
        let renamed = |head_repo: &str| {
            new_pull_request(
                "acme/api",
                &pull_from(serde_json::json!({
                    "number": 7,
                    "user": {"login": "ana", "type": "User"},
                    "head": {"ref": "feat/y", "sha": "a7", "repo": {"full_name": head_repo}},
                    "base": {"ref": "main", "sha": "b7", "repo": {"full_name": "acme/api-v2"}},
                })),
            )
        };
        let own = renamed("acme/api-v2");
        assert!(!own.is_fork);
        assert_eq!(own.head_repo, "acme/api-v2");
        assert!(renamed("bo/api-v2").is_fork);

        // Dependabot, under both logins; a bot by its account type.
        for login in ["dependabot[bot]", "dependabot-preview[bot]"] {
            let dependabot = new_pull_request(
                "acme/api",
                &pull_from(serde_json::json!({
                    "number": 4,
                    "user": {"login": login, "type": "Bot"},
                    "head": {"ref": "dependabot/cargo/serde-1.0.200", "sha": "d4",
                             "repo": {"full_name": "acme/api"}},
                })),
            );
            assert!(dependabot.is_bot && dependabot.is_dependabot, "{login}");
            assert!(!dependabot.is_fork);
        }
        let other_bot = new_pull_request(
            "acme/api",
            &pull_from(serde_json::json!({
                "number": 5,
                "user": {"login": "renovate[bot]", "type": "Bot"},
                "head": {"ref": "renovate/serde-1.x", "sha": "e5", "repo": {"full_name": "acme/api"}},
            })),
        );
        assert!(other_bot.is_bot && !other_bot.is_dependabot);
        // A person whose login merely mentions Dependabot is neither.
        let person = new_pull_request(
            "acme/api",
            &pull_from(serde_json::json!({
                "number": 6,
                "user": {"login": "dependabot-fan", "type": "User"},
                "head": {"ref": "x", "sha": "f6", "repo": {"full_name": "acme/api"}},
            })),
        );
        assert!(!person.is_bot && !person.is_dependabot);
    }

    #[test]
    fn a_sync_mark_holds_only_the_last_applied_listing() {
        let marks = SyncMarks::default();
        let project = (Uuid::now_v7(), Uuid::now_v7());
        assert!(!marks.applied(project, "open", 50, 1));
        assert!(marks.confirmed_at(project).is_none());

        marks.confirm(project, "open", 50, 1);
        assert!(marks.applied(project, "open", 50, 1));
        assert!(marks.confirmed_at(project).is_some());
        // Another listing, another limit, another filter: all would write.
        assert!(!marks.applied(project, "open", 50, 2));
        assert!(!marks.applied(project, "open", 10, 1));
        assert!(!marks.applied(project, "all", 50, 1));
        assert!(!marks.applied((project.0, Uuid::now_v7()), "open", 50, 1));

        // A sync with another filter replaces the mark, so the open
        // listing is written again the next time, whatever it holds.
        marks.confirm(project, "all", 50, 9);
        assert!(!marks.applied(project, "open", 50, 1));
        assert_eq!(marks.held(), 1);

        marks.forget_workspace(project.0);
        assert_eq!(marks.held(), 0);
    }

    #[test]
    fn sync_marks_are_bounded() {
        let marks = SyncMarks::default();
        let workspace = Uuid::now_v7();
        for _ in 0..(SYNC_MARKS_CAPACITY + 50) {
            marks.confirm((workspace, Uuid::now_v7()), "open", 50, 1);
        }
        assert_eq!(marks.held(), SYNC_MARKS_CAPACITY);
    }
}
