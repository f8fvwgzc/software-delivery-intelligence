//! Continuous delivery through a whole scan (CONTRACT.md §22): a fake
//! GitHub serves a repository whose `.sdi.yml` declares delivery
//! environments, the real scan pipeline runs against the dev Postgres, and
//! a recording execution client stands in for `ExecutionService`.
//!
//! Covers what the unit tests cannot: that the scan stores the delivery
//! section and the image-build facts, that `SyncEnvironments` is called
//! once the knowledge is stored, as the workspace's system principal, and
//! that neither a removed section nor an execution outage fails a scan.
//!
//! Skipped when `DATABASE_URL` is not set, like the other database tests.

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use sdi_analyzer::DeliveryConfigDraft;
use sdi_common::principal::{Principal, Role, SYSTEM_USER};
use sdi_knowledge::{
    auth::Caller,
    controls::Controls,
    db::{self, projects::NewProject, scans::ScanRow, settings::SettingsRow},
    events::EventBus,
    execution::DeliveryGateway,
    github::{Github, GithubAccess, TokenProvider},
    grpc::KnowledgeState,
    identity::Identity,
    scanner::{self, ScanInputs, ScanLimits},
};
use sdi_proto::{
    execution::v1::{Environment, Registry},
    knowledge::v1::{GetProjectContextRequest, knowledge_service_server::KnowledgeService},
};
use serde_json::json;
use sqlx::PgPool;
use tonic::{Request, Status};
use uuid::Uuid;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

const SDI_WITH_DELIVERY: &str = r#"version: 1
delivery:
  registry: { kind: ghcr }
  build_workflow: build.yml
  environments:
    - name: staging
      kind: github_actions
      url: https://staging.example.com
      health_path: /health
      auto_promote: true
    - name: production
      kind: gitops_flux
      protected: true
      config: { repo: acme/fleet, path: apps/api/kustomization.yaml, strategy: kustomize_image }
"#;

const BUILD_WORKFLOW: &str = r#"name: Build
on:
  push:
    branches: [main]
permissions:
  contents: read
  packages: write
jobs:
  image:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}
      - uses: docker/build-push-action@v6
        with:
          context: .
          push: true
          tags: ghcr.io/acme/api:${{ github.sha }}
"#;

const CI_WORKFLOW: &str = r#"name: CI
on: [pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: npm test
"#;

/// A GitHub that serves one repository's files and 404s everything else
/// (tarball, runs, deployments, pulls), which the scanner tolerates.
#[derive(Default)]
struct FakeGithub {
    files: Mutex<BTreeMap<String, String>>,
}

impl FakeGithub {
    fn set(&self, path: &str, content: &str) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), content.to_owned());
    }
}

async fn github_handler(
    State(fake): State<Arc<FakeGithub>>,
    request: axum::extract::Request,
) -> Response {
    let path = request.uri().path().to_owned();
    let files = fake.files.lock().unwrap().clone();
    let body = if path == "/repos/acme/api" {
        json!({"id": 7, "full_name": "acme/api", "name": "api", "owner": {"login": "acme"},
               "default_branch": "main", "language": "JavaScript", "private": true})
    } else if path == "/repos/acme/api/languages" {
        json!({"JavaScript": 1200})
    } else if path == "/repos/acme/api/commits/main" {
        json!({"sha": SHA})
    } else if path == format!("/repos/acme/api/git/trees/{SHA}") {
        let tree: Vec<_> = files
            .iter()
            .map(|(p, c)| json!({"path": p, "type": "blob", "mode": "100644", "sha": format!("sha-{p}"), "size": c.len()}))
            .collect();
        json!({"sha": SHA, "tree": tree, "truncated": false})
    } else if let Some(file) = path.strip_prefix("/repos/acme/api/contents/") {
        match files.get(file) {
            Some(content) => json!({"content": content}),
            None => return (StatusCode::NOT_FOUND, r#"{"message":"Not Found"}"#).into_response(),
        }
    } else {
        return (StatusCode::NOT_FOUND, r#"{"message":"Not Found"}"#).into_response();
    };
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

async fn serve_github(fake: Arc<FakeGithub>) -> SocketAddr {
    let app = Router::new().fallback(get(github_handler)).with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

/// What `SyncEnvironments` was called with, and by whom.
#[derive(Debug, Clone)]
struct SyncCall {
    principal: Principal,
    project_id: String,
    environments: Vec<Environment>,
}

#[derive(Default)]
struct RecordingExecution {
    registries: Vec<Registry>,
    down: AtomicBool,
    calls: Mutex<Vec<SyncCall>>,
}

#[tonic::async_trait]
impl DeliveryGateway for RecordingExecution {
    async fn list_registries(&self, _: &Principal) -> Result<Vec<Registry>, Status> {
        if self.down.load(Ordering::SeqCst) {
            return Err(Status::unavailable("connection refused"));
        }
        Ok(self.registries.clone())
    }

    async fn sync_environments(
        &self,
        principal: &Principal,
        project_id: &str,
        environments: Vec<Environment>,
    ) -> Result<Vec<Environment>, Status> {
        self.calls.lock().unwrap().push(SyncCall {
            principal: principal.clone(),
            project_id: project_id.to_owned(),
            environments: environments.clone(),
        });
        if self.down.load(Ordering::SeqCst) {
            return Err(Status::unavailable("connection refused"));
        }
        Ok(environments)
    }
}

async fn connect() -> Option<(PgPool, String)> {
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
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations apply");
    Some((pool, schema))
}

/// Runs one scan to its end and returns the finished row.
async fn scan(
    pool: &PgPool,
    hub: &Github,
    api_base: &str,
    execution: Option<Arc<RecordingExecution>>,
    workspace: Uuid,
    project_id: Uuid,
) -> ScanRow {
    let mut access = GithubAccess::dev("ghp_test");
    access.api_base_url = api_base.to_owned();
    let inputs = ScanInputs {
        github: hub.bind(workspace, access, false),
        settings: SettingsRow::default(),
        controls: Controls::default(),
        limits: ScanLimits::default(),
        delivery: execution.map(|e| e as Arc<dyn DeliveryGateway>),
    };
    let project = db::projects::get(pool, workspace, project_id)
        .await
        .unwrap();
    let started = scanner::start(pool, &EventBus::new(), inputs, project)
        .await
        .unwrap();
    for _ in 0..600 {
        let row = db::scans::get(pool, workspace, started.id).await.unwrap();
        if matches!(row.status.as_str(), "succeeded" | "failed") {
            return row;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("scan {} never finished", started.id);
}

fn delivery_detail(scan: &ScanRow) -> String {
    scan.stages
        .0
        .iter()
        .find(|s| s.key == "delivery")
        .map(|s| s.detail.clone())
        .unwrap_or_default()
}

#[tokio::test]
async fn a_scan_stores_the_delivery_config_and_syncs_the_environments() {
    let Some((pool, schema)) = connect().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    let fake = Arc::new(FakeGithub::default());
    fake.set(".sdi.yml", SDI_WITH_DELIVERY);
    fake.set(
        "package.json",
        r#"{"name":"api","dependencies":{"express":"^4.19.0"}}"#,
    );
    fake.set(
        "Dockerfile",
        "FROM node:22-alpine\nCMD [\"node\", \"server.js\"]\n",
    );
    fake.set(".github/workflows/build.yml", BUILD_WORKFLOW);
    fake.set(".github/workflows/ci.yml", CI_WORKFLOW);
    let api_base = format!("http://{}", serve_github(Arc::clone(&fake)).await);
    let hub = Github::new(TokenProvider::new(Identity::disabled(), None)).unwrap();

    let workspace = Uuid::now_v7();
    let project = db::projects::insert(
        &pool,
        workspace,
        &NewProject {
            github_repo_id: Some(7),
            name: "api".into(),
            repo_full_name: "acme/api".into(),
            repo_url: "https://github.com/acme/api".into(),
            default_branch: "main".into(),
            is_private: true,
            description: String::new(),
            primary_language: "JavaScript".into(),
        },
    )
    .await
    .unwrap();
    let execution = Arc::new(RecordingExecution {
        registries: vec![Registry {
            id: "reg-default".into(),
            name: "GitHub Container Registry".into(),
            kind: "ghcr".into(),
            host: "ghcr.io".into(),
            namespace: "acme".into(),
            auth: "github_token".into(),
            is_default: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    // --- 1. The first scan stores the section and syncs both environments.
    let first = scan(
        &pool,
        &hub,
        &api_base,
        Some(Arc::clone(&execution)),
        workspace,
        project.id,
    )
    .await;
    assert_eq!(first.status, "succeeded", "{}", first.error);
    assert_eq!(first.commit_sha, SHA);
    let detail = delivery_detail(&first);
    assert!(
        detail.ends_with("2 configured environments synced"),
        "{detail}"
    );

    let calls = execution.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "one SyncEnvironments per scan");
    let call = &calls[0];
    assert_eq!(call.project_id, project.id.to_string());
    assert_eq!(call.principal.user_id, SYSTEM_USER);
    assert_eq!(call.principal.workspace_id, workspace.to_string());
    assert_eq!(call.principal.role, Role::Owner);
    let names: Vec<(&str, u32, &str, &str)> = call
        .environments
        .iter()
        .map(|e| {
            (
                e.name.as_str(),
                e.position,
                e.kind.as_str(),
                e.source.as_str(),
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            ("staging", 0, "github_actions", "config"),
            ("production", 1, "gitops_flux", "config")
        ]
    );
    let staging = &call.environments[0];
    assert_eq!(staging.url, "https://staging.example.com");
    assert_eq!(staging.health_path, "/health");
    assert!(staging.auto_promote && !staging.protected);
    // `delivery.registry: { kind: ghcr }` is the workspace's GHCR registry.
    assert_eq!(staging.registry_id, "reg-default");
    let production = &call.environments[1];
    assert!(production.protected);
    assert_eq!(production.config["repo"], "acme/fleet");
    assert_eq!(production.config["path"], "apps/api/kustomization.yaml");
    assert_eq!(production.config["strategy"], "kustomize_image");

    // The knowledge model carries the section and the image-build evidence.
    let facts = db::facts::list(&pool, workspace, project.id, None, None)
        .await
        .unwrap();
    let fact = |key: &str| facts.iter().find(|f| f.key == key);
    let build = fact("ci:build_image:.github/workflows/build.yml")
        .expect("the build workflow is marked as building an image");
    assert_eq!(build.value, "push");
    assert_eq!(build.source_path, ".github/workflows/build.yml");
    assert!(
        fact("ci:build_image:.github/workflows/ci.yml").is_none(),
        "a test-only workflow builds no image"
    );
    assert_eq!(
        fact("registry:ghcr").expect("registry fact").kind,
        "detected"
    );
    let environments = fact("config:delivery.environments").expect("configured fact");
    assert_eq!(environments.kind, "configured");
    assert!(
        environments
            .value
            .contains("production (gitops_flux, protected)")
    );
    assert_eq!(
        fact("config:delivery.registry").unwrap().value,
        "ghcr (ghcr.io)"
    );

    // `GetProjectContext` hands the canonical JSON to the other services.
    let state = KnowledgeState::new(
        pool.clone(),
        Github::new(TokenProvider::new(Identity::disabled(), None)).unwrap(),
        EventBus::new(),
        Identity::disabled(),
    );
    let mut request = Request::new(GetProjectContextRequest {
        project_id: project.id.to_string(),
    });
    Caller {
        user_id: "u1".into(),
        workspace_id: workspace,
        role: Role::Viewer,
    }
    .principal()
    .apply(&mut request);
    let context = state
        .get_project_context(request)
        .await
        .unwrap()
        .into_inner();
    let config = context.config.expect("project config");
    assert!(
        config.present && config.errors.is_empty(),
        "{:?}",
        config.errors
    );
    let delivery = DeliveryConfigDraft::from_json(&config.delivery_json).expect("delivery_json");
    assert_eq!(delivery.build_workflow, "build.yml");
    assert_eq!(delivery.registry.kind, "ghcr");
    assert_eq!(delivery.environments.len(), 2);
    assert!(context.capabilities.contains(&"registry:ghcr".to_owned()));

    // --- 2. An execution outage is reported in the stage, not a failed scan.
    execution.down.store(true, Ordering::SeqCst);
    let outage = scan(
        &pool,
        &hub,
        &api_base,
        Some(Arc::clone(&execution)),
        workspace,
        project.id,
    )
    .await;
    assert_eq!(outage.status, "succeeded", "{}", outage.error);
    let detail = delivery_detail(&outage);
    assert!(
        detail.contains("configured environments not synced: connection refused"),
        "{detail}"
    );
    execution.down.store(false, Ordering::SeqCst);
    assert_eq!(execution.calls.lock().unwrap().len(), 2);

    // --- 3. A config with errors is ignored entirely: nothing is synced.
    fake.set(
        ".sdi.yml",
        "version: 1\ndelivery:\n  environments:\n    - { name: staging, kind: teleport }\n",
    );
    let broken = scan(
        &pool,
        &hub,
        &api_base,
        Some(Arc::clone(&execution)),
        workspace,
        project.id,
    )
    .await;
    assert_eq!(broken.status, "succeeded", "{}", broken.error);
    assert!(
        !delivery_detail(&broken).contains("configured environment"),
        "{}",
        delivery_detail(&broken)
    );
    assert_eq!(
        execution.calls.lock().unwrap().len(),
        2,
        "a typo must not touch the environments"
    );

    // --- 4. The section comes back, then is removed: execution is cleared.
    fake.set(".sdi.yml", SDI_WITH_DELIVERY);
    scan(
        &pool,
        &hub,
        &api_base,
        Some(Arc::clone(&execution)),
        workspace,
        project.id,
    )
    .await;
    fake.set(".sdi.yml", "version: 1\ndeploy:\n  target: kubernetes\n");
    let removed = scan(
        &pool,
        &hub,
        &api_base,
        Some(Arc::clone(&execution)),
        workspace,
        project.id,
    )
    .await;
    assert_eq!(removed.status, "succeeded", "{}", removed.error);
    assert!(
        delivery_detail(&removed).ends_with("0 configured environments synced"),
        "{}",
        delivery_detail(&removed)
    );
    let calls = execution.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[2].environments.len(), 2);
    assert!(calls[3].environments.is_empty());
    // Nothing left to clear: the next scan stays silent.
    let quiet = scan(
        &pool,
        &hub,
        &api_base,
        Some(Arc::clone(&execution)),
        workspace,
        project.id,
    )
    .await;
    assert_eq!(quiet.status, "succeeded", "{}", quiet.error);
    assert_eq!(execution.calls.lock().unwrap().len(), 4);

    // --- 5. Without an execution client the scan simply skips the step.
    fake.set(".sdi.yml", SDI_WITH_DELIVERY);
    let alone = scan(&pool, &hub, &api_base, None, workspace, project.id).await;
    assert_eq!(alone.status, "succeeded", "{}", alone.error);
    assert!(!delivery_detail(&alone).contains("configured environment"));

    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&pool)
    .await
    .expect("schema dropped");
}
