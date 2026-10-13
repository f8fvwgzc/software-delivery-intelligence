//! Local stand-ins for the systems deployment runs talk to: GitHub (the
//! Actions, deployments, contents, refs, pulls, commits and compare APIs),
//! an image registry and a webhook receiver. Plain axum servers on random
//! loopback ports that keep what they were sent, so a test can assert on
//! the exact requests. Nothing here is compiled outside tests.

/// A repository with real git state, for the pull request autopilot.
pub mod repo;

pub mod github {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
    };

    use axum::{
        Json, Router,
        extract::{Path, Query, Request, State},
        http::StatusCode,
        middleware::{self, Next},
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use chrono::{SecondsFormat, Utc};
    use serde_json::{Value, json};

    /// The only token the fake accepts: proves the workspace's access (not
    /// some process-wide default) reached the request.
    pub const TOKEN: &str = "ghs_fake_workspace_token_5e8f1a2b3c4d";

    #[derive(Debug, Clone)]
    pub struct Commit {
        pub branch: String,
        pub message: String,
    }

    #[derive(Debug, Clone)]
    pub struct Pull {
        pub number: u64,
        pub repo: String,
        pub head: String,
        pub base: String,
        pub title: String,
        pub body: String,
        pub html_url: String,
        pub open: bool,
        pub merged: bool,
    }

    #[derive(Default)]
    pub struct FakeGithub {
        files: Mutex<BTreeMap<(String, String, String), String>>,
        commits: Mutex<Vec<Commit>>,
        pulls: Mutex<Vec<Pull>>,
        pub pull_polls: AtomicUsize,
        merge_after: Option<usize>,
        close_after: Option<usize>,
        conflict_on_write: bool,
        forbid_writes: bool,
        /// Workflow files that exist, as `repo/file`.
        workflows: Mutex<Vec<String>>,
        /// What the next dispatched runs conclude as.
        conclusion: Mutex<String>,
        /// Polls of the run list before a dispatched run shows up.
        run_appears_after: usize,
        run_list_polls: AtomicUsize,
        runs: Mutex<Vec<Value>>,
        run_polls: Mutex<BTreeMap<u64, usize>>,
        next_id: AtomicU64,
        pub dispatches: Mutex<Vec<Value>>,
        pub deployments: Mutex<Vec<Value>>,
        pub deployment_statuses: Mutex<Vec<Value>>,
        /// `repo` → (head sha of the default branch, its message).
        heads: Mutex<BTreeMap<String, (String, String)>>,
        /// `repo` → commit messages `compare` reports, oldest first.
        compares: Mutex<BTreeMap<String, Vec<String>>>,
        pub compare_calls: Mutex<Vec<String>>,
    }

    type Shared = Arc<FakeGithub>;

    fn blob_sha(content: &str) -> String {
        let sum: u64 = content.bytes().map(u64::from).sum();
        format!("blob-{}-{sum}", content.len())
    }

    fn branch_sha(branch: &str) -> String {
        format!("sha-of-{branch}")
    }

    fn not_found() -> Response {
        (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"}))).into_response()
    }

    impl FakeGithub {
        pub fn with_file(self, repo: &str, branch: &str, path: &str, content: &str) -> Self {
            self.files.lock().unwrap().insert(
                (repo.to_owned(), branch.to_owned(), path.to_owned()),
                content.to_owned(),
            );
            self
        }

        /// A workflow file dispatches can target.
        pub fn with_workflow(self, repo: &str, file: &str) -> Self {
            self.workflows
                .lock()
                .unwrap()
                .push(format!("{repo}/{file}"));
            self
        }

        /// The head commit of the repository's default branch.
        pub fn with_head(self, repo: &str, sha: &str, message: &str) -> Self {
            self.heads
                .lock()
                .unwrap()
                .insert(repo.to_owned(), (sha.to_owned(), message.to_owned()));
            self
        }

        /// Pull requests merge themselves on the n-th poll.
        pub fn merge_pulls_after(mut self, polls: usize) -> Self {
            self.merge_after = Some(polls);
            self
        }

        /// Pull requests are closed unmerged on the n-th poll.
        pub fn close_pulls_after(mut self, polls: usize) -> Self {
            self.close_after = Some(polls);
            self
        }

        pub fn conflict_on_write(mut self) -> Self {
            self.conflict_on_write = true;
            self
        }

        pub fn forbid_writes(mut self) -> Self {
            self.forbid_writes = true;
            self
        }

        /// The run a dispatch creates only shows up after this many listings.
        pub fn run_appears_after(mut self, polls: usize) -> Self {
            self.run_appears_after = polls;
            self
        }

        /// What dispatched workflow runs conclude as from now on.
        pub fn conclude(&self, conclusion: &str) {
            *self.conclusion.lock().unwrap() = conclusion.to_owned();
        }

        /// Moves the default branch head and says what `compare` lists.
        pub fn push(&self, repo: &str, sha: &str, messages: &[&str]) {
            self.heads.lock().unwrap().insert(
                repo.to_owned(),
                (
                    sha.to_owned(),
                    messages.last().copied().unwrap_or_default().to_owned(),
                ),
            );
            self.compares.lock().unwrap().insert(
                repo.to_owned(),
                messages.iter().map(|m| (*m).to_owned()).collect(),
            );
        }

        pub fn file(&self, repo: &str, branch: &str, path: &str) -> Option<String> {
            self.files
                .lock()
                .unwrap()
                .get(&(repo.to_owned(), branch.to_owned(), path.to_owned()))
                .cloned()
        }

        pub fn commits(&self) -> Vec<Commit> {
            self.commits.lock().unwrap().clone()
        }

        pub fn pulls(&self) -> Vec<Pull> {
            self.pulls.lock().unwrap().clone()
        }

        fn branch_exists(&self, repo: &str, branch: &str) -> bool {
            self.files
                .lock()
                .unwrap()
                .keys()
                .any(|(r, b, _)| r == repo && b == branch)
        }

        pub async fn serve(self) -> (Shared, String) {
            if self.conclusion.lock().unwrap().is_empty() {
                *self.conclusion.lock().unwrap() = "success".to_owned();
            }
            self.next_id.store(100, Ordering::SeqCst);
            let fake = Arc::new(self);
            let app = Router::new()
                .route("/repos/{o}/{r}", get(repository))
                .route("/repos/{o}/{r}/commits/{reference}", get(commit))
                .route("/repos/{o}/{r}/compare/{basehead}", get(compare))
                .route(
                    "/repos/{o}/{r}/contents/{*path}",
                    get(read_file).put(write_file),
                )
                .route("/repos/{o}/{r}/git/ref/{*reference}", get(get_ref))
                .route("/repos/{o}/{r}/git/refs", post(create_ref))
                .route("/repos/{o}/{r}/pulls", get(list_pulls).post(open_pull))
                .route("/repos/{o}/{r}/pulls/{number}", get(get_pull))
                .route(
                    "/repos/{o}/{r}/actions/workflows/{file}/dispatches",
                    post(dispatch),
                )
                .route(
                    "/repos/{o}/{r}/actions/workflows/{file}/runs",
                    get(list_runs),
                )
                .route("/repos/{o}/{r}/actions/runs/{id}", get(get_run))
                .route("/repos/{o}/{r}/actions/runs/{id}/jobs", get(run_jobs))
                .route("/repos/{o}/{r}/deployments", post(create_deployment))
                .route(
                    "/repos/{o}/{r}/deployments/{id}/statuses",
                    post(create_deployment_status),
                )
                .layer(middleware::from_fn(authenticated))
                .with_state(Arc::clone(&fake));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            (fake, format!("http://{addr}"))
        }
    }

    async fn authenticated(request: Request, next: Next) -> Response {
        let ok = request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == format!("Bearer {TOKEN}"));
        if !ok {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"message": "Bad credentials"})),
            )
                .into_response();
        }
        next.run(request).await
    }

    async fn repository(Path((o, r)): Path<(String, String)>) -> Json<Value> {
        Json(json!({"full_name": format!("{o}/{r}"), "default_branch": "main"}))
    }

    async fn commit(
        State(fake): State<Shared>,
        Path((o, r, reference)): Path<(String, String, String)>,
    ) -> Response {
        let repo = format!("{o}/{r}");
        let Some((sha, message)) = fake.heads.lock().unwrap().get(&repo).cloned() else {
            return not_found();
        };
        if reference == "main" || sha.starts_with(&reference) {
            return Json(json!({"sha": sha, "commit": {"message": message}})).into_response();
        }
        // Any other full sha is a commit the repository knows.
        if reference.len() == 40 && reference.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Json(json!({"sha": reference, "commit": {"message": "An earlier commit"}}))
                .into_response();
        }
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"message": format!("No commit found for SHA: {reference}")})),
        )
            .into_response()
    }

    async fn compare(
        State(fake): State<Shared>,
        Path((o, r, basehead)): Path<(String, String, String)>,
    ) -> Json<Value> {
        let repo = format!("{o}/{r}");
        fake.compare_calls
            .lock()
            .unwrap()
            .push(format!("{repo} {basehead}"));
        let messages = fake
            .compares
            .lock()
            .unwrap()
            .get(&repo)
            .cloned()
            .unwrap_or_default();
        let commits: Vec<Value> = messages
            .iter()
            .enumerate()
            .map(|(i, message)| {
                json!({"sha": format!("{:040x}", 0xc0ffee00u64 + i as u64), "commit": {"message": message}})
            })
            .collect();
        Json(json!({"total_commits": commits.len(), "commits": commits}))
    }

    async fn read_file(
        State(fake): State<Shared>,
        Path((o, r, path)): Path<(String, String, String)>,
        Query(query): Query<BTreeMap<String, String>>,
    ) -> Response {
        let branch = query.get("ref").cloned().unwrap_or_else(|| "main".into());
        let Some(content) = fake.file(&format!("{o}/{r}"), &branch, &path) else {
            return not_found();
        };
        // GitHub wraps the base64 at 60 characters.
        let encoded = STANDARD.encode(&content);
        let wrapped: Vec<&str> = encoded
            .as_bytes()
            .chunks(60)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect();
        Json(json!({
            "type": "file",
            "encoding": "base64",
            "path": path,
            "sha": blob_sha(&content),
            "content": format!("{}\n", wrapped.join("\n")),
        }))
        .into_response()
    }

    async fn write_file(
        State(fake): State<Shared>,
        Path((o, r, path)): Path<(String, String, String)>,
        Json(body): Json<Value>,
    ) -> Response {
        let repo = format!("{o}/{r}");
        if fake.forbid_writes {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"message": "Resource not accessible by integration"})),
            )
                .into_response();
        }
        let branch = body["branch"].as_str().unwrap_or("main").to_owned();
        let key = (repo.clone(), branch.clone(), path.clone());
        let current = fake.files.lock().unwrap().get(&key).cloned();
        let stale = current
            .as_deref()
            .is_some_and(|content| blob_sha(content) != body["sha"].as_str().unwrap_or_default());
        if fake.conflict_on_write || stale {
            return (
                StatusCode::CONFLICT,
                Json(json!({"message": format!("{path} does not match {}", body["sha"].as_str().unwrap_or_default())})),
            )
                .into_response();
        }
        let content = String::from_utf8(
            STANDARD
                .decode(body["content"].as_str().unwrap_or_default())
                .unwrap(),
        )
        .unwrap();
        let sha = format!("commit-{}", fake.next_id.fetch_add(1, Ordering::SeqCst));
        fake.commits.lock().unwrap().push(Commit {
            branch,
            message: body["message"].as_str().unwrap_or_default().to_owned(),
        });
        let blob = blob_sha(&content);
        fake.files.lock().unwrap().insert(key, content);
        Json(json!({
            "content": {"path": path, "sha": blob},
            "commit": {"sha": sha, "html_url": format!("https://github.com/{repo}/commit/{sha}")},
        }))
        .into_response()
    }

    async fn get_ref(
        State(fake): State<Shared>,
        Path((o, r, reference)): Path<(String, String, String)>,
    ) -> Response {
        let branch = reference.trim_start_matches("heads/");
        if fake.branch_exists(&format!("{o}/{r}"), branch) {
            Json(json!({"ref": format!("refs/heads/{branch}"), "object": {"sha": branch_sha(branch)}}))
                .into_response()
        } else {
            not_found()
        }
    }

    async fn create_ref(
        State(fake): State<Shared>,
        Path((o, r)): Path<(String, String)>,
        Json(body): Json<Value>,
    ) -> Response {
        let repo = format!("{o}/{r}");
        if fake.forbid_writes {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"message": "Resource not accessible by integration"})),
            )
                .into_response();
        }
        let new = body["ref"]
            .as_str()
            .unwrap_or_default()
            .trim_start_matches("refs/heads/")
            .to_owned();
        let base = body["sha"]
            .as_str()
            .unwrap_or_default()
            .trim_start_matches("sha-of-")
            .to_owned();
        let mut files = fake.files.lock().unwrap();
        let copied: Vec<_> = files
            .iter()
            .filter(|((file_repo, branch, _), _)| *file_repo == repo && *branch == base)
            .map(|((_, _, path), content)| (path.clone(), content.clone()))
            .collect();
        for (path, content) in copied {
            files.insert((repo.clone(), new.clone(), path), content);
        }
        (
            StatusCode::CREATED,
            Json(json!({"ref": format!("refs/heads/{new}"), "object": {"sha": branch_sha(&new)}})),
        )
            .into_response()
    }

    fn pull_json(pull: &Pull) -> Value {
        json!({
            "number": pull.number,
            "html_url": pull.html_url,
            "state": if pull.open { "open" } else { "closed" },
            "merged": pull.merged,
            "merge_commit_sha": if pull.merged { format!("merge-{}", pull.number) } else { String::new() },
            "head": {"ref": pull.head},
            "base": {"ref": pull.base},
        })
    }

    async fn list_pulls(
        State(fake): State<Shared>,
        Path((o, r)): Path<(String, String)>,
        Query(query): Query<BTreeMap<String, String>>,
    ) -> Json<Value> {
        let repo = format!("{o}/{r}");
        let head = query
            .get("head")
            .and_then(|head| head.split_once(':'))
            .map(|(_, branch)| branch.to_owned())
            .unwrap_or_default();
        let pulls: Vec<Value> = fake
            .pulls
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.repo == repo && p.open && (head.is_empty() || p.head == head))
            .map(pull_json)
            .collect();
        Json(json!(pulls))
    }

    async fn open_pull(
        State(fake): State<Shared>,
        Path((o, r)): Path<(String, String)>,
        Json(body): Json<Value>,
    ) -> Response {
        let repo = format!("{o}/{r}");
        let number = fake.next_id.fetch_add(1, Ordering::SeqCst);
        let pull = Pull {
            number,
            html_url: format!("https://github.com/{repo}/pull/{number}"),
            repo,
            head: body["head"].as_str().unwrap_or_default().to_owned(),
            base: body["base"].as_str().unwrap_or_default().to_owned(),
            title: body["title"].as_str().unwrap_or_default().to_owned(),
            body: body["body"].as_str().unwrap_or_default().to_owned(),
            open: true,
            merged: false,
        };
        let json = pull_json(&pull);
        fake.pulls.lock().unwrap().push(pull);
        (StatusCode::CREATED, Json(json)).into_response()
    }

    async fn get_pull(
        State(fake): State<Shared>,
        Path((o, r, number)): Path<(String, String, u64)>,
    ) -> Response {
        let repo = format!("{o}/{r}");
        let polls = fake.pull_polls.fetch_add(1, Ordering::SeqCst) + 1;
        let mut pulls = fake.pulls.lock().unwrap();
        let Some(pull) = pulls
            .iter_mut()
            .find(|p| p.repo == repo && p.number == number)
        else {
            return not_found();
        };
        if pull.open && fake.merge_after.is_some_and(|after| polls >= after) {
            pull.open = false;
            pull.merged = true;
        } else if pull.open && fake.close_after.is_some_and(|after| polls >= after) {
            pull.open = false;
        }
        Json(pull_json(pull)).into_response()
    }

    async fn dispatch(
        State(fake): State<Shared>,
        Path((o, r, file)): Path<(String, String, String)>,
        Json(body): Json<Value>,
    ) -> Response {
        let repo = format!("{o}/{r}");
        if fake.forbid_writes {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"message": "Resource not accessible by integration"})),
            )
                .into_response();
        }
        if !fake
            .workflows
            .lock()
            .unwrap()
            .contains(&format!("{repo}/{file}"))
        {
            return not_found();
        }
        let expected = ["digest", "environment", "image", "version"];
        let inputs = body["inputs"].as_object().cloned().unwrap_or_default();
        if let Some(unexpected) = inputs.keys().find(|k| !expected.contains(&k.as_str())) {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"message": format!("Unexpected inputs provided: [\"{unexpected}\"]")})),
            )
                .into_response();
        }
        let id = fake.next_id.fetch_add(1, Ordering::SeqCst);
        fake.dispatches.lock().unwrap().push(json!({
            "repo": repo,
            "workflow": file,
            "ref": body["ref"],
            "inputs": body["inputs"],
            "run_id": id,
        }));
        fake.run_list_polls.store(0, Ordering::SeqCst);
        fake.runs.lock().unwrap().push(json!({
            "id": id,
            "repo": repo,
            "workflow": file,
            "event": "workflow_dispatch",
            "status": "queued",
            "conclusion": null,
            "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            "html_url": format!("https://github.com/{repo}/actions/runs/{id}"),
            "final": fake.conclusion.lock().unwrap().clone(),
        }));
        StatusCode::NO_CONTENT.into_response()
    }

    async fn list_runs(
        State(fake): State<Shared>,
        Path((o, r, file)): Path<(String, String, String)>,
        Query(query): Query<BTreeMap<String, String>>,
    ) -> Json<Value> {
        let repo = format!("{o}/{r}");
        assert_eq!(
            query.get("event").map(String::as_str),
            Some("workflow_dispatch")
        );
        assert!(
            query.get("created").is_some_and(|c| c.starts_with(">=")),
            "runs are filtered by creation time: {query:?}"
        );
        let polls = fake.run_list_polls.fetch_add(1, Ordering::SeqCst);
        // A run of the same workflow from long ago must never be picked.
        let mut runs = vec![json!({
            "id": 1,
            "event": "workflow_dispatch",
            "status": "completed",
            "conclusion": "success",
            "created_at": "2020-01-01T00:00:00Z",
            "html_url": format!("https://github.com/{repo}/actions/runs/1"),
        })];
        if polls >= fake.run_appears_after {
            runs.extend(
                fake.runs
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|run| run["repo"] == repo.as_str() && run["workflow"] == file.as_str())
                    .cloned(),
            );
        }
        runs.reverse();
        Json(json!({"total_count": runs.len(), "workflow_runs": runs}))
    }

    async fn get_run(
        State(fake): State<Shared>,
        Path((_o, _r, id)): Path<(String, String, u64)>,
    ) -> Response {
        let Some(mut run) = fake
            .runs
            .lock()
            .unwrap()
            .iter()
            .find(|run| run["id"] == id)
            .cloned()
        else {
            return not_found();
        };
        let mut polls = fake.run_polls.lock().unwrap();
        let seen = polls.entry(id).or_insert(0);
        *seen += 1;
        if *seen >= 2 {
            run["status"] = json!("completed");
            run["conclusion"] = run["final"].clone();
        } else {
            run["status"] = json!("in_progress");
        }
        Json(run).into_response()
    }

    async fn run_jobs(
        State(fake): State<Shared>,
        Path((_o, _r, id)): Path<(String, String, u64)>,
    ) -> Json<Value> {
        let conclusion = fake
            .runs
            .lock()
            .unwrap()
            .iter()
            .find(|run| run["id"] == id)
            .map(|run| run["final"].clone())
            .unwrap_or(json!("failure"));
        Json(json!({"jobs": [
            {"name": "build", "conclusion": "success", "steps": [
                {"name": "Checkout", "conclusion": "success"},
            ]},
            {"name": "deploy", "conclusion": conclusion, "steps": [
                {"name": "Checkout", "conclusion": "success"},
                {"name": "Apply manifests", "conclusion": conclusion},
                {"name": "Smoke test", "conclusion": "skipped"},
            ]},
        ]}))
    }

    async fn create_deployment(
        State(fake): State<Shared>,
        Path((o, r)): Path<(String, String)>,
        Json(mut body): Json<Value>,
    ) -> Response {
        if fake.forbid_writes {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"message": "Resource not accessible by integration"})),
            )
                .into_response();
        }
        let id = fake.next_id.fetch_add(1, Ordering::SeqCst);
        body["repo"] = json!(format!("{o}/{r}"));
        body["id"] = json!(id);
        fake.deployments.lock().unwrap().push(body);
        (StatusCode::CREATED, Json(json!({"id": id}))).into_response()
    }

    async fn create_deployment_status(
        State(fake): State<Shared>,
        Path((_o, _r, id)): Path<(String, String, u64)>,
        Json(mut body): Json<Value>,
    ) -> Response {
        body["deployment_id"] = json!(id);
        fake.deployment_statuses.lock().unwrap().push(body);
        (StatusCode::CREATED, Json(json!({"id": id + 1}))).into_response()
    }
}

pub mod registry {
    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        extract::{Path, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::get,
    };
    use serde_json::json;

    /// An open registry (no challenge) that knows the manifests it was given.
    #[derive(Default)]
    pub struct FakeRegistry {
        /// `(repository path, reference)` → digest.
        manifests: Mutex<Vec<(String, String, String)>>,
        pub lookups: Mutex<Vec<String>>,
    }

    impl FakeRegistry {
        /// Publishes `repository:reference` with `digest`.
        pub fn publish(&self, repository: &str, reference: &str, digest: &str) {
            self.manifests.lock().unwrap().push((
                repository.to_owned(),
                reference.to_owned(),
                digest.to_owned(),
            ));
        }

        /// Serves on a random port; the second value is `host:port`.
        pub async fn serve() -> (Arc<Self>, String) {
            let fake = Arc::new(Self::default());
            let app = Router::new()
                .route("/v2/{*rest}", get(manifest).head(manifest))
                .with_state(Arc::clone(&fake));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap().to_string();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            (fake, addr)
        }
    }

    async fn manifest(State(fake): State<Arc<FakeRegistry>>, Path(rest): Path<String>) -> Response {
        fake.lookups.lock().unwrap().push(rest.clone());
        let found = rest
            .split_once("/manifests/")
            .and_then(|(repository, reference)| {
                fake.manifests
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(r, t, _)| r == repository && t == reference)
                    .map(|(_, _, digest)| digest.clone())
            });
        match found {
            Some(digest) => (StatusCode::OK, [("docker-content-digest", digest)], "").into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({"errors": [{"code": "MANIFEST_UNKNOWN", "message": "manifest unknown"}]})),
            )
                .into_response(),
        }
    }
}

pub mod webhook {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    use serde_json::Value;

    use crate::adapters::webhook::signature;

    /// A deploy hook that verifies the signature and can be told to fail.
    pub struct FakeWebhook {
        secret: String,
        pub deliveries: Mutex<Vec<Value>>,
        /// Deliveries still to be answered with a 500 that echoes the secret.
        pub fail_next: AtomicUsize,
        pub health_hits: AtomicUsize,
    }

    impl FakeWebhook {
        /// Serves `POST /deploy` and `GET /health`; the second value is the
        /// base URL.
        pub async fn serve(secret: &str) -> (Arc<Self>, String) {
            let fake = Arc::new(Self {
                secret: secret.to_owned(),
                deliveries: Mutex::default(),
                fail_next: AtomicUsize::new(0),
                health_hits: AtomicUsize::new(0),
            });
            let app = Router::new()
                .route("/deploy", post(deliver))
                .route("/health", get(health))
                .with_state(Arc::clone(&fake));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            (fake, format!("http://{addr}"))
        }
    }

    async fn health(State(fake): State<Arc<FakeWebhook>>) -> &'static str {
        fake.health_hits.fetch_add(1, Ordering::SeqCst);
        "ok"
    }

    async fn deliver(
        State(fake): State<Arc<FakeWebhook>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let signed = headers
            .get("x-sdi-signature-256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if signed != signature(&fake.secret, &body) {
            return (StatusCode::UNAUTHORIZED, "signature mismatch").into_response();
        }
        let failing = fake
            .fail_next
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if failing {
            // A deployer that leaks its configuration into the error body.
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("rollout failed; DEPLOY_HOOK_SECRET={}", fake.secret),
            )
                .into_response();
        }
        fake.deliveries
            .lock()
            .unwrap()
            .push(serde_json::from_slice(&body).unwrap());
        (StatusCode::OK, "accepted").into_response()
    }
}
