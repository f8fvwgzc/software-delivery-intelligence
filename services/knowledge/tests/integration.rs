//! Runs only when `DATABASE_URL` is set: migrates into a throwaway schema,
//! exercises the repositories end to end and drops the schema again. The
//! gRPC-level tests that need no database (authentication) run always.

use std::collections::HashSet;

use futures::StreamExt;
use sdi_common::{embed::embed, principal::Role};
use sdi_knowledge::{
    auth::Caller,
    catalog,
    db::{
        self, BOOTSTRAP_WORKSPACE,
        chunks::NewChunk,
        code::{NewFile, TreeEntryRecord},
        projects::NewProject,
    },
    domain::similarity,
    events::EventBus,
    github::{Github, TokenProvider},
    grpc::KnowledgeState,
    identity::Identity,
};
use sdi_proto::knowledge::v1::{knowledge_service_server::KnowledgeService, *};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tonic::{Code, Request};
use uuid::Uuid;

async fn connect() -> Option<(PgPool, String)> {
    let url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|u| !u.is_empty())?;
    // `db::connect` only accepts lowercase letters, so spell the random suffix with them.
    let suffix: String = uuid::Uuid::new_v4()
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

async fn drop_schema(pool: &PgPool, schema: &str) {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(pool)
    .await
    .expect("schema dropped");
}

fn project(full_name: &str) -> NewProject {
    NewProject {
        github_repo_id: None,
        name: full_name.rsplit('/').next().unwrap().to_owned(),
        repo_full_name: full_name.to_owned(),
        repo_url: format!("https://github.com/{full_name}"),
        default_branch: "main".to_owned(),
        is_private: false,
        description: String::new(),
        primary_language: "Rust".to_owned(),
    }
}

/// A service with no identity and no GitHub token: enough for every RPC
/// that only touches the database.
fn service(pool: PgPool) -> KnowledgeState {
    let identity = Identity::disabled();
    let github = Github::new(TokenProvider::new(identity.clone(), None)).unwrap();
    KnowledgeState::new(pool, github, EventBus::new(), identity)
}

fn request<T>(message: T, workspace: Uuid, role: Role) -> Request<T> {
    let mut request = Request::new(message);
    Caller {
        user_id: "u1".to_owned(),
        workspace_id: workspace,
        role,
    }
    .principal()
    .apply(&mut request);
    request
}

#[tokio::test]
async fn migrations_catalog_projects_and_search() {
    let Some((pool, schema)) = connect().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    let ws = Uuid::now_v7();

    // Catalog seeds idempotently and keeps usage across re-seeds.
    let expected = catalog::catalog().len();
    assert_eq!(catalog::seed(&pool).await.unwrap(), expected);
    assert_eq!(catalog::seed(&pool).await.unwrap(), expected);
    let patterns = db::patterns::list(&pool, ws, None, false).await.unwrap();
    assert_eq!(patterns.len(), expected);
    assert!(patterns.iter().all(|p| p.source == "catalog"));
    assert!(
        patterns
            .iter()
            .all(|p| p.workspace_id.is_none() && p.scope == "public_template")
    );
    // The cost model and maturity targets round-trip through jsonb.
    let modernization = db::patterns::list(&pool, ws, Some("modernization"), false)
        .await
        .unwrap();
    assert_eq!(modernization.len(), 6);
    assert!(modernization.iter().all(|p| {
        !p.attributes.0["targets_axes"]
            .as_array()
            .unwrap()
            .is_empty()
    }));
    let hetzner = db::patterns::get(&pool, ws, "hetzner-vm-compose")
        .await
        .unwrap();
    assert_eq!(
        hetzner.attributes.0["cost_lines"].as_array().unwrap().len(),
        3
    );

    // Settings: seeded lazily per workspace, update, re-read.
    let settings = db::settings::get(&pool, ws).await.unwrap();
    assert_eq!(settings.scan_max_files, 150);
    assert_eq!(settings.scan_max_file_bytes, 200_000);
    assert_eq!(settings.scan_concurrency, 6);
    assert_eq!(settings.pipeline_runs_limit, 50);
    assert!(settings.cache_github);
    let updated = db::settings::update(
        &pool,
        ws,
        &db::settings::SettingsRow {
            scan_max_files: 80,
            cache_github: false,
            ..settings
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.scan_max_files, 80);
    assert!(!updated.cache_github);
    assert_eq!(
        db::settings::get(&pool, ws).await.unwrap().scan_max_files,
        80
    );
    // Another workspace still gets the defaults; the migrated bootstrap row exists.
    assert_eq!(
        db::settings::get(&pool, Uuid::now_v7())
            .await
            .unwrap()
            .scan_max_files,
        150
    );
    assert_eq!(
        db::settings::get(&pool, BOOTSTRAP_WORKSPACE)
            .await
            .unwrap()
            .scan_max_files,
        150
    );

    // Projects: insert, duplicate rejected, lookup by repo is case-insensitive.
    let a = db::projects::insert(&pool, ws, &project("f8fvwgzc/view-engine"))
        .await
        .unwrap();
    assert_eq!(a.status, "pending");
    assert_eq!(a.mode, "observe");
    assert_eq!(a.workspace_id, ws);
    assert!(
        db::projects::insert(&pool, ws, &project("f8fvwgzc/view-engine"))
            .await
            .is_err()
    );
    assert!(
        db::projects::get_by_repo(&pool, ws, "F8FVWGZC/View-Engine")
            .await
            .unwrap()
            .is_some()
    );
    let b = db::projects::insert(&pool, ws, &project("f8fvwgzc/texc-symphony"))
        .await
        .unwrap();
    assert_eq!(a.maturity_score, 0);
    assert!(!a.has_config);

    // Maturity, config and configured answers persist with the scan outcome.
    let maturity = sdi_analyzer::MaturityDraft {
        overall: 42,
        stage: "transitional".to_owned(),
        axes: vec![sdi_analyzer::MaturityAxisDraft {
            key: "ci".to_owned(),
            label: "CI".to_owned(),
            score: 60,
            summary: "CI runs tests but never deploys".to_owned(),
            findings: vec!["No deploy job".to_owned()],
            recommended_patterns: vec!["gha-docker-publish".to_owned()],
        }],
    };
    let config = sdi_analyzer::ProjectConfigDraft {
        present: true,
        path: ".sdi.yml".to_owned(),
        deploy_target: "azure".to_owned(),
        environments: vec!["staging".to_owned(), "production".to_owned()],
        budget_monthly_usd: 20.0,
        port: 8080,
        scan_exclude: vec!["vendor/**".to_owned()],
        ..Default::default()
    };
    let mut tx = pool.begin().await.unwrap();
    db::maturity::upsert(&mut *tx, ws, a.id, &maturity)
        .await
        .unwrap();
    db::config::upsert(&mut *tx, ws, a.id, &config)
        .await
        .unwrap();
    db::questions::sync(
        &mut tx,
        ws,
        a.id,
        &[
            sdi_analyzer::QuestionDraft {
                key: "deployment_target".to_owned(),
                prompt: "Where does this deploy?".to_owned(),
                reason: "No deployment files".to_owned(),
                options: vec!["azure".to_owned(), "aws".to_owned()],
                allow_free_text: false,
                answer: "azure".to_owned(),
                answered_by: "config".to_owned(),
            },
            sdi_analyzer::QuestionDraft {
                key: "owner".to_owned(),
                prompt: "Who owns it?".to_owned(),
                reason: "No CODEOWNERS".to_owned(),
                options: vec![],
                allow_free_text: true,
                answer: String::new(),
                answered_by: String::new(),
            },
        ],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    db::projects::finish_scan(
        &pool,
        ws,
        a.id,
        uuid::Uuid::now_v7(),
        &db::projects::ScanOutcome {
            tldr: "A view engine",
            stack: &["TypeScript".to_owned()],
            maturity_score: maturity.overall,
            has_config: config.present,
        },
    )
    .await
    .unwrap();

    let stored = db::maturity::get(&pool, ws, a.id).await.unwrap().unwrap();
    assert_eq!(stored.overall, 42);
    assert_eq!(stored.stage, "transitional");
    assert_eq!(
        stored.axes.0[0].recommended_patterns,
        vec!["gha-docker-publish"]
    );
    let stored = db::config::get(&pool, ws, a.id).await.unwrap().unwrap();
    assert_eq!(stored.config.0, config);
    assert!(db::config::get(&pool, ws, b.id).await.unwrap().is_none());
    let a_now = db::projects::get(&pool, ws, a.id).await.unwrap();
    assert_eq!(a_now.maturity_score, 42);
    assert!(a_now.has_config);
    assert_eq!(a_now.status, "ready");
    assert_eq!(a_now.open_questions, 1, "the config answered one of two");
    let questions = db::questions::list(&pool, ws, a.id, false).await.unwrap();
    let target = questions
        .iter()
        .find(|q| q.key == "deployment_target")
        .unwrap();
    assert_eq!(target.answer, "azure");
    assert_eq!(target.answered_by, "config");
    assert!(target.answered_at.is_some());
    let owner = questions.iter().find(|q| q.key == "owner").unwrap();
    assert_eq!(owner.answered_by, "");
    let owner = db::questions::answer(&pool, ws, owner.id, "platform team")
        .await
        .unwrap();
    assert_eq!(owner.answered_by, "user");
    // A rescan whose config no longer answers the question reopens it, but
    // keeps the user's answer on the other one.
    let mut tx = pool.begin().await.unwrap();
    db::questions::sync(
        &mut tx,
        ws,
        a.id,
        &[
            sdi_analyzer::QuestionDraft {
                key: "deployment_target".to_owned(),
                prompt: "Where does this deploy?".to_owned(),
                reason: "No deployment files".to_owned(),
                options: vec!["azure".to_owned(), "aws".to_owned()],
                allow_free_text: false,
                answer: String::new(),
                answered_by: String::new(),
            },
            sdi_analyzer::QuestionDraft {
                key: "owner".to_owned(),
                prompt: "Who owns it?".to_owned(),
                reason: "No CODEOWNERS".to_owned(),
                options: vec![],
                allow_free_text: true,
                answer: String::new(),
                answered_by: String::new(),
            },
        ],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let questions = db::questions::list(&pool, ws, a.id, true).await.unwrap();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].key, "deployment_target");
    assert_eq!(questions[0].answered_by, "");
    let owner = db::questions::list(&pool, ws, a.id, false)
        .await
        .unwrap()
        .into_iter()
        .find(|q| q.key == "owner")
        .unwrap();
    assert_eq!(owner.answer, "platform team");

    // Pattern outcomes roll up per project.
    db::patterns::record_outcome(&pool, ws, "vm-docker-compose", a.id, true, false)
        .await
        .unwrap();
    db::patterns::record_outcome(&pool, ws, "vm-docker-compose", b.id, false, true)
        .await
        .unwrap();
    let p = db::patterns::get(&pool, ws, "vm-docker-compose")
        .await
        .unwrap();
    assert_eq!(p.deployments, 2);
    assert_eq!(p.successes, 1);
    assert_eq!(p.incidents, 1);
    assert_eq!(p.used_by, vec!["texc-symphony", "view-engine"]);
    catalog::seed(&pool).await.unwrap();
    let p = db::patterns::get(&pool, ws, "vm-docker-compose")
        .await
        .unwrap();
    assert_eq!(p.deployments, 2, "re-seeding must keep usage");

    // Semantic search over chunks.
    for (project_id, title, content) in [
        (
            Some(a.id),
            "Deploy workflow",
            "GitHub Actions workflow builds a Docker image and deploys to Azure Container Apps",
        ),
        (
            Some(a.id),
            "README",
            "A TypeScript view engine library published to npm",
        ),
        (
            Some(b.id),
            "Cargo workspace",
            "Rust workspace with tokio, axum and sqlx against PostgreSQL",
        ),
    ] {
        db::chunks::insert(
            &pool,
            ws,
            &NewChunk {
                kind: "document".to_owned(),
                project_id,
                ref_id: String::new(),
                title: title.to_owned(),
                path: String::new(),
                content: content.to_owned(),
                embedding: embed(content),
            },
        )
        .await
        .unwrap();
    }
    let hits = db::chunks::search(
        &pool,
        ws,
        &embed("docker image deploy azure"),
        None,
        &[],
        10,
    )
    .await
    .unwrap();
    assert_eq!(hits[0].title, "Deploy workflow");
    assert!(hits[0].score > hits[1].score);
    assert_eq!(hits[0].project_name, "view-engine");
    let only_b = db::chunks::search(&pool, ws, &embed("rust"), Some(b.id), &[], 10)
        .await
        .unwrap();
    assert_eq!(only_b.len(), 1);
    let none = db::chunks::search(&pool, ws, &embed("rust"), None, &["code".to_owned()], 10)
        .await
        .unwrap();
    assert!(none.is_empty());

    // Profiles round-trip through pgvector and feed similarity.
    db::profiles::upsert(
        &pool,
        ws,
        a.id,
        "typescript library",
        &["TypeScript".to_owned()],
        &["lang:typescript".to_owned(), "ci:github_actions".to_owned()],
        &embed("typescript library"),
    )
    .await
    .unwrap();
    db::profiles::upsert(
        &pool,
        ws,
        b.id,
        "rust service with postgres",
        &["Rust".to_owned(), "PostgreSQL".to_owned()],
        &[
            "lang:rust".to_owned(),
            "database:postgres".to_owned(),
            "ci:github_actions".to_owned(),
        ],
        &embed("rust service with postgres"),
    )
    .await
    .unwrap();
    let profiles = db::profiles::list(&pool, ws).await.unwrap();
    assert_eq!(profiles.len(), 2);
    let pa = profiles
        .iter()
        .find(|p| p.project_id == a.id)
        .unwrap()
        .to_profile();
    let pb = profiles
        .iter()
        .find(|p| p.project_id == b.id)
        .unwrap()
        .to_profile();
    assert_eq!(pa.embedding.len(), sdi_common::embed::EMBEDDING_DIM);
    let breakdown = similarity::compare(&pa, &pb);
    assert_eq!(breakdown.deployment, 1.0);
    assert_eq!(breakdown.database, 0.0);
    assert!(breakdown.overall > 0.0 && breakdown.overall < 1.0);

    // Stats and cascade on delete.
    let counts = db::stats::counts(&pool, ws).await.unwrap();
    assert_eq!(counts.projects, 2);
    assert_eq!(counts.chunks, 3);
    assert_eq!(counts.patterns, expected as i64);
    assert!(db::projects::delete(&pool, ws, a.id).await.unwrap());
    assert!(!db::projects::delete(&pool, ws, a.id).await.unwrap());
    let counts = db::stats::counts(&pool, ws).await.unwrap();
    assert_eq!(counts.projects, 1);
    assert_eq!(counts.chunks, 1);
    let p = db::patterns::get(&pool, ws, "vm-docker-compose")
        .await
        .unwrap();
    assert_eq!(p.used_by, vec!["texc-symphony"]);

    drop_schema(&pool, &schema).await;
}

/// Tenancy (§18): a workspace never sees another's rows, through the
/// repositories and through the service; learned patterns and usage
/// roll-ups are per workspace while the catalog is shared.
#[tokio::test]
async fn workspaces_are_isolated() {
    let Some((pool, schema)) = connect().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    catalog::seed(&pool).await.unwrap();
    let ws_a = Uuid::now_v7();
    let ws_b = Uuid::now_v7();

    // The same repository may be connected in both workspaces.
    let a = db::projects::insert(&pool, ws_a, &project("acme/api"))
        .await
        .unwrap();
    let b = db::projects::insert(&pool, ws_b, &project("acme/api"))
        .await
        .unwrap();
    assert_ne!(a.id, b.id);

    // Repositories: the id alone addresses nothing.
    assert!(matches!(
        db::projects::get(&pool, ws_b, a.id).await,
        Err(sqlx::Error::RowNotFound)
    ));
    assert!(
        db::projects::get_by_repo(&pool, ws_b, "acme/api")
            .await
            .unwrap()
            .is_some_and(|p| p.id == b.id)
    );
    assert_eq!(db::projects::list(&pool, ws_a).await.unwrap().len(), 1);
    assert!(!db::projects::delete(&pool, ws_b, a.id).await.unwrap());
    assert!(matches!(
        db::projects::set_mode(&pool, ws_b, a.id, "execute").await,
        Err(sqlx::Error::RowNotFound)
    ));
    let scan = db::scans::insert(&pool, ws_a, a.id, &[]).await.unwrap();
    assert!(matches!(
        db::scans::get(&pool, ws_b, scan.id).await,
        Err(sqlx::Error::RowNotFound)
    ));
    assert!(
        db::scans::list(&pool, ws_b, a.id, 10)
            .await
            .unwrap()
            .is_empty()
    );

    // Chunks, profiles and stats stay inside the workspace.
    for (ws, project_id) in [(ws_a, a.id), (ws_b, b.id)] {
        db::chunks::insert(
            &pool,
            ws,
            &NewChunk {
                kind: "document".to_owned(),
                project_id: Some(project_id),
                ref_id: String::new(),
                title: "readme".to_owned(),
                path: String::new(),
                content: "deploys with docker compose".to_owned(),
                embedding: embed("deploys with docker compose"),
            },
        )
        .await
        .unwrap();
        db::profiles::upsert(&pool, ws, project_id, "x", &[], &[], &embed("x"))
            .await
            .unwrap();
    }
    let hits = db::chunks::search(&pool, ws_a, &embed("docker compose"), None, &[], 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].project_id, Some(a.id));
    assert_eq!(db::profiles::list(&pool, ws_b).await.unwrap().len(), 1);
    assert_eq!(db::stats::counts(&pool, ws_a).await.unwrap().chunks, 1);

    // Learned patterns belong to their workspace; the catalog to everyone;
    // usage roll-ups never name another workspace's projects.
    let learned = db::patterns::PatternUpsert {
        id: db::patterns::learned_id(ws_a, "compose-deploy"),
        workspace_id: Some(ws_a),
        name: "Compose deploy".to_owned(),
        summary: String::new(),
        category: "deployment".to_owned(),
        technologies: vec![],
        cloud: "any".to_owned(),
        runtime: String::new(),
        iac: String::new(),
        delivery: String::new(),
        requires: vec![],
        scope: "organization".to_owned(),
        source: "learned".to_owned(),
        estimated_monthly_cost_usd: 0.0,
        security_score: 0.5,
        complexity: 2,
        pros: vec![],
        cons: vec![],
        plan: vec![],
        attributes: serde_json::json!({}),
        embedding: embed("compose deploy"),
    };
    db::patterns::upsert(&pool, &learned).await.unwrap();
    db::patterns::add_used_by(&pool, ws_a, &learned.id, a.id)
        .await
        .unwrap();
    let catalog_len = catalog::catalog().len();
    let seen_by_a = db::patterns::list(&pool, ws_a, None, false).await.unwrap();
    assert_eq!(seen_by_a.len(), catalog_len + 1);
    assert_eq!(
        db::patterns::list(&pool, ws_b, None, false)
            .await
            .unwrap()
            .len(),
        catalog_len
    );
    assert!(matches!(
        db::patterns::get(&pool, ws_b, &learned.id).await,
        Err(sqlx::Error::RowNotFound)
    ));
    assert!(
        !db::patterns::exists(&pool, ws_b, &learned.id)
            .await
            .unwrap()
    );
    db::patterns::record_outcome(&pool, ws_a, "vm-docker-compose", a.id, true, false)
        .await
        .unwrap();
    db::patterns::record_outcome(&pool, ws_b, "vm-docker-compose", b.id, false, true)
        .await
        .unwrap();
    let from_a = db::patterns::get(&pool, ws_a, "vm-docker-compose")
        .await
        .unwrap();
    assert_eq!(from_a.used_by, vec!["api"]);
    assert_eq!(
        (from_a.deployments, from_a.successes, from_a.incidents),
        (1, 1, 0)
    );
    let from_b = db::patterns::get(&pool, ws_b, "vm-docker-compose")
        .await
        .unwrap();
    assert_eq!(
        (from_b.deployments, from_b.successes, from_b.incidents),
        (1, 0, 1)
    );
    assert_eq!(
        db::stats::counts(&pool, ws_a).await.unwrap().patterns,
        catalog_len as i64 + 1
    );

    // Through the service: workspace B gets NOT_FOUND for A's project, and
    // the lists are scoped.
    let svc = service(pool.clone());
    let found = svc
        .get_project(request(
            GetProjectRequest {
                id: a.id.to_string(),
            },
            ws_a,
            Role::Viewer,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(found.id, a.id.to_string());
    let error = svc
        .get_project(request(
            GetProjectRequest {
                id: a.id.to_string(),
            },
            ws_b,
            Role::Owner,
        ))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::NotFound);
    let listed = svc
        .list_projects(request(ListProjectsRequest {}, ws_b, Role::Viewer))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.projects.len(), 1);
    assert_eq!(listed.projects[0].id, b.id.to_string());
    let patterns = svc
        .list_patterns(request(ListPatternsRequest::default(), ws_b, Role::Viewer))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(patterns.patterns.len(), catalog_len);
    let stats = svc
        .get_knowledge_stats(request(GetKnowledgeStatsRequest {}, ws_b, Role::Viewer))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(stats.projects, 1);
    assert_eq!(
        svc.delete_project(request(
            DeleteProjectRequest {
                id: a.id.to_string()
            },
            ws_b,
            Role::Owner,
        ))
        .await
        .unwrap_err()
        .code(),
        Code::NotFound
    );
    // Role checks: a viewer cannot write, an admin cannot delete the workspace.
    assert_eq!(
        svc.delete_project(request(
            DeleteProjectRequest {
                id: a.id.to_string()
            },
            ws_a,
            Role::Viewer,
        ))
        .await
        .unwrap_err()
        .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        svc.update_settings(request(
            UpdateSettingsRequest { settings: None },
            ws_a,
            Role::Member,
        ))
        .await
        .unwrap_err()
        .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        svc.delete_workspace_data(request(
            DeleteWorkspaceDataRequest::default(),
            ws_a,
            Role::Admin,
        ))
        .await
        .unwrap_err()
        .code(),
        Code::PermissionDenied
    );
    // Settings are per workspace through the service too.
    let settings = svc
        .update_settings(request(
            UpdateSettingsRequest {
                settings: Some(KnowledgeSettings {
                    scan_max_files: 42,
                    scan_max_file_bytes: 1000,
                    scan_concurrency: 2,
                    pipeline_runs_limit: 10,
                    cache_github: false,
                }),
            },
            ws_a,
            Role::Admin,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(settings.scan_max_files, 42);
    let other = svc
        .get_settings(request(GetSettingsRequest {}, ws_b, Role::Viewer))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(other.scan_max_files, 150);
    // Events carry the workspace and WatchEvents only forwards the caller's.
    let mut feed = svc
        .watch_events(request(
            sdi_proto::common::v1::WatchEventsRequest {},
            ws_b,
            Role::Viewer,
        ))
        .await
        .unwrap()
        .into_inner();
    svc.events.project(ws_a, a.id, "updated", "ready");
    svc.events.project(ws_b, b.id, "updated", "ready");
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), feed.next())
        .await
        .expect("an event arrives")
        .unwrap()
        .unwrap();
    assert_eq!(event.entity_id, b.id.to_string());
    assert_eq!(event.data["workspace_id"], ws_b.to_string());

    drop_schema(&pool, &schema).await;
}

/// The code browser caches (§15) are keyed by commit, pruned to the latest
/// commit and gone with the project; session rows expire (§19).
#[tokio::test]
async fn code_browser_caches_round_trip_prune_and_expire() {
    let Some((pool, schema)) = connect().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    let ws = Uuid::now_v7();
    let project = db::projects::insert(&pool, ws, &project("f8fvwgzc/view-engine"))
        .await
        .unwrap();

    let entries = vec![
        TreeEntryRecord {
            path: "src".into(),
            is_dir: true,
            size: 0,
        },
        TreeEntryRecord {
            path: "src/main.rs".into(),
            is_dir: false,
            size: 1234,
        },
    ];
    for commit in ["aaa", "bbb"] {
        db::code::upsert_tree(&pool, ws, project.id, commit, &entries, commit == "aaa")
            .await
            .unwrap();
        let stored = db::code::upsert_file(
            &pool,
            ws,
            project.id,
            &NewFile {
                commit_sha: commit,
                path: "src/main.rs",
                content: "fn main() {}\n",
                binary: false,
                truncated: false,
                size: 13,
                language: "rust",
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(stored.commit_sha, commit);
        assert_eq!(stored.language, "rust");
        assert!(stored.expires_at.is_none());
    }
    // Re-upserting the same key refreshes in place.
    db::code::upsert_file(
        &pool,
        ws,
        project.id,
        &NewFile {
            commit_sha: "bbb",
            path: "src/main.rs",
            content: "",
            binary: true,
            truncated: true,
            size: 1 << 21,
            language: "",
        },
        None,
    )
    .await
    .unwrap();

    let tree = db::code::get_tree(&pool, ws, project.id, "aaa")
        .await
        .unwrap()
        .expect("tree at aaa");
    assert!(tree.truncated);
    assert_eq!(tree.entries.0.len(), 2);
    assert!(
        db::code::get_tree(&pool, Uuid::now_v7(), project.id, "aaa")
            .await
            .unwrap()
            .is_none(),
        "another workspace sees nothing"
    );
    assert_eq!(
        db::code::tree_entry_size(&pool, ws, project.id, "bbb", "src/main.rs")
            .await
            .unwrap(),
        Some(1234)
    );
    assert_eq!(
        db::code::tree_entry_size(&pool, ws, project.id, "bbb", "nope")
            .await
            .unwrap(),
        None
    );
    let file = db::code::get_file(&pool, ws, project.id, "bbb", "src/main.rs")
        .await
        .unwrap()
        .expect("file at bbb");
    assert!(file.binary && file.truncated && file.content.is_empty());
    assert_eq!(file.size, 1 << 21);
    assert!(
        db::code::get_file(&pool, ws, project.id, "bbb", "src/other.rs")
            .await
            .unwrap()
            .is_none()
    );

    // Session mode: a row with an expiry in the future is served, one in the
    // past is a miss even before the sweeper runs, and the sweeper deletes it.
    let now = chrono::Utc::now();
    let session = |path: &'static str| NewFile {
        commit_sha: "bbb",
        path,
        content: "x",
        binary: false,
        truncated: false,
        size: 1,
        language: "",
    };
    db::code::upsert_file(
        &pool,
        ws,
        project.id,
        &session("live.txt"),
        Some(now + chrono::Duration::hours(1)),
    )
    .await
    .unwrap();
    db::code::upsert_file(
        &pool,
        ws,
        project.id,
        &session("stale.txt"),
        Some(now - chrono::Duration::seconds(1)),
    )
    .await
    .unwrap();
    assert!(
        db::code::get_file(&pool, ws, project.id, "bbb", "live.txt")
            .await
            .unwrap()
            .is_some_and(|row| row.expires_at.is_some())
    );
    assert!(
        db::code::get_file(&pool, ws, project.id, "bbb", "stale.txt")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(db::code::sweep_expired(&pool).await.unwrap(), 1);
    assert_eq!(db::code::sweep_expired(&pool).await.unwrap(), 0);

    // A scan at `bbb` leaves only `bbb` rows behind.
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        db::code::prune(&mut conn, ws, project.id, "bbb")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        db::code::prune(&mut conn, ws, project.id, "bbb")
            .await
            .unwrap(),
        0
    );
    // Purging one project drops its files and trees; another workspace's purge touches nothing.
    assert_eq!(
        db::code::purge(&mut conn, Uuid::now_v7(), None)
            .await
            .unwrap(),
        db::code::Purged::default()
    );
    let purged = db::code::purge(&mut conn, ws, Some(project.id))
        .await
        .unwrap();
    assert_eq!(purged, db::code::Purged { files: 2, trees: 1 });
    drop(conn);
    assert!(
        db::code::get_tree(&pool, ws, project.id, "bbb")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db::code::get_file(&pool, ws, project.id, "bbb", "src/main.rs")
            .await
            .unwrap()
            .is_none()
    );

    // Everything cascades with the project.
    db::code::upsert_tree(&pool, ws, project.id, "bbb", &entries, false)
        .await
        .unwrap();
    assert!(db::projects::delete(&pool, ws, project.id).await.unwrap());
    assert!(
        db::code::get_tree(&pool, ws, project.id, "bbb")
            .await
            .unwrap()
            .is_none()
    );

    drop_schema(&pool, &schema).await;
}

/// Data controls (§19) through the service: purge, export and delete act on
/// the caller's workspace only.
#[tokio::test]
async fn purge_export_and_delete_cover_the_workspace() {
    let Some((pool, schema)) = connect().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    catalog::seed(&pool).await.unwrap();
    let ws = Uuid::now_v7();
    let other = Uuid::now_v7();
    let mine = db::projects::insert(&pool, ws, &project("acme/mine"))
        .await
        .unwrap();
    let theirs = db::projects::insert(&pool, other, &project("acme/theirs"))
        .await
        .unwrap();
    for (ws, project_id) in [(ws, mine.id), (other, theirs.id)] {
        db::code::upsert_file(
            &pool,
            ws,
            project_id,
            &NewFile {
                commit_sha: "abc",
                path: "README.md",
                content: "# hi",
                binary: false,
                truncated: false,
                size: 4,
                language: "markdown",
            },
            None,
        )
        .await
        .unwrap();
        db::chunks::insert(
            &pool,
            ws,
            &NewChunk {
                kind: "document".to_owned(),
                project_id: Some(project_id),
                ref_id: String::new(),
                title: "readme".to_owned(),
                path: "README.md".to_owned(),
                content: "hi".to_owned(),
                embedding: embed("hi"),
            },
        )
        .await
        .unwrap();
    }
    db::settings::get(&pool, ws).await.unwrap();
    let svc = service(pool.clone());

    // Purge: ADMIN, own workspace only, optionally one project.
    assert_eq!(
        svc.purge_code_cache(request(PurgeCodeCacheRequest::default(), ws, Role::Member))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        svc.purge_code_cache(request(
            PurgeCodeCacheRequest {
                workspace_id: other.to_string(),
                project_id: String::new(),
            },
            ws,
            Role::Admin,
        ))
        .await
        .unwrap_err()
        .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        svc.purge_code_cache(request(
            PurgeCodeCacheRequest {
                workspace_id: String::new(),
                project_id: theirs.id.to_string(),
            },
            ws,
            Role::Admin,
        ))
        .await
        .unwrap_err()
        .code(),
        Code::NotFound
    );
    let purged = svc
        .purge_code_cache(request(
            PurgeCodeCacheRequest {
                workspace_id: ws.to_string(),
                project_id: String::new(),
            },
            ws,
            Role::Admin,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(purged.files_deleted, 1);
    assert!(
        db::code::get_file(&pool, other, theirs.id, "abc", "README.md")
            .await
            .unwrap()
            .is_some(),
        "the other workspace keeps its cache"
    );

    // Export: every table of the workspace, nothing of the other.
    let mut export = svc
        .export_workspace_data(request(
            ExportWorkspaceDataRequest::default(),
            ws,
            Role::Admin,
        ))
        .await
        .unwrap()
        .into_inner();
    let mut entities = HashSet::new();
    let mut records = 0;
    while let Some(record) = export.next().await {
        let record = record.unwrap();
        let json: serde_json::Value = serde_json::from_str(&record.record).unwrap();
        assert_eq!(json["workspace_id"], ws.to_string());
        assert!(json.get("embedding").is_none());
        entities.insert(record.entity);
        records += 1;
    }
    assert_eq!(records, 3, "project, chunk, settings");
    assert_eq!(
        entities,
        HashSet::from([
            "projects".to_owned(),
            "chunks".to_owned(),
            "workspace_settings".to_owned()
        ])
    );

    // Delete: OWNER; everything of the workspace goes, the other stays.
    let deleted = svc
        .delete_workspace_data(request(
            DeleteWorkspaceDataRequest::default(),
            ws,
            Role::Owner,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(deleted.deleted["projects"], 1);
    assert_eq!(deleted.deleted["chunks"], 1);
    assert_eq!(deleted.deleted["workspace_settings"], 1);
    assert_eq!(
        deleted.deleted["patterns"], 0,
        "the catalog is not the workspace's"
    );
    assert!(db::projects::list(&pool, ws).await.unwrap().is_empty());
    assert_eq!(db::projects::list(&pool, other).await.unwrap().len(), 1);
    assert_eq!(db::stats::counts(&pool, other).await.unwrap().chunks, 1);
    assert_eq!(
        db::patterns::list(&pool, other, None, false)
            .await
            .unwrap()
            .len(),
        catalog::catalog().len()
    );

    drop_schema(&pool, &schema).await;
}

/// No database needed: every RPC refuses a request without the principal
/// metadata before touching anything.
#[tokio::test]
async fn rpcs_without_a_principal_are_unauthenticated() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://nobody@127.0.0.1:1/none")
        .unwrap();
    let svc = service(pool);
    assert_eq!(
        svc.list_projects(Request::new(ListProjectsRequest {}))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    assert_eq!(
        svc.get_settings(Request::new(GetSettingsRequest {}))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    let export = svc
        .export_workspace_data(Request::new(ExportWorkspaceDataRequest::default()))
        .await;
    assert_eq!(export.err().map(|e| e.code()), Some(Code::Unauthenticated));
    let mut bad = Request::new(ListProjectsRequest {});
    bad.metadata_mut()
        .insert("x-sdi-workspace", "ws".parse().unwrap());
    assert_eq!(
        svc.list_projects(bad).await.unwrap_err().code(),
        Code::Unauthenticated
    );
    // Without GitHub access a scan is FAILED_PRECONDITION, not a crash; the
    // project lookup comes first, so a missing project is NOT_FOUND. Both
    // need the database, which this pool does not have, so only the role
    // check is exercised here.
    assert_eq!(
        svc.start_scan(request(
            StartScanRequest {
                project_id: Uuid::now_v7().to_string()
            },
            Uuid::now_v7(),
            Role::Viewer,
        ))
        .await
        .unwrap_err()
        .code(),
        Code::PermissionDenied
    );
}

#[tokio::test]
async fn pull_request_caches_round_trip_filter_and_expire() {
    let Some((pool, schema)) = connect().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };
    use sdi_knowledge::{
        db::pull_requests::{self, FileRecord, NewFiles, NewPullRequest},
        domain::diff::Hunk,
    };
    let ws = Uuid::now_v7();
    let project = db::projects::insert(&pool, ws, &project("acme/prs"))
        .await
        .unwrap();

    let pull = |number: i64, state: &str| NewPullRequest {
        number,
        title: format!("PR {number}"),
        author: "ana".into(),
        state: state.into(),
        base_ref: "main".into(),
        head_ref: format!("feat/{number}"),
        head_sha: format!("sha{number}"),
        changed_files: 1,
        pr_updated_at: Some(chrono::Utc::now() - chrono::Duration::minutes(number)),
        ..Default::default()
    };
    let mut conn = pool.acquire().await.unwrap();
    pull_requests::upsert_all(
        &mut conn,
        ws,
        project.id,
        &[
            pull(1, "open"),
            pull(2, "draft"),
            pull(3, "merged"),
            pull(4, "closed"),
        ],
    )
    .await
    .unwrap();
    let open = pull_requests::list(
        &pool,
        ws,
        project.id,
        pull_requests::states_for_filter("open"),
        50,
    )
    .await
    .unwrap();
    assert_eq!(
        open.iter().map(|p| p.number).collect::<Vec<_>>(),
        vec![1, 2]
    );
    let closed = pull_requests::list(
        &pool,
        ws,
        project.id,
        pull_requests::states_for_filter("closed"),
        50,
    )
    .await
    .unwrap();
    assert_eq!(
        closed.iter().map(|p| p.number).collect::<Vec<_>>(),
        vec![3, 4]
    );
    assert_eq!(
        pull_requests::list(
            &pool,
            ws,
            project.id,
            pull_requests::states_for_filter("all"),
            3
        )
        .await
        .unwrap()
        .len(),
        3
    );
    assert!(
        pull_requests::last_synced(&pool, ws, project.id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        pull_requests::list(
            &pool,
            Uuid::now_v7(),
            project.id,
            pull_requests::states_for_filter("all"),
            50
        )
        .await
        .unwrap()
        .is_empty(),
        "another workspace sees nothing"
    );

    // A complete open sync closes what GitHub no longer lists.
    let changed = pull_requests::close_missing_open(&pool, ws, project.id, &[2])
        .await
        .unwrap();
    assert_eq!(changed, 1);
    assert_eq!(
        pull_requests::get(&pool, ws, project.id, 1)
            .await
            .unwrap()
            .unwrap()
            .state,
        "closed"
    );

    // Files are cached per head commit; a new head replaces the old one.
    let files = vec![FileRecord {
        path: "src/main.rs".into(),
        status: "modified".into(),
        additions: 1,
        language: "rust".into(),
        hunks: vec![Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 2,
            lines: vec![" a".into(), "+b".into()],
        }],
        ..Default::default()
    }];
    pull_requests::upsert_files(
        &mut conn,
        ws,
        project.id,
        &NewFiles {
            number: 2,
            head_sha: "sha2",
            files: &files,
            truncated: false,
            expires_at: None,
        },
    )
    .await
    .unwrap();
    let cached = pull_requests::get_files(&pool, ws, project.id, 2, "sha2")
        .await
        .unwrap()
        .expect("files at sha2");
    assert_eq!(cached.files.0, files);
    pull_requests::upsert_files(
        &mut conn,
        ws,
        project.id,
        &NewFiles {
            number: 2,
            head_sha: "sha2b",
            files: &files,
            truncated: true,
            expires_at: None,
        },
    )
    .await
    .unwrap();
    assert!(
        pull_requests::get_files(&pool, ws, project.id, 2, "sha2")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        pull_requests::get_files(&pool, ws, project.id, 2, "sha2b")
            .await
            .unwrap()
            .unwrap()
            .truncated
    );

    // An expired row is a miss and the sweeper removes it.
    let expired = chrono::Utc::now() - chrono::Duration::minutes(1);
    pull_requests::upsert_files(
        &mut conn,
        ws,
        project.id,
        &NewFiles {
            number: 3,
            head_sha: "sha3",
            files: &files,
            truncated: false,
            expires_at: Some(expired),
        },
    )
    .await
    .unwrap();
    assert!(
        pull_requests::get_files(&pool, ws, project.id, 3, "sha3")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(pull_requests::sweep_expired(&pool).await.unwrap(), 1);

    // Purging the code cache takes the diffs with it; summaries stay.
    let purged = db::code::purge(&mut conn, ws, Some(project.id))
        .await
        .unwrap();
    assert_eq!(purged.files, 1);
    assert!(
        pull_requests::get_files(&pool, ws, project.id, 2, "sha2b")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        pull_requests::list(
            &pool,
            ws,
            project.id,
            pull_requests::states_for_filter("all"),
            50
        )
        .await
        .unwrap()
        .len(),
        4
    );

    // Facts by several paths at once.
    let ids = db::facts::ids_for_paths(&pool, ws, project.id, &["src/main.rs".to_owned()])
        .await
        .unwrap();
    assert!(ids.is_empty());
    assert!(
        db::facts::ids_for_paths(&pool, ws, project.id, &[])
            .await
            .unwrap()
            .is_empty()
    );

    drop(conn);
    drop_schema(&pool, &schema).await;
}
