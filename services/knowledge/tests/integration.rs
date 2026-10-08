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
    assert_eq!(catalog::seed(&pool).await.unwrap(), 15);
    assert_eq!(catalog::seed(&pool).await.unwrap(), 15);
    let patterns = db::patterns::list(&pool, None, false).await.unwrap();
    assert_eq!(patterns.len(), 15);
    assert!(patterns.iter().all(|p| p.source == "catalog"));

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
    assert_eq!(counts.patterns, 15);
    assert!(db::projects::delete(&pool, a.id).await.unwrap());
    assert!(!db::projects::delete(&pool, a.id).await.unwrap());
    let counts = db::stats::counts(&pool).await.unwrap();
    assert_eq!(counts.projects, 1);
    assert_eq!(counts.chunks, 1);
    let p = db::patterns::get(&pool, "vm-docker-compose").await.unwrap();
    assert_eq!(p.used_by, vec!["texc-symphony"]);

    drop_schema(&pool, &schema).await;
}
