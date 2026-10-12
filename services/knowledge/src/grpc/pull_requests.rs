//! Pull requests (CONTRACT.md §20): the review's input. `ListPullRequests`
//! serves the per-project summary cache and re-syncs it from GitHub on
//! `refresh` (or when nothing is cached yet); `GetPullRequest` always reads
//! the PR itself (its head moves) and the changed files from the diff cache
//! keyed by head commit, fetching and parsing them on a miss.
//!
//! The diff is bounded: at most [`MAX_FILES`] files and [`MAX_DIFF_BYTES`]
//! of patch text in total; a file beyond the byte budget (or one GitHub
//! serves without a patch) keeps its counters and is flagged `truncated`.
//! Diffs are code, so the cache follows the workspace's `code_cache` mode
//! (§19). Every read of a PR is audited as `pr.read`.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sdi_common::{grpc::db_status, time::opt_to_proto};
use sdi_proto::knowledge::v1::*;
use tonic::Status;

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
        sync(state, caller, &project, filter, limit as usize).await?;
        rows = db::pull_requests::list(&state.pool, ws, project.id, states, limit)
            .await
            .map_err(db_status)?;
    }
    let synced_at = db::pull_requests::last_synced(&state.pool, ws, project.id)
        .await
        .map_err(db_status)?;
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
async fn sync(
    state: &KnowledgeState,
    caller: &Caller,
    project: &ProjectRow,
    filter: &str,
    limit: usize,
) -> Result<(), Status> {
    let (owner, name) = repo_parts(project)?;
    let github = state.github(caller).await?;
    let pulls = github
        .pulls(owner, name, filter, limit)
        .await
        .map_err(github_status)?;
    let new_rows: Vec<NewPullRequest> = pulls.iter().map(new_pull_request).collect();
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
    tracing::debug!(project = %project.id, filter, count = pulls.len(), "pull requests synced");
    Ok(())
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
    let row = db::pull_requests::upsert(&state.pool, ws, project.id, &new_pull_request(&pull))
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

fn new_pull_request(pull: &Pull) -> NewPullRequest {
    NewPullRequest {
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

fn repo_parts(project: &ProjectRow) -> Result<(&str, &str), Status> {
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
        let row = new_pull_request(&pull);
        assert_eq!(row.number, 7);
        assert_eq!(row.state, "merged");
        assert_eq!(row.author, "");
    }
}
