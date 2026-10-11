//! Fixtures: a project shaped like the real view-engine repository (a
//! React web app, an axum API, a FastAPI quant service, PostgreSQL and
//! Redis, Docker Compose, a GitHub Actions workflow and SQL migrations), a
//! Rust workspace shaped like texc-symphony (a CLI binary, core / runtime /
//! store / server / trackers / codex crates, a Preact dashboard, SQLite)
//! and a six-table SQL schema with ALTERs and a DROP.

use std::collections::{BTreeMap, HashMap};

use sdi_proto::knowledge::v1::{
    Dependency, Fact, FactCategory, FactKind, Graph, GraphEdge, GraphNode, NodeKind, Project,
    ProjectContext, RelationCategory, RepoTreeEntry, Understanding,
};

use super::{
    DiagramInput,
    catalog::{tree_entry, tree_entry_sized},
};

pub const PROJECT_ID: &str = "proj-ve";
pub const SYMPHONY_PROJECT_ID: &str = "proj-symphony";

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
        "web/src/features/views/ViewEditor.tsx",
        "web/src/features/views/useViews.ts",
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
        "services/api/src/routes/health.rs",
        "services/api/src/db/",
        "services/api/src/db/mod.rs",
        "services/api/src/db/views.rs",
        "services/api/src/db/users.rs",
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
mod db;

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
        "services/api/src/routes/views.rs".to_owned(),
        "use axum::Json;\nuse crate::db::views as store;\n\npub async fn list(pool: &PgPool) -> Json<Vec<View>> { Json(store::list(pool).await) }\npub async fn create(pool: &PgPool, name: String) { store::insert(pool, &name).await; }\n".to_owned(),
    );
    files.insert(
        "services/api/src/db/mod.rs".to_owned(),
        "pub mod views;\npub mod users;\nuse sqlx::PgPool;\n\npub async fn connect(url: &str) -> PgPool { PgPool::connect(url).await.unwrap() }\n".to_owned(),
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
        "web/src/App.tsx".to_owned(),
        "import { listViews } from './lib/api';\nimport { ViewList } from './features/views/ViewList';\nimport { Button } from './components/Button';\nexport function App() { return <ViewList load={listViews} />; }\n".to_owned(),
    );
    files.insert(
        "web/src/features/views/ViewList.tsx".to_owned(),
        "import { listViews } from '../../lib/api';\nexport function ViewList() { return <ul />; }\n".to_owned(),
    );
    files.insert(
        "web/src/features/views/ViewEditor.tsx".to_owned(),
        "import { saveView } from '../../lib/api';\nexport function ViewEditor() { return <form />; }\n".to_owned(),
    );
    files.insert(
        "web/src/lib/api.ts".to_owned(),
        "const base = import.meta.env.VITE_API_URL ?? '/api';\nexport async function listViews() { const r = await fetch(`${base}/views`); return r.json(); }\nexport async function saveView(v: unknown) { await fetch(`${base}/views`, { method: 'POST', body: JSON.stringify(v) }); }\n".to_owned(),
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

// -- texc-symphony-like Rust workspace -------------------------------------------

fn snode(
    id: &str,
    kind: NodeKind,
    key: &str,
    label: &str,
    path: &str,
    attributes: &[(&str, &str)],
) -> GraphNode {
    let mut node = node(id, kind, key, label, path, attributes);
    node.project_id = SYMPHONY_PROJECT_ID.to_owned();
    node
}

fn sedge(
    id: &str,
    source: &str,
    target: &str,
    relation: &str,
    category: RelationCategory,
) -> GraphEdge {
    let mut edge = edge(id, source, target, relation, category);
    edge.project_id = SYMPHONY_PROJECT_ID.to_owned();
    edge
}

pub fn symphony_graph() -> Graph {
    use NodeKind::*;
    let cargo = |dir: &str| -> Vec<(&str, &str)> {
        vec![
            ("ecosystem", "cargo"),
            ("package_manager", "cargo"),
            ("directory", Box::leak(dir.to_owned().into_boxed_str())),
        ]
    };
    let nodes = vec![
        snode(
            "s-repo",
            Repository,
            "repository:f8fvwgzc/texc-symphony",
            "texc-symphony",
            "",
            &[],
        ),
        snode(
            "s-bin",
            Service,
            "service:symphony",
            "symphony",
            "crates/symphony/Cargo.toml",
            &cargo("crates/symphony"),
        ),
        snode(
            "s-web",
            Service,
            "service:symphony-web",
            "symphony-web",
            "web/package.json",
            &[
                ("ecosystem", "npm"),
                ("package_manager", "pnpm"),
                ("directory", "web"),
            ],
        ),
        snode(
            "s-xtask",
            Service,
            "service:xtask",
            "xtask",
            "xtask/Cargo.toml",
            &cargo("xtask"),
        ),
        snode(
            "s-core",
            Library,
            "library:symphony-core",
            "symphony-core",
            "crates/symphony-core/Cargo.toml",
            &cargo("crates/symphony-core"),
        ),
        snode(
            "s-runtime",
            Library,
            "library:symphony-runtime",
            "symphony-runtime",
            "crates/symphony-runtime/Cargo.toml",
            &cargo("crates/symphony-runtime"),
        ),
        snode(
            "s-store",
            Library,
            "library:symphony-store",
            "symphony-store",
            "crates/symphony-store/Cargo.toml",
            &cargo("crates/symphony-store"),
        ),
        snode(
            "s-server",
            Library,
            "library:symphony-server",
            "symphony-server",
            "crates/symphony-server/Cargo.toml",
            &[
                ("ecosystem", "cargo"),
                ("package_manager", "cargo"),
                ("directory", "crates/symphony-server"),
                ("frameworks", "axum"),
            ],
        ),
        snode(
            "s-trackers",
            Library,
            "library:symphony-trackers",
            "symphony-trackers",
            "crates/symphony-trackers/Cargo.toml",
            &cargo("crates/symphony-trackers"),
        ),
        snode(
            "s-codex",
            Library,
            "library:symphony-codex",
            "symphony-codex",
            "crates/symphony-codex/Cargo.toml",
            &cargo("crates/symphony-codex"),
        ),
        snode(
            "s-sqlite",
            Datastore,
            "datastore:sqlite",
            "SQLite",
            "crates/symphony-store/Cargo.toml",
            &[("engine", "sqlite"), ("defined_by", "dependency")],
        ),
        snode(
            "s-img",
            ContainerImage,
            "container_image:texc-symphony",
            "texc-symphony image",
            "docker/Dockerfile",
            &[
                ("base_image", "debian:slim"),
                ("ports", "4000"),
                ("directory", "docker"),
            ],
        ),
        snode(
            "s-ci",
            Pipeline,
            "pipeline:.github/workflows/ci.yml",
            "ci",
            ".github/workflows/ci.yml",
            &[
                ("provider", "github_actions"),
                ("triggers", "push,pull_request"),
                ("jobs", "5"),
            ],
        ),
        snode(
            "s-release",
            Pipeline,
            "pipeline:.github/workflows/release.yml",
            "release",
            ".github/workflows/release.yml",
            &[
                ("provider", "github_actions"),
                ("triggers", "push"),
                ("jobs", "6"),
            ],
        ),
        snode(
            "s-dep",
            Deployment,
            "deployment:release.yml:image",
            "image → production",
            ".github/workflows/release.yml",
            &[
                ("target", "release"),
                ("environment", "production"),
                ("job", "image"),
            ],
        ),
        snode(
            "s-env",
            Environment,
            "environment:production",
            "production",
            ".github/workflows/release.yml",
            &[("defined_by", "workflow job")],
        ),
        snode(
            "s-mig",
            Migration,
            "migration:crates/symphony-store/src/migrations",
            "store migrations",
            "crates/symphony-store/src/migrations/0001_init.sql",
            &[("tool", "sql"), ("count", "2")],
        ),
        snode(
            "s-tests",
            TestSuite,
            "test_suite:cargo_test",
            "cargo test",
            "Cargo.toml",
            &[],
        ),
    ];
    let mut edges = Vec::new();
    for (i, target) in [
        "s-bin",
        "s-web",
        "s-xtask",
        "s-core",
        "s-runtime",
        "s-store",
        "s-server",
        "s-trackers",
        "s-codex",
        "s-img",
        "s-ci",
        "s-release",
    ]
    .iter()
    .enumerate()
    {
        edges.push(sedge(
            &format!("se{i}"),
            "s-repo",
            target,
            "CONTAINS",
            RelationCategory::Structure,
        ));
    }
    edges.extend([
        sedge(
            "se20",
            "s-store",
            "s-sqlite",
            "DEPENDS_ON",
            RelationCategory::Infrastructure,
        ),
        sedge(
            "se21",
            "s-store",
            "s-mig",
            "CONTAINS",
            RelationCategory::Structure,
        ),
        sedge(
            "se22",
            "s-mig",
            "s-sqlite",
            "APPLIES_TO",
            RelationCategory::Infrastructure,
        ),
        sedge(
            "se23",
            "s-ci",
            "s-img",
            "BUILDS",
            RelationCategory::Execution,
        ),
        sedge(
            "se24",
            "s-release",
            "s-dep",
            "DEPLOYS",
            RelationCategory::Execution,
        ),
        sedge(
            "se25",
            "s-dep",
            "s-env",
            "DEPLOYED_TO",
            RelationCategory::Infrastructure,
        ),
        sedge(
            "se26",
            "s-dep",
            "s-img",
            "RELEASES",
            RelationCategory::Execution,
        ),
        sedge(
            "se27",
            "s-codex",
            "s-tests",
            "TESTED_BY",
            RelationCategory::Validation,
        ),
    ]);
    Graph { nodes, edges }
}

pub fn symphony_context() -> ProjectContext {
    ProjectContext {
        project: Some(Project {
            id: SYMPHONY_PROJECT_ID.to_owned(),
            name: "texc-symphony".to_owned(),
            repo_full_name: "f8fvwgzc/texc-symphony".to_owned(),
            default_branch: "main".to_owned(),
            primary_language: "Rust".to_owned(),
            ..Default::default()
        }),
        understanding: Some(Understanding {
            project_id: SYMPHONY_PROJECT_ID.to_owned(),
            tldr: "Turns tracker issues into isolated autonomous coding-agent runs".to_owned(),
            request_flow: vec!["Operator".into(), "symphony".into(), "Codex".into()],
            commit_sha: "04d188731d9d121ed3ff6519a8736473326a44b1".to_owned(),
            ..Default::default()
        }),
        facts: vec![
            Fact {
                project_id: SYMPHONY_PROJECT_ID.to_owned(),
                ..fact(
                    "framework:axum",
                    FactCategory::Framework,
                    "axum",
                    "crates/symphony-server/Cargo.toml",
                )
            },
            Fact {
                project_id: SYMPHONY_PROJECT_ID.to_owned(),
                ..fact(
                    "datastore:sqlite",
                    FactCategory::Datastore,
                    "SQLite",
                    "crates/symphony-store/Cargo.toml",
                )
            },
        ],
        questions: vec![],
        graph: Some(symphony_graph()),
        dependabot: None,
        pipeline_stats: None,
        deployment_stats: None,
        technologies: vec![
            "Rust".into(),
            "axum".into(),
            "SQLite".into(),
            "Preact".into(),
        ],
        capabilities: vec!["dockerfile".into(), "ci:github_actions".into()],
        profile_text: String::new(),
        maturity: None,
        config: None,
        dependencies: vec![
            Dependency {
                project_id: SYMPHONY_PROJECT_ID.to_owned(),
                ..dependency(
                    "cargo",
                    "tokio",
                    "crates/symphony-runtime/Cargo.toml",
                    "runtime",
                )
            },
            Dependency {
                project_id: SYMPHONY_PROJECT_ID.to_owned(),
                ..dependency(
                    "cargo",
                    "rusqlite",
                    "crates/symphony-store/Cargo.toml",
                    "runtime",
                )
            },
            Dependency {
                project_id: SYMPHONY_PROJECT_ID.to_owned(),
                ..dependency(
                    "cargo",
                    "axum",
                    "crates/symphony-server/Cargo.toml",
                    "runtime",
                )
            },
            Dependency {
                project_id: SYMPHONY_PROJECT_ID.to_owned(),
                ..dependency("npm", "preact", "web/package.json", "runtime")
            },
        ],
    }
}

/// `(path, size)`; a trailing `/` marks a directory.
pub fn symphony_tree() -> Vec<RepoTreeEntry> {
    [
        ("Cargo.toml", 2128),
        ("README.md", 9055),
        ("Makefile", 3587),
        (".github/", 0),
        (".github/workflows/", 0),
        (".github/workflows/ci.yml", 5692),
        (".github/workflows/release.yml", 12527),
        ("docker/", 0),
        ("docker/Dockerfile", 8302),
        ("docker/compose.yaml", 3135),
        ("docs/", 0),
        ("docs/architecture.md", 10587),
        ("crates/", 0),
        ("crates/symphony/", 0),
        ("crates/symphony/Cargo.toml", 1653),
        ("crates/symphony/src/", 0),
        ("crates/symphony/src/main.rs", 142),
        ("crates/symphony/src/lib.rs", 1097),
        ("crates/symphony/src/app.rs", 17556),
        ("crates/symphony/src/cli.rs", 23915),
        ("crates/symphony/src/control.rs", 11019),
        ("crates/symphony/src/logging.rs", 6668),
        ("crates/symphony/src/version.rs", 1105),
        ("crates/symphony/src/dashboard/", 0),
        ("crates/symphony/src/dashboard/mod.rs", 19337),
        ("crates/symphony/src/dashboard/format.rs", 27914),
        ("crates/symphony/src/dashboard/tps.rs", 8159),
        ("crates/symphony/tests/", 0),
        ("crates/symphony/tests/cli.rs", 10049),
        ("crates/symphony-core/", 0),
        ("crates/symphony-core/Cargo.toml", 1238),
        ("crates/symphony-core/src/", 0),
        ("crates/symphony-core/src/lib.rs", 1379),
        ("crates/symphony-core/src/workflow.rs", 4305),
        ("crates/symphony-core/src/workflow_store.rs", 12422),
        ("crates/symphony-core/src/issue.rs", 8397),
        ("crates/symphony-core/src/prompt.rs", 8076),
        ("crates/symphony-core/src/env.rs", 6962),
        ("crates/symphony-core/src/error.rs", 9050),
        ("crates/symphony-core/src/config/", 0),
        ("crates/symphony-core/src/config/mod.rs", 7399),
        ("crates/symphony-core/src/config/schema.rs", 26843),
        ("crates/symphony-core/src/config/cast.rs", 7166),
        ("crates/symphony-core/src/config/tracker.rs", 16269),
        ("crates/symphony-core/tests/", 0),
        ("crates/symphony-core/tests/config_workflow.rs", 47590),
        ("crates/symphony-runtime/", 0),
        ("crates/symphony-runtime/Cargo.toml", 1182),
        ("crates/symphony-runtime/src/", 0),
        ("crates/symphony-runtime/src/lib.rs", 2902),
        ("crates/symphony-runtime/src/runtime.rs", 19442),
        ("crates/symphony-runtime/src/handle.rs", 9835),
        ("crates/symphony-runtime/src/runner.rs", 14436),
        ("crates/symphony-runtime/src/worker.rs", 5941),
        ("crates/symphony-runtime/src/workspace.rs", 33607),
        ("crates/symphony-runtime/src/process.rs", 10920),
        ("crates/symphony-runtime/src/ssh.rs", 9997),
        ("crates/symphony-runtime/src/tracker.rs", 6909),
        ("crates/symphony-runtime/src/recorder.rs", 6976),
        ("crates/symphony-runtime/src/snapshot.rs", 14065),
        ("crates/symphony-runtime/src/orchestrator/", 0),
        ("crates/symphony-runtime/src/orchestrator/mod.rs", 15598),
        ("crates/symphony-runtime/src/orchestrator/dispatch.rs", 9607),
        (
            "crates/symphony-runtime/src/orchestrator/reconcile.rs",
            12743,
        ),
        ("crates/symphony-runtime/src/orchestrator/retry.rs", 12009),
        ("crates/symphony-runtime/src/orchestrator/state.rs", 19718),
        ("crates/symphony-runtime/src/orchestrator/workers.rs", 11341),
        ("crates/symphony-runtime/src/orchestrator/tests.rs", 54336),
        ("crates/symphony-runtime/tests/", 0),
        ("crates/symphony-runtime/tests/runner.rs", 14990),
        ("crates/symphony-store/", 0),
        ("crates/symphony-store/Cargo.toml", 707),
        ("crates/symphony-store/src/", 0),
        ("crates/symphony-store/src/lib.rs", 1997),
        ("crates/symphony-store/src/store.rs", 14573),
        ("crates/symphony-store/src/model.rs", 12375),
        ("crates/symphony-store/src/ops.rs", 17239),
        ("crates/symphony-store/src/schema.rs", 4510),
        ("crates/symphony-store/src/config.rs", 5796),
        ("crates/symphony-store/src/error.rs", 2774),
        ("crates/symphony-store/src/migrations/", 0),
        ("crates/symphony-store/src/migrations/0001_init.sql", 1568),
        (
            "crates/symphony-store/src/migrations/0002_retry_queue.sql",
            481,
        ),
        ("crates/symphony-server/", 0),
        ("crates/symphony-server/Cargo.toml", 1448),
        ("crates/symphony-server/src/", 0),
        ("crates/symphony-server/src/lib.rs", 2359),
        ("crates/symphony-server/src/server.rs", 4039),
        ("crates/symphony-server/src/app.rs", 9240),
        ("crates/symphony-server/src/api.rs", 2698),
        ("crates/symphony-server/src/sse.rs", 8086),
        ("crates/symphony-server/src/history.rs", 6094),
        ("crates/symphony-server/src/control.rs", 3727),
        ("crates/symphony-server/src/presenter.rs", 16118),
        ("crates/symphony-server/src/view.rs", 16037),
        ("crates/symphony-server/src/assets.rs", 6550),
        ("crates/symphony-server/src/config.rs", 3636),
        ("crates/symphony-server/src/error.rs", 4666),
        ("crates/symphony-trackers/", 0),
        ("crates/symphony-trackers/Cargo.toml", 906),
        ("crates/symphony-trackers/src/", 0),
        ("crates/symphony-trackers/src/lib.rs", 8655),
        ("crates/symphony-trackers/src/linear.rs", 26288),
        ("crates/symphony-trackers/src/github.rs", 12111),
        ("crates/symphony-trackers/src/jira.rs", 19825),
        ("crates/symphony-trackers/src/tool.rs", 7539),
        ("crates/symphony-trackers/src/transport.rs", 28878),
        ("crates/symphony-trackers/src/rest.rs", 13845),
        ("crates/symphony-trackers/src/error.rs", 13150),
        ("crates/symphony-codex/", 0),
        ("crates/symphony-codex/Cargo.toml", 891),
        ("crates/symphony-codex/src/", 0),
        ("crates/symphony-codex/src/lib.rs", 2271),
        ("crates/symphony-codex/src/session.rs", 26611),
        ("crates/symphony-codex/src/protocol.rs", 14319),
        ("crates/symphony-codex/src/event.rs", 17969),
        ("crates/symphony-codex/src/launch.rs", 10487),
        ("crates/symphony-codex/src/transport.rs", 13715),
        ("crates/symphony-codex/src/tokens.rs", 15810),
        ("crates/symphony-codex/src/dynamic_tool.rs", 8773),
        ("crates/symphony-codex/src/error.rs", 8219),
        ("xtask/", 0),
        ("xtask/Cargo.toml", 361),
        ("xtask/src/", 0),
        ("xtask/src/main.rs", 1480),
        ("web/", 0),
        ("web/package.json", 1421),
        ("web/index.html", 538),
        ("web/vite.config.ts", 1312),
        ("web/src/", 0),
        ("web/src/main.tsx", 299),
        ("web/src/App.tsx", 4164),
        ("web/src/App.test.tsx", 4722),
        ("web/src/api/", 0),
        ("web/src/api/client.ts", 6084),
        ("web/src/api/context.ts", 386),
        ("web/src/api/types.ts", 6458),
        ("web/src/features/", 0),
        ("web/src/features/overview/", 0),
        ("web/src/features/overview/OverviewPage.tsx", 1378),
        ("web/src/features/overview/RunningTable.tsx", 2556),
        ("web/src/features/overview/StatGrid.tsx", 2570),
        ("web/src/features/runs/", 0),
        ("web/src/features/runs/RunsPage.tsx", 5425),
        ("web/src/features/runs/RunDetailPage.tsx", 4301),
        ("web/src/features/runs/useRunEvents.ts", 1860),
        ("web/src/live/", 0),
        ("web/src/live/liveSource.ts", 11241),
        ("web/src/live/useLiveState.ts", 2096),
        ("web/src/layout/", 0),
        ("web/src/layout/AppShell.tsx", 2384),
        ("web/src/layout/ConnectionBadge.tsx", 1824),
        ("web/src/layout/ThemeToggle.tsx", 939),
        ("web/src/lib/", 0),
        ("web/src/lib/router.ts", 3293),
        ("web/src/lib/format.ts", 10142),
    ]
    .iter()
    .map(|(p, size)| tree_entry_sized(p, *size))
    .collect()
}

pub fn symphony_files() -> BTreeMap<String, String> {
    let entries: [(&str, &str); 30] = [
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\", \"xtask\"]\n[workspace.dependencies]\nsymphony-core = { path = \"crates/symphony-core\" }\n",
        ),
        (
            "crates/symphony/Cargo.toml",
            "[package]\nname = \"symphony\"\n\n[lib]\nname = \"symphony_cli\"\npath = \"src/lib.rs\"\n\n[[bin]]\nname = \"symphony\"\npath = \"src/main.rs\"\n\n[dependencies]\nsymphony-core.workspace = true\nsymphony-runtime.workspace = true\n",
        ),
        (
            "crates/symphony/src/main.rs",
            "fn main() -> std::process::ExitCode {\n    symphony_cli::app::main()\n}\n",
        ),
        (
            "crates/symphony/src/lib.rs",
            "//! The symphony binary as a library.\npub mod app;\npub mod cli;\npub mod control;\npub mod dashboard;\npub mod logging;\npub mod version;\n",
        ),
        (
            "crates/symphony/src/app.rs",
            "//! Startup order, signal handling and graceful shutdown.\nuse std::sync::Arc;\nuse symphony_core::WorkflowStore;\nuse symphony_runtime::{Runtime, RuntimeOptions};\nuse symphony_server::{serve, ServerConfig};\nuse symphony_store::Store;\nuse symphony_trackers::TrackerDeps;\nuse tokio_util::sync::CancellationToken;\n\nuse crate::cli::{self, Invocation};\nuse crate::control::RuntimeControlPlane;\nuse crate::dashboard::{self, RuntimeSource};\nuse crate::logging::{self, Logging};\n\npub fn main() -> std::process::ExitCode {\n    let invocation = cli::evaluate(std::env::args_os());\n    let workflow = WorkflowStore::start(invocation.workflow_path.clone()).expect(\"workflow\");\n    let store = Store::open(&invocation.db_path).expect(\"store\");\n    let deps = TrackerDeps::new().expect(\"trackers\");\n    let runtime = Runtime::start(RuntimeOptions::new(workflow.clone(), deps));\n    let shutdown = CancellationToken::new();\n    let control = Arc::new(RuntimeControlPlane::new(runtime.handle()));\n    let server = tokio::spawn(serve(ServerConfig::new(\"127.0.0.1\", 4000), control, store.clone(), shutdown.clone()));\n    tokio::spawn(dashboard::run(RuntimeSource::new(runtime.handle()), workflow));\n    tokio::select! { _ = server => {}, _ = shutdown.cancelled() => {} }\n    std::process::ExitCode::SUCCESS\n}\n",
        ),
        (
            "crates/symphony/src/dashboard/mod.rs",
            "//! The ANSI terminal dashboard.\npub mod format;\npub mod tps;\nuse symphony_core::WorkflowStore;\nuse symphony_runtime::{RuntimeHandle, Snapshot};\nuse tokio::sync::watch;\n\npub struct RuntimeSource { handle: RuntimeHandle }\nimpl RuntimeSource { pub fn new(handle: RuntimeHandle) -> Self { Self { handle } } }\npub async fn run(source: RuntimeSource, workflow: WorkflowStore) {\n    let mut updates = source.handle.subscribe();\n    while updates.changed().await.is_ok() { let frame = format::render(&source.handle.snapshot().await); print!(\"{frame}\"); }\n}\n",
        ),
        (
            "crates/symphony/src/control.rs",
            "use std::sync::Arc;\nuse symphony_runtime::RuntimeHandle;\nuse symphony_server::ControlPlane;\npub struct RuntimeControlPlane { handle: RuntimeHandle }\nimpl RuntimeControlPlane { pub fn new(handle: RuntimeHandle) -> Self { Self { handle } } }\n",
        ),
        (
            "crates/symphony-core/src/lib.rs",
            "//! symphony-core: the pure heart.\npub mod config;\npub mod env;\npub mod error;\npub mod issue;\npub mod prompt;\npub mod workflow;\npub mod workflow_store;\n\npub use config::Settings;\npub use issue::{BlockerRef, Issue};\npub use workflow::LoadedWorkflow;\npub use workflow_store::{WorkflowSnapshot, WorkflowStore};\n",
        ),
        (
            "crates/symphony-core/src/workflow_store.rs",
            "//! Last-known-good cache of WORKFLOW.md with stamp-based reload.\nuse std::fs;\nuse std::path::PathBuf;\nuse arc_swap::ArcSwap;\nuse crate::config::{cast_settings, Settings};\nuse crate::workflow::{load_workflow, LoadedWorkflow};\n\npub struct WorkflowStore { current: ArcSwap<LoadedWorkflow> }\nimpl WorkflowStore {\n    pub fn start(path: Option<PathBuf>) -> Result<Self, crate::ConfigError> {\n        let text = fs::read_to_string(path.unwrap_or_else(|| PathBuf::from(\"WORKFLOW.md\")))?;\n        let loaded = load_workflow(&text)?;\n        let settings: Settings = cast_settings(&loaded.front_matter)?;\n        settings.validate()?;\n        Ok(Self { current: ArcSwap::from_pointee(loaded) })\n    }\n}\n",
        ),
        (
            "crates/symphony-core/src/config/mod.rs",
            "//! Ecto-compatible casting of the front matter into typed settings.\nmod cast;\nmod schema;\nmod tracker;\npub use schema::{Settings, TrackerSettings, CodexSettings};\npub use cast::cast_settings;\n",
        ),
        (
            "crates/symphony-core/src/config/schema.rs",
            "use serde::Deserialize;\nuse super::tracker::validate_tracker;\n#[derive(Deserialize)]\npub struct Settings { pub tracker: TrackerSettings, pub codex: CodexSettings }\n#[derive(Deserialize)]\npub struct TrackerSettings { pub kind: String }\n#[derive(Deserialize)]\npub struct CodexSettings { pub command: String }\nimpl Settings { pub fn validate(&self) -> Result<(), crate::ConfigError> { validate_tracker(&self.tracker) } }\n",
        ),
        (
            "crates/symphony-runtime/src/lib.rs",
            "//! symphony-runtime: the scheduling heart.\nmod agents;\npub mod handle;\npub mod orchestrator;\npub mod process;\nmod recorder;\npub mod runner;\npub mod runtime;\npub mod snapshot;\npub mod ssh;\npub mod tracker;\npub mod worker;\npub mod workspace;\n\npub use handle::{RuntimeCommand, RuntimeHandle};\npub use runner::{AgentRunner, CodexWorkerFactory};\npub use runtime::{Runtime, RuntimeOptions};\npub use snapshot::{RunningSnapshot, Snapshot};\npub use worker::{WorkerContext, WorkerFactory};\npub use workspace::WorkspaceManager;\n",
        ),
        (
            "crates/symphony-runtime/src/runtime.rs",
            "//! The supervisor with the 3-restarts-in-5-s budget.\nuse std::sync::Arc;\nuse symphony_core::WorkflowStore;\nuse symphony_trackers::TrackerDeps;\nuse tokio::task::JoinHandle;\nuse crate::handle::RuntimeHandle;\nuse crate::orchestrator::Orchestrator;\nuse crate::runner::CodexWorkerFactory;\n\npub struct RuntimeOptions { workflow: WorkflowStore, deps: TrackerDeps }\nimpl RuntimeOptions { pub fn new(workflow: WorkflowStore, deps: TrackerDeps) -> Self { Self { workflow, deps } } }\npub struct Runtime { task: JoinHandle<()>, handle: RuntimeHandle }\nimpl Runtime {\n    pub fn start(options: RuntimeOptions) -> Self {\n        let (orchestrator, handle) = Orchestrator::new(options.workflow, Arc::new(CodexWorkerFactory::new(options.deps)));\n        let task = tokio::spawn(orchestrator.run());\n        Self { task, handle }\n    }\n    pub fn handle(&self) -> RuntimeHandle { self.handle.clone() }\n}\n",
        ),
        (
            "crates/symphony-runtime/src/orchestrator/mod.rs",
            "//! Scheduling rules: dispatch order, backoff, host selection.\nmod dispatch;\nmod reconcile;\nmod retry;\nmod state;\nmod workers;\n\nuse std::sync::Arc;\nuse symphony_codex::EventSink;\nuse symphony_core::{Issue, WorkflowStore};\nuse symphony_store::{NewRun, Store};\nuse tokio::sync::mpsc;\nuse crate::handle::RuntimeHandle;\nuse crate::recorder::RunRecorder;\nuse crate::tracker::IssueFetcher;\nuse crate::worker::WorkerFactory;\npub use state::OrchestratorState;\n\npub struct Orchestrator { state: OrchestratorState, store: Store, workers: Arc<dyn WorkerFactory> }\nimpl Orchestrator {\n    pub fn new(workflow: WorkflowStore, workers: Arc<dyn WorkerFactory>) -> (Self, RuntimeHandle) { todo!() }\n    pub async fn run(mut self) {\n        loop {\n            let issues = IssueFetcher::fetch(&self.state).await;\n            for issue in dispatch::ready(&self.state, issues) {\n                let run = self.store.start_run(NewRun::new(&issue.id, &issue.identifier)).await;\n                workers::spawn(&mut self.state, self.workers.clone(), issue, run);\n            }\n            reconcile::tick(&mut self.state);\n        }\n    }\n}\n",
        ),
        (
            "crates/symphony-runtime/src/orchestrator/dispatch.rs",
            "use symphony_core::Issue;\nuse super::state::OrchestratorState;\nuse super::retry::backoff;\npub fn ready(state: &OrchestratorState, issues: Vec<Issue>) -> Vec<Issue> { issues }\n",
        ),
        (
            "crates/symphony-runtime/src/orchestrator/workers.rs",
            "use std::sync::Arc;\nuse symphony_core::Issue;\nuse symphony_store::RunId;\nuse tokio::task::JoinSet;\nuse super::state::OrchestratorState;\nuse crate::worker::{WorkerContext, WorkerFactory};\npub fn spawn(state: &mut OrchestratorState, factory: Arc<dyn WorkerFactory>, issue: Issue, run: RunId) {\n    let context = WorkerContext::new(issue, run);\n    state.join_set.spawn(factory.run(context));\n}\n",
        ),
        (
            "crates/symphony-runtime/src/worker.rs",
            "//! The WorkerFactory seam between orchestrator and runner.\nuse futures::future::BoxFuture;\nuse symphony_core::Issue;\nuse symphony_store::RunId;\npub struct WorkerContext { pub issue: Issue, pub run: RunId }\nimpl WorkerContext { pub fn new(issue: Issue, run: RunId) -> Self { Self { issue, run } } }\npub trait WorkerFactory: Send + Sync { fn run(&self, context: WorkerContext) -> BoxFuture<'static, ()>; }\n",
        ),
        (
            "crates/symphony-runtime/src/runner.rs",
            "//! AgentRunner: workspace, hooks and a multi-turn Codex session per dispatch.\nuse std::sync::Arc;\nuse symphony_codex::{AppServerSession, StartOptions};\nuse symphony_core::prompt::build_turn_prompt;\nuse symphony_core::{Issue, WorkflowStore};\nuse symphony_trackers::ToolBinding;\nuse crate::process::EnvPolicy;\nuse crate::worker::{WorkerContext, WorkerFactory};\nuse crate::workspace::WorkspaceManager;\n\npub struct AgentRunner { workspaces: WorkspaceManager, workflow: WorkflowStore }\nimpl AgentRunner {\n    pub async fn run(&self, context: WorkerContext) {\n        let workspace = self.workspaces.prepare(&context.issue).await;\n        let mut session = AppServerSession::start(StartOptions::from_store(&self.workflow, &workspace)).await.unwrap();\n        let prompt = build_turn_prompt(&self.workflow, &context.issue);\n        session.run_turn(&prompt, &context.issue).await;\n    }\n}\npub struct CodexWorkerFactory;\nimpl CodexWorkerFactory { pub fn new(_: symphony_trackers::TrackerDeps) -> Self { Self } }\n",
        ),
        (
            "crates/symphony-runtime/src/workspace.rs",
            "//! WorkspaceManager: layout, reuse, hooks, removal, path safety.\nuse std::path::PathBuf;\nuse symphony_core::path_safety;\nuse symphony_core::{workspace_key, Issue};\nuse tokio::process::Command;\nuse crate::process::{run_hook, EnvPolicy};\npub struct WorkspaceManager { root: PathBuf }\nimpl WorkspaceManager {\n    pub async fn prepare(&self, issue: &Issue) -> PathBuf {\n        let dir = self.root.join(workspace_key(issue));\n        run_hook(Command::new(\"bash\").arg(\"-lc\").arg(\"after_create\"), EnvPolicy::default()).await;\n        dir\n    }\n}\n",
        ),
        (
            "crates/symphony-store/src/lib.rs",
            "//! symphony-store: SQLite persistence for agent runs.\nmod config;\nmod error;\nmod model;\nmod ops;\nmod schema;\nmod store;\n\npub use config::StoreConfig;\npub use error::{Result, StoreError};\npub use model::{NewRun, RunEvent, RunId, RunRecord, RunStatus};\npub use store::{Pending, Store};\n",
        ),
        (
            "crates/symphony-store/src/store.rs",
            "//! Cloneable handle; operations queue to a dedicated database thread.\nuse std::path::Path;\nuse rusqlite::{Connection, OpenFlags};\nuse crate::error::{Result, StoreError};\nuse crate::model::{NewRun, RunId, RunRecord, RunStatus};\nuse crate::ops;\nuse crate::schema;\n\n#[derive(Clone)]\npub struct Store { path: std::path::PathBuf }\npub struct Pending<T>(T);\nimpl Store {\n    pub fn open(path: impl AsRef<Path>) -> Result<Self> {\n        let conn = Connection::open_with_flags(path.as_ref(), OpenFlags::default())?;\n        schema::migrate(&conn)?;\n        Ok(Self { path: path.as_ref().to_owned() })\n    }\n    pub async fn start_run(&self, run: NewRun) -> RunId {\n        let conn = Connection::open(&self.path).unwrap();\n        conn.execute(\"INSERT INTO runs (issue_id, issue_identifier, started_at) VALUES (?1, ?2, ?3)\", (&run.issue_id, &run.identifier, 0)).unwrap();\n        RunId(conn.last_insert_rowid())\n    }\n    pub fn list_runs(&self) -> Vec<RunRecord> { ops::list_runs(&Connection::open(&self.path).unwrap()) }\n}\n",
        ),
        (
            "crates/symphony-server/src/lib.rs",
            "//! symphony-server: the observability HTTP server (axum 0.8).\nmod api;\nmod app;\nmod assets;\nmod config;\nmod control;\nmod error;\nmod history;\npub mod presenter;\nmod server;\nmod sse;\npub mod view;\n\npub use app::{App, router};\npub use config::ServerConfig;\npub use control::ControlPlane;\npub use server::{BoundServer, serve};\n",
        ),
        (
            "crates/symphony-server/src/server.rs",
            "use std::sync::Arc;\nuse tokio::net::TcpListener;\nuse tokio_util::sync::CancellationToken;\nuse symphony_store::Store;\nuse crate::app::{router, App};\nuse crate::config::ServerConfig;\nuse crate::control::ControlPlane;\npub struct BoundServer;\npub async fn serve(config: ServerConfig, control: Arc<dyn ControlPlane>, store: Store, shutdown: CancellationToken) -> Result<BoundServer, crate::ServerError> {\n    let listener = TcpListener::bind((config.host.as_str(), config.port)).await?;\n    let app = router(App::new(control, store, config));\n    tokio::spawn(axum::serve(listener, app).with_graceful_shutdown(async move { shutdown.cancelled().await }));\n    Ok(BoundServer)\n}\n",
        ),
        (
            "crates/symphony-server/src/app.rs",
            "//! Router assembly, shared state and middleware.\nuse std::sync::Arc;\nuse axum::{routing::{get, post}, Router};\nuse symphony_store::Store;\nuse tokio::sync::broadcast;\nuse crate::config::ServerConfig;\nuse crate::control::ControlPlane;\nuse crate::history;\nuse crate::sse;\nuse crate::assets;\n\n#[derive(Clone)]\npub struct App { pub control: Arc<dyn ControlPlane>, pub store: Store, pub updates: broadcast::Sender<()> }\nimpl App { pub fn new(control: Arc<dyn ControlPlane>, store: Store, _config: ServerConfig) -> Self { let (updates, _) = broadcast::channel(16); Self { control, store, updates } } }\npub fn router(app: App) -> Router {\n    Router::new()\n        .route(\"/api/v1/state\", get(crate::api::state))\n        .route(\"/api/v1/refresh\", post(crate::api::refresh))\n        .route(\"/api/v1/events\", get(sse::events))\n        .route(\"/api/v1/runs\", get(history::list_runs))\n        .fallback(assets::serve_embedded)\n        .with_state(app)\n}\n",
        ),
        (
            "crates/symphony-server/src/sse.rs",
            "//! Server-Sent Events replacing the LiveView socket.\nuse axum::extract::State;\nuse axum::response::sse::{Event, Sse};\nuse futures::Stream;\nuse tokio_stream::wrappers::BroadcastStream;\nuse crate::app::App;\nuse crate::presenter::present;\npub async fn events(State(app): State<App>) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {\n    let updates = BroadcastStream::new(app.updates.subscribe());\n    Sse::new(futures::StreamExt::map(updates, |_| Ok(Event::default().data(present()))))\n}\n",
        ),
        (
            "crates/symphony-server/src/history.rs",
            "use axum::{extract::State, Json};\nuse symphony_store::RunRecord;\nuse crate::app::App;\npub async fn list_runs(State(app): State<App>) -> Json<Vec<RunRecord>> { Json(app.store.list_runs()) }\n",
        ),
        (
            "crates/symphony-trackers/src/lib.rs",
            "//! Issue tracker adapters.\npub mod error;\npub mod github;\npub mod jira;\npub mod linear;\npub mod rest;\npub mod tool;\npub mod transport;\n\nuse symphony_core::config::{Settings, TrackerSettings};\nuse symphony_core::Issue;\npub use linear::LinearTracker;\npub use tool::{ToolBinding, ToolContext};\npub use transport::HttpTransport;\n\npub struct TrackerDeps { transport: HttpTransport }\nimpl TrackerDeps { pub fn new() -> Result<Self, error::TrackerError> { Ok(Self { transport: HttpTransport::new()? }) } }\npub fn select(settings: &TrackerSettings, deps: &TrackerDeps) -> LinearTracker { LinearTracker::new(deps.transport.clone(), settings) }\n",
        ),
        (
            "crates/symphony-trackers/src/linear.rs",
            "//! Linear GraphQL adapter. Endpoint `tracker.endpoint` (default `https://api.linear.app/graphql`).\nuse serde_json::{json, Value};\nuse symphony_core::config::TrackerSettings;\nuse symphony_core::Issue;\nuse crate::error::TrackerError;\nuse crate::rest::opt_string;\nuse crate::tool::{ToolContext, ToolResult};\nuse crate::transport::HttpTransport;\n\npub struct LinearTracker { transport: HttpTransport, endpoint: String }\nimpl LinearTracker {\n    pub fn new(transport: HttpTransport, settings: &TrackerSettings) -> Self { Self { transport, endpoint: \"https://api.linear.app/graphql\".to_owned() } }\n    pub async fn fetch_candidate_issues(&self) -> Result<Vec<Issue>, TrackerError> {\n        let body = json!({ \"query\": \"{ issues { nodes { id identifier title } } }\" });\n        let value: Value = self.transport.post_json(&self.endpoint, &body).await?;\n        Ok(Vec::new())\n    }\n}\n",
        ),
        (
            "crates/symphony-codex/src/lib.rs",
            "//! symphony-codex: the Codex app-server client.\npub mod dynamic_tool;\npub mod error;\npub mod event;\npub mod launch;\npub mod protocol;\npub mod session;\npub mod tokens;\nmod transport;\n\npub use event::{CodexEvent, EventSink};\npub use session::{AppServerSession, StartOptions};\n",
        ),
        (
            "crates/symphony-codex/src/session.rs",
            "//! AppServerSession: start / run_turn / stop over newline-delimited JSON.\nuse std::path::PathBuf;\nuse serde_json::{json, Value};\nuse symphony_core::{Issue, WorkflowStore};\nuse tokio::process::Command;\nuse crate::event::{CodexEvent, EventSink};\nuse crate::launch::launch_command;\nuse crate::protocol::{initialize_request, turn_request};\nuse crate::transport::Transport;\n\npub struct StartOptions { pub cwd: PathBuf, pub command: String }\nimpl StartOptions { pub fn from_store(store: &WorkflowStore, cwd: impl Into<PathBuf>) -> Self { Self { cwd: cwd.into(), command: \"codex app-server\".to_owned() } } }\npub struct AppServerSession { transport: Transport }\nimpl AppServerSession {\n    pub async fn start(options: StartOptions) -> Result<Self, crate::error::CodexError> {\n        let mut command = Command::new(\"bash\");\n        command.arg(\"-lc\").arg(launch_command(&options.command)).current_dir(&options.cwd);\n        let transport = Transport::spawn(command)?;\n        transport.send(initialize_request()).await?;\n        Ok(Self { transport })\n    }\n    pub async fn run_turn(&mut self, prompt: &str, issue: &Issue) -> Value { self.transport.send(turn_request(prompt)).await.unwrap(); json!({}) }\n}\n",
        ),
    ];
    let mut files = BTreeMap::new();
    for (path, text) in entries {
        files.insert(path.to_owned(), text.to_owned());
    }
    files.insert(
        "web/src/main.tsx".to_owned(),
        "import { render } from 'preact';\nimport { App } from './App';\nimport { applyThemePreference } from './lib/theme';\nrender(<App />, document.getElementById('root')!);\n".to_owned(),
    );
    files.insert(
        "web/src/App.tsx".to_owned(),
        "import { useApi } from './api/context';\nimport { OverviewPage } from './features/overview/OverviewPage';\nimport { RunsPage } from './features/runs/RunsPage';\nimport { AppShell } from './layout/AppShell';\nimport { useLiveState } from './live/useLiveState';\nexport function App() { const api = useApi(); const state = useLiveState(api); return <AppShell><OverviewPage state={state} /><RunsPage /></AppShell>; }\n".to_owned(),
    );
    files.insert(
        "web/src/api/client.ts".to_owned(),
        "import type { StatePayload, RunPage } from './types';\nexport class ApiClient {\n  constructor(private base = '/api/v1') {}\n  async state(): Promise<StatePayload> { const r = await fetch(`${this.base}/state`); return r.json(); }\n  async runs(): Promise<RunPage> { const r = await fetch(`${this.base}/runs`); return r.json(); }\n  events(): EventSource { return new EventSource(`${this.base}/events`); }\n}\n".to_owned(),
    );
    files.insert(
        "web/src/live/useLiveState.ts".to_owned(),
        "import { useEffect, useState } from 'preact/hooks';\nimport { createLiveSource } from './liveSource';\nimport type { ApiClient } from '../api/client';\nexport function useLiveState(api: ApiClient) { const [state, setState] = useState(null); useEffect(() => createLiveSource(api, setState), [api]); return state; }\n".to_owned(),
    );
    files
}

// -- six-table SQL schema with ALTERs and a DROP --------------------------------

pub fn schema_tree() -> Vec<RepoTreeEntry> {
    [
        "db/",
        "db/migrations/",
        "db/migrations/0001_init.sql",
        "db/migrations/0002_users.sql",
        "db/migrations/0003_tasks_owner.sql",
    ]
    .iter()
    .map(|p| tree_entry(p))
    .collect()
}

pub fn schema_files() -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    files.insert(
        "db/migrations/0001_init.sql".to_owned(),
        r#"-- Projects, tasks and the agents that used to run them.
CREATE TABLE projects (
    id uuid PRIMARY KEY,
    name text NOT NULL
);

CREATE TABLE tasks (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title text NOT NULL,
    priority int DEFAULT 0,
    status text NOT NULL DEFAULT 'ready' CHECK (status IN ('ready', 'running'))
);

CREATE TABLE agents (
    id uuid PRIMARY KEY,
    name text
);

CREATE TABLE IF NOT EXISTS runs (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'planning',
    started_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS run_events (
    run_id  INTEGER NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    seq     INTEGER NOT NULL,
    kind    TEXT    NOT NULL,
    payload TEXT,   -- JSON document, or NULL
    PRIMARY KEY (run_id, seq)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS runs_status_idx ON runs (status, id);
"#
        .to_owned(),
    );
    files.insert(
        "db/migrations/0002_users.sql".to_owned(),
        "CREATE TABLE users (\n    id uuid PRIMARY KEY,\n    email text NOT NULL UNIQUE\n);\n\nCREATE TABLE sessions (\n    id uuid PRIMARY KEY,\n    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,\n    expires_at timestamptz NOT NULL\n);\n".to_owned(),
    );
    files.insert(
        "db/migrations/0003_tasks_owner.sql".to_owned(),
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS latest_run_id UUID;\nALTER TABLE tasks DROP COLUMN priority;\nALTER TABLE tasks RENAME COLUMN title TO name;\nALTER TABLE tasks ADD COLUMN owner_id uuid;\nALTER TABLE tasks ADD CONSTRAINT tasks_owner_fk FOREIGN KEY (owner_id) REFERENCES users (id);\nDROP TABLE IF EXISTS agents;\n".to_owned(),
    );
    files
}
