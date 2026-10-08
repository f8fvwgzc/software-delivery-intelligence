//! End-to-end fixtures: whole repositories as inline strings. Each asserts
//! the facts, capabilities, graph, questions and understanding the
//! knowledge service relies on, plus that no secret ever reaches a chunk.

use std::collections::BTreeMap;

use sdi_analyzer::*;

fn snapshot(full_name: &str, files: &[(&str, &str)], extra_tree: &[&str]) -> RepoSnapshot {
    let mut tree: Vec<TreeEntry> = files
        .iter()
        .map(|(p, c)| TreeEntry {
            path: p.to_string(),
            size: c.len() as u64,
            is_dir: false,
        })
        .collect();
    tree.extend(extra_tree.iter().map(|p| TreeEntry {
        path: p.to_string(),
        size: 100,
        is_dir: false,
    }));
    let wanted = wanted_files(&tree);
    let files: BTreeMap<String, String> = files
        .iter()
        .filter(|(p, _)| wanted.iter().any(|w| w == p))
        .map(|(p, c)| (p.to_string(), c.to_string()))
        .collect();
    RepoSnapshot {
        full_name: full_name.into(),
        default_branch: "main".into(),
        commit_sha: "abc123".into(),
        tree,
        files,
        ..Default::default()
    }
}

fn keys(a: &Analysis) -> Vec<&str> {
    a.facts.iter().map(|f| f.key.as_str()).collect()
}

fn has_edge(a: &Analysis, source: &str, relation: &str, target: &str) -> bool {
    a.edges
        .iter()
        .any(|e| e.source_key == source && e.relation == relation && e.target_key == target)
}

fn has_edge_kind(a: &Analysis, relation: &str, target: &str) -> bool {
    a.edges
        .iter()
        .any(|e| e.relation == relation && e.target_key == target)
}

fn question<'a>(a: &'a Analysis, key: &str) -> Option<&'a QuestionDraft> {
    a.questions.iter().find(|q| q.key == key)
}

fn assert_common_invariants(a: &Analysis) {
    assert!(!a.understanding.tldr.is_empty(), "tldr must not be empty");
    assert!(!a.understanding.architecture.is_empty());
    assert!(!a.understanding.request_flow.is_empty());
    assert_eq!(a.understanding.sections.len(), 11);
    assert!(!a.profile_text.is_empty());
    // Facts: unique keys, detected == 1.0, inferred < 1.0.
    let mut seen = std::collections::HashSet::new();
    for f in &a.facts {
        assert!(seen.insert(&f.key), "duplicate fact key {}", f.key);
        match f.kind {
            FactKind::Detected => assert_eq!(f.confidence, 1.0, "{}", f.key),
            FactKind::Inferred => assert!(f.confidence < 1.0, "{}", f.key),
        }
    }
    // Graph: unique node keys, every edge endpoint exists, one repository root.
    let mut node_keys = std::collections::HashSet::new();
    for n in &a.nodes {
        assert!(node_keys.insert(n.key.as_str()), "duplicate node {}", n.key);
    }
    for e in &a.edges {
        assert!(
            node_keys.contains(e.source_key.as_str()),
            "dangling source {}",
            e.source_key
        );
        assert!(
            node_keys.contains(e.target_key.as_str()),
            "dangling target {}",
            e.target_key
        );
    }
    assert_eq!(
        a.nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Repository)
            .count(),
        1
    );
    // Chunks never carry secrets.
    for c in &a.chunks {
        assert!(
            !c.content.contains("SUPERSECRET"),
            "secret leaked in chunk {}",
            c.title
        );
        assert!(
            !c.path.ends_with(".env") && !c.path.ends_with(".pem"),
            "secret path {}",
            c.path
        );
        assert!(!c.content.is_empty());
    }
    assert!(a.chunks.iter().any(|c| c.kind == ChunkKind::Architecture));
}

// ---------------------------------------------------------------------------
// (a) React + Vite + TypeScript with Dockerfile and GitHub Actions
// ---------------------------------------------------------------------------

const REACT_PACKAGE_JSON: &str = r#"{
  "name": "view-engine",
  "private": true,
  "packageManager": "pnpm@9.0.0",
  "scripts": { "dev": "vite", "build": "tsc -b && vite build", "test": "vitest run" },
  "dependencies": { "react": "^19.0.0", "react-dom": "^19.0.0" },
  "devDependencies": { "typescript": "^5.6.0", "vite": "^6.0.0", "vitest": "^2.0.0", "@playwright/test": "^1.48.0" }
}"#;

const REACT_DOCKERFILE: &str = r#"FROM node:22-alpine AS build
WORKDIR /app
COPY . .
RUN corepack enable && pnpm install && pnpm build

FROM nginx:1.27-alpine
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80
"#;

const REACT_WORKFLOW: &str = r#"name: CI
on:
  push:
    branches: [main]
  pull_request:
permissions:
  contents: read
  packages: write
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - run: pnpm install
      - name: Unit tests
        run: pnpm test
      - run: pnpm build
  publish:
    needs: test
    runs-on: ubuntu-latest
    environment: production
    steps:
      - uses: actions/checkout@v4
      - uses: docker/login-action@v3
      - name: Build and push
        uses: docker/build-push-action@v6
        with:
          push: true
          tags: ghcr.io/f8fvwgzc/view-engine:latest
"#;

const REACT_README: &str = r#"# view-engine

A small React playground.

## Getting started

```sh
pnpm install
pnpm dev
```

## Deployment

The image is pushed to GHCR by GitHub Actions.
"#;

#[test]
fn react_vite_repo() {
    let snap = snapshot(
        "f8fvwgzc/view-engine",
        &[
            ("package.json", REACT_PACKAGE_JSON),
            ("Dockerfile", REACT_DOCKERFILE),
            (".github/workflows/ci.yml", REACT_WORKFLOW),
            ("README.md", REACT_README),
            ("tsconfig.json", "{\"compilerOptions\":{\"strict\":true}}"),
            ("vite.config.ts", "export default {}"),
            (".env", "VITE_API_KEY=SUPERSECRET"),
        ],
        &["pnpm-lock.yaml", "src/main.tsx", "src/App.test.tsx"],
    );
    assert!(
        !snap.files.contains_key(".env"),
        ".env must never be downloaded"
    );
    let a = analyze(&snap);
    assert_common_invariants(&a);

    let k = keys(&a);
    for expected in [
        "lang:typescript",
        "framework:react",
        "framework:vite",
        "package:pnpm",
        "container:dockerfile:Dockerfile",
        "container:multistage:Dockerfile",
        "ci:github_actions",
        "ci:workflow:.github/workflows/ci.yml",
        "testing:vitest",
        "testing:playwright",
        "tests:unit",
        "tests:e2e",
        "security:lockfile",
        "security:workflow_permissions",
        "docs:readme",
        "environment:production",
    ] {
        assert!(k.contains(&expected), "missing fact {expected}; got {k:?}");
    }
    for cap in [
        "lang:typescript",
        "framework:react",
        "package:pnpm",
        "dockerfile",
        "ci:github_actions",
        "tests:unit",
        "tests:e2e",
        "lockfile",
        "docs:readme",
    ] {
        assert!(
            a.capabilities.contains(&cap.to_string()),
            "missing capability {cap}; got {:?}",
            a.capabilities
        );
    }
    assert!(!a.capabilities.contains(&"static_site".to_string()));

    assert!(has_edge(
        &a,
        "repository:f8fvwgzc/view-engine",
        "CONTAINS",
        "service:view-engine"
    ));
    assert!(has_edge(
        &a,
        "service:view-engine",
        "PACKAGED_AS",
        "container_image:view-engine"
    ));
    assert!(has_edge(
        &a,
        "pipeline:.github/workflows/ci.yml",
        "TESTS",
        "service:view-engine"
    ));
    assert!(has_edge(
        &a,
        "pipeline:.github/workflows/ci.yml",
        "BUILDS",
        "container_image:view-engine"
    ));
    assert!(has_edge_kind(&a, "DEPLOYED_TO", "environment:production"));
    assert!(has_edge(
        &a,
        "service:view-engine",
        "TESTED_BY",
        "test_suite:vitest"
    ));

    assert_eq!(a.pipelines.len(), 1);
    let p = &a.pipelines[0];
    assert_eq!(p.name, "CI");
    assert_eq!(p.triggers, vec!["push", "pull_request"]);
    assert_eq!(p.jobs.len(), 2);
    assert_eq!(p.jobs[1].needs, vec!["test"]);
    assert!(p.jobs[1].steps.iter().any(|s| s == "Build and push"));

    // The image is pushed to a registry from CI, so the target is known enough not to ask.
    assert!(
        question(&a, "deployment_target").is_none(),
        "{:?}",
        a.questions
    );
    assert!(question(&a, "production_database").is_none());
    assert!(question(&a, "environments").is_none());

    assert!(
        a.understanding.tldr.contains("React"),
        "{}",
        a.understanding.tldr
    );
    assert_eq!(a.understanding.request_flow[0], "Browser");
    assert!(a.technologies.contains(&"React".to_string()));
    assert!(
        a.learned_patterns
            .iter()
            .any(|p| p.key == "deployment:docker+github_actions")
    );
    assert!(
        a.learned_patterns
            .iter()
            .any(|p| p.key == "ci:github_actions")
    );
    assert!(a.learned_patterns.iter().any(|p| p.key == "testing:vitest"));
    assert!(
        a.chunks
            .iter()
            .any(|c| c.kind == ChunkKind::Document && c.title.contains("Getting started"))
    );
    assert!(a.chunks.iter().any(|c| c.kind == ChunkKind::CiCd));
    assert!(
        a.chunks
            .iter()
            .any(|c| c.kind == ChunkKind::Code && c.path == "package.json")
    );
}

// ---------------------------------------------------------------------------
// (b) Rust axum + sqlx + docker-compose + Terraform on Azure
// ---------------------------------------------------------------------------

const RUST_CARGO: &str = r#"[package]
name = "texc-api"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "migrate"] }
redis = "0.27"
octocrab = "0.41"

[dev-dependencies]
proptest = "1"
"#;

const RUST_COMPOSE: &str = r#"services:
  api:
    build: .
    ports: ["8080:8080"]
    depends_on: [db, cache]
  db:
    image: postgres:16-alpine
    environment:
      POSTGRES_PASSWORD: SUPERSECRET
  cache:
    image: redis:7
"#;

const RUST_TF_MAIN: &str = r#"terraform {
  required_providers {
    azurerm = {
      source  = "hashicorp/azurerm"
      version = "~> 4.0"
    }
  }
  backend "azurerm" {}
}

provider "azurerm" {
  features {}
}

resource "azurerm_resource_group" "main" {
  name     = "rg-texc-prod"
  location = "westeurope"
}

resource "azurerm_container_app" "api" {
  name                = "texc-api"
  resource_group_name = azurerm_resource_group.main.name
}

resource "azurerm_postgresql_flexible_server" "db" {
  name                = "texc-db"
  administrator_password = var.db_password
}
"#;

const RUST_DOCKERFILE: &str = r#"FROM rust:1.85 AS builder
WORKDIR /src
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /src/target/release/texc-api /usr/local/bin/
USER 1000
EXPOSE 8080
CMD ["texc-api"]
"#;

#[test]
fn rust_axum_repo() {
    let snap = snapshot(
        "f8fvwgzc/texc-symphony",
        &[
            ("Cargo.toml", RUST_CARGO),
            ("docker-compose.yml", RUST_COMPOSE),
            ("Dockerfile", RUST_DOCKERFILE),
            ("infra/main.tf", RUST_TF_MAIN),
            ("infra/prod.tfvars", "db_password = \"SUPERSECRET\""),
            (
                "migrations/0001_init.sql",
                "CREATE TABLE users (id uuid primary key);\nCREATE TABLE IF NOT EXISTS scores (id bigserial);",
            ),
            (
                ".env.example",
                "DATABASE_URL=postgres://user:pass@localhost/db\nREDIS_URL=redis://localhost\n",
            ),
            (
                "certs/server.pem",
                "-----BEGIN PRIVATE KEY----- SUPERSECRET",
            ),
        ],
        &["Cargo.lock", "src/main.rs", "tests/api_test.rs"],
    );
    assert!(!snap.files.contains_key("certs/server.pem"));
    let a = analyze(&snap);
    assert_common_invariants(&a);

    let k = keys(&a);
    for expected in [
        "lang:rust",
        "framework:axum",
        "package:cargo",
        "datastore:postgres",
        "datastore:redis",
        "infra:datastore:postgres",
        "infra:datastore:redis",
        "container:dockerfile:Dockerfile",
        "container:compose:docker-compose.yml",
        "iac:terraform",
        "iac:terraform:provider:azurerm",
        "iac:terraform:backend:azurerm",
        "cloud:azure",
        "migrations:sqlx",
        "testing:cargo_test",
        "testing:proptest",
        "tests:unit",
        "security:env_example",
        "security:lockfile",
        "environment:production",
        "external_api:github",
    ] {
        assert!(k.contains(&expected), "missing fact {expected}; got {k:?}");
    }
    let azure = a.facts.iter().find(|f| f.key == "cloud:azure").unwrap();
    assert_eq!(azure.kind, FactKind::Detected);
    assert_eq!(azure.source_path, "infra/main.tf");
    let pg = a
        .facts
        .iter()
        .find(|f| f.key == "datastore:postgres")
        .unwrap();
    assert!(pg.evidence.contains("sqlx"), "{}", pg.evidence);
    assert!(
        a.facts
            .iter()
            .any(|f| f.kind == FactKind::Inferred && f.key == "architecture:polyglot_persistence")
    );
    // The Dockerfile sets USER, so no root-user inference.
    assert!(!k.iter().any(|k| k.starts_with("security:dockerfile_root")));

    for cap in [
        "lang:rust",
        "framework:axum",
        "database:postgres",
        "database:redis",
        "package:cargo",
        "dockerfile",
        "docker_compose",
        "iac:terraform",
        "cloud:azure",
        "migrations",
        "tests:unit",
        "env_example",
        "lockfile",
    ] {
        assert!(
            a.capabilities.contains(&cap.to_string()),
            "missing capability {cap}; got {:?}",
            a.capabilities
        );
    }

    assert!(has_edge(
        &a,
        "repository:f8fvwgzc/texc-symphony",
        "CONTAINS",
        "service:texc-api"
    ));
    assert!(has_edge(
        &a,
        "service:texc-api",
        "DEPENDS_ON",
        "datastore:postgres"
    ));
    assert!(has_edge(
        &a,
        "service:texc-api",
        "DEPENDS_ON",
        "datastore:redis"
    ));
    assert!(has_edge(
        &a,
        "service:texc-api",
        "USES",
        "external_api:github"
    ));
    assert!(has_edge(
        &a,
        "service:texc-api",
        "PACKAGED_AS",
        "container_image:texc-api"
    ));
    assert!(has_edge(
        &a,
        "service:texc-api",
        "DEPLOYED_TO",
        "environment:local"
    ));
    assert!(has_edge(
        &a,
        "iac_module:infra",
        "PROVISIONS",
        "infrastructure:azurerm_container_app.api"
    ));
    assert!(has_edge(
        &a,
        "infrastructure:azurerm_postgresql_flexible_server.db",
        "PROVIDES",
        "datastore:postgres"
    ));
    assert!(has_edge(
        &a,
        "migration:migrations",
        "APPLIES_TO",
        "datastore:postgres"
    ));
    assert!(has_edge(
        &a,
        "service:texc-api",
        "TESTED_BY",
        "test_suite:cargo_test"
    ));
    let migration = a
        .nodes
        .iter()
        .find(|n| n.key == "migration:migrations")
        .unwrap();
    assert_eq!(migration.attributes["tables"], "users,scores");

    // Everything deploy-related is known: nothing to ask.
    assert!(a.questions.is_empty(), "{:?}", a.questions);

    assert!(
        a.understanding.tldr.contains("axum"),
        "{}",
        a.understanding.tldr
    );
    assert!(a.understanding.tldr.contains("PostgreSQL"));
    assert!(a.understanding.tldr.contains("Azure"));
    assert_eq!(
        a.understanding.request_flow,
        vec!["Browser", "axum", "PostgreSQL", "Redis"]
    );
    assert!(
        a.learned_patterns
            .iter()
            .any(|p| p.key == "infrastructure:terraform+azure" && p.cloud == "azure")
    );
    assert!(
        a.learned_patterns
            .iter()
            .any(|p| p.key == "migrations:sqlx")
    );
    assert!(
        a.chunks
            .iter()
            .any(|c| c.kind == ChunkKind::Iac && c.path == "infra/main.tf")
    );
    assert!(a.profile_text.contains("Azure"));
    assert!(a.profile_text.contains("PostgreSQL"));
}

// ---------------------------------------------------------------------------
// (c) Python FastAPI + requirements, no CI: must ask where to deploy
// ---------------------------------------------------------------------------

#[test]
fn python_fastapi_repo_without_ci() {
    let snap = snapshot(
        "f8fvwgzc/book-api",
        &[
            (
                "requirements.txt",
                "fastapi>=0.110\nuvicorn[standard]\npsycopg[binary]==3.1\n",
            ),
            ("requirements-dev.txt", "pytest\nhttpx\n"),
            (
                "app/main.py",
                "from fastapi import FastAPI\napp = FastAPI()\n",
            ),
            ("README.md", "# book-api\n\nServes the study book.\n"),
        ],
        &["tests/test_main.py"],
    );
    let a = analyze(&snap);
    assert_common_invariants(&a);

    let k = keys(&a);
    for expected in [
        "lang:python",
        "framework:fastapi",
        "package:pip",
        "datastore:postgres",
        "testing:pytest",
        "tests:unit",
        "docs:readme",
    ] {
        assert!(k.contains(&expected), "missing fact {expected}; got {k:?}");
    }
    assert!(!k.iter().any(|k| k.starts_with("ci:")));
    assert!(!k.iter().any(|k| k.starts_with("cloud:")));
    assert!(a.capabilities.contains(&"framework:fastapi".to_string()));
    assert!(a.capabilities.contains(&"database:postgres".to_string()));
    assert!(!a.capabilities.contains(&"dockerfile".to_string()));
    assert!(a.pipelines.is_empty());

    let q = question(&a, "deployment_target").expect("deployment_target must be asked");
    assert_eq!(
        q.options,
        vec![
            "Azure",
            "AWS",
            "GCP",
            "Kubernetes",
            "VM",
            "I don't deploy yet"
        ]
    );
    assert!(q.allow_free_text);
    assert!(!q.reason.is_empty());
    let db = question(&a, "production_database").expect("psycopg without infra must ask");
    assert!(db.prompt.contains("PostgreSQL"), "{}", db.prompt);
    assert!(db.reason.contains("psycopg"), "{}", db.reason);
    assert!(question(&a, "environments").is_some());

    assert!(has_edge(
        &a,
        "service:book-api",
        "DEPENDS_ON",
        "datastore:postgres"
    ));
    assert!(has_edge(
        &a,
        "service:book-api",
        "TESTED_BY",
        "test_suite:pytest"
    ));
    assert!(
        a.understanding.tldr.contains("FastAPI"),
        "{}",
        a.understanding.tldr
    );
    assert_eq!(
        a.understanding.request_flow,
        vec!["Browser", "FastAPI", "PostgreSQL"]
    );
    let deploy = a
        .understanding
        .sections
        .iter()
        .find(|s| s.key == "deployment")
        .unwrap();
    assert!(deploy.summary.contains("No deployment target"));
    assert!(
        a.learned_patterns
            .iter()
            .all(|p| !p.key.starts_with("deployment:"))
    );
}

// ---------------------------------------------------------------------------
// (d) Static site: index.html only
// ---------------------------------------------------------------------------

#[test]
fn static_site_repo() {
    let snap = snapshot(
        "f8fvwgzc/book",
        &[
            (
                "index.html",
                "<!doctype html><html><head><title>Book</title></head><body><h1>Study book</h1></body></html>",
            ),
            ("assets/style.css", "body { margin: 0 }"),
        ],
        &["assets/logo.png"],
    );
    let a = analyze(&snap);
    assert_common_invariants(&a);

    let k = keys(&a);
    assert!(k.contains(&"lang:html"), "{k:?}");
    assert!(k.contains(&"repository:static_site"), "{k:?}");
    assert!(a.capabilities.contains(&"static_site".to_string()));
    assert!(a.capabilities.contains(&"lang:html".to_string()));
    assert!(!a.capabilities.iter().any(|c| c.starts_with("framework:")));
    assert!(a.dependencies.is_empty());
    assert!(a.nodes.iter().any(|n| n.kind == NodeKind::Repository));
    assert!(question(&a, "deployment_target").is_some());
    assert!(question(&a, "production_database").is_none());
    assert!(
        a.understanding.tldr.contains("static website"),
        "{}",
        a.understanding.tldr
    );
    assert_eq!(
        a.understanding.request_flow,
        vec!["Browser", "Static files"]
    );
}

// ---------------------------------------------------------------------------
// Robustness
// ---------------------------------------------------------------------------

#[test]
fn empty_snapshot_does_not_panic() {
    let a = analyze(&RepoSnapshot::default());
    assert!(!a.understanding.tldr.is_empty());
    assert_eq!(a.nodes.len(), 1);
    assert!(a.edges.is_empty());
    assert!(a.facts.is_empty());
    assert!(question(&a, "deployment_target").is_some());
    assert!(wanted_files(&[]).is_empty());
}

#[test]
fn garbage_files_do_not_panic() {
    let junk = "{{{ not: [yaml, or json\n\t= toml ???";
    let paths = [
        "package.json",
        "Cargo.toml",
        "pyproject.toml",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "Gemfile",
        "composer.json",
        "mix.exs",
        "requirements.txt",
        "Dockerfile",
        "docker-compose.yml",
        ".github/workflows/x.yml",
        ".gitlab-ci.yml",
        "infra/main.tf",
        "infra/app.bicep",
        "Pulumi.yaml",
        "k8s/deploy.yaml",
        "charts/app/Chart.yaml",
        ".github/dependabot.yml",
        "README.md",
        "migrations/1.sql",
        ".env.example",
    ];
    let files: Vec<(&str, &str)> = paths.iter().map(|p| (*p, junk)).collect();
    let snap = snapshot("x/y", &files, &[]);
    let a = analyze(&snap);
    assert_common_invariants(&a);
    assert!(a.facts.iter().any(|f| f.key == "iac:terraform"));
}
