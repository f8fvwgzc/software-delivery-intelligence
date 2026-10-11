//! A project shaped like the real view-engine repository: a React web app,
//! an axum API, a FastAPI quant service, PostgreSQL and Redis, Docker
//! Compose, a GitHub Actions workflow and SQL migrations.

use std::collections::{BTreeMap, HashMap};

use sdi_proto::knowledge::v1::{
    Dependency, Fact, FactCategory, FactKind, Graph, GraphEdge, GraphNode, NodeKind, Project,
    ProjectContext, RelationCategory, RepoTreeEntry, Understanding,
};

use super::{DiagramInput, catalog::tree_entry};

pub const PROJECT_ID: &str = "proj-ve";

fn attrs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn node(
    id: &str,
    kind: NodeKind,
    key: &str,
    label: &str,
    path: &str,
    attributes: &[(&str, &str)],
) -> GraphNode {
    GraphNode {
        id: id.to_owned(),
        project_id: PROJECT_ID.to_owned(),
        kind: kind as i32,
        key: key.to_owned(),
        label: label.to_owned(),
        description: format!("{label} ({key})"),
        source_path: path.to_owned(),
        attributes: attrs(attributes),
    }
}

fn edge(
    id: &str,
    source: &str,
    target: &str,
    relation: &str,
    category: RelationCategory,
) -> GraphEdge {
    GraphEdge {
        id: id.to_owned(),
        project_id: PROJECT_ID.to_owned(),
        source_id: source.to_owned(),
        target_id: target.to_owned(),
        relation: relation.to_owned(),
        category: category as i32,
        valid_from: None,
        valid_to: None,
        attributes: HashMap::new(),
    }
}

fn fact(key: &str, category: FactCategory, label: &str, path: &str) -> Fact {
    Fact {
        id: format!("f-{key}"),
        project_id: PROJECT_ID.to_owned(),
        kind: FactKind::Detected as i32,
        category: category as i32,
        key: key.to_owned(),
        label: label.to_owned(),
        value: String::new(),
        confidence: 1.0,
        source_path: path.to_owned(),
        evidence: String::new(),
        detected_at: None,
    }
}

fn dependency(ecosystem: &str, name: &str, manifest: &str, scope: &str) -> Dependency {
    Dependency {
        id: format!("d-{ecosystem}-{name}-{manifest}"),
        project_id: PROJECT_ID.to_owned(),
        ecosystem: ecosystem.to_owned(),
        name: name.to_owned(),
        version: "1".to_owned(),
        manifest_path: manifest.to_owned(),
        scope: scope.to_owned(),
    }
}

pub fn graph() -> Graph {
    use NodeKind::*;

    let nodes = vec![
        node(
            "n-repo",
            Repository,
            "repository:view-engine",
            "view-engine",
            "",
            &[],
        ),
        node(
            "n-web",
            Service,
            "service:web",
            "web",
            "web/package.json",
            &[
                ("ecosystem", "npm"),
                ("package_manager", "pnpm"),
                ("directory", "web"),
                ("frameworks", "React, Vite"),
            ],
        ),
        node(
            "n-api",
            Service,
            "service:api",
            "api",
            "services/api/Cargo.toml",
            &[
                ("ecosystem", "cargo"),
                ("package_manager", "cargo"),
                ("directory", "services/api"),
                ("frameworks", "axum"),
            ],
        ),
        node(
            "n-quant",
            Service,
            "service:quant",
            "quant",
            "services/quant/pyproject.toml",
            &[
                ("ecosystem", "pip"),
                ("package_manager", "uv"),
                ("directory", "services/quant"),
                ("frameworks", "FastAPI"),
            ],
        ),
        node(
            "n-shared",
            Library,
            "library:shared",
            "shared",
            "crates/shared/Cargo.toml",
            &[
                ("ecosystem", "cargo"),
                ("package_manager", "cargo"),
                ("directory", "crates/shared"),
            ],
        ),
        node(
            "n-e2e",
            Library,
            "library:e2e",
            "e2e",
            "tests/e2e/package.json",
            &[("ecosystem", "npm"), ("directory", "tests/e2e")],
        ),
        node(
            "n-pg",
            Datastore,
            "datastore:postgres",
            "PostgreSQL",
            "docker-compose.yml",
            &[
                ("engine", "postgres"),
                ("defined_by", "docker_compose"),
                ("image", "postgres:16"),
            ],
        ),
        node(
            "n-redis",
            Datastore,
            "datastore:redis",
            "Redis",
            "docker-compose.yml",
            &[("engine", "redis"), ("defined_by", "docker_compose")],
        ),
        node(
            "n-kafka",
            Datastore,
            "datastore:kafka",
            "Kafka",
            "docker-compose.yml",
            &[("engine", "kafka"), ("defined_by", "docker_compose")],
        ),
        node(
            "n-stripe",
            ExternalApi,
            "external_api:stripe",
            "Stripe",
            "services/api/Cargo.toml",
            &[("dependency", "stripe-rust")],
        ),
        node(
            "n-img-api",
            ContainerImage,
            "container_image:api",
            "api image",
            "services/api/Dockerfile",
            &[
                ("base_image", "rust:1.85-slim"),
                ("ports", "8080"),
                ("directory", "services/api"),
            ],
        ),
        node(
            "n-img-web",
            ContainerImage,
            "container_image:web",
            "web image",
            "web/Dockerfile",
            &[
                ("base_image", "nginx:alpine"),
                ("ports", "80"),
                ("directory", "web"),
            ],
        ),
        node(
            "n-env-local",
            Environment,
            "environment:local",
            "local",
            "docker-compose.yml",
            &[("defined_by", "docker_compose")],
        ),
        node(
            "n-env-prod",
            Environment,
            "environment:production",
            "production",
            ".github/workflows/deploy.yml",
            &[("defined_by", "workflow")],
        ),
        node(
            "n-ci",
            Pipeline,
            "pipeline:ci.yml",
            "ci",
            ".github/workflows/ci.yml",
            &[
                ("provider", "github_actions"),
                ("triggers", "push,pull_request"),
                ("jobs", "3"),
            ],
        ),
        node(
            "n-deploy",
            Pipeline,
            "pipeline:deploy.yml",
            "deploy",
            ".github/workflows/deploy.yml",
            &[
                ("provider", "github_actions"),
                ("triggers", "push"),
                ("jobs", "1"),
            ],
        ),
        node(
            "n-dep-prod",
            Deployment,
            "deployment:deploy.yml:release",
            "release → production",
            ".github/workflows/deploy.yml",
            &[
                ("target", "azure_container_apps"),
                ("environment", "production"),
                ("job", "release"),
            ],
        ),
        node(
            "n-tests",
            TestSuite,
            "test_suite:api",
            "api tests",
            "services/api/tests/health.rs",
            &[],
        ),
        node(
            "n-readme",
            Document,
            "document:README.md",
            "README",
            "README.md",
            &[],
        ),
        node(
            "n-mig",
            Migration,
            "migration:services/api/migrations",
            "api migrations",
            "services/api/migrations/0001_init.sql",
            &[],
        ),
    ];
    let edges = vec![
        edge(
            "e1",
            "n-repo",
            "n-web",
            "CONTAINS",
            RelationCategory::Structure,
        ),
        edge(
            "e2",
            "n-repo",
            "n-api",
            "CONTAINS",
            RelationCategory::Structure,
        ),
        edge(
            "e3",
            "n-repo",
            "n-quant",
            "CONTAINS",
            RelationCategory::Structure,
        ),
        edge(
            "e4",
            "n-repo",
            "n-shared",
            "CONTAINS",
            RelationCategory::Structure,
        ),
        edge(
            "e5",
            "n-repo",
            "n-e2e",
            "CONTAINS",
            RelationCategory::Structure,
        ),
        edge(
            "e6",
            "n-api",
            "n-pg",
            "DEPENDS_ON",
            RelationCategory::Infrastructure,
        ),
        edge(
            "e7",
            "n-api",
            "n-redis",
            "DEPENDS_ON",
            RelationCategory::Infrastructure,
        ),
        edge(
            "e8",
            "n-quant",
            "n-pg",
            "DEPENDS_ON",
            RelationCategory::Infrastructure,
        ),
        edge(
            "e9",
            "n-api",
            "n-kafka",
            "DEPENDS_ON",
            RelationCategory::Infrastructure,
        ),
        edge("e10", "n-api", "n-stripe", "USES", RelationCategory::Source),
        edge(
            "e11",
            "n-api",
            "n-img-api",
            "PACKAGED_AS",
            RelationCategory::Structure,
        ),
        edge(
            "e12",
            "n-web",
            "n-img-web",
            "PACKAGED_AS",
            RelationCategory::Structure,
        ),
        edge(
            "e13",
            "n-ci",
            "n-api",
            "TESTS",
            RelationCategory::Validation,
        ),
        edge(
            "e14",
            "n-ci",
            "n-img-api",
            "BUILDS",
            RelationCategory::Execution,
        ),
        edge(
            "e15",
            "n-deploy",
            "n-dep-prod",
            "DEPLOYS",
            RelationCategory::Execution,
        ),
        edge(
            "e16",
            "n-dep-prod",
            "n-env-prod",
            "DEPLOYED_TO",
            RelationCategory::Infrastructure,
        ),
        edge(
            "e17",
            "n-img-api",
            "n-env-local",
            "RUNS_ON",
            RelationCategory::Infrastructure,
        ),
        edge(
            "e18",
            "n-api",
            "n-tests",
            "TESTED_BY",
            RelationCategory::Validation,
        ),
        edge(
            "e19",
            "n-repo",
            "n-readme",
            "DOCUMENTED_BY",
            RelationCategory::Ownership,
        ),
        edge(
            "e20",
            "n-mig",
            "n-pg",
            "MIGRATES",
            RelationCategory::Infrastructure,
        ),
        edge(
            "e21",
            "n-repo",
            "n-ci",
            "CONTAINS",
            RelationCategory::Structure,
        ),
        edge(
            "e22",
            "n-repo",
            "n-deploy",
            "CONTAINS",
            RelationCategory::Structure,
        ),
    ];
    let mut expired = edge(
        "e23",
        "n-web",
        "n-kafka",
        "DEPENDS_ON",
        RelationCategory::Infrastructure,
    );
    expired.valid_to = Some(prost_types::Timestamp {
        seconds: 1,
        nanos: 0,
    });
    let mut edges = edges;
    edges.push(expired);
    Graph { nodes, edges }
}

pub fn context() -> ProjectContext {
    ProjectContext {
        project: Some(Project {
            id: PROJECT_ID.to_owned(),
            name: "view-engine".to_owned(),
            repo_full_name: "f8fvwgzc/view-engine".to_owned(),
            default_branch: "main".to_owned(),
            primary_language: "TypeScript".to_owned(),
            ..Default::default()
        }),
        understanding: Some(Understanding {
            project_id: PROJECT_ID.to_owned(),
            tldr: "A view engine with a React front end, an axum API and a quant service"
                .to_owned(),
            request_flow: vec![
                "Browser".into(),
                "React".into(),
                "API".into(),
                "PostgreSQL".into(),
            ],
            commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            ..Default::default()
        }),
        facts: vec![
            fact(
                "framework:react",
                FactCategory::Framework,
                "React",
                "web/package.json",
            ),
            fact(
                "framework:axum",
                FactCategory::Framework,
                "axum",
                "services/api/Cargo.toml",
            ),
            fact(
                "framework:fastapi",
                FactCategory::Framework,
                "FastAPI",
                "services/quant/pyproject.toml",
            ),
            fact(
                "datastore:postgres",
                FactCategory::Datastore,
                "PostgreSQL",
                "services/api/Cargo.toml",
            ),
            fact(
                "datastore:redis",
                FactCategory::Datastore,
                "Redis",
                "services/api/Cargo.toml",
            ),
            fact(
                "container:dockerfile:services/api/Dockerfile",
                FactCategory::Container,
                "Dockerfile",
                "services/api/Dockerfile",
            ),
            fact(
                "container:compose:docker-compose.yml",
                FactCategory::Container,
                "Docker Compose",
                "docker-compose.yml",
            ),
            fact(
                "ci:github_actions",
                FactCategory::CiCd,
                "GitHub Actions",
                ".github/workflows/ci.yml",
            ),
            fact(
                "runtime:node:22",
                FactCategory::Runtime,
                "Node 22",
                "web/package.json",
            ),
            fact(
                "runtime:rust:1.85",
                FactCategory::Runtime,
                "Rust 1.85",
                "services/api/Cargo.toml",
            ),
        ],
        questions: vec![],
        graph: Some(graph()),
        dependabot: None,
        pipeline_stats: None,
        deployment_stats: None,
        technologies: vec![
            "React".into(),
            "axum".into(),
            "FastAPI".into(),
            "PostgreSQL".into(),
            "Redis".into(),
            "Docker".into(),
        ],
        capabilities: vec!["dockerfile".into(), "ci:github_actions".into()],
        profile_text: String::new(),
        maturity: None,
        config: None,
        dependencies: vec![
            dependency("npm", "react", "web/package.json", "runtime"),
            dependency("npm", "react-dom", "web/package.json", "runtime"),
            dependency(
                "npm",
                "@tanstack/react-query",
                "web/package.json",
                "runtime",
            ),
            dependency("npm", "vite", "web/package.json", "dev"),
            dependency("npm", "typescript", "web/package.json", "dev"),
            dependency("npm", "@playwright/test", "tests/e2e/package.json", "dev"),
            dependency("npm", "typescript", "tests/e2e/package.json", "dev"),
            dependency("cargo", "axum", "services/api/Cargo.toml", "runtime"),
            dependency("cargo", "sqlx", "services/api/Cargo.toml", "runtime"),
            dependency("cargo", "tokio", "services/api/Cargo.toml", "runtime"),
            dependency("cargo", "serde", "services/api/Cargo.toml", "runtime"),
            dependency("cargo", "shared", "services/api/Cargo.toml", "runtime"),
            dependency("cargo", "redis", "services/api/Cargo.toml", "runtime"),
            dependency("cargo", "serde", "crates/shared/Cargo.toml", "runtime"),
            dependency("cargo", "tokio", "crates/shared/Cargo.toml", "runtime"),
            dependency("pip", "fastapi", "services/quant/pyproject.toml", "runtime"),
            dependency("pip", "uvicorn", "services/quant/pyproject.toml", "runtime"),
            dependency("pip", "asyncpg", "services/quant/pyproject.toml", "runtime"),
            dependency("pip", "numpy", "services/quant/pyproject.toml", "runtime"),
            dependency("pip", "pytest", "services/quant/pyproject.toml", "dev"),
        ],
    }
}

pub fn tree() -> Vec<RepoTreeEntry> {
    [
        "README.md",
        "docker-compose.yml",
        "Cargo.toml",
        "package.json",
        "pnpm-workspace.yaml",
        ".github/",
        ".github/workflows/",
        ".github/workflows/ci.yml",
        ".github/workflows/deploy.yml",
        "docs/",
        "docs/architecture.md",
        "web/",
        "web/package.json",
        "web/Dockerfile",
        "web/index.html",
        "web/src/",
        "web/src/main.tsx",
        "web/src/App.tsx",
        "web/src/lib/",
        "web/src/lib/api.ts",
        "web/src/features/",
        "web/src/features/views/",
        "web/src/features/views/ViewList.tsx",
        "web/src/components/",
        "web/src/components/Button.tsx",
        "web/src/App.test.tsx",
        "services/",
        "services/api/",
        "services/api/Cargo.toml",
        "services/api/Dockerfile",
        "services/api/src/",
        "services/api/src/main.rs",
        "services/api/src/routes/",
        "services/api/src/routes/mod.rs",
        "services/api/src/routes/views.rs",
        "services/api/src/db/",
        "services/api/src/db/mod.rs",
        "services/api/src/db/views.rs",
        "services/api/src/cache.rs",
        "services/api/src/events.rs",
        "services/api/migrations/",
        "services/api/migrations/0001_init.sql",
        "services/api/migrations/0002_sessions.sql",
        "services/api/tests/",
        "services/api/tests/health.rs",
        "services/quant/",
        "services/quant/pyproject.toml",
        "services/quant/Dockerfile",
        "services/quant/app/",
        "services/quant/app/main.py",
        "services/quant/app/pricing.py",
        "services/quant/app/db.py",
        "services/quant/tests/",
        "services/quant/tests/test_pricing.py",
        "crates/",
        "crates/shared/",
        "crates/shared/Cargo.toml",
        "crates/shared/src/",
        "crates/shared/src/lib.rs",
        "crates/shared/src/models.rs",
        "tests/",
        "tests/e2e/",
        "tests/e2e/package.json",
        "tests/e2e/smoke.spec.ts",
        "deploy/",
        "deploy/main.bicep",
    ]
    .iter()
    .map(|p| tree_entry(p))
    .collect()
}

pub fn files() -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    files.insert(
        "services/api/src/main.rs".to_owned(),
        r#"use axum::{routing::get, Router};
use shared::models::View;
use sqlx::PgPool;

mod routes;
mod cache;

#[tokio::main]
async fn main() {
    let pool = PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let quant = reqwest::Client::new();
    let quant_url = std::env::var("QUANT_URL").unwrap_or("http://quant:8000".into());
    let app = Router::new()
        .route("/health", get(health))
        .route("/api/views", get(routes::views::list).post(routes::views::create))
        .route("/api/views/:id/price", get(routes::views::price));
    axum::serve(listener, app).await.unwrap();
}
"#
        .to_owned(),
    );
    files.insert(
        "services/api/src/routes/mod.rs".to_owned(),
        "pub mod views;\nuse sqlx::query_as;\nasync fn list(pool: &PgPool) { sqlx::query_as::<_, View>(\"SELECT id, name FROM views\").fetch_all(pool).await; }\nasync fn create() { sqlx::query(\"INSERT INTO views (name) VALUES ($1)\").execute(pool).await; }\n".to_owned(),
    );
    files.insert(
        "services/api/src/cache.rs".to_owned(),
        "use redis::Client;\npub fn connect() -> Client { redis::Client::open(std::env::var(\"REDIS_URL\").unwrap()).unwrap() }\n".to_owned(),
    );
    files.insert(
        "web/src/main.tsx".to_owned(),
        "import React from 'react';\nimport { createRoot } from 'react-dom/client';\nimport { App } from './App';\ncreateRoot(document.getElementById('root')!).render(<App />);\n".to_owned(),
    );
    files.insert(
        "web/src/lib/api.ts".to_owned(),
        "const base = import.meta.env.VITE_API_URL ?? '/api';\nexport async function listViews() { const r = await fetch(`${base}/views`); return r.json(); }\n".to_owned(),
    );
    files.insert(
        "services/quant/app/main.py".to_owned(),
        "from fastapi import FastAPI\nimport asyncpg\nfrom app.pricing import price\n\napp = FastAPI()\n\n@app.get(\"/price\")\nasync def get_price(view_id: int):\n    conn = await asyncpg.connect(DATABASE_URL)\n    row = await conn.fetchrow(\"SELECT * FROM views WHERE id = $1\", view_id)\n    return price(row)\n".to_owned(),
    );
    files.insert(
        "services/api/migrations/0001_init.sql".to_owned(),
        "CREATE TABLE IF NOT EXISTS views (\n    id uuid PRIMARY KEY,\n    name text NOT NULL,\n    owner_id uuid NOT NULL REFERENCES users (id),\n    created_at timestamptz NOT NULL DEFAULT now()\n);\n\nCREATE TABLE users (\n    id uuid PRIMARY KEY,\n    email text NOT NULL UNIQUE\n);\n\nCREATE INDEX views_owner_idx ON views (owner_id);\n".to_owned(),
    );
    files.insert(
        "services/api/migrations/0002_sessions.sql".to_owned(),
        "CREATE TABLE sessions (\n    id uuid PRIMARY KEY,\n    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,\n    expires_at timestamptz NOT NULL\n);\n".to_owned(),
    );
    files.insert(
        "services/api/Cargo.toml".to_owned(),
        "[package]\nname = \"api\"\n[dependencies]\naxum = \"0.8\"\nsqlx = \"0.8\"\nshared = { path = \"../../crates/shared\" }\n".to_owned(),
    );
    files.insert(
        "docker-compose.yml".to_owned(),
        "services:\n  api:\n    build: services/api\n    ports: ['8080:8080']\n  postgres:\n    image: postgres:16\n  redis:\n    image: redis:7\n".to_owned(),
    );
    files
}

pub fn input<'a>(
    context: &'a ProjectContext,
    tree: &'a [RepoTreeEntry],
    files: &'a BTreeMap<String, String>,
    prompt: &'a str,
) -> DiagramInput<'a> {
    DiagramInput {
        context,
        tree,
        files,
        prompt,
    }
}
