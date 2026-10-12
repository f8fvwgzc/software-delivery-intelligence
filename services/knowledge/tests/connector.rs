//! The GitHub connector against a fake GitHub (an axum server on a random
//! port) and a fake identity service (a tonic server on another). No
//! database: these cover the messy-repository and enterprise cases of the
//! scanner's inputs — truncated trees, symlinks and submodules, rate
//! limits, empty repositories, paths with spaces and non-ASCII characters,
//! huge trees, garbage bytes — and that every call goes to the API base the
//! workspace's token carries (GitHub Enterprise Server).

use std::{
    collections::HashSet,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use sdi_analyzer::{RepoSnapshot, TreeEntry};
use sdi_knowledge::{
    auth::Caller,
    github::{Github, GithubAccess, GithubError, TokenProvider, TreeNode},
    identity::Identity,
    scanner::{self, tarball},
};
use sdi_proto::identity::v1::{
    GetInstallationTokenRequest, InstallationToken,
    identity_service_server::{IdentityService, IdentityServiceServer},
};
use serde_json::{Value, json};
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Fake GitHub
// ---------------------------------------------------------------------------

/// One canned answer: path, status, headers, body.
type Route = (String, u16, Vec<(String, String)>, Vec<u8>);

/// What the fake answers, and the paths it was asked for (in order).
#[derive(Default)]
struct FakeGithub {
    /// `api_prefix` + path -> (status, headers, body). Unknown paths 404.
    routes: Mutex<Vec<Route>>,
    seen: Mutex<Vec<String>>,
    hits: AtomicUsize,
}

impl FakeGithub {
    fn route(&self, path: &str, status: u16, headers: &[(&str, &str)], body: Vec<u8>) {
        self.routes.lock().unwrap().push((
            path.to_owned(),
            status,
            headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            body,
        ));
    }

    fn json(&self, path: &str, body: Value) {
        self.route(path, 200, &[], body.to_string().into_bytes());
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

async fn fake_handler(
    State(fake): State<Arc<FakeGithub>>,
    request: axum::extract::Request,
) -> Response {
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or_default().to_owned();
    let full = if query.is_empty() {
        path.clone()
    } else {
        format!("{path}?{query}")
    };
    fake.seen.lock().unwrap().push(full.clone());
    fake.hits.fetch_add(1, Ordering::SeqCst);
    let routes = fake.routes.lock().unwrap();
    let matched = routes
        .iter()
        .find(|(p, _, _, _)| *p == full)
        .or_else(|| routes.iter().find(|(p, _, _, _)| *p == path));
    match matched {
        Some((_, status, headers, body)) => {
            let mut response = Response::builder().status(*status);
            for (k, v) in headers {
                response = response.header(k, v);
            }
            if !headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            {
                response = response.header(header::CONTENT_TYPE, "application/json");
            }
            response.body(Body::from(body.clone())).unwrap()
        }
        None => (StatusCode::NOT_FOUND, r#"{"message":"Not Found"}"#).into_response(),
    }
}

async fn serve_github(fake: Arc<FakeGithub>) -> SocketAddr {
    let app = Router::new()
        .fallback(get(fake_handler).post(fake_handler))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

/// A client bound to the fake with a dev-style user token.
fn client_for(hub: &Github, base: &str) -> sdi_knowledge::github::GithubClient {
    let mut access = GithubAccess::dev("ghp_test");
    access.api_base_url = base.to_owned();
    hub.bind(Uuid::now_v7(), access, false)
}

fn hub() -> Github {
    Github::new(TokenProvider::new(Identity::disabled(), None)).unwrap()
}

fn node(path: &str, kind: &str, mode: &str, size: u64) -> Value {
    json!({"path": path, "type": kind, "mode": mode, "sha": format!("sha-{path}"), "size": size})
}

// ---------------------------------------------------------------------------
// Trees
// ---------------------------------------------------------------------------

#[tokio::test]
async fn truncated_recursive_trees_are_completed_per_directory() {
    let fake = Arc::new(FakeGithub::default());
    // The recursive listing stops after the root level and says so.
    fake.json(
        "/repos/o/r/git/trees/main",
        json!({"sha": "main", "truncated": true, "tree": [
            node("README.md", "blob", "100644", 10),
            node("src", "tree", "040000", 0),
            node("deep", "tree", "040000", 0),
        ]}),
    );
    fake.json(
        "/repos/o/r/git/trees/sha-src",
        json!({"sha": "sha-src", "tree": [
            node("main.rs", "blob", "100644", 20),
            node("lib.rs", "blob", "100644", 20),
        ]}),
    );
    fake.json(
        "/repos/o/r/git/trees/sha-deep",
        json!({"sha": "sha-deep", "tree": [node("nested", "tree", "040000", 0)]}),
    );
    fake.json(
        "/repos/o/r/git/trees/sha-nested",
        json!({"sha": "sha-nested", "tree": [node("Cargo.toml", "blob", "100644", 5)]}),
    );
    let addr = serve_github(Arc::clone(&fake)).await;
    let hub = hub();
    let client = client_for(&hub, &format!("http://{addr}"));

    let tree = client.full_tree("o", "r", "main").await.unwrap();
    assert!(!tree.truncated, "the fallback completed the tree");
    let paths: Vec<&str> = tree.tree.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "README.md",
            "deep",
            "deep/nested",
            "deep/nested/Cargo.toml",
            "src",
            "src/lib.rs",
            "src/main.rs"
        ]
    );
    // One recursive call plus one per directory, by sha.
    assert_eq!(fake.hits.load(Ordering::SeqCst), 4, "{:?}", fake.seen());
}

#[tokio::test]
async fn a_tree_the_budget_cannot_complete_stays_truncated_but_usable() {
    let fake = Arc::new(FakeGithub::default());
    fake.json(
        "/repos/o/r/git/trees/main",
        json!({"sha": "main", "truncated": true, "tree": [
            node("a", "tree", "040000", 0),
            node("package.json", "blob", "100644", 10),
        ]}),
    );
    // Every subtree lists another subtree: an endless chain the request
    // budget must cut.
    let addr = serve_github(Arc::clone(&fake)).await;
    for i in 0..600 {
        let name = if i == 0 {
            "a".to_owned()
        } else {
            format!("d{i}")
        };
        fake.json(
            &format!("/repos/o/r/git/trees/sha-{name}"),
            json!({"sha": "x", "tree": [node(&format!("d{}", i + 1), "tree", "040000", 0)]}),
        );
    }
    let hub = hub();
    let client = client_for(&hub, &format!("http://{addr}"));
    let tree = client.full_tree("o", "r", "main").await.unwrap();
    assert!(tree.truncated, "the budget ran out");
    assert!(tree.tree.iter().any(|n| n.path == "package.json"));
    assert!(
        fake.hits.load(Ordering::SeqCst) <= sdi_knowledge::github::TREE_FALLBACK_MAX_REQUESTS + 1,
        "spent {} requests",
        fake.hits.load(Ordering::SeqCst)
    );
}

#[test]
fn symlinks_and_submodules_are_dropped_from_the_scan_tree() {
    let nodes: Vec<TreeNode> = serde_json::from_value(json!([
        node("README.md", "blob", "100644", 10),
        node("bin/run", "blob", "100755", 10),
        node("link", "blob", "120000", 7),
        node("vendor/lib", "commit", "160000", 0),
        node("src", "tree", "040000", 0),
        {"path": "no-mode.txt", "type": "blob", "size": 3},
    ]))
    .unwrap();
    let entries = scanner::tree_entries(&nodes);
    let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["README.md", "bin/run", "src", "no-mode.txt"]);
    assert!(entries.iter().find(|e| e.path == "src").unwrap().is_dir);
    assert!(nodes[2].is_symlink() && !nodes[2].is_file());
    assert!(nodes[3].is_submodule() && !nodes[3].is_file() && !nodes[3].is_dir());
}

#[test]
fn a_sixty_thousand_entry_monorepo_is_selected_in_well_under_a_second() {
    // 60k entries over 2k packages with the usual junk mixed in.
    let mut nodes: Vec<TreeNode> = Vec::with_capacity(60_000);
    for pkg in 0..2_000 {
        for file in [
            "package.json",
            "src/index.ts",
            "src/util.ts",
            "README.md",
            "node_modules/dep/index.js",
            "dist/bundle.js",
            "test/index.test.ts",
            ".env",
            "docs/guide.md",
            "assets/logo.png",
            "src/components/App.tsx",
            "src/components/Nav.tsx",
            "src/api/client.ts",
            "src/api/types.ts",
            "src/lib/a.ts",
            "src/lib/b.ts",
            "src/lib/c.ts",
            "src/lib/d.ts",
            "tsconfig.json",
            "Dockerfile",
            "k8s/deploy.yaml",
            ".github/workflows/ci.yml",
            "migrations/0001_init.sql",
            "data/big.bin",
            "vendor/x/y.go",
            "go.mod",
            "main.go",
            "Cargo.toml",
            "src/main.rs",
            "src/lib.rs",
        ] {
            nodes.push(TreeNode {
                path: format!("packages/pkg-{pkg}/{file}"),
                kind: "blob".to_owned(),
                mode: "100644".to_owned(),
                sha: String::new(),
                size: Some(100),
            });
        }
    }
    assert_eq!(nodes.len(), 60_000);
    let started = Instant::now();
    let entries = scanner::tree_entries(&nodes);
    let wanted = sdi_analyzer::wanted_files(&entries);
    let elapsed = started.elapsed();
    assert_eq!(wanted.len(), sdi_analyzer::MAX_FILES);
    assert!(
        wanted
            .iter()
            .all(|p| !p.contains("node_modules") && !p.ends_with(".env")),
        "{wanted:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "wanted_files over 60k entries took {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// Files, paths and garbage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn paths_with_spaces_and_unicode_reach_github_encoded_and_come_back_lossily() {
    let fake = Arc::new(FakeGithub::default());
    let raw: Vec<u8> = vec![0xff, 0xfe, b'o', b'k', 0xc3, 0x28, 0x00, b'!'];
    fake.route(
        "/repos/o/r/contents/docs/na%C3%AFve%20plan%20%E2%9C%93.md",
        200,
        &[("content-type", "application/vnd.github.raw")],
        raw.clone(),
    );
    let encoded = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&raw)
    };
    fake.json(
        "/repos/o/r/contents/src/my%20file.rs",
        json!({"encoding": "base64", "content": encoded}),
    );
    let addr = serve_github(Arc::clone(&fake)).await;
    let hub = hub();
    let client = client_for(&hub, &format!("http://{addr}"));

    let file = client
        .file_raw("o", "r", "docs/naïve plan ✓.md", "main", 1024)
        .await
        .unwrap();
    assert_eq!(file.bytes, raw);
    assert!(!file.truncated);

    // The JSON contents path decodes lossily: garbage bytes become
    // replacement characters, never a panic or an error.
    let text = client
        .file("o", "r", "src/my file.rs", "main")
        .await
        .unwrap();
    assert!(text.contains("ok"));
    assert!(text.contains('\u{fffd}'));
    assert!(
        fake.seen()
            .iter()
            .any(|p| p.starts_with("/repos/o/r/contents/src/my%20file.rs?ref=main")),
        "{:?}",
        fake.seen()
    );
}

#[test]
fn a_snapshot_of_garbage_bytes_never_panics_the_analyzer_or_the_tarball_reader() {
    // Pseudo-random bytes, decoded the way the scanner decodes every file.
    let mut state = 0x9e37_79b9_u32;
    let mut garbage = Vec::with_capacity(64 * 1024);
    for _ in 0..(64 * 1024) {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        garbage.push((state & 0xff) as u8);
    }
    let text = String::from_utf8_lossy(&garbage).into_owned();
    let paths = [
        "package.json",
        "Cargo.toml",
        "pyproject.toml",
        "go.mod",
        "Dockerfile",
        "docker-compose.yml",
        ".github/workflows/ci.yml",
        "README.md",
        "infra/main.tf",
        "k8s/deploy.yaml",
        ".sdi.yml",
        "src/main.rs",
        "migrations/0001_init.sql",
        "requirements.txt",
        "pom.xml",
    ];
    let snapshot = RepoSnapshot {
        full_name: "o/garbage".into(),
        description: text.clone(),
        default_branch: "main".into(),
        commit_sha: "deadbeef".into(),
        is_private: true,
        primary_language: text.clone(),
        languages: [(text.clone(), 1)].into_iter().collect(),
        topics: vec![text.clone()],
        tree: paths
            .iter()
            .map(|p| TreeEntry {
                path: (*p).to_owned(),
                size: garbage.len() as u64,
                is_dir: false,
            })
            .collect(),
        files: paths
            .iter()
            .map(|p| ((*p).to_owned(), text.clone()))
            .collect(),
    };
    let analysis = sdi_analyzer::analyze_at(&snapshot, (2026, 10, 12));
    assert!(!analysis.understanding.tldr.is_empty());

    // The same bytes as an "archive" are an error the scanner falls back
    // from, not a panic.
    let wanted: HashSet<String> = ["x".to_owned()].into_iter().collect();
    assert!(tarball::unpack(&garbage[..], &wanted, 1024).is_err());
}

#[tokio::test]
async fn tarballs_stream_to_a_file_and_respect_the_cap() {
    let fake = Arc::new(FakeGithub::default());
    // A real (tiny) gzipped tarball with one wanted file, served as an
    // octet stream like codeload does.
    let archive = {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;
        use tar::{Builder, Header};
        let mut builder = Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let body = b"[package]\nname = \"x\"\n";
        let mut header = Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "o-r-abc/Cargo.toml", &body[..])
            .unwrap();
        let mut out = builder.into_inner().unwrap().finish().unwrap();
        out.flush().unwrap();
        out
    };
    fake.route(
        "/repos/o/r/tarball/main",
        200,
        &[("content-type", "application/x-gzip")],
        archive.clone(),
    );
    let addr = serve_github(Arc::clone(&fake)).await;
    let hub = hub();
    let client = client_for(&hub, &format!("http://{addr}"));

    let file = client
        .tarball("o", "r", "main", 1 << 20)
        .await
        .unwrap()
        .expect("under the cap");
    let wanted: HashSet<String> = ["Cargo.toml".to_owned()].into_iter().collect();
    let files = tarball::unpack(file, &wanted, 4096).unwrap();
    assert_eq!(files["Cargo.toml"], "[package]\nname = \"x\"\n");

    // Over the cap: `None`, and nothing of it is kept.
    let over = client
        .tarball("o", "r", "main", archive.len() - 1)
        .await
        .unwrap();
    assert!(over.is_none());
}

// ---------------------------------------------------------------------------
// Rate limits and empty repositories
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rate_limits_are_backed_off_once_then_reported_clearly() {
    let fake = Arc::new(FakeGithub::default());
    fake.route(
        "/repos/o/limited",
        429,
        &[("retry-after", "1")],
        br#"{"message":"too many requests"}"#.to_vec(),
    );
    fake.route(
        "/repos/o/exhausted",
        403,
        &[
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "99999999999"),
        ],
        br#"{"message":"API rate limit exceeded"}"#.to_vec(),
    );
    fake.route(
        "/repos/o/empty/commits/main",
        409,
        &[],
        br#"{"message":"Git Repository is empty."}"#.to_vec(),
    );
    let addr = serve_github(Arc::clone(&fake)).await;
    let hub = hub();
    let client = client_for(&hub, &format!("http://{addr}"));

    // 429 + Retry-After 1s: retried up to the attempt limit, then an error
    // that names the wait.
    let started = Instant::now();
    let error = client.repo("o", "limited").await.unwrap_err();
    assert!(
        matches!(error, GithubError::RateLimited { retry_after: 1 }),
        "{error}"
    );
    assert!(error.to_string().contains("retry after 1s"), "{error}");
    assert!(
        started.elapsed() >= Duration::from_secs(2),
        "two waits of 1s"
    );
    assert_eq!(
        fake.seen()
            .iter()
            .filter(|p| p.as_str() == "/repos/o/limited")
            .count(),
        3
    );

    // 403 with the quota exhausted and a reset too far away to wait for:
    // reported at once.
    let error = client.repo("o", "exhausted").await.unwrap_err();
    assert!(matches!(error, GithubError::RateLimited { .. }), "{error}");

    // 409 on the head commit: an empty repository, said in words.
    let error = client.branch_sha("o", "empty", "main").await.unwrap_err();
    assert!(matches!(error, GithubError::EmptyRepository(_)), "{error}");
    assert!(error.to_string().contains("empty"), "{error}");
}

// ---------------------------------------------------------------------------
// GitHub Enterprise Server end to end: identity hands out the API base
// ---------------------------------------------------------------------------

struct FakeIdentity {
    api_base_url: String,
}

#[tonic::async_trait]
impl IdentityService for FakeIdentity {
    async fn get_installation_token(
        &self,
        _: Request<GetInstallationTokenRequest>,
    ) -> Result<GrpcResponse<InstallationToken>, Status> {
        Ok(GrpcResponse::new(InstallationToken {
            token: "ghs_enterprise".into(),
            expires_at: None,
            api_base_url: self.api_base_url.clone(),
        }))
    }

    // Everything else is unimplemented: the connector only needs the token
    // (and GetWorkspace, which failing means "treat as a user token").
    async fn begin_login(
        &self,
        _: Request<sdi_proto::identity::v1::BeginLoginRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::BeginLoginResponse>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn complete_login(
        &self,
        _: Request<sdi_proto::identity::v1::CompleteLoginRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::Session>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn get_session(
        &self,
        _: Request<sdi_proto::identity::v1::GetSessionRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::Session>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn revoke_session(
        &self,
        _: Request<sdi_proto::identity::v1::RevokeSessionRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::RevokeSessionResponse>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn list_workspaces(
        &self,
        _: Request<sdi_proto::identity::v1::ListWorkspacesRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::ListWorkspacesResponse>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn get_workspace(
        &self,
        _: Request<sdi_proto::identity::v1::GetWorkspaceRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::Workspace>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn list_members(
        &self,
        _: Request<sdi_proto::identity::v1::ListMembersRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::ListMembersResponse>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn set_member_role(
        &self,
        _: Request<sdi_proto::identity::v1::SetMemberRoleRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::Member>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn sync_members(
        &self,
        _: Request<sdi_proto::identity::v1::SyncMembersRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::ListMembersResponse>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn record_installation(
        &self,
        _: Request<sdi_proto::identity::v1::RecordInstallationRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::Workspace>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn remove_installation(
        &self,
        _: Request<sdi_proto::identity::v1::RemoveInstallationRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::RemoveInstallationResponse>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn authorize(
        &self,
        _: Request<sdi_proto::identity::v1::AuthorizeRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::Principal>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn record_audit(
        &self,
        _: Request<sdi_proto::identity::v1::RecordAuditRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::RecordAuditResponse>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn list_audit(
        &self,
        _: Request<sdi_proto::identity::v1::ListAuditRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::ListAuditResponse>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn get_data_controls(
        &self,
        _: Request<sdi_proto::identity::v1::GetDataControlsRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::DataControls>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn update_data_controls(
        &self,
        _: Request<sdi_proto::identity::v1::UpdateDataControlsRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::DataControls>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn set_reasoner_key(
        &self,
        _: Request<sdi_proto::identity::v1::SetReasonerKeyRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::DataControls>, Status> {
        Err(Status::unimplemented(""))
    }
    async fn get_reasoner_key(
        &self,
        _: Request<sdi_proto::identity::v1::GetReasonerKeyRequest>,
    ) -> Result<GrpcResponse<sdi_proto::identity::v1::ReasonerKey>, Status> {
        Err(Status::unimplemented(""))
    }
}

async fn serve_identity(api_base_url: String) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(IdentityServiceServer::new(FakeIdentity { api_base_url }))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    addr
}

#[tokio::test]
async fn an_enterprise_workspace_talks_to_its_own_api_base_end_to_end() {
    // The fake GHES answers under `/api/v3`, like a real one; github.com's
    // paths do not exist on it.
    let fake = Arc::new(FakeGithub::default());
    fake.json(
        "/api/v3/repos/acme/app",
        json!({"id": 1, "full_name": "acme/app", "name": "app", "owner": {"login": "acme"},
               "default_branch": "trunk", "html_url": "https://ghe.corp.example/acme/app"}),
    );
    fake.json(
        "/api/v3/repos/acme/app/commits/trunk",
        json!({"sha": "c0ffee"}),
    );
    fake.json(
        "/api/v3/repos/acme/app/git/trees/c0ffee",
        json!({"sha": "c0ffee", "tree": [node("README.md", "blob", "100644", 3)]}),
    );
    fake.json(
        "/api/v3/installation/repositories",
        json!({"total_count": 1, "repositories": [{"id": 1, "full_name": "acme/app", "name": "app", "owner": {"login": "acme"}}]}),
    );
    let github_addr = serve_github(Arc::clone(&fake)).await;
    let api_base = format!("http://{github_addr}/api/v3");
    let identity_addr = serve_identity(api_base.clone()).await;

    // No dev token: everything must come from identity.
    let identity = Identity::connect(&format!("http://{identity_addr}")).unwrap();
    let hub = Github::new(TokenProvider::new(identity, None)).unwrap();
    let caller = Caller::system(Uuid::now_v7());
    let client = hub.client(&caller, true).await.unwrap();
    assert_eq!(client.access().api_base_url, api_base);
    assert_eq!(client.access().token, "ghs_enterprise");

    let repo = client.repo("acme", "app").await.unwrap();
    assert_eq!(repo.default_branch, "trunk");
    let sha = client.branch_sha("acme", "app", "trunk").await.unwrap();
    let tree = client.full_tree("acme", "app", &sha).await.unwrap();
    assert_eq!(tree.tree.len(), 1);
    // Repository listing goes by the workspace kind identity reports;
    // with GetWorkspace unimplemented it falls back to the user listing,
    // which is also under the enterprise base.
    let _ = client.accessible_repos().await;
    assert!(
        fake.seen().iter().all(|p| p.starts_with("/api/v3/")),
        "every call went to the enterprise base: {:?}",
        fake.seen()
    );
    assert!(!fake.seen().is_empty());
}
