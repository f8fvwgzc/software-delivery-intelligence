//! A GitHub repository that keeps real git state, for the pull request
//! autopilot (CONTRACT.md §23): blobs, trees and commits addressed by their
//! content, branches that only move forward unless forced, pull requests
//! whose head follows their branch and whose merge creates a commit on the
//! base, check runs, commit statuses, the two Dependabot switches and the
//! tarball a checkout downloads.
//!
//! Nothing a test asserts is scripted here: a commit exists because the
//! adapter created its blobs, its tree and the commit itself, a branch
//! moved because the adapter moved it. The switches in [`Behaviour`] only
//! decide how the repository answers (a read-only token, a push that lands
//! in between, a merge GitHub refuses).

use std::{
    collections::BTreeMap,
    io::Write,
    sync::{Arc, Mutex, MutexGuard},
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::State as Shared,
    http::{HeaderMap, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub use super::github::TOKEN;

pub const FILE: &str = "100644";
pub const EXECUTABLE: &str = "100755";
pub const SYMLINK: &str = "120000";
const DIRECTORY: &str = "040000";
const DEFAULT_BRANCH: &str = "main";

#[derive(Clone)]
struct Entry {
    mode: String,
    kind: &'static str,
    sha: String,
}

#[derive(Clone)]
struct Commit {
    tree: String,
    parents: Vec<String>,
    message: String,
}

/// A tree seen flat: path -> (mode, blob sha).
type Files = BTreeMap<String, (String, String)>;

#[derive(Debug, Clone)]
pub struct Pull {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub head_ref: String,
    pub head_sha: String,
    /// `owner/name` of the repository the head branch lives in; empty when
    /// that fork was deleted.
    pub head_repo: String,
    pub base_ref: String,
    pub open: bool,
    pub merged: bool,
    pub draft: bool,
    pub mergeable: Option<bool>,
    pub mergeable_state: String,
    pub merge_commit_sha: String,
    /// Reads that still answer `mergeable: null` (GitHub computes it lazily).
    pub unknown_reads: usize,
}

/// How the repository answers. Everything is off by default.
#[derive(Default)]
pub struct Behaviour {
    /// Every write answers 403: the token may only read.
    pub read_only: bool,
    /// The repository settings endpoints answer 404: not an admin.
    pub not_admin: bool,
    /// `permissions.admin` in `GET /repos/{repo}`.
    pub says_admin: bool,
    /// What `PUT /pulls/{n}/merge` answers instead of merging.
    pub merge_refusal: Option<(u16, String)>,
    /// Somebody's push that lands on `(branch, path, content)` right before
    /// the next ref update arrives.
    pub push_before_ref_update: Option<(String, String, String)>,
    /// What the next ref update is refused with (422), nothing moved.
    pub ref_update_refusal: Option<String>,
    /// Branches GitHub refuses to delete.
    pub protected: Vec<String>,
    /// Status of the refusal to turn security updates on while alerts are
    /// off (422 when unset).
    pub updates_refusal: Option<u16>,
    /// Turning security updates on answers 403 whatever the alerts say.
    pub updates_forbidden: bool,
}

struct State {
    name: String,
    prefix: String,
    blobs: BTreeMap<String, String>,
    trees: BTreeMap<String, BTreeMap<String, Entry>>,
    commits: BTreeMap<String, Commit>,
    refs: BTreeMap<String, String>,
    pulls: BTreeMap<u64, Pull>,
    check_runs: BTreeMap<String, Vec<Value>>,
    statuses: BTreeMap<String, Vec<(String, String)>>,
    alerts: bool,
    security_updates: bool,
    counter: u64,
    behaviour: Behaviour,
    calls: Vec<String>,
    merges: Vec<Value>,
}

fn oid(kind: &str, data: &str) -> String {
    let digest = Sha256::digest(format!("{kind}\0{data}").as_bytes());
    hex::encode(digest)[..40].to_owned()
}

impl State {
    fn blob(&mut self, content: &str) -> String {
        let sha = oid("blob", content);
        self.blobs.insert(sha.clone(), content.to_owned());
        sha
    }

    /// Stores the (nested) tree of `files` and returns its sha. The same
    /// files always give the same sha.
    fn write_tree(&mut self, files: &Files) -> String {
        let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
        let mut directories: BTreeMap<String, Files> = BTreeMap::new();
        for (path, (mode, sha)) in files {
            match path.split_once('/') {
                None => {
                    entries.insert(
                        path.clone(),
                        Entry {
                            mode: mode.clone(),
                            kind: "blob",
                            sha: sha.clone(),
                        },
                    );
                }
                Some((directory, rest)) => {
                    directories
                        .entry(directory.to_owned())
                        .or_default()
                        .insert(rest.to_owned(), (mode.clone(), sha.clone()));
                }
            }
        }
        for (name, inner) in directories {
            let sha = self.write_tree(&inner);
            entries.insert(
                name,
                Entry {
                    mode: DIRECTORY.to_owned(),
                    kind: "tree",
                    sha,
                },
            );
        }
        let listing: String = entries
            .iter()
            .map(|(name, entry)| format!("{} {} {}\t{name}\n", entry.mode, entry.kind, entry.sha))
            .collect();
        let sha = oid("tree", &listing);
        self.trees.insert(sha.clone(), entries);
        sha
    }

    fn files(&self, tree: &str) -> Files {
        let mut files = Files::new();
        self.collect(tree, "", &mut files);
        files
    }

    fn collect(&self, tree: &str, prefix: &str, files: &mut Files) {
        let Some(entries) = self.trees.get(tree) else {
            return;
        };
        for (name, entry) in entries {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if entry.kind == "tree" {
                self.collect(&entry.sha, &path, files);
            } else {
                files.insert(path, (entry.mode.clone(), entry.sha.clone()));
            }
        }
    }

    /// A new commit object. Two commits of the same tree are still two
    /// commits, as on GitHub, where the timestamp tells them apart.
    fn commit(&mut self, tree: &str, parents: &[String], message: &str) -> String {
        self.counter += 1;
        let sha = oid(
            "commit",
            &format!("{tree}\n{}\n{message}\n{}", parents.join(" "), self.counter),
        );
        self.commits.insert(
            sha.clone(),
            Commit {
                tree: tree.to_owned(),
                parents: parents.to_vec(),
                message: message.to_owned(),
            },
        );
        sha
    }

    fn descends(&self, sha: &str, ancestor: &str) -> bool {
        let mut pending = vec![sha.to_owned()];
        while let Some(next) = pending.pop() {
            if next == ancestor {
                return true;
            }
            if let Some(commit) = self.commits.get(&next) {
                pending.extend(commit.parents.iter().cloned());
            }
        }
        false
    }

    /// The tree of a branch or of a commit.
    fn tree_at(&self, reference: &str) -> Option<String> {
        let sha = self.refs.get(reference).map_or(reference, String::as_str);
        self.commits.get(sha).map(|commit| commit.tree.clone())
    }

    /// Moves a branch; open pull requests of this repository follow it.
    fn move_ref(&mut self, branch: &str, sha: &str) {
        self.refs.insert(branch.to_owned(), sha.to_owned());
        let name = self.name.clone();
        for pull in self.pulls.values_mut() {
            if pull.open && pull.head_ref == branch && pull.head_repo == name {
                pull.head_sha = sha.to_owned();
            }
        }
    }

    /// One commit on top of `branch` with `changes` (path, mode, content).
    fn push(&mut self, branch: &str, changes: &[(&str, &str, &str)], message: &str) -> String {
        let parent = self.refs.get(branch).cloned();
        let mut files = parent
            .as_deref()
            .and_then(|sha| self.tree_at(sha))
            .map(|tree| self.files(&tree))
            .unwrap_or_default();
        for (path, mode, content) in changes {
            let blob = self.blob(content);
            files.insert((*path).to_owned(), ((*mode).to_owned(), blob));
        }
        let tree = self.write_tree(&files);
        let sha = self.commit(&tree, &parent.into_iter().collect::<Vec<_>>(), message);
        self.move_ref(branch, &sha);
        sha
    }

    fn pull_json(&self, pull: &Pull, computing: bool) -> Value {
        json!({
            "number": pull.number,
            "title": pull.title,
            "body": pull.body,
            "html_url": format!("https://github.com/{}/pull/{}", self.name, pull.number),
            "state": if pull.open { "open" } else { "closed" },
            "merged": pull.merged,
            "draft": pull.draft,
            "mergeable": if computing { Value::Null } else { json!(pull.mergeable) },
            "mergeable_state": if computing { "unknown" } else { pull.mergeable_state.as_str() },
            // Before the merge GitHub keeps the sha of its own test merge here.
            "merge_commit_sha": if pull.merged {
                pull.merge_commit_sha.clone()
            } else {
                oid("test-merge", &pull.head_sha)
            },
            "head": {
                "ref": pull.head_ref,
                "sha": pull.head_sha,
                "repo": if pull.head_repo.is_empty() {
                    Value::Null
                } else {
                    json!({ "full_name": pull.head_repo })
                },
            },
            "base": {
                "ref": pull.base_ref,
                "repo": { "full_name": self.name, "default_branch": DEFAULT_BRANCH },
            },
        })
    }

    /// `GET /compare/{base}...{head}`: how `head` relates to `base` and
    /// the commits only `head` has, oldest first.
    fn compare(&self, range: &str, query: &BTreeMap<String, String>) -> Response {
        let Some((base, head)) = range.split_once("...") else {
            return not_found();
        };
        if !self.commits.contains_key(base) || !self.commits.contains_key(head) {
            return not_found();
        }
        let ahead = self.only_in(head, base);
        let behind = self.only_in(base, head);
        let status = match (ahead.is_empty(), behind.is_empty()) {
            (true, true) => "identical",
            (false, true) => "ahead",
            (true, false) => "behind",
            (false, false) => "diverged",
        };
        let commits: Vec<Value> = ahead
            .iter()
            .map(|sha| {
                let commit = &self.commits[sha];
                json!({
                    "sha": sha,
                    "html_url": format!("https://github.com/{}/commit/{sha}", self.name),
                    "commit": { "message": commit.message, "tree": { "sha": commit.tree } },
                    "parents": commit.parents.iter().map(|p| json!({ "sha": p })).collect::<Vec<_>>(),
                })
            })
            .collect();
        answer(
            StatusCode::OK,
            json!({
                "status": status,
                "ahead_by": ahead.len(),
                "behind_by": behind.len(),
                "total_commits": ahead.len(),
                "commits": page_of(&commits, query),
            }),
        )
    }

    /// The commits reachable from `tip` and not from `other`, parents
    /// before children.
    fn only_in(&self, tip: &str, other: &str) -> Vec<String> {
        fn visit(state: &State, sha: &str, other: &str, seen: &mut Vec<String>) {
            if seen.iter().any(|s| s == sha) || state.descends(other, sha) {
                return;
            }
            if let Some(commit) = state.commits.get(sha) {
                for parent in &commit.parents {
                    visit(state, parent, other, seen);
                }
            }
            seen.push(sha.to_owned());
        }
        let mut seen = Vec::new();
        visit(self, tip, other, &mut seen);
        seen
    }

    fn commit_json(&self, sha: &str) -> Option<Value> {
        let commit = self.commits.get(sha)?;
        Some(json!({
            "sha": sha,
            "html_url": format!("https://github.com/{}/commit/{sha}", self.name),
            "message": commit.message,
            "tree": { "sha": commit.tree },
            "parents": commit.parents.iter().map(|p| json!({ "sha": p })).collect::<Vec<_>>(),
        }))
    }
}

fn answer(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

fn refuse(status: u16, message: &str) -> Response {
    answer(
        StatusCode::from_u16(status).expect("a status code"),
        json!({ "message": message }),
    )
}

fn not_found() -> Response {
    refuse(404, "Not Found")
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_of(uri: &Uri) -> BTreeMap<String, String> {
    uri.query()
        .unwrap_or_default()
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (decode(key), decode(value)))
        .collect()
}

/// One page of `items`, as GitHub cuts it (`per_page` 30 by default, 100
/// at most).
fn page_of<T: Clone>(items: &[T], query: &BTreeMap<String, String>) -> Vec<T> {
    let per_page = query
        .get("per_page")
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(30)
        .clamp(1, 100);
    let page = query
        .get("page")
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    items
        .iter()
        .skip((page - 1) * per_page)
        .take(per_page)
        .cloned()
        .collect()
}

#[derive(Clone)]
pub struct FakeRepo(Arc<Mutex<State>>);

impl FakeRepo {
    /// A repository whose default branch `main` holds `files` in one commit.
    pub fn new(name: &str, files: &[(&str, &str)]) -> Self {
        let mut state = State {
            name: name.to_owned(),
            prefix: String::new(),
            blobs: BTreeMap::new(),
            trees: BTreeMap::new(),
            commits: BTreeMap::new(),
            refs: BTreeMap::new(),
            pulls: BTreeMap::new(),
            check_runs: BTreeMap::new(),
            statuses: BTreeMap::new(),
            alerts: false,
            security_updates: false,
            counter: 0,
            behaviour: Behaviour::default(),
            calls: Vec::new(),
            merges: Vec::new(),
        };
        let changes: Vec<(&str, &str, &str)> = files
            .iter()
            .map(|(path, content)| (*path, FILE, *content))
            .collect();
        state.push(DEFAULT_BRANCH, &changes, "Initial commit");
        Self(Arc::new(Mutex::new(state)))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Serves the repository's API on a random port; returns the API base.
    pub async fn serve(&self) -> String {
        self.serve_at("").await
    }

    /// The API under `prefix` (`/api/v3` on an enterprise host); the
    /// returned base includes it.
    pub async fn serve_at(&self, prefix: &str) -> String {
        self.lock().prefix = prefix.to_owned();
        let app = Router::new().fallback(handle).with_state(self.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}{prefix}")
    }

    // --- what happens to the repository outside SDI -------------------------

    /// A new branch at the head of `from`; returns that commit.
    pub fn branch(&self, name: &str, from: &str) -> String {
        let mut state = self.lock();
        let sha = state.refs[from].clone();
        state.move_ref(name, &sha);
        sha
    }

    /// Somebody pushes one commit changing `files` to `branch`.
    pub fn push(&self, branch: &str, files: &[(&str, &str)], message: &str) -> String {
        let changes: Vec<(&str, &str, &str)> = files
            .iter()
            .map(|(path, content)| (*path, FILE, *content))
            .collect();
        self.lock().push(branch, &changes, message)
    }

    /// Like [`Self::push`] for one file with a git mode of its own.
    pub fn push_with_mode(&self, branch: &str, path: &str, mode: &str, content: &str) -> String {
        self.lock()
            .push(branch, &[(path, mode, content)], &format!("Add {path}"))
    }

    /// Opens a pull request from a branch of this repository into `main`.
    pub fn open_pull(&self, head: &str, title: &str) -> u64 {
        let mut state = self.lock();
        let head_sha = state.refs[head].clone();
        let head_repo = state.name.clone();
        state.add_pull(head, &head_sha, &head_repo, DEFAULT_BRANCH, title, "")
    }

    /// Opens a pull request whose head lives in a fork.
    pub fn open_fork_pull(&self, fork: &str, head: &str, head_sha: &str) -> u64 {
        self.lock()
            .add_pull(head, head_sha, fork, DEFAULT_BRANCH, "From a fork", "")
    }

    pub fn update_pull(&self, number: u64, change: impl FnOnce(&mut Pull)) {
        change(self.lock().pulls.get_mut(&number).expect("a pull request"));
    }

    /// Reports a check run on a commit; a later run of the same name
    /// replaces the earlier one, as GitHub lists the latest.
    pub fn check_run(&self, sha: &str, name: &str, status: &str, conclusion: Option<&str>) {
        let mut state = self.lock();
        state.counter += 1;
        let id = state.counter;
        let runs = state.check_runs.entry(sha.to_owned()).or_default();
        runs.retain(|run| run["name"] != name);
        runs.push(json!({ "id": id, "name": name, "status": status, "conclusion": conclusion }));
    }

    /// Reports a commit status; the latest per context counts.
    pub fn status(&self, sha: &str, context: &str, state_name: &str) {
        let mut state = self.lock();
        let statuses = state.statuses.entry(sha.to_owned()).or_default();
        statuses.retain(|(existing, _)| existing != context);
        statuses.push((context.to_owned(), state_name.to_owned()));
    }

    pub fn behave(&self, change: impl FnOnce(&mut Behaviour)) {
        change(&mut self.lock().behaviour);
    }

    // --- what a test looks at -----------------------------------------------

    pub fn head(&self, branch: &str) -> Option<String> {
        self.lock().refs.get(branch).cloned()
    }

    pub fn file(&self, branch: &str, path: &str) -> Option<String> {
        let state = self.lock();
        let tree = state.tree_at(branch)?;
        let (_, blob) = state.files(&tree).get(path)?.clone();
        state.blobs.get(&blob).cloned()
    }

    pub fn mode(&self, branch: &str, path: &str) -> Option<String> {
        let state = self.lock();
        let tree = state.tree_at(branch)?;
        state.files(&tree).get(path).map(|(mode, _)| mode.clone())
    }

    /// Parents and message of a commit.
    pub fn commit(&self, sha: &str) -> (Vec<String>, String) {
        let state = self.lock();
        let commit = &state.commits[sha];
        (commit.parents.clone(), commit.message.clone())
    }

    /// How many commits `branch` is ahead of `ancestor` on its first-parent line.
    pub fn commits_since(&self, branch: &str, ancestor: &str) -> usize {
        let state = self.lock();
        let mut at = state.refs.get(branch).cloned();
        let mut count = 0;
        while let Some(sha) = at {
            if sha == ancestor {
                return count;
            }
            count += 1;
            at = state
                .commits
                .get(&sha)
                .and_then(|c| c.parents.first().cloned());
        }
        count
    }

    pub fn pull(&self, number: u64) -> Pull {
        self.lock().pulls[&number].clone()
    }

    pub fn pulls(&self) -> Vec<Pull> {
        self.lock().pulls.values().cloned().collect()
    }

    /// Every request so far, as `METHOD /path?query`.
    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    /// The requests that were not reads.
    pub fn writes(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|call| !call.starts_with("GET "))
            .collect()
    }

    pub fn forget_calls(&self) {
        self.lock().calls.clear();
    }

    /// The bodies `PUT /pulls/{n}/merge` received.
    pub fn merges(&self) -> Vec<Value> {
        self.lock().merges.clone()
    }

    pub fn alerts(&self) -> bool {
        self.lock().alerts
    }

    pub fn security_updates(&self) -> bool {
        self.lock().security_updates
    }
}

impl State {
    fn add_pull(
        &mut self,
        head: &str,
        head_sha: &str,
        head_repo: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> u64 {
        let number = self.pulls.keys().next_back().map_or(1, |last| last + 1);
        self.pulls.insert(
            number,
            Pull {
                number,
                title: title.to_owned(),
                body: body.to_owned(),
                head_ref: head.to_owned(),
                head_sha: head_sha.to_owned(),
                head_repo: head_repo.to_owned(),
                base_ref: base.to_owned(),
                open: true,
                merged: false,
                draft: false,
                mergeable: Some(true),
                mergeable_state: "clean".to_owned(),
                merge_commit_sha: String::new(),
                unknown_reads: 0,
            },
        );
        number
    }

    fn repository(&self) -> Response {
        answer(
            StatusCode::OK,
            json!({
                "full_name": self.name,
                "default_branch": DEFAULT_BRANCH,
                "html_url": format!("https://github.com/{}", self.name),
                "permissions": { "admin": self.behaviour.says_admin, "push": true, "pull": true },
            }),
        )
    }

    fn tarball(&self, reference: &str) -> Response {
        let Some(tree) = self.tree_at(reference) else {
            return not_found();
        };
        let top = format!("{}-{}", self.name.replace('/', "-"), &tree[..7]);
        let mut builder = tar::Builder::new(Vec::new());
        for (path, (mode, blob)) in self.files(&tree) {
            let content = self.blobs[&blob].as_bytes();
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(u32::from_str_radix(&mode[3..], 8).unwrap_or(0o644));
            header.set_cksum();
            builder
                .append_data(&mut header, format!("{top}/{path}"), content)
                .unwrap();
        }
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        (
            [(header::CONTENT_TYPE, "application/gzip")],
            gz.finish().unwrap(),
        )
            .into_response()
    }

    fn contents(&self, path: &str, query: &BTreeMap<String, String>) -> Response {
        let reference = query.get("ref").map_or(DEFAULT_BRANCH, String::as_str);
        let Some(tree) = self.tree_at(reference) else {
            return not_found();
        };
        let files = self.files(&tree);
        if let Some((_, blob)) = files.get(path) {
            return answer(
                StatusCode::OK,
                json!({ "type": "file", "path": path, "sha": blob }),
            );
        }
        let inside: Vec<Value> = files
            .keys()
            .filter(|file| file.starts_with(&format!("{path}/")))
            .map(|file| json!({ "type": "file", "path": file }))
            .collect();
        if inside.is_empty() {
            not_found()
        } else {
            answer(StatusCode::OK, json!(inside))
        }
    }

    fn get_ref(&self, branch: &str) -> Response {
        match self.refs.get(branch) {
            Some(sha) => answer(
                StatusCode::OK,
                json!({
                    "ref": format!("refs/heads/{branch}"),
                    "object": { "type": "commit", "sha": sha },
                }),
            ),
            None => not_found(),
        }
    }

    fn create_ref(&mut self, body: &Value) -> Response {
        let name = body["ref"].as_str().unwrap_or_default();
        let Some(branch) = name.strip_prefix("refs/heads/") else {
            return refuse(422, "Reference name must start with refs/heads/");
        };
        let sha = body["sha"].as_str().unwrap_or_default();
        if self.refs.contains_key(branch) {
            return refuse(422, "Reference already exists");
        }
        if !self.commits.contains_key(sha) {
            return refuse(422, "Object does not exist");
        }
        self.move_ref(branch, sha);
        answer(
            StatusCode::CREATED,
            json!({ "ref": name, "object": { "type": "commit", "sha": sha } }),
        )
    }

    fn update_ref(&mut self, branch: &str, body: &Value) -> Response {
        if let Some((pushed, path, content)) = self.behaviour.push_before_ref_update.take()
            && pushed == branch
        {
            self.push(branch, &[(&path, FILE, &content)], "Somebody else's commit");
        }
        if let Some(message) = self.behaviour.ref_update_refusal.take() {
            return refuse(422, &message);
        }
        let Some(current) = self.refs.get(branch).cloned() else {
            return refuse(422, "Reference does not exist");
        };
        let sha = body["sha"].as_str().unwrap_or_default();
        if !self.commits.contains_key(sha) {
            return refuse(422, "Object does not exist");
        }
        let force = body["force"].as_bool().unwrap_or(false);
        if !force && !self.descends(sha, &current) {
            return refuse(422, "Update is not a fast forward");
        }
        self.move_ref(branch, sha);
        answer(
            StatusCode::OK,
            json!({
                "ref": format!("refs/heads/{branch}"),
                "object": { "type": "commit", "sha": sha },
            }),
        )
    }

    fn delete_ref(&mut self, branch: &str) -> Response {
        if !self.refs.contains_key(branch) {
            return refuse(422, "Reference does not exist");
        }
        if self.behaviour.protected.iter().any(|p| p == branch) {
            return refuse(422, "Cannot delete this protected branch");
        }
        self.refs.remove(branch);
        StatusCode::NO_CONTENT.into_response()
    }

    fn get_tree(&self, sha: &str, query: &BTreeMap<String, String>) -> Response {
        let Some(entries) = self.trees.get(sha) else {
            return not_found();
        };
        let tree: Vec<Value> = if query.contains_key("recursive") {
            self.files(sha)
                .into_iter()
                .map(|(path, (mode, blob))| {
                    json!({ "path": path, "mode": mode, "type": "blob", "sha": blob })
                })
                .collect()
        } else {
            entries
                .iter()
                .map(|(name, entry)| {
                    json!({
                        "path": name,
                        "mode": entry.mode,
                        "type": entry.kind,
                        "sha": entry.sha,
                    })
                })
                .collect()
        };
        answer(
            StatusCode::OK,
            json!({ "sha": sha, "truncated": false, "tree": tree }),
        )
    }

    fn create_tree(&mut self, body: &Value) -> Response {
        let mut files = match body["base_tree"].as_str() {
            Some(base) if self.trees.contains_key(base) => self.files(base),
            Some(_) => return refuse(422, "base_tree is not a valid tree oid"),
            None => Files::new(),
        };
        for entry in body["tree"].as_array().into_iter().flatten() {
            let path = entry["path"].as_str().unwrap_or_default();
            let mode = entry["mode"].as_str().unwrap_or_default();
            let sha = entry["sha"].as_str().unwrap_or_default();
            if path.is_empty() || entry["type"] != "blob" || !self.blobs.contains_key(sha) {
                return refuse(422, "tree.sha is not a valid blob");
            }
            if !matches!(mode, FILE | EXECUTABLE | SYMLINK) {
                return refuse(422, "tree.mode is not a valid file mode");
            }
            files.insert(path.to_owned(), (mode.to_owned(), sha.to_owned()));
        }
        let sha = self.write_tree(&files);
        answer(StatusCode::CREATED, json!({ "sha": sha }))
    }

    fn create_commit(&mut self, body: &Value) -> Response {
        let tree = body["tree"].as_str().unwrap_or_default();
        let parents: Vec<String> = body["parents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str().map(str::to_owned))
            .collect();
        if !self.trees.contains_key(tree) {
            return refuse(422, "Tree SHA does not exist");
        }
        if parents.iter().any(|p| !self.commits.contains_key(p)) {
            return refuse(422, "Parent SHA does not exist or is not a commit object");
        }
        let message = body["message"].as_str().unwrap_or_default();
        let sha = self.commit(tree, &parents, message);
        answer(
            StatusCode::CREATED,
            self.commit_json(&sha).expect("just created"),
        )
    }

    fn list_pulls(&self, query: &BTreeMap<String, String>) -> Response {
        let wanted = query.get("state").map_or("open", String::as_str);
        let head = query
            .get("head")
            .and_then(|head| head.split_once(':'))
            .map(|(_, branch)| branch.to_owned());
        let pulls: Vec<Value> = self
            .pulls
            .values()
            .filter(|pull| match wanted {
                "open" => pull.open,
                "closed" => !pull.open,
                _ => true,
            })
            .filter(|pull| head.as_ref().is_none_or(|head| pull.head_ref == *head))
            .filter(|pull| query.get("base").is_none_or(|base| pull.base_ref == *base))
            .map(|pull| self.pull_json(pull, false))
            .collect();
        answer(StatusCode::OK, json!(page_of(&pulls, query)))
    }

    fn open_pull(&mut self, body: &Value) -> Response {
        let head = body["head"].as_str().unwrap_or_default();
        let base = body["base"].as_str().unwrap_or_default();
        let (Some(head_sha), Some(base_sha)) =
            (self.refs.get(head).cloned(), self.refs.get(base).cloned())
        else {
            return refuse(422, "Validation Failed: head or base does not exist");
        };
        if head_sha == base_sha {
            return refuse(422, &format!("No commits between {base} and {head}"));
        }
        if self
            .pulls
            .values()
            .any(|pull| pull.open && pull.head_ref == head && pull.base_ref == base)
        {
            return refuse(422, "A pull request already exists for this branch");
        }
        let name = self.name.clone();
        let number = self.add_pull(
            head,
            &head_sha,
            &name,
            base,
            body["title"].as_str().unwrap_or_default(),
            body["body"].as_str().unwrap_or_default(),
        );
        answer(
            StatusCode::CREATED,
            self.pull_json(&self.pulls[&number], false),
        )
    }

    fn get_pull(&mut self, number: u64) -> Response {
        let Some(pull) = self.pulls.get_mut(&number) else {
            return not_found();
        };
        let computing = pull.unknown_reads > 0;
        pull.unknown_reads = pull.unknown_reads.saturating_sub(1);
        let pull = pull.clone();
        answer(StatusCode::OK, self.pull_json(&pull, computing))
    }

    fn merge(&mut self, number: u64, body: &Value) -> Response {
        self.merges.push(body.clone());
        let Some(pull) = self.pulls.get(&number).cloned() else {
            return not_found();
        };
        if let Some((status, message)) = self.behaviour.merge_refusal.clone() {
            return refuse(status, &message);
        }
        if !pull.open {
            return refuse(405, "Pull Request is not mergeable");
        }
        if body["sha"].as_str().is_some_and(|sha| sha != pull.head_sha) {
            return refuse(
                409,
                "Head branch was modified. Review and try the merge again.",
            );
        }
        if pull.draft {
            return refuse(405, "Pull Request is still a draft");
        }
        if pull.mergeable == Some(false) {
            return refuse(405, "Pull Request is not mergeable");
        }
        if pull.mergeable_state == "blocked" {
            return refuse(
                405,
                "At least 1 approving review is required by reviewers with write access.",
            );
        }
        let base_sha = self.refs[&pull.base_ref].clone();
        let mut files = self
            .tree_at(&base_sha)
            .map(|tree| self.files(&tree))
            .unwrap_or_default();
        if let Some(tree) = self.tree_at(&pull.head_sha) {
            files.extend(self.files(&tree));
        }
        let tree = self.write_tree(&files);
        let method = body["merge_method"].as_str().unwrap_or("merge");
        let parents = if method == "merge" {
            vec![base_sha, pull.head_sha.clone()]
        } else {
            vec![base_sha]
        };
        let title = body["commit_title"]
            .as_str()
            .map_or_else(|| format!("Merge pull request #{number}"), str::to_owned);
        let sha = self.commit(&tree, &parents, &title);
        self.refs.insert(pull.base_ref.clone(), sha.clone());
        let merged = self.pulls.get_mut(&number).expect("read above");
        merged.open = false;
        merged.merged = true;
        merged.merge_commit_sha = sha.clone();
        answer(
            StatusCode::OK,
            json!({ "sha": sha, "merged": true, "message": "Pull Request successfully merged" }),
        )
    }

    fn list_check_runs(&self, sha: &str, query: &BTreeMap<String, String>) -> Response {
        let runs = self.check_runs.get(sha).cloned().unwrap_or_default();
        answer(
            StatusCode::OK,
            json!({ "total_count": runs.len(), "check_runs": page_of(&runs, query) }),
        )
    }

    fn combined_status(&self, sha: &str, query: &BTreeMap<String, String>) -> Response {
        let statuses: Vec<Value> = self
            .statuses
            .get(sha)
            .into_iter()
            .flatten()
            .map(|(context, state)| json!({ "context": context, "state": state }))
            .collect();
        let overall = if statuses
            .iter()
            .any(|s| s["state"] == "failure" || s["state"] == "error")
        {
            "failure"
        } else if statuses.iter().any(|s| s["state"] == "pending") || statuses.is_empty() {
            "pending"
        } else {
            "success"
        };
        answer(
            StatusCode::OK,
            json!({
                "state": overall,
                "total_count": statuses.len(),
                "statuses": page_of(&statuses, query),
            }),
        )
    }

    /// The two Dependabot switches. A `PUT` carries no body, and GitHub
    /// insists on being told so (`Content-Length: 0`).
    fn setting(&mut self, which: &str, method: &Method, headers: &HeaderMap) -> Response {
        if self.behaviour.not_admin {
            return not_found();
        }
        let alerts = which == "vulnerability-alerts";
        if *method == Method::GET {
            return match (alerts, self.alerts, self.security_updates) {
                (true, true, _) => StatusCode::NO_CONTENT.into_response(),
                (false, _, true) => {
                    answer(StatusCode::OK, json!({ "enabled": true, "paused": false }))
                }
                _ => not_found(),
            };
        }
        if !headers.contains_key(header::CONTENT_LENGTH) {
            return refuse(411, "Length Required");
        }
        if alerts {
            self.alerts = true;
        } else if self.behaviour.updates_forbidden {
            return refuse(403, "Resource not accessible by integration");
        } else if !self.alerts {
            return refuse(
                self.behaviour.updates_refusal.unwrap_or(422),
                "Vulnerability alerts must be enabled to configure automated security fixes",
            );
        } else {
            self.security_updates = true;
        }
        StatusCode::NO_CONTENT.into_response()
    }
}

async fn handle(
    Shared(repo): Shared<FakeRepo>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut state = repo.lock();
    let Some(path) = uri.path().strip_prefix(state.prefix.as_str()) else {
        return not_found();
    };
    let path = path.to_owned();
    state.calls.push(match uri.query() {
        Some(query) => format!("{method} {path}?{query}"),
        None => format!("{method} {path}"),
    });
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == format!("Bearer {TOKEN}"));
    if !authorized {
        return refuse(401, "Bad credentials");
    }
    let decoded: Vec<String> = path.trim_matches('/').split('/').map(decode).collect();
    let segments: Vec<&str> = decoded.iter().map(String::as_str).collect();
    let ["repos", owner, name, rest @ ..] = segments.as_slice() else {
        return not_found();
    };
    if !format!("{owner}/{name}").eq_ignore_ascii_case(&state.name) {
        return not_found();
    }
    if state.behaviour.read_only && method != Method::GET {
        return refuse(403, "Resource not accessible by integration");
    }
    let query = query_of(&uri);
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let number = |text: &str| text.parse::<u64>().unwrap_or(0);

    match (method.as_str(), rest) {
        ("GET", []) => state.repository(),
        ("GET", ["tarball"]) => state.tarball(DEFAULT_BRANCH),
        ("GET", ["tarball", reference @ ..]) => state.tarball(&reference.join("/")),
        ("GET", ["contents", file @ ..]) => state.contents(&file.join("/"), &query),
        ("GET", ["git", "ref", "heads", branch @ ..]) => state.get_ref(&branch.join("/")),
        ("POST", ["git", "refs"]) => state.create_ref(&body),
        ("PATCH", ["git", "refs", "heads", branch @ ..]) => {
            state.update_ref(&branch.join("/"), &body)
        }
        ("DELETE", ["git", "refs", "heads", branch @ ..]) => state.delete_ref(&branch.join("/")),
        ("GET", ["git", "commits", sha]) => match state.commit_json(sha) {
            Some(commit) => answer(StatusCode::OK, commit),
            None => not_found(),
        },
        ("POST", ["git", "commits"]) => state.create_commit(&body),
        ("GET", ["compare", range]) => state.compare(range, &query),
        ("GET", ["git", "trees", sha]) => state.get_tree(sha, &query),
        ("POST", ["git", "trees"]) => state.create_tree(&body),
        ("POST", ["git", "blobs"]) => {
            let sha = state.blob(body["content"].as_str().unwrap_or_default());
            answer(StatusCode::CREATED, json!({ "sha": sha }))
        }
        ("GET", ["pulls"]) => state.list_pulls(&query),
        ("POST", ["pulls"]) => state.open_pull(&body),
        ("GET", ["pulls", pull]) => state.get_pull(number(pull)),
        ("PUT", ["pulls", pull, "merge"]) => state.merge(number(pull), &body),
        ("GET", ["commits", sha, "check-runs"]) => state.list_check_runs(sha, &query),
        ("GET", ["commits", sha, "status"]) => state.combined_status(sha, &query),
        ("GET" | "PUT", [which @ ("vulnerability-alerts" | "automated-security-fixes")]) => {
            state.setting(which, &method, &headers)
        }
        _ => not_found(),
    }
}
