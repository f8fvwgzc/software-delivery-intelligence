//! Pull request autopilot, the knowledge half (CONTRACT.md §23), through
//! the gRPC service against a fake GitHub (an axum server on a random port)
//! and the dev Postgres:
//!
//! - `GetPullRequestStatus`: mergeability, checks, reviews, merge methods;
//! - `GetRepositorySecurity`: Dependabot's switches, alerts, fix links;
//! - `GetRepoFile` at a `ref`: read from GitHub, never cached;
//! - the origin flags of a pull request summary (fork, bot, Dependabot);
//! - the polling path: an unchanged list costs GitHub a `304` and the
//!   database nothing.
//!
//! The database tests are skipped when `DATABASE_URL` is not set, like the
//! others; the client tests at the end need no database.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use base64::Engine;
use chrono::{DateTime, Utc};
use sdi_analyzer::DependabotDraft;
use sdi_common::principal::Role;
use sdi_knowledge::{
    auth::Caller,
    db::{
        self,
        projects::{NewProject, ProjectRow},
    },
    events::EventBus,
    github::{Github, GithubAccess, GithubError, TokenProvider},
    grpc::KnowledgeState,
    identity::Identity,
};
use sdi_proto::knowledge::v1::{knowledge_service_server::KnowledgeService, *};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::broadcast::error::TryRecvError;
use tonic::{Code, Request};
use uuid::Uuid;

/// The workspace's token in these tests. No answer may ever contain it.
const TOKEN: &str = "ghp_autopilot_test_token";

// ---------------------------------------------------------------------------
// Fake GitHub
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Canned {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// One request the fake answered.
#[derive(Debug, Clone)]
struct Seen {
    /// Path and query, as sent.
    target: String,
    status: u16,
    /// The request carried `If-None-Match`.
    conditional: bool,
    /// The request carried the workspace's token.
    authorized: bool,
}

/// Answers by exact path-and-query first, then by path. An answer with an
/// `ETag` turns into `304` for a request whose `If-None-Match` names it,
/// the way GitHub does. Everything else is `404`.
#[derive(Default)]
struct FakeGithub {
    routes: Mutex<HashMap<String, Canned>>,
    seen: Mutex<Vec<Seen>>,
}

impl FakeGithub {
    fn set(&self, target: &str, status: u16, headers: &[(&str, &str)], body: impl Into<Vec<u8>>) {
        self.routes.lock().unwrap().insert(
            target.to_owned(),
            Canned {
                status,
                headers: headers
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
                body: body.into(),
            },
        );
    }

    fn json(&self, target: &str, body: Value) {
        self.set(target, 200, &[], body.to_string());
    }

    fn json_etag(&self, target: &str, etag: &str, body: Value) {
        self.set(target, 200, &[("etag", etag)], body.to_string());
    }

    /// An error answer in GitHub's shape.
    fn error(&self, target: &str, status: u16, message: &str) {
        self.set(target, status, &[], json!({"message": message}).to_string());
    }

    fn remove(&self, target: &str) {
        self.routes.lock().unwrap().remove(target);
    }

    /// Requests whose target starts with `prefix`, in order.
    fn seen(&self, prefix: &str) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.target.starts_with(prefix))
            .cloned()
            .collect()
    }

    fn count(&self, prefix: &str) -> usize {
        self.seen(prefix).len()
    }

    /// Requests for exactly `target` (path and query), in order.
    fn exact(&self, target: &str) -> Vec<Seen> {
        self.seen(target)
            .into_iter()
            .filter(|s| s.target == target)
            .collect()
    }

    fn statuses(&self, prefix: &str) -> Vec<u16> {
        self.seen(prefix).iter().map(|s| s.status).collect()
    }
}

async fn fake_handler(
    State(fake): State<Arc<FakeGithub>>,
    request: axum::extract::Request,
) -> Response {
    let path = request.uri().path().to_owned();
    let target = match request.uri().query() {
        Some(query) if !query.is_empty() => format!("{path}?{query}"),
        _ => path.clone(),
    };
    let header_text = |name: header::HeaderName| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let if_none_match = header_text(header::IF_NONE_MATCH);
    let authorized =
        header_text(header::AUTHORIZATION).is_some_and(|v| v == format!("Bearer {TOKEN}"));
    let canned = {
        let routes = fake.routes.lock().unwrap();
        routes.get(&target).or_else(|| routes.get(&path)).cloned()
    };
    let record = |status: u16| {
        fake.seen.lock().unwrap().push(Seen {
            target: target.clone(),
            status,
            conditional: if_none_match.is_some(),
            authorized,
        });
    };
    let Some(canned) = canned else {
        record(404);
        return (StatusCode::NOT_FOUND, r#"{"message":"Not Found"}"#).into_response();
    };
    let etag = canned
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
        .map(|(_, v)| v.clone());
    if canned.status == 200 && etag.is_some() && etag == if_none_match {
        record(304);
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag.unwrap_or_default())
            .body(Body::empty())
            .unwrap();
    }
    record(canned.status);
    let mut response = Response::builder().status(canned.status);
    for (k, v) in &canned.headers {
        response = response.header(k, v);
    }
    if !canned
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        response = response.header(header::CONTENT_TYPE, "application/json");
    }
    response.body(Body::from(canned.body)).unwrap()
}

async fn serve_github(fake: Arc<FakeGithub>) -> SocketAddr {
    let app = Router::new().fallback(get(fake_handler)).with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

// ---------------------------------------------------------------------------
// Harness: a schema, the service, a workspace pointed at the fake
// ---------------------------------------------------------------------------

struct Harness {
    pool: PgPool,
    schema: String,
    state: KnowledgeState,
    fake: Arc<FakeGithub>,
    workspace: Uuid,
}

impl Harness {
    /// `None` when there is no database to run against.
    async fn start() -> Option<Self> {
        let url = std::env::var("DATABASE_URL")
            .ok()
            .filter(|u| !u.is_empty())?;
        // `db::connect` only accepts lowercase letters, so spell the random suffix with them.
        let suffix: String = Uuid::new_v4()
            .simple()
            .to_string()
            .bytes()
            .map(|b| (b'a' + (b % 26)) as char)
            .take(12)
            .collect();
        let schema = format!("knowledge_test_{suffix}");
        let pool = sdi_common::db::connect(&url, &schema)
            .await
            .expect("dev database reachable");
        sdi_knowledge::db::migrate(&pool)
            .await
            .expect("migrations apply");

        let fake = Arc::new(FakeGithub::default());
        let api_base = format!("http://{}", serve_github(Arc::clone(&fake)).await);
        // No identity service: the workspace's access is handed to the hub
        // directly, pointing at the fake.
        let identity = Identity::disabled();
        let github = Github::new(TokenProvider::new(identity.clone(), None)).unwrap();
        let workspace = Uuid::now_v7();
        let mut access = GithubAccess::dev(TOKEN);
        access.api_base_url = api_base;
        github.remember_token(workspace, access);
        let state = KnowledgeState::new(pool.clone(), github, EventBus::new(), identity);
        Some(Self {
            pool,
            schema,
            state,
            fake,
            workspace,
        })
    }

    async fn project(&self, full_name: &str) -> ProjectRow {
        self.project_in(self.workspace, full_name).await
    }

    async fn project_in(&self, workspace: Uuid, full_name: &str) -> ProjectRow {
        db::projects::insert(
            &self.pool,
            workspace,
            &NewProject {
                github_repo_id: None,
                name: full_name.rsplit('/').next().unwrap().to_owned(),
                repo_full_name: full_name.to_owned(),
                repo_url: format!("https://github.com/{full_name}"),
                default_branch: "main".to_owned(),
                is_private: true,
                description: String::new(),
                primary_language: "Rust".to_owned(),
            },
        )
        .await
        .unwrap()
    }

    /// A request from a viewer of the harness's workspace: every call
    /// under test is a read.
    fn request<T>(&self, message: T) -> Request<T> {
        request_in(self.workspace, message)
    }

    async fn status(
        &self,
        project: &ProjectRow,
        number: u64,
    ) -> Result<PullRequestStatus, tonic::Status> {
        self.state
            .get_pull_request_status(self.request(GetPullRequestStatusRequest {
                project_id: project.id.to_string(),
                number,
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    async fn security(&self, project: &ProjectRow, refresh: bool) -> RepositorySecurity {
        self.state
            .get_repository_security(self.request(GetRepositorySecurityRequest {
                project_id: project.id.to_string(),
                refresh,
            }))
            .await
            .expect("GetRepositorySecurity never fails for a known project")
            .into_inner()
    }

    async fn file(
        &self,
        project: &ProjectRow,
        path: &str,
        reference: &str,
    ) -> Result<RepoFile, tonic::Status> {
        self.state
            .get_repo_file(self.request(GetRepoFileRequest {
                project_id: project.id.to_string(),
                path: path.to_owned(),
                r#ref: reference.to_owned(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    async fn list(&self, project: &ProjectRow, refresh: bool) -> ListPullRequestsResponse {
        self.state
            .list_pull_requests(self.request(ListPullRequestsRequest {
                project_id: project.id.to_string(),
                state: "open".to_owned(),
                refresh,
                limit: 50,
            }))
            .await
            .unwrap()
            .into_inner()
    }

    /// `(number, state, xmin, synced_at)` of the project's cached pull
    /// requests. `xmin` changes whenever a row is written, even with the
    /// values it already had.
    async fn rows(&self, project: &ProjectRow) -> Vec<(i64, String, String, DateTime<Utc>)> {
        sqlx::query_as(
            "SELECT number, state, xmin::text, synced_at FROM pull_requests WHERE project_id = $1 ORDER BY number",
        )
        .bind(project.id)
        .fetch_all(&self.pool)
        .await
        .unwrap()
    }

    async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn finish(self) {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            self.schema
        )))
        .execute(&self.pool)
        .await
        .expect("schema dropped");
    }
}

fn request_in<T>(workspace: Uuid, message: T) -> Request<T> {
    let mut request = Request::new(message);
    Caller {
        user_id: "u1".to_owned(),
        workspace_id: workspace,
        role: Role::Viewer,
    }
    .principal()
    .apply(&mut request);
    request
}

fn sha(seed: char) -> String {
    seed.to_string().repeat(40)
}

/// A pull request the way GitHub serialises one (the parts that are read,
/// and a few that are not).
fn pull(
    number: u64,
    author: (&str, &str),
    head: (&str, &str, Option<&str>),
    extra: Value,
) -> Value {
    let (login, kind) = author;
    let (head_ref, head_sha, head_repo) = head;
    let mut pull = json!({
        "number": number,
        "title": format!("Change {number}"),
        "body": "Why this change exists.",
        "state": "open",
        "draft": false,
        "user": {"login": login, "type": kind, "avatar_url": format!("https://avatars.example/{login}"), "site_admin": false},
        "head": {"ref": head_ref, "sha": head_sha, "label": "x", "repo": head_repo.map(|full_name| json!({"full_name": full_name, "private": true}))},
        "base": {"ref": "main", "sha": sha('b'), "repo": {"full_name": "acme/api"}},
        "html_url": format!("https://github.com/acme/api/pull/{number}"),
        "created_at": "2026-10-10T08:00:00Z",
        "updated_at": "2026-10-12T08:00:00Z",
        "_links": {"self": {"href": "https://api.github.com/..."}},
    });
    for (key, value) in extra.as_object().cloned().unwrap_or_default() {
        pull[key] = value;
    }
    pull
}

fn check_run(id: i64, name: &str, status: &str, conclusion: Option<&str>, minute: u32) -> Value {
    json!({
        "id": id,
        "name": name,
        "status": status,
        "conclusion": conclusion,
        "started_at": format!("2026-10-13T09:{minute:02}:00Z"),
        "completed_at": conclusion.map(|_| format!("2026-10-13T09:{:02}:30Z", minute)),
        "app": {"slug": "github-actions"},
        "output": {"title": null, "summary": null},
    })
}

fn review(login: &str, state: &str, minute: Option<u32>) -> Value {
    json!({
        "id": 1,
        "user": {"login": login, "type": "User"},
        "state": state,
        "submitted_at": minute.map(|m| format!("2026-10-13T10:{m:02}:00Z")),
        "body": "",
    })
}

// ---------------------------------------------------------------------------
// GetPullRequestStatus
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pull_request_status_is_read_from_github_and_folded() {
    let Some(h) = Harness::start().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    let project = h.project("acme/api").await;
    let head = sha('a');
    let fake = &h.fake;

    // --- A Dependabot pull request GitHub has not judged yet -------------
    fake.json_etag(
        "/repos/acme/api/pulls/7",
        "\"p7-v1\"",
        pull(
            7,
            ("dependabot[bot]", "Bot"),
            ("dependabot/cargo/serde-1.0.200", &head, Some("acme/api")),
            json!({"title": "Bump serde from 1.0.199 to 1.0.200", "merged": false,
                   "mergeable": null, "mergeable_state": "unknown",
                   "changed_files": 2, "additions": 3, "deletions": 3}),
        ),
    );
    fake.json(
        "/repos/acme/api",
        json!({"id": 1, "full_name": "acme/api", "name": "api", "owner": {"login": "acme"},
               "allow_squash_merge": true, "allow_merge_commit": false, "allow_rebase_merge": true}),
    );
    fake.json(
        &format!("/repos/acme/api/commits/{head}/check-runs"),
        json!({"total_count": 9, "check_runs": [
            // Two things called `build`, one of them failing: one check, failed.
            check_run(1, "build", "completed", Some("failure"), 0),
            check_run(2, "build", "completed", Some("success"), 10),
            check_run(3, "test", "in_progress", None, 11),
            check_run(4, "lint", "completed", Some("skipped"), 2),
            check_run(5, "deploy", "completed", Some("timed_out"), 3),
            check_run(6, "docs", "completed", Some("neutral"), 4),
            check_run(7, "e2e", "queued", None, 12),
            check_run(8, "audit", "completed", Some("action_required"), 5),
        ]}),
    );
    fake.json(
        &format!("/repos/acme/api/commits/{head}/status"),
        json!({"state": "failure", "total_count": 5, "statuses": [
            {"context": "ci/circleci", "state": "success", "updated_at": "2026-10-13T09:06:00Z"},
            {"context": "license/cla", "state": "pending", "updated_at": "2026-10-13T09:06:00Z"},
            {"context": "security/snyk", "state": "error", "updated_at": "2026-10-13T09:06:00Z"},
            {"context": "coverage", "state": "failure", "updated_at": "2026-10-13T09:06:00Z"},
            // A status with a check run's name: one check called `lint`,
            // and the failing one is the one that counts.
            {"context": "lint", "state": "failure", "updated_at": "2026-10-13T08:00:00Z"},
        ]}),
    );
    fake.json(
        "/repos/acme/api/pulls/7/reviews",
        json!([
            review("ana", "CHANGES_REQUESTED", Some(0)),
            review("ben", "APPROVED", Some(1)),
            review("ana", "APPROVED", Some(5)),
            review("cy", "APPROVED", Some(6)),
            review("cy", "DISMISSED", Some(7)),
            review("dee", "CHANGES_REQUESTED", Some(8)),
            review("ben", "COMMENTED", Some(9)),
            review("eve", "PENDING", None),
        ]),
    );
    fake.json(
        "/repos/acme/api/branches/main",
        json!({"name": "main", "protected": true}),
    );
    fake.json(
        &format!("/repos/acme/api/compare/main...{head}"),
        json!({"status": "diverged", "ahead_by": 1, "behind_by": 3, "total_commits": 1,
               "commits": [{"sha": head}], "files": [{"filename": "Cargo.toml", "patch": "@@"}]}),
    );

    let status = h.status(&project, 7).await.unwrap();
    assert_eq!(
        status.mergeable, "unknown",
        "null means GitHub is still computing"
    );
    assert_eq!(status.mergeable_state, "unknown");
    let checks = status.checks.clone().unwrap();
    assert_eq!(
        (checks.total, checks.passed, checks.failed, checks.pending),
        (11, 2, 6, 3),
        "{checks:?}"
    );
    assert_eq!(
        checks.failed_names,
        [
            "audit",
            "build",
            "coverage",
            "deploy",
            "lint",
            "security/snyk"
        ]
    );
    assert_eq!(checks.pending_names, ["e2e", "license/cla", "test"]);
    assert_eq!((status.approvals, status.changes_requested), (2, 1));
    assert_eq!(status.allowed_methods, ["squash", "rebase"]);
    assert!(status.base_protected);
    assert_eq!(status.behind_by, 3);
    assert!(status.checked_at.is_some());
    let summary = status.summary.clone().unwrap();
    assert_eq!(summary.project_id, project.id.to_string());
    assert_eq!(summary.number, 7);
    assert_eq!(summary.title, "Bump serde from 1.0.199 to 1.0.200");
    assert_eq!(summary.author, "dependabot[bot]");
    assert_eq!(summary.state, "open");
    assert_eq!(summary.base_ref, "main");
    assert_eq!(summary.head_ref, "dependabot/cargo/serde-1.0.200");
    assert_eq!(summary.head_sha, head);
    assert_eq!(
        (summary.changed_files, summary.additions, summary.deletions),
        (2, 3, 3)
    );
    assert_eq!(summary.head_repo, "acme/api");
    assert!(summary.is_bot && summary.is_dependabot && !summary.is_fork);
    // The answer is GitHub's; nothing of it is stored.
    assert_eq!(h.count("pull_requests").await, 0);
    // Every call carried the workspace's token and went to the fake.
    assert!(fake.seen("/repos/acme/api").iter().all(|s| s.authorized));
    assert_eq!(
        fake.count(&format!(
            "/repos/acme/api/commits/{head}/check-runs?per_page=100"
        )),
        1
    );
    assert_eq!(
        fake.count("/repos/acme/api/pulls/7/reviews?per_page=100&page=1"),
        1
    );

    // --- "Unknown" is never revalidated ------------------------------------
    // GitHub computes mergeability after the first read. A conditional
    // request could be told "not modified" about an answer that only said
    // "ask again", so the second read is a plain one.
    let again = h.status(&project, 7).await.unwrap();
    let reads = fake.exact("/repos/acme/api/pulls/7");
    assert_eq!(
        reads.iter().map(|s| s.status).collect::<Vec<_>>(),
        [200, 200]
    );
    assert!(reads.iter().all(|s| !s.conditional));
    assert_eq!(again.checks, status.checks);
    assert_eq!(again.summary, status.summary);
    assert_eq!(again.mergeable, "unknown");

    // GitHub finished computing: the new answer is served.
    let judged = |mergeable: bool, word: &str, etag: &str| {
        fake.json_etag(
            "/repos/acme/api/pulls/7",
            etag,
            pull(
                7,
                ("dependabot[bot]", "Bot"),
                ("dependabot/cargo/serde-1.0.200", &head, Some("acme/api")),
                json!({"mergeable": mergeable, "mergeable_state": word}),
            ),
        );
    };
    judged(true, "clean", "\"p7-v2\"");
    let clean = h.status(&project, 7).await.unwrap();
    assert_eq!(
        (clean.mergeable.as_str(), clean.mergeable_state.as_str()),
        ("mergeable", "clean")
    );
    // --- A settled answer is revalidated, not trusted from memory ---------
    let same = h.status(&project, 7).await.unwrap();
    let reads = fake.exact("/repos/acme/api/pulls/7");
    assert_eq!(
        reads.iter().map(|s| s.status).collect::<Vec<_>>(),
        [200, 200, 200, 304],
        "an unchanged pull request costs GitHub a 304"
    );
    assert!(reads[3].conditional);
    assert_eq!(same.mergeable, "mergeable");
    assert_eq!(same.summary, clean.summary);
    // And a changed one is a new answer, whatever was remembered.
    judged(false, "dirty", "\"p7-v3\"");
    let dirty = h.status(&project, 7).await.unwrap();
    assert_eq!(
        (dirty.mergeable.as_str(), dirty.mergeable_state.as_str()),
        ("conflicting", "dirty")
    );

    // --- A fork, and a fork that no longer exists ------------------------
    let fork_head = sha('c');
    fake.json(
        "/repos/acme/api/pulls/8",
        pull(
            8,
            ("bo", "User"),
            ("patch-1", &fork_head, Some("bo/api")),
            json!({"mergeable": true, "mergeable_state": "blocked"}),
        ),
    );
    fake.json(
        &format!("/repos/acme/api/commits/{fork_head}/check-runs"),
        json!({"total_count": 0, "check_runs": []}),
    );
    fake.json(
        &format!("/repos/acme/api/commits/{fork_head}/status"),
        json!({"state": "pending", "statuses": []}),
    );
    fake.json("/repos/acme/api/pulls/8/reviews", json!([]));
    fake.json(
        &format!("/repos/acme/api/compare/main...{fork_head}"),
        json!({"ahead_by": 2, "behind_by": 0}),
    );
    let fork = h.status(&project, 8).await.unwrap();
    let summary = fork.summary.unwrap();
    assert!(summary.is_fork && !summary.is_bot && !summary.is_dependabot);
    assert_eq!(summary.head_repo, "bo/api");
    assert_eq!(fork.mergeable_state, "blocked");
    let checks = fork.checks.unwrap();
    assert_eq!(
        (checks.total, checks.passed, checks.failed, checks.pending),
        (0, 0, 0, 0)
    );
    assert!(checks.failed_names.is_empty() && checks.pending_names.is_empty());
    assert_eq!(
        (fork.approvals, fork.changes_requested, fork.behind_by),
        (0, 0, 0)
    );

    let gone_head = sha('d');
    fake.json(
        "/repos/acme/api/pulls/9",
        pull(
            9,
            ("bo", "User"),
            ("patch-2", &gone_head, None),
            json!({"draft": true, "mergeable": null, "mergeable_state": "draft"}),
        ),
    );
    fake.json(
        &format!("/repos/acme/api/commits/{gone_head}/check-runs"),
        json!({"total_count": 0, "check_runs": []}),
    );
    fake.json(
        &format!("/repos/acme/api/commits/{gone_head}/status"),
        json!({"statuses": []}),
    );
    fake.json("/repos/acme/api/pulls/9/reviews", json!([]));
    let gone = h.status(&project, 9).await.unwrap();
    let summary = gone.summary.unwrap();
    assert!(summary.is_fork, "a deleted fork is still a fork");
    assert_eq!(summary.head_repo, "");
    assert_eq!(summary.state, "draft");
    assert_eq!(gone.mergeable_state, "draft");
    // The comparison for this head is not served (404): the default stands.
    assert_eq!(gone.behind_by, 0);

    // --- What the repository does not say --------------------------------
    // A token without push access is not told the merge methods; a branch
    // or a comparison GitHub will not give leaves the defaults.
    let plain = h.project("acme/plain").await;
    let plain_head = sha('e');
    fake.json(
        "/repos/acme/plain/pulls/1",
        pull(
            1,
            ("ana", "User"),
            ("feat/x", &plain_head, Some("acme/plain")),
            json!({"mergeable": true, "mergeable_state": "clean"}),
        ),
    );
    fake.json(
        "/repos/acme/plain",
        json!({"id": 2, "full_name": "acme/plain"}),
    );
    fake.json(
        &format!("/repos/acme/plain/commits/{plain_head}/check-runs"),
        json!({"check_runs": [check_run(1, "ci", "completed", Some("success"), 0)]}),
    );
    fake.json(
        &format!("/repos/acme/plain/commits/{plain_head}/status"),
        json!({"statuses": []}),
    );
    fake.json("/repos/acme/plain/pulls/1/reviews", json!([]));
    fake.error(
        "/repos/acme/plain/branches/main",
        403,
        "Resource not accessible by integration",
    );
    fake.error(
        &format!("/repos/acme/plain/compare/main...{plain_head}"),
        500,
        "Server Error",
    );
    let untold = h.status(&plain, 1).await.unwrap();
    assert_eq!(untold.allowed_methods, ["squash", "merge", "rebase"]);
    assert!(!untold.base_protected);
    assert_eq!(untold.behind_by, 0);
    assert_eq!(untold.mergeable, "mergeable");
    assert_eq!(untold.checks.unwrap().passed, 1);

    // --- Refusals ---------------------------------------------------------
    let missing = h.status(&project, 99).await.unwrap_err();
    assert_eq!(missing.code(), Code::NotFound);
    assert!(missing.message().contains("#99"), "{}", missing.message());
    assert!(
        missing.message().contains("acme/api"),
        "{}",
        missing.message()
    );

    assert_eq!(
        h.status(&project, 0).await.unwrap_err().code(),
        Code::InvalidArgument
    );

    // What decides a merge must be readable: no guessing "no checks".
    let locked = h.project("acme/locked").await;
    let locked_head = sha('f');
    fake.json(
        "/repos/acme/locked/pulls/2",
        pull(
            2,
            ("ana", "User"),
            ("feat/y", &locked_head, Some("acme/locked")),
            json!({}),
        ),
    );
    fake.json("/repos/acme/locked", json!({"id": 3}));
    fake.error(
        &format!("/repos/acme/locked/commits/{locked_head}/check-runs"),
        403,
        "Resource not accessible by integration",
    );
    fake.json(
        &format!("/repos/acme/locked/commits/{locked_head}/status"),
        json!({"statuses": []}),
    );
    fake.json("/repos/acme/locked/pulls/2/reviews", json!([]));
    let forbidden = h.status(&locked, 2).await.unwrap_err();
    assert_eq!(forbidden.code(), Code::PermissionDenied);
    assert!(
        forbidden.message().contains("check runs"),
        "{}",
        forbidden.message()
    );
    assert!(!forbidden.message().contains(TOKEN));

    // Nor is it judged on part of a list: more check runs than are read
    // is said, not passed off as "all green".
    let crowded_head = sha('9');
    fake.json(
        "/repos/acme/locked/pulls/3",
        pull(
            3,
            ("ana", "User"),
            ("feat/z", &crowded_head, Some("acme/locked")),
            json!({}),
        ),
    );
    fake.json(
        &format!("/repos/acme/locked/commits/{crowded_head}/check-runs"),
        json!({"total_count": 5000, "check_runs": (0..100)
            .map(|id| check_run(id, &format!("job-{id}"), "completed", Some("success"), 1))
            .collect::<Vec<_>>()}),
    );
    fake.json(
        &format!("/repos/acme/locked/commits/{crowded_head}/status"),
        json!({"statuses": []}),
    );
    fake.json("/repos/acme/locked/pulls/3/reviews", json!([]));
    let partial = h.status(&locked, 3).await.unwrap_err();
    assert_eq!(partial.code(), Code::FailedPrecondition);
    assert!(
        partial.message().contains("more than 500 check runs"),
        "{}",
        partial.message()
    );

    // Another workspace cannot see the project, and an id that is no
    // project is the same answer.
    let stranger = h
        .state
        .get_pull_request_status(request_in(
            Uuid::now_v7(),
            GetPullRequestStatusRequest {
                project_id: project.id.to_string(),
                number: 7,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(stranger.code(), Code::NotFound);
    let unknown = h
        .state
        .get_pull_request_status(h.request(GetPullRequestStatusRequest {
            project_id: Uuid::now_v7().to_string(),
            number: 7,
        }))
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), Code::NotFound);
    assert_eq!(h.count("pull_requests").await, 0);

    h.finish().await;
}

// ---------------------------------------------------------------------------
// Summaries: origin flags, and the polling path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn summaries_carry_origin_flags_and_an_unchanged_list_costs_nothing() {
    let Some(h) = Harness::start().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    let project = h.project("acme/web").await;
    let fake = &h.fake;
    let listed = |title: &str| {
        json!([
            pull(
                1,
                ("ana", "User"),
                ("feat/x", &sha('1'), Some("acme/web")),
                json!({"title": title})
            ),
            pull(
                2,
                ("bo", "User"),
                ("patch-1", &sha('2'), Some("bo/web")),
                json!({})
            ),
            pull(
                3,
                ("dependabot[bot]", "Bot"),
                (
                    "dependabot/npm_and_yarn/vite-5.4.8",
                    &sha('3'),
                    Some("acme/web")
                ),
                json!({})
            ),
            pull(
                4,
                ("cy", "User"),
                ("patch-9", &sha('4'), None),
                json!({"draft": true})
            ),
            pull(
                5,
                ("renovate[bot]", "Bot"),
                ("renovate/react-19.x", &sha('5'), Some("acme/web")),
                json!({})
            ),
        ])
    };
    fake.json_etag("/repos/acme/web/pulls", "\"list-v1\"", listed("Add search"));
    let mut events = h.state.events.subscribe();

    // --- First look: GitHub is read, the cache is written ----------------
    let first = h.list(&project, true).await;
    let by_number: HashMap<u64, PullRequestSummary> = first
        .pull_requests
        .iter()
        .map(|p| (p.number, p.clone()))
        .collect();
    assert_eq!(by_number.len(), 5);
    let flags = |n: u64| {
        let p = &by_number[&n];
        (p.head_repo.as_str(), p.is_fork, p.is_bot, p.is_dependabot)
    };
    assert_eq!(flags(1), ("acme/web", false, false, false));
    assert_eq!(flags(2), ("bo/web", true, false, false));
    assert_eq!(flags(3), ("acme/web", false, true, true));
    assert_eq!(flags(4), ("", true, false, false), "a deleted fork");
    assert_eq!(
        flags(5),
        ("acme/web", false, true, false),
        "a bot that is not Dependabot"
    );
    assert_eq!(by_number[&4].state, "draft");
    assert_eq!(by_number[&1].title, "Add search");
    assert!(first.synced_at.is_some(), "a sync happened");
    let written = h.rows(&project).await;
    assert_eq!(written.len(), 5);
    // The flags are stored, not recomputed: a plain read gives them back.
    let cached = h.list(&project, false).await;
    assert_eq!(cached.pull_requests, first.pull_requests);
    assert_eq!(
        fake.statuses("/repos/acme/web/pulls?"),
        [200],
        "one page, read once"
    );
    assert_eq!(h.state.pull_sync_marks(), 1);

    // --- The poll: nothing changed on GitHub ------------------------------
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let before_poll = Utc::now();
    let polled = h.list(&project, true).await;
    let requests = fake.seen("/repos/acme/web/pulls?");
    assert_eq!(requests.len(), 2);
    assert!(requests[1].conditional, "the poll sent If-None-Match");
    assert_eq!(requests[1].status, 304, "and GitHub answered Not Modified");
    assert!(
        requests[1].target.contains("state=open") && requests[1].target.contains("per_page=50"),
        "{}",
        requests[1].target
    );
    assert_eq!(polled.pull_requests, first.pull_requests);
    assert_eq!(
        h.rows(&project).await,
        written,
        "no row was rewritten: same xmin, same synced_at"
    );
    // The list is confirmed as of now, though nothing was written: the
    // rows still say when they were written, which was before this poll.
    let polled_synced = polled
        .synced_at
        .as_ref()
        .and_then(sdi_common::time::from_proto)
        .expect("confirmed");
    assert!(
        polled_synced >= before_poll,
        "synced_at reports the confirmation ({polled_synced} vs {before_poll})"
    );
    // A later plain read reports that confirmation too.
    let plain = h.list(&project, false).await;
    assert_eq!(plain.synced_at, polled.synced_at);
    // Polling many times stays free, for GitHub and for the database.
    for _ in 0..3 {
        h.list(&project, true).await;
    }
    assert_eq!(
        fake.statuses("/repos/acme/web/pulls?"),
        [200, 304, 304, 304, 304]
    );
    assert_eq!(h.rows(&project).await, written);
    assert!(
        matches!(events.try_recv(), Err(TryRecvError::Empty)),
        "a poll emits no event"
    );

    // --- A cache that lost its rows is rewritten even on a 304 -----------
    sqlx::query("DELETE FROM pull_requests WHERE project_id = $1")
        .bind(project.id)
        .execute(&h.pool)
        .await
        .unwrap();
    let restored = h.list(&project, true).await;
    assert_eq!(restored.pull_requests.len(), 5);
    assert_eq!(fake.statuses("/repos/acme/web/pulls?").last(), Some(&304));
    let written = h.rows(&project).await;
    assert_eq!(written.len(), 5);

    // --- GitHub changed: the cache follows --------------------------------
    // #1 was retitled and #2 is no longer open.
    let mut changed = listed("Add search and filters");
    changed.as_array_mut().unwrap().remove(1);
    fake.json_etag("/repos/acme/web/pulls", "\"list-v2\"", changed);
    let updated = h.list(&project, true).await;
    assert_eq!(fake.statuses("/repos/acme/web/pulls?").last(), Some(&200));
    let titles: HashMap<u64, String> = updated
        .pull_requests
        .iter()
        .map(|p| (p.number, p.title.clone()))
        .collect();
    assert_eq!(titles.len(), 4);
    assert_eq!(titles[&1], "Add search and filters");
    let rows = h.rows(&project).await;
    assert_eq!(rows.len(), 5, "the closed one stays cached, as closed");
    assert_eq!(rows[1].1, "closed");
    assert_ne!(rows[0].2, written[0].2, "rows were rewritten this time");

    // --- One pull request carries the same flags --------------------------
    fake.json(
        "/repos/acme/web/pulls/3",
        pull(
            3,
            ("dependabot[bot]", "Bot"),
            (
                "dependabot/npm_and_yarn/vite-5.4.8",
                &sha('3'),
                Some("acme/web"),
            ),
            json!({"changed_files": 1, "additions": 1, "deletions": 1}),
        ),
    );
    fake.json("/repos/acme/web/pulls/3/files", json!([]));
    let one = h
        .state
        .get_pull_request(h.request(GetPullRequestRequest {
            project_id: project.id.to_string(),
            number: 3,
        }))
        .await
        .unwrap()
        .into_inner();
    let summary = one.summary.unwrap();
    assert!(summary.is_dependabot && summary.is_bot && !summary.is_fork);
    assert_eq!(summary.head_repo, "acme/web");
    assert_eq!(summary.changed_files, 1);
    // The list has no size counters; the next rewrite of the list keeps
    // the ones this read filled in, as long as the head is the same.
    let mut retitled = listed("Add search, filters and sorting");
    retitled.as_array_mut().unwrap().remove(1);
    fake.json_etag("/repos/acme/web/pulls", "\"list-v3\"", retitled);
    let relisted = h.list(&project, true).await;
    let third = relisted
        .pull_requests
        .iter()
        .find(|p| p.number == 3)
        .unwrap();
    assert_eq!(
        (third.changed_files, third.additions, third.deletions),
        (1, 1, 1)
    );
    assert!(
        relisted
            .pull_requests
            .iter()
            .any(|p| p.title == "Add search, filters and sorting"),
        "and the list was rewritten"
    );

    // The decoded slices are what is kept next to the ETags: a fraction of
    // what GitHub sent.
    let (entries, bytes) = h.state.github.slim_cached();
    assert!(entries >= 1, "{entries} slim entries");
    assert!(bytes < 16 * 1024, "{bytes} bytes held for a short list");

    h.finish().await;
}

// ---------------------------------------------------------------------------
// GetRepoFile at a ref
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_file_at_a_ref_is_read_from_github_and_never_cached() {
    let Some(h) = Harness::start().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    // Never scanned: a pull request head can be read all the same.
    let project = h.project("acme/code").await;
    let fake = &h.fake;
    let head = sha('a');
    let raw = [("content-type", "application/vnd.github.raw; charset=utf-8")];
    fake.set(
        "/repos/acme/code/contents/src/main.rs",
        200,
        &raw,
        "fn main() {\n    println!(\"head\");\n}\n",
    );
    fake.set(
        "/repos/acme/code/contents/assets/blob.dat",
        200,
        &raw,
        vec![1_u8, 2, 0, 3],
    );
    let big = vec![b'a'; (1 << 20) + 10];
    fake.set("/repos/acme/code/contents/big.txt", 200, &raw, big);
    // A fact about this path exists (from some scan); a read at a ref must
    // not claim it.
    sqlx::query(
        "INSERT INTO facts (id, workspace_id, project_id, kind, category, key, source_path)
         VALUES ($1, $2, $3, 'detected', 'language', 'language:rust', 'src/main.rs')",
    )
    .bind(Uuid::now_v7())
    .bind(h.workspace)
    .bind(project.id)
    .execute(&h.pool)
    .await
    .unwrap();

    let file = h.file(&project, "src/main.rs", &head).await.unwrap();
    assert_eq!(file.path, "src/main.rs");
    assert_eq!(file.commit_sha, head);
    assert_eq!(file.content, "fn main() {\n    println!(\"head\");\n}\n");
    assert!(!file.binary && !file.truncated);
    assert_eq!(file.size, file.content.len() as u64);
    assert_eq!(file.language, "rust");
    assert_eq!(
        file.html_url,
        format!("https://github.com/acme/code/blob/{head}/src/main.rs")
    );
    assert!(
        file.fact_ids.is_empty(),
        "facts belong to the scanned commit"
    );
    let asked = fake.seen("/repos/acme/code/contents/src/main.rs");
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].target,
        format!("/repos/acme/code/contents/src/main.rs?ref={head}")
    );
    assert!(asked[0].authorized);

    // Upper-case hex is the same commit; it is read again, not remembered.
    let again = h
        .file(&project, "src/main.rs", &head.to_ascii_uppercase())
        .await
        .unwrap();
    assert_eq!(again.commit_sha, head);
    assert_eq!(fake.count("/repos/acme/code/contents/src/main.rs"), 2);
    assert_eq!(
        h.count("repo_files").await,
        0,
        "nothing reached the code cache"
    );

    // The same limits as the scanned commit's files.
    let binary = h.file(&project, "assets/blob.dat", &head).await.unwrap();
    assert!(binary.binary && binary.content.is_empty());
    let large = h.file(&project, "big.txt", &head).await.unwrap();
    assert!(large.truncated);
    assert_eq!(large.content.len(), 1 << 20);
    assert_eq!(large.size, (1 << 20) + 10);
    assert_eq!(h.count("repo_files").await, 0);

    // A file that does not exist at that commit.
    assert_eq!(
        h.file(&project, "src/missing.rs", &head)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );

    // Anything but a full SHA is refused before GitHub is asked.
    let before = fake.count("/repos/acme/code/");
    for bad in [
        "main",
        "refs/pull/7/head",
        "abc1234",
        &head[..39],
        &format!("{head}0"),
    ] {
        let error = h.file(&project, "src/main.rs", bad).await.unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument, "{bad}");
        assert!(
            error.message().contains("40-character"),
            "{}",
            error.message()
        );
    }
    // The path rules are the same with a ref as without.
    for bad in [
        "../secrets.txt",
        "/etc/passwd",
        "src/../../x",
        "src//main.rs",
    ] {
        assert_eq!(
            h.file(&project, bad, &head).await.unwrap_err().code(),
            Code::InvalidArgument,
            "{bad}"
        );
    }
    for secret in [".env", "config/server.pem", "deploy/id_rsa"] {
        assert_eq!(
            h.file(&project, secret, &head).await.unwrap_err().code(),
            Code::PermissionDenied,
            "{secret}"
        );
    }
    assert_eq!(
        fake.count("/repos/acme/code/"),
        before,
        "none of those reached GitHub"
    );

    // Without a ref the scanned commit is what is read, and there is none.
    assert_eq!(
        h.file(&project, "src/main.rs", "")
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    // Another workspace cannot read through this project.
    let stranger = h
        .state
        .get_repo_file(request_in(
            Uuid::now_v7(),
            GetRepoFileRequest {
                project_id: project.id.to_string(),
                path: "src/main.rs".into(),
                r#ref: head.clone(),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(stranger.code(), Code::NotFound);

    h.finish().await;
}

// ---------------------------------------------------------------------------
// GetRepositorySecurity
// ---------------------------------------------------------------------------

fn alert(number: u64, package: (&str, &str), manifest: &str, severity: &str, day: u32) -> Value {
    let (ecosystem, name) = package;
    json!({
        "number": number,
        "state": "open",
        "dependency": {
            "package": {"ecosystem": ecosystem, "name": name},
            "manifest_path": manifest,
            "scope": "runtime"
        },
        "security_advisory": {
            "ghsa_id": format!("GHSA-0000-0000-{number:04}"),
            "cve_id": format!("CVE-2026-{number:04}"),
            "summary": format!("{name} is vulnerable"),
            "description": "A long description nobody stores.",
            "severity": severity,
            "references": [{"url": "https://example.com/advisory"}]
        },
        "security_vulnerability": {
            "package": {"ecosystem": ecosystem, "name": name},
            "severity": severity,
            "vulnerable_version_range": "< 2.0.0",
            "first_patched_version": {"identifier": "2.0.0"}
        },
        "html_url": format!("https://github.com/acme/ok/security/dependabot/{number}"),
        "created_at": format!("2026-10-{day:02}T08:00:00Z"),
    })
}

fn contents(text: &str) -> Value {
    json!({
        "type": "file",
        "encoding": "base64",
        "content": base64::engine::general_purpose::STANDARD.encode(text),
    })
}

const DEPENDABOT_YML: &str = "version: 2\nupdates:\n  - package-ecosystem: npm\n    directory: /web\n    schedule:\n      interval: weekly\n  - package-ecosystem: cargo\n    directory: /\n    schedule:\n      interval: weekly\n";

#[tokio::test]
async fn repository_security_reports_dependabot_and_its_alerts() {
    let Some(h) = Harness::start().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    let fake = &h.fake;

    // --- Everything on, alerts readable -----------------------------------
    let ok = h.project("acme/ok").await;
    fake.json(
        "/repos/acme/ok/pulls",
        json!([
            pull(
                41,
                ("dependabot[bot]", "Bot"),
                (
                    "dependabot/npm_and_yarn/web/babel/traverse-7.23.2",
                    &sha('1'),
                    Some("acme/ok")
                ),
                json!({})
            ),
            pull(
                42,
                ("dependabot[bot]", "Bot"),
                ("dependabot/cargo/time-0.3.36", &sha('2'), Some("acme/ok")),
                json!({})
            ),
            pull(
                43,
                ("ana", "User"),
                ("feat/lodash-4.17.21", &sha('3'), Some("acme/ok")),
                json!({})
            ),
        ]),
    );
    fake.json(
        "/repos/acme/ok/contents/.github/dependabot.yml",
        contents(DEPENDABOT_YML),
    );
    fake.set("/repos/acme/ok/vulnerability-alerts", 204, &[], "");
    fake.json(
        "/repos/acme/ok/automated-security-fixes",
        json!({"enabled": true, "paused": false}),
    );
    fake.json(
        "/repos/acme/ok/dependabot/alerts",
        json!([
            alert(1, ("npm", "lodash"), "package-lock.json", "low", 5),
            alert(
                2,
                ("npm", "@babel/traverse"),
                "web/package-lock.json",
                "critical",
                1
            ),
            alert(3, ("rust", "time"), "Cargo.lock", "high", 3),
            alert(4, ("pip", "django"), "requirements.txt", "high", 4),
            alert(5, ("npm", "minimist"), "package-lock.json", "medium", 2),
        ]),
    );

    let answer = h.security(&ok, false).await;
    assert_eq!(answer.project_id, ok.id.to_string());
    assert!(answer.config_present);
    assert_eq!(answer.config_path, ".github/dependabot.yml");
    assert_eq!(answer.config_ecosystems, ["npm", "cargo"]);
    assert_eq!(answer.alerts, "enabled");
    assert_eq!(answer.security_updates, "enabled");
    assert_eq!(answer.alerts_status, "ok");
    assert_eq!(answer.alerts_message, "");
    assert!(!answer.truncated);
    assert_eq!(
        answer.open_pull_requests, 2,
        "Dependabot's two, not the person's"
    );
    assert!(answer.checked_at.is_some());
    // Most severe first, then newest.
    let order: Vec<(u64, &str)> = answer
        .open_alerts
        .iter()
        .map(|a| (a.number, a.severity.as_str()))
        .collect();
    assert_eq!(
        order,
        [
            (2, "critical"),
            (4, "high"),
            (3, "high"),
            (5, "medium"),
            (1, "low")
        ]
    );
    let traverse = &answer.open_alerts[0];
    assert_eq!(traverse.ecosystem, "npm");
    assert_eq!(traverse.package, "@babel/traverse");
    assert_eq!(traverse.manifest_path, "web/package-lock.json");
    assert_eq!(traverse.scope, "runtime");
    assert_eq!(traverse.ghsa_id, "GHSA-0000-0000-0002");
    assert_eq!(traverse.cve_id, "CVE-2026-0002");
    assert_eq!(traverse.summary, "@babel/traverse is vulnerable");
    assert_eq!(traverse.vulnerable_range, "< 2.0.0");
    assert_eq!(traverse.fixed_version, "2.0.0");
    assert!(traverse.html_url.ends_with("/security/dependabot/2"));
    assert!(traverse.created_at.is_some());
    assert_eq!(
        traverse.fix_pull_request, 41,
        "the scoped npm name matched its branch"
    );
    let by_number: HashMap<u64, &DependabotAlert> =
        answer.open_alerts.iter().map(|a| (a.number, a)).collect();
    assert_eq!(
        by_number[&3].ecosystem, "cargo",
        "GitHub's \"rust\" in the platform's word"
    );
    assert_eq!(by_number[&3].fix_pull_request, 42);
    assert_eq!(by_number[&4].fix_pull_request, 0);
    assert_eq!(
        by_number[&1].fix_pull_request, 0,
        "a person's branch that names the package is not Dependabot's fix"
    );
    // Two passes, the worse severities first; an alert both passes return
    // (this fake does not filter) is listed once.
    let alerts_asked = fake.seen("/repos/acme/ok/dependabot/alerts");
    assert_eq!(alerts_asked.len(), 2);
    assert_eq!(
        alerts_asked[0].target,
        "/repos/acme/ok/dependabot/alerts?state=open&per_page=100&severity=critical%2Chigh"
    );
    assert_eq!(
        alerts_asked[1].target,
        "/repos/acme/ok/dependabot/alerts?state=open&per_page=100&severity=medium%2Clow"
    );

    // Five minutes of memory: the same answer, GitHub not asked.
    let asked = fake.count("/repos/acme/ok");
    let cached = h.security(&ok, false).await;
    assert_eq!(cached, answer);
    assert_eq!(fake.count("/repos/acme/ok"), asked);
    assert_eq!(h.state.cached_security_answers(), 1);
    // `refresh` asks again.
    fake.json(
        "/repos/acme/ok/dependabot/alerts",
        json!([alert(9, ("npm", "ws"), "package-lock.json", "high", 12)]),
    );
    assert_eq!(h.security(&ok, false).await.open_alerts.len(), 5);
    let fresh = h.security(&ok, true).await;
    assert!(fake.count("/repos/acme/ok") > asked);
    assert_eq!(fresh.open_alerts.len(), 1);
    assert_eq!(fresh.open_alerts[0].number, 9);
    // And what `refresh` read is what is remembered from then on.
    assert_eq!(h.security(&ok, false).await, fresh);

    // --- Alerts switched off ----------------------------------------------
    let off = h.project("acme/off").await;
    fake.json("/repos/acme/off/pulls", json!([]));
    fake.error(
        "/repos/acme/off/dependabot/alerts",
        403,
        "Dependabot alerts are disabled for this repository.",
    );
    // vulnerability-alerts, automated-security-fixes and both config paths
    // are 404 here, as GitHub answers for a repository with none of it.
    let answer = h.security(&off, false).await;
    assert_eq!(answer.alerts_status, "disabled");
    assert_eq!(
        answer.alerts_message,
        "Dependabot alerts are turned off for this repository."
    );
    assert_eq!(answer.alerts, "disabled");
    assert_eq!(
        answer.security_updates, "unknown",
        "only a 200 says anything"
    );
    assert!(!answer.config_present);
    assert_eq!(answer.config_path, "");
    assert!(answer.config_ecosystems.is_empty());
    assert!(answer.open_alerts.is_empty());
    assert_eq!(answer.open_pull_requests, 0);
    // Both spellings of the configuration were looked for.
    assert_eq!(
        fake.count("/repos/acme/off/contents/.github/dependabot.yml"),
        1
    );
    assert_eq!(
        fake.count("/repos/acme/off/contents/.github/dependabot.yaml"),
        1
    );

    // --- A token that may not read alerts ---------------------------------
    let locked = h.project("acme/locked").await;
    fake.json("/repos/acme/locked/pulls", json!([]));
    fake.set("/repos/acme/locked/vulnerability-alerts", 204, &[], "");
    fake.json(
        "/repos/acme/locked/automated-security-fixes",
        json!({"enabled": false, "paused": false}),
    );
    fake.error(
        "/repos/acme/locked/dependabot/alerts",
        403,
        "Resource not accessible by integration",
    );
    let answer = h.security(&locked, false).await;
    assert_eq!(answer.alerts_status, "forbidden");
    assert!(
        answer.alerts_message.contains("acme/locked"),
        "{}",
        answer.alerts_message
    );
    assert!(
        answer
            .alerts_message
            .contains("`repo` or `security_events`")
            && answer
                .alerts_message
                .contains("\"Dependabot alerts: read\""),
        "{}",
        answer.alerts_message
    );
    assert!(!answer.alerts_message.contains(TOKEN));
    assert_eq!(answer.alerts, "enabled", "the switch itself was readable");
    assert_eq!(answer.security_updates, "disabled");
    assert!(answer.open_alerts.is_empty());
    // A rejected token is the same status, said differently.
    fake.error(
        "/repos/acme/locked/dependabot/alerts",
        401,
        "Bad credentials",
    );
    let answer = h.security(&locked, true).await;
    assert_eq!(answer.alerts_status, "forbidden");
    assert!(
        answer.alerts_message.contains("rejected"),
        "{}",
        answer.alerts_message
    );
    assert!(!answer.alerts_message.contains(TOKEN));

    // --- GitHub having a bad moment ----------------------------------------
    let down = h.project("acme/down").await;
    fake.json("/repos/acme/down/pulls", json!([]));
    fake.set("/repos/acme/down/vulnerability-alerts", 204, &[], "");
    fake.error("/repos/acme/down/dependabot/alerts", 502, "Server Error");
    let answer = h.security(&down, false).await;
    assert_eq!(answer.alerts_status, "unavailable");
    assert!(
        answer.alerts_message.contains("502"),
        "{}",
        answer.alerts_message
    );
    assert!(
        answer.alerts_message.contains("Server Error"),
        "{}",
        answer.alerts_message
    );
    assert!(!answer.alerts_message.contains(TOKEN));
    assert!(answer.open_alerts.is_empty());
    // An archived repository: GitHub's own words, not a permission hint.
    fake.error(
        "/repos/acme/down/dependabot/alerts",
        403,
        "Dependabot alerts are not available for archived repositories.",
    );
    let answer = h.security(&down, true).await;
    assert_eq!(answer.alerts_status, "unavailable");
    assert!(
        answer.alerts_message.contains("archived repositories"),
        "{}",
        answer.alerts_message
    );

    // --- More alerts than are read -----------------------------------------
    let many = h.project("acme/many").await;
    fake.json("/repos/acme/many/pulls", json!([]));
    // Only the `.yaml` spelling exists here.
    fake.json(
        "/repos/acme/many/contents/.github/dependabot.yaml",
        contents("version: 2\nupdates:\n  - package-ecosystem: pip\n    directory: /\n"),
    );
    fake.set("/repos/acme/many/vulnerability-alerts", 204, &[], "");
    fake.json(
        "/repos/acme/many/automated-security-fixes",
        json!({"enabled": true, "paused": true}),
    );
    // What GitHub returns for the first pass (`severity=critical,high`).
    let severities = ["high", "critical"];
    let page = |first: u64| -> Value {
        Value::Array(
            (first..first + 100)
                .map(|n| {
                    alert(
                        n,
                        ("pip", &format!("package-{n}")),
                        "requirements.txt",
                        severities[(n % 2) as usize],
                        1 + (n % 28) as u32,
                    )
                })
                .collect(),
        )
    };
    let worst = "severity=critical%2Chigh";
    let rest = "severity=medium%2Clow";
    let base = format!("/repos/acme/many/dependabot/alerts?state=open&per_page=100&{worst}");
    let base = base.as_str();
    // GitHub's cursors: opaque, percent-encoded, in a URL that names the
    // repository by id on another host. Only the cursor is followed.
    let next = |cursor: &str| {
        format!(
            "<https://api.github.example/repositories/9/dependabot/alerts?state=open&per_page=100&severity=critical%2Chigh&after={cursor}>; rel=\"next\""
        )
    };
    let (to_two, to_three, to_four) = (
        next("Y3Vyc29yOjI%3D"),
        next("Y3Vyc29yOjM%3D"),
        next("Y3Vyc29yOjQ%3D"),
    );
    fake.set(base, 200, &[("link", to_two.as_str())], page(1).to_string());
    fake.set(
        &format!("{base}&after=Y3Vyc29yOjI%3D"),
        200,
        &[("link", to_three.as_str())],
        page(101).to_string(),
    );
    fake.set(
        &format!("{base}&after=Y3Vyc29yOjM%3D"),
        200,
        &[("link", to_four.as_str())],
        page(201).to_string(),
    );
    fake.set(
        &format!("{base}&after=Y3Vyc29yOjQ%3D"),
        200,
        &[],
        page(301).to_string(),
    );
    let answer = h.security(&many, false).await;
    assert_eq!(answer.alerts_status, "ok");
    assert_eq!(answer.open_alerts.len(), 300);
    assert!(answer.truncated, "a fourth page exists and was not read");
    assert_eq!(
        fake.statuses("/repos/acme/many/dependabot/alerts"),
        [200, 200, 200],
        "three pages of the worst, never the fourth, never the lower severities"
    );
    assert!(answer.open_alerts.iter().all(|a| a.number <= 300));
    let rank = |severity: &str| severities_rank(severity);
    assert!(
        answer
            .open_alerts
            .windows(2)
            .all(|pair| rank(&pair[0].severity) <= rank(&pair[1].severity)),
        "most severe first across all three pages"
    );
    assert_eq!(answer.open_alerts[0].severity, "critical");
    assert_eq!(answer.open_alerts[299].severity, "high");
    assert_eq!(answer.config_path, ".github/dependabot.yaml");
    assert_eq!(answer.config_ecosystems, ["pip"]);
    assert_eq!(answer.security_updates, "paused");

    // Two pages and no more: complete, not truncated.
    let two = h.project("acme/two").await;
    fake.json("/repos/acme/two/pulls", json!([]));
    let base = format!("/repos/acme/two/dependabot/alerts?state=open&per_page=100&{worst}");
    let base = base.as_str();
    // Nothing of the lower severities.
    fake.json(
        &format!("/repos/acme/two/dependabot/alerts?state=open&per_page=100&{rest}"),
        json!([]),
    );
    let to_two = next("cGFnZTI");
    fake.set(base, 200, &[("link", to_two.as_str())], page(1).to_string());
    fake.json(
        &format!("{base}&after=cGFnZTI"),
        json!([alert(
            101,
            ("npm", "ws"),
            "package-lock.json",
            "critical",
            9
        )]),
    );
    let answer = h.security(&two, false).await;
    assert_eq!(answer.open_alerts.len(), 101);
    assert!(!answer.truncated);
    assert_eq!(answer.open_alerts[0].severity, "critical");

    // Exactly as many of the worst as the limit holds, and GitHub has no
    // further page of them. Whether the list is complete depends on the
    // lower severities: one probe (a page of one) answers that, and its
    // alert is never listed.
    let full = h.project("acme/full").await;
    fake.json("/repos/acme/full/pulls", json!([]));
    let base = format!("/repos/acme/full/dependabot/alerts?state=open&per_page=100&{worst}");
    let base = base.as_str();
    let (to_two, to_three) = (next("ZnVsbDI"), next("ZnVsbDM"));
    fake.set(base, 200, &[("link", to_two.as_str())], page(1).to_string());
    fake.set(
        &format!("{base}&after=ZnVsbDI"),
        200,
        &[("link", to_three.as_str())],
        page(101).to_string(),
    );
    fake.set(
        &format!("{base}&after=ZnVsbDM"),
        200,
        &[],
        page(201).to_string(),
    );
    let probe = format!("/repos/acme/full/dependabot/alerts?state=open&per_page=1&{rest}");
    fake.json(
        &probe,
        json!([alert(
            900,
            ("npm", "left-pad"),
            "package-lock.json",
            "low",
            2
        )]),
    );
    let answer = h.security(&full, false).await;
    assert_eq!(answer.open_alerts.len(), 300);
    assert!(
        answer.truncated,
        "lower severities exist and were not listed"
    );
    assert!(answer.open_alerts.iter().all(|a| a.number != 900));
    assert_eq!(fake.exact(&probe).len(), 1);
    // The same repository with nothing below high: complete.
    fake.json(&probe, json!([]));
    let answer = h.security(&full, true).await;
    assert_eq!(answer.open_alerts.len(), 300);
    assert!(!answer.truncated);

    // --- A repository this token cannot see at all -------------------------
    // Everything is 404. The last scan's row stands for what GitHub will
    // not say, and "404" is not read as "alerts are off".
    let gone = h.project("acme/gone").await;
    db::dependencies::upsert_dependabot(
        &h.pool,
        h.workspace,
        gone.id,
        &DependabotDraft {
            configured: true,
            config_path: ".github/dependabot.yml".into(),
            ecosystems: vec!["npm".into(), "npm".into(), "docker".into()],
        },
        2,
    )
    .await
    .unwrap();
    let answer = h.security(&gone, false).await;
    assert!(answer.config_present);
    assert_eq!(answer.config_path, ".github/dependabot.yml");
    assert_eq!(answer.config_ecosystems, ["npm", "docker"]);
    assert_eq!(answer.open_pull_requests, 2);
    assert_eq!(answer.alerts, "unknown");
    assert_eq!(answer.security_updates, "unknown");
    assert_eq!(answer.alerts_status, "forbidden");
    assert!(
        answer.alerts_message.contains("acme/gone"),
        "{}",
        answer.alerts_message
    );

    // --- A workspace with no GitHub access at all ---------------------------
    // Still an answer, never an error: the sentence says what is missing.
    let other_workspace = Uuid::now_v7();
    let orphan = h.project_in(other_workspace, "acme/orphan").await;
    let answer = h
        .state
        .get_repository_security(request_in(
            other_workspace,
            GetRepositorySecurityRequest {
                project_id: orphan.id.to_string(),
                refresh: false,
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(answer.alerts_status, "unavailable");
    assert!(
        answer
            .alerts_message
            .contains("GitHub access is not configured"),
        "{}",
        answer.alerts_message
    );
    assert_eq!(answer.alerts, "unknown");
    assert_eq!(fake.count("/repos/acme/orphan"), 0);

    // --- Only an unknown project is an error --------------------------------
    for (workspace, project_id) in [
        (h.workspace, Uuid::now_v7().to_string()),
        // Another workspace's project is as unknown as a random id.
        (Uuid::now_v7(), ok.id.to_string()),
    ] {
        let error = h
            .state
            .get_repository_security(request_in(
                workspace,
                GetRepositorySecurityRequest {
                    project_id,
                    refresh: false,
                },
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::NotFound);
    }
    let malformed = h
        .state
        .get_repository_security(h.request(GetRepositorySecurityRequest {
            project_id: "not-an-id".into(),
            refresh: false,
        }))
        .await
        .unwrap_err();
    assert_eq!(malformed.code(), Code::InvalidArgument);

    // Disconnecting a project forgets its answer.
    let held = h.state.cached_security_answers();
    let mut delete = request_in(
        h.workspace,
        DeleteProjectRequest {
            id: ok.id.to_string(),
        },
    );
    Caller {
        user_id: "u1".to_owned(),
        workspace_id: h.workspace,
        role: Role::Member,
    }
    .principal()
    .apply(&mut delete);
    h.state.delete_project(delete).await.unwrap();
    assert_eq!(h.state.cached_security_answers(), held - 1);

    // No answer of this test ever carried the token.
    assert!(fake.seen("/repos/").iter().all(|s| s.authorized));

    h.finish().await;
}

fn severities_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        other => panic!("unexpected severity {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// No database needed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_new_rpcs_refuse_a_call_without_a_principal() {
    // A pool that never connects: the principal is checked before anything
    // touches the database or GitHub.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://nobody@127.0.0.1:1/none")
        .unwrap();
    let identity = Identity::disabled();
    let github = Github::new(TokenProvider::new(identity.clone(), None)).unwrap();
    let state = KnowledgeState::new(pool, github, EventBus::new(), identity);
    let project_id = Uuid::now_v7().to_string();
    assert_eq!(
        state
            .get_pull_request_status(Request::new(GetPullRequestStatusRequest {
                project_id: project_id.clone(),
                number: 1,
            }))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    assert_eq!(
        state
            .get_repository_security(Request::new(GetRepositorySecurityRequest {
                project_id: project_id.clone(),
                refresh: true,
            }))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    assert_eq!(
        state
            .get_repo_file(Request::new(GetRepoFileRequest {
                project_id,
                path: "src/main.rs".into(),
                r#ref: sha('a'),
            }))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    assert_eq!(state.cached_security_answers(), 0);
    assert_eq!(state.pull_sync_marks(), 0);
}

fn bound_client(base: &str, cache: bool) -> (Github, sdi_knowledge::github::GithubClient) {
    let hub = Github::new(TokenProvider::new(Identity::disabled(), None)).unwrap();
    let mut access = GithubAccess::dev(TOKEN);
    access.api_base_url = base.to_owned();
    let client = hub.bind(Uuid::now_v7(), access, cache);
    (hub, client)
}

#[tokio::test]
async fn a_polled_listing_is_conditional_slim_and_fingerprinted() {
    let fake = Arc::new(FakeGithub::default());
    let base = format!("http://{}", serve_github(Arc::clone(&fake)).await);
    // Fifty pull requests padded the way GitHub pads them: each carries
    // kilobytes this client never reads.
    let padding = "x".repeat(8 * 1024);
    let listing = |title: &str| {
        Value::Array(
            (1..=50)
                .map(|n| {
                    pull(
                        n,
                        ("ana", "User"),
                        ("feat/x", &sha('1'), Some("acme/big")),
                        json!({"title": format!("{title} {n}"), "_padding": padding}),
                    )
                })
                .collect(),
        )
    };
    let body_len = listing("Change").to_string().len();
    fake.json_etag("/repos/acme/big/pulls", "\"v1\"", listing("Change"));
    // Exactly fifty were asked for and fifty came: no second page.
    let (hub, client) = bound_client(&base, true);

    let first = client
        .pulls_listing("acme", "big", "open", 50)
        .await
        .unwrap();
    assert_eq!(first.pulls.len(), 50);
    assert!(!first.not_modified);
    let (entries, bytes) = hub.slim_cached();
    assert_eq!(entries, 1);
    assert!(
        bytes * 10 < body_len,
        "{bytes} bytes kept of a {body_len}-byte answer"
    );

    let second = client
        .pulls_listing("acme", "big", "open", 50)
        .await
        .unwrap();
    assert!(second.not_modified, "GitHub said 304");
    assert_eq!(second.fingerprint, first.fingerprint);
    assert_eq!(second.pulls.len(), 50);
    assert_eq!(second.pulls[49].title, "Change 50");
    assert_eq!(fake.statuses("/repos/acme/big/pulls"), [200, 304]);

    // A change is a new answer with a new fingerprint.
    fake.json_etag("/repos/acme/big/pulls", "\"v2\"", listing("Changed"));
    let third = client
        .pulls_listing("acme", "big", "open", 50)
        .await
        .unwrap();
    assert!(!third.not_modified);
    assert_ne!(third.fingerprint, first.fingerprint);
    assert_eq!(third.pulls[0].title, "Changed 1");

    // The same content under a new ETag (a token rotated, say) is a full
    // read with the fingerprint it had: nothing for a poller to write.
    fake.json_etag("/repos/acme/big/pulls", "\"v3\"", listing("Changed"));
    let fourth = client
        .pulls_listing("acme", "big", "open", 50)
        .await
        .unwrap();
    assert!(!fourth.not_modified);
    assert_eq!(fourth.fingerprint, third.fingerprint);

    // A workspace that turned the GitHub cache off sends no conditions and
    // keeps nothing.
    let (hub, uncached) = bound_client(&base, false);
    for _ in 0..2 {
        let listing = uncached
            .pulls_listing("acme", "big", "open", 50)
            .await
            .unwrap();
        assert!(!listing.not_modified);
        assert_eq!(listing.fingerprint, third.fingerprint);
    }
    assert_eq!(hub.slim_cached(), (0, 0));
    let last_two: Vec<Seen> = fake
        .seen("/repos/acme/big/pulls")
        .into_iter()
        .rev()
        .take(2)
        .collect();
    assert!(last_two.iter().all(|s| !s.conditional && s.status == 200));
}

#[tokio::test]
async fn client_reads_are_bounded_and_say_when_a_list_goes_on() {
    let fake = Arc::new(FakeGithub::default());
    let base = format!("http://{}", serve_github(Arc::clone(&fake)).await);
    let (_hub, client) = bound_client(&base, true);
    // A comparison whose file list is larger than any JSON this client
    // accepts.
    let huge = format!(
        "{{\"behind_by\": 4, \"files\": \"{}\"}}",
        "x".repeat(sdi_knowledge::github::MAX_JSON_BYTES)
    );
    fake.set("/repos/acme/big/compare/main...abc", 200, &[], huge);
    let error = client
        .behind_by("acme", "big", "main", "abc")
        .await
        .unwrap_err();
    assert!(matches!(error, GithubError::Decode(_)), "{error}");
    assert!(error.to_string().contains("larger than"), "{error}");

    // An ordinary one, with everything a comparison carries, is just a number.
    fake.json(
        "/repos/acme/big/compare/main...def",
        json!({"ahead_by": 2, "behind_by": 4, "commits": [{"sha": "x"}], "files": [{"patch": "@@"}]}),
    );
    assert_eq!(
        client
            .behind_by("acme", "big", "main", "def")
            .await
            .unwrap(),
        4
    );
    // The comparison asks for one commit, not GitHub's default page.
    assert!(
        fake.seen("/repos/acme/big/compare/main...def")[0]
            .target
            .ends_with("?per_page=1")
    );

    // Reviews are read page by page up to the cap, and a list that goes on
    // past it says so instead of passing for complete.
    let hundred = |from: u32| {
        Value::Array(
            (from..from + 100)
                .map(|i| review(&format!("r{i}"), "APPROVED", Some(i % 60)))
                .collect(),
        )
    };
    fake.json("/repos/acme/big/pulls/1/reviews", hundred(0));
    let reviews = client.pull_reviews("acme", "big", 1).await.unwrap();
    let pages = sdi_knowledge::github::MAX_STATUS_PAGES as usize;
    assert_eq!(reviews.items.len(), 100 * pages);
    assert!(reviews.more);
    assert_eq!(fake.count("/repos/acme/big/pulls/1/reviews"), pages);
    // Two pages, the second short: complete.
    fake.remove("/repos/acme/big/pulls/1/reviews");
    fake.json(
        "/repos/acme/big/pulls/2/reviews?per_page=100&page=1",
        hundred(0),
    );
    fake.json(
        "/repos/acme/big/pulls/2/reviews?per_page=100&page=2",
        json!([review("late", "CHANGES_REQUESTED", Some(59))]),
    );
    let reviews = client.pull_reviews("acme", "big", 2).await.unwrap();
    assert_eq!(reviews.items.len(), 101);
    assert!(!reviews.more);

    // Check runs follow GitHub's count: exactly a hundred of a hundred is
    // one request, a hundred and fifty is two.
    let runs = |from: i64, count: i64, total: u64| {
        json!({"total_count": total, "check_runs": (from..from + count)
            .map(|id| check_run(id, &format!("job-{id}"), "completed", Some("success"), 1))
            .collect::<Vec<_>>()})
    };
    fake.json("/repos/acme/big/commits/aaa/check-runs", runs(0, 100, 100));
    let listed = client.check_runs("acme", "big", "aaa").await.unwrap();
    assert_eq!((listed.items.len(), listed.more), (100, false));
    assert_eq!(fake.count("/repos/acme/big/commits/aaa/check-runs"), 1);
    fake.json(
        "/repos/acme/big/commits/bbb/check-runs?per_page=100&page=1",
        runs(0, 100, 150),
    );
    fake.json(
        "/repos/acme/big/commits/bbb/check-runs?per_page=100&page=2",
        runs(100, 50, 150),
    );
    let listed = client.check_runs("acme", "big", "bbb").await.unwrap();
    assert_eq!((listed.items.len(), listed.more), (150, false));
    // More than the cap reads: reported, not hidden.
    fake.json("/repos/acme/big/commits/ccc/check-runs", runs(0, 100, 900));
    let listed = client.check_runs("acme", "big", "ccc").await.unwrap();
    assert_eq!((listed.items.len(), listed.more), (100 * pages, true));
    let statuses = json!({"state": "success", "total_count": 2, "statuses": [
        {"context": "a", "state": "success"}, {"context": "b", "state": "pending"}]});
    fake.json("/repos/acme/big/commits/aaa/status", statuses);
    let listed = client.commit_statuses("acme", "big", "aaa").await.unwrap();
    assert_eq!((listed.items.len(), listed.more), (2, false));
}
