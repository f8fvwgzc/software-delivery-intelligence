//! Runs only when `DATABASE_URL` is set: migrates into a throwaway schema,
//! exercises the repositories end to end and drops the schema again.

use sdi_common::embed::embed;
use sdi_knowledge::{
    catalog,
    db::{self, chunks::NewChunk, projects::NewProject},
    domain::similarity,
};
use sqlx::PgPool;

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

#[tokio::test]
async fn migrations_catalog_projects_and_search() {
    let Some((pool, schema)) = connect().await else {
        eprintln!("DATABASE_URL not set; skipping");
        return;
    };

    // Catalog seeds idempotently and keeps usage across re-seeds.
    let expected = catalog::catalog().len();
    assert_eq!(catalog::seed(&pool).await.unwrap(), expected);
    assert_eq!(catalog::seed(&pool).await.unwrap(), expected);
    let patterns = db::patterns::list(&pool, None, false).await.unwrap();
    assert_eq!(patterns.len(), expected);
    assert!(patterns.iter().all(|p| p.source == "catalog"));
    // The cost model and maturity targets round-trip through jsonb.
    let modernization = db::patterns::list(&pool, Some("modernization"), false)
        .await
        .unwrap();
    assert_eq!(modernization.len(), 6);
    assert!(modernization.iter().all(|p| {
        !p.attributes.0["targets_axes"]
            .as_array()
            .unwrap()
            .is_empty()
    }));
    let hetzner = db::patterns::get(&pool, "hetzner-vm-compose")
        .await
        .unwrap();
    assert_eq!(
        hetzner.attributes.0["cost_lines"].as_array().unwrap().len(),
        3
    );

    // Settings: seeded row, update, re-read.
    let settings = db::settings::get(&pool).await.unwrap();
    assert_eq!(settings.scan_max_files, 150);
    assert_eq!(settings.scan_max_file_bytes, 200_000);
    assert_eq!(settings.scan_concurrency, 6);
    assert_eq!(settings.pipeline_runs_limit, 50);
    assert!(settings.cache_github);
    let updated = db::settings::update(
        &pool,
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
    assert_eq!(db::settings::get(&pool).await.unwrap().scan_max_files, 80);

    // Projects: insert, duplicate rejected, lookup by repo is case-insensitive.
    let a = db::projects::insert(&pool, &project("f8fvwgzc/view-engine"))
        .await
        .unwrap();
    assert_eq!(a.status, "pending");
    assert_eq!(a.mode, "observe");
    assert!(
        db::projects::insert(&pool, &project("f8fvwgzc/view-engine"))
            .await
            .is_err()
    );
    assert!(
        db::projects::get_by_repo(&pool, "F8FVWGZC/View-Engine")
            .await
            .unwrap()
            .is_some()
    );
    let b = db::projects::insert(&pool, &project("f8fvwgzc/texc-symphony"))
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
    db::maturity::upsert(&mut *tx, a.id, &maturity)
        .await
        .unwrap();
    db::config::upsert(&mut *tx, a.id, &config).await.unwrap();
    db::questions::sync(
        &mut tx,
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

    let stored = db::maturity::get(&pool, a.id).await.unwrap().unwrap();
    assert_eq!(stored.overall, 42);
    assert_eq!(stored.stage, "transitional");
    assert_eq!(
        stored.axes.0[0].recommended_patterns,
        vec!["gha-docker-publish"]
    );
    let stored = db::config::get(&pool, a.id).await.unwrap().unwrap();
    assert_eq!(stored.config.0, config);
    assert!(db::config::get(&pool, b.id).await.unwrap().is_none());
    let a_now = db::projects::get(&pool, a.id).await.unwrap();
    assert_eq!(a_now.maturity_score, 42);
    assert!(a_now.has_config);
    assert_eq!(a_now.status, "ready");
    assert_eq!(a_now.open_questions, 1, "the config answered one of two");
    let questions = db::questions::list(&pool, a.id, false).await.unwrap();
    let target = questions
        .iter()
        .find(|q| q.key == "deployment_target")
        .unwrap();
    assert_eq!(target.answer, "azure");
    assert_eq!(target.answered_by, "config");
    assert!(target.answered_at.is_some());
    let owner = questions.iter().find(|q| q.key == "owner").unwrap();
    assert_eq!(owner.answered_by, "");
    let owner = db::questions::answer(&pool, owner.id, "platform team")
        .await
        .unwrap();
    assert_eq!(owner.answered_by, "user");
    // A rescan whose config no longer answers the question reopens it, but
    // keeps the user's answer on the other one.
    let mut tx = pool.begin().await.unwrap();
    db::questions::sync(
        &mut tx,
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
    let questions = db::questions::list(&pool, a.id, true).await.unwrap();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].key, "deployment_target");
    assert_eq!(questions[0].answered_by, "");
    let owner = db::questions::list(&pool, a.id, false)
        .await
        .unwrap()
        .into_iter()
        .find(|q| q.key == "owner")
        .unwrap();
    assert_eq!(owner.answer, "platform team");

    // Pattern outcomes roll up per project.
    db::patterns::record_outcome(&pool, "vm-docker-compose", a.id, true, false)
        .await
        .unwrap();
    db::patterns::record_outcome(&pool, "vm-docker-compose", b.id, false, true)
        .await
        .unwrap();
    let p = db::patterns::get(&pool, "vm-docker-compose").await.unwrap();
    assert_eq!(p.deployments, 2);
    assert_eq!(p.successes, 1);
    assert_eq!(p.incidents, 1);
    assert_eq!(p.used_by, vec!["texc-symphony", "view-engine"]);
    catalog::seed(&pool).await.unwrap();
    let p = db::patterns::get(&pool, "vm-docker-compose").await.unwrap();
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
    let hits = db::chunks::search(&pool, &embed("docker image deploy azure"), None, &[], 10)
        .await
        .unwrap();
    assert_eq!(hits[0].title, "Deploy workflow");
    assert!(hits[0].score > hits[1].score);
    assert_eq!(hits[0].project_name, "view-engine");
    let only_b = db::chunks::search(&pool, &embed("rust"), Some(b.id), &[], 10)
        .await
        .unwrap();
    assert_eq!(only_b.len(), 1);
    let none = db::chunks::search(&pool, &embed("rust"), None, &["code".to_owned()], 10)
        .await
        .unwrap();
    assert!(none.is_empty());

    // Profiles round-trip through pgvector and feed similarity.
    db::profiles::upsert(
        &pool,
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
    let profiles = db::profiles::list(&pool).await.unwrap();
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
    let counts = db::stats::counts(&pool).await.unwrap();
    assert_eq!(counts.projects, 2);
    assert_eq!(counts.chunks, 3);
    assert_eq!(counts.patterns, expected as i64);
    assert!(db::projects::delete(&pool, a.id).await.unwrap());
    assert!(!db::projects::delete(&pool, a.id).await.unwrap());
    let counts = db::stats::counts(&pool).await.unwrap();
    assert_eq!(counts.projects, 1);
    assert_eq!(counts.chunks, 1);
    let p = db::patterns::get(&pool, "vm-docker-compose").await.unwrap();
    assert_eq!(p.used_by, vec!["texc-symphony"]);

    drop_schema(&pool, &schema).await;
}
