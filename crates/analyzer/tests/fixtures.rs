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
    // Maturity: eight axes in contract order, bounded, stage consistent.
    assert_eq!(a.maturity.axes.len(), 8);
    assert_eq!(
        a.maturity
            .axes
            .iter()
            .map(|x| x.key.as_str())
            .collect::<Vec<_>>(),
        [
            "containerization",
            "ci",
            "tests",
            "iac",
            "dependencies",
            "docs",
            "secrets",
            "runtime"
        ]
    );
    assert!(a.maturity.axes.iter().all(|x| x.score <= 100));
    assert!(a.maturity.overall <= 100);
    let expected_stage = if a.maturity.overall < 35 {
        "legacy"
    } else if a.maturity.overall < 70 {
        "transitional"
    } else {
        "modern"
    };
    assert_eq!(a.maturity.stage, expected_stage);
    // Facts: unique keys, detected == 1.0, inferred < 1.0.
    let mut seen = std::collections::HashSet::new();
    for f in &a.facts {
        assert!(seen.insert(&f.key), "duplicate fact key {}", f.key);
        match f.kind {
            FactKind::Detected => assert_eq!(f.confidence, 1.0, "{}", f.key),
            FactKind::Inferred => assert!(f.confidence < 1.0, "{}", f.key),
            FactKind::Configured => {
                assert_eq!(f.confidence, 1.0, "{}", f.key);
                assert_eq!(f.category, FactCategory::Configuration, "{}", f.key);
                assert_eq!(f.source_path, ".sdi.yml", "{}", f.key);
            }
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
    // Phase 2: a Vite build with no server framework is static output, so the
    // project keeps React *and* fits the static-hosting patterns.
    assert!(a.capabilities.contains(&"static_site".to_string()));
    assert!(a.capabilities.contains(&"framework:react".to_string()));
    assert!(
        !a.understanding.tldr.contains("static website"),
        "{}",
        a.understanding.tldr
    );
    assert!(
        a.understanding.architecture.contains("static files"),
        "{}",
        a.understanding.architecture
    );
    // The publish job pushes an image but deploys nowhere: that is a release.
    let deploy = a
        .facts
        .iter()
        .find(|f| f.key == "deploy:pipeline:.github/workflows/ci.yml")
        .expect("release pipeline fact");
    assert_eq!(deploy.value, "release");
    assert!(
        a.understanding
            .tldr
            .contains("built and released by GitHub Actions"),
        "{}",
        a.understanding.tldr
    );
    assert!(k.contains(&"ci:build:.github/workflows/ci.yml"), "{k:?}");
    // Runtime from the Dockerfile base image: Node 22 is supported in 2026.
    assert!(k.contains(&"runtime:node:22"), "{k:?}");
    assert!(!k.iter().any(|k| k.starts_with("runtime:eol:")), "{k:?}");
    assert!(a.maturity.overall >= 35, "{:#?}", a.maturity);

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

// ---------------------------------------------------------------------------
// Phase 2: legacy and mixed codebases
// ---------------------------------------------------------------------------

fn fact<'a>(a: &'a Analysis, key: &str) -> Option<&'a FactDraft> {
    a.facts.iter().find(|f| f.key == key)
}

fn axis<'a>(a: &'a Analysis, key: &str) -> &'a MaturityAxisDraft {
    a.maturity.axes.iter().find(|x| x.key == key).unwrap()
}

const JAVA_EE_POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>ordering</artifactId>
  <version>2.3.1</version>
  <packaging>war</packaging>
  <properties>
    <maven.compiler.source>1.8</maven.compiler.source>
    <maven.compiler.target>1.8</maven.compiler.target>
  </properties>
  <dependencies>
    <dependency>
      <groupId>javax.servlet</groupId>
      <artifactId>javax.servlet-api</artifactId>
      <version>3.1.0</version>
      <scope>provided</scope>
    </dependency>
    <dependency>
      <groupId>javax</groupId>
      <artifactId>javaee-api</artifactId>
      <version>7.0</version>
      <scope>provided</scope>
    </dependency>
    <dependency>
      <groupId>org.postgresql</groupId>
      <artifactId>postgresql</artifactId>
      <version>42.2.5</version>
    </dependency>
    <dependency>
      <groupId>junit</groupId>
      <artifactId>junit</artifactId>
      <version>4.12</version>
      <scope>test</scope>
    </dependency>
  </dependencies>
</project>
"#;

const WEB_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<web-app xmlns="http://xmlns.jcp.org/xml/ns/javaee" version="3.1">
  <servlet>
    <servlet-name>orders</servlet-name>
    <servlet-class>com.acme.OrdersServlet</servlet-class>
  </servlet>
  <context-param>
    <param-name>jdbcUrl</param-name>
    <param-value>jdbc:postgresql://db.internal/orders</param-value>
  </context-param>
</web-app>
"#;

const FTP_DEPLOY: &str = r#"#!/bin/sh
# upload the WAR to the production host
mvn package
lftp -u deploy,SUPERSECRET ftp://prod.acme.internal -e "put target/ordering.war; bye"
"#;

#[test]
fn java_ee_war_on_java_8() {
    let snap = snapshot(
        "acme/ordering",
        &[
            ("pom.xml", JAVA_EE_POM),
            ("src/main/webapp/WEB-INF/web.xml", WEB_XML),
            ("deploy.sh", FTP_DEPLOY),
            ("README.md", "# ordering\n\nInternal order entry.\n"),
        ],
        &[
            "src/main/webapp/index.jsp",
            "src/main/webapp/orders.jsp",
            "src/main/java/com/acme/OrdersServlet.java",
            "src/test/java/com/acme/OrdersServletTest.java",
        ],
    );
    let a = analyze_at(&snap, (2026, 10, 9));
    assert_common_invariants(&a);
    let k = keys(&a);
    for expected in [
        "lang:java",
        "package:maven",
        "framework:java_ee",
        "packaging:war",
        "config:web_xml",
        "pages:jsp",
        "deploy:ftp_script",
        "runtime:java:8",
        "runtime:eol:java",
        "testing:junit",
        "tests:unit",
        "docs:readme",
    ] {
        assert!(k.contains(&expected), "missing fact {expected}; got {k:?}");
    }
    assert!(!k.contains(&"framework:spring"));
    let java = fact(&a, "runtime:java:8").unwrap();
    assert_eq!(java.category, FactCategory::Runtime);
    assert_eq!(java.label, "Java 8");
    assert_eq!(java.value, "8");
    assert_eq!(java.source_path, "pom.xml");
    assert!(
        java.evidence.contains("maven.compiler.source"),
        "{}",
        java.evidence
    );
    let eol = fact(&a, "runtime:eol:java").unwrap();
    assert_eq!(eol.value, "2022-03-31");
    assert_eq!(eol.kind, FactKind::Detected);
    assert!(
        eol.evidence
            .starts_with("Java 8 reached end of life 2022-03-31"),
        "{}",
        eol.evidence
    );
    let ftp = fact(&a, "deploy:ftp_script").unwrap();
    assert_eq!(ftp.source_path, "deploy.sh");
    assert!(ftp.evidence.contains("lftp"), "{}", ftp.evidence);
    assert!(!ftp.evidence.contains("SUPERSECRET"));
    assert!(a.capabilities.contains(&"framework:java_ee".to_string()));
    assert!(a.capabilities.contains(&"lang:java".to_string()));
    assert!(has_edge(
        &a,
        "repository:acme/ordering",
        "CONTAINS",
        "service:ordering"
    ));
    assert!(has_edge(
        &a,
        "service:ordering",
        "DEPENDS_ON",
        "datastore:postgres"
    ));

    // Maturity: no Dockerfile, no CI, Java 8 past end of life → legacy.
    assert!(a.maturity.overall < 35, "{:#?}", a.maturity);
    assert_eq!(a.maturity.stage, "legacy");
    assert_eq!(axis(&a, "runtime").score, 0);
    assert!(
        axis(&a, "runtime").findings[0].starts_with("Java 8 reached end of life 2022-03-31"),
        "{:?}",
        axis(&a, "runtime").findings
    );
    assert_eq!(
        axis(&a, "runtime").recommended_patterns,
        vec!["upgrade-runtime"]
    );
    assert!(
        axis(&a, "containerization")
            .recommended_patterns
            .contains(&"containerize-legacy-app".to_string())
    );
    assert_eq!(axis(&a, "ci").recommended_patterns[0], "add-ci-baseline");
    assert!(
        axis(&a, "secrets")
            .recommended_patterns
            .contains(&"config-to-environment".to_string())
    );
    assert!(
        a.understanding.tldr.contains("Java EE"),
        "{}",
        a.understanding.tldr
    );
    assert!(
        a.understanding.architecture.contains("Legacy signals"),
        "{}",
        a.understanding.architecture
    );
    assert!(a.understanding.architecture.contains("WAR packaging"));
    assert!(
        a.understanding
            .architecture
            .contains("Java 8 past end of life")
    );
    assert!(a.profile_text.contains("Delivery maturity: legacy"));
    assert!(question(&a, "deployment_target").is_some());
    // Same commit a year earlier is still legacy; two years before the EOL date the runtime axis is clean.
    let earlier = analyze_at(&snap, (2021, 1, 1));
    assert!(!keys(&earlier).contains(&"runtime:eol:java"));
    assert_eq!(axis(&earlier, "runtime").score, 100);
}

const DOTNET_SLN: &str = r#"
Microsoft Visual Studio Solution File, Format Version 12.00
Project("{FAE04EC0-301F-11D3-BF4B-00C04F79EFBC}") = "Intranet", "Intranet\Intranet.csproj", "{1234}"
EndProject
"#;

const DOTNET_CSPROJ: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Project ToolsVersion="15.0" xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <PropertyGroup>
    <OutputType>Library</OutputType>
    <RootNamespace>Acme.Intranet</RootNamespace>
    <AssemblyName>Acme.Intranet</AssemblyName>
    <TargetFrameworkVersion>v4.8</TargetFrameworkVersion>
  </PropertyGroup>
  <ItemGroup>
    <Reference Include="System.Web" />
    <Reference Include="System.Web.Extensions" />
    <Reference Include="System.Data.SqlClient, Version=4.6.1.0, Culture=neutral" />
  </ItemGroup>
  <ItemGroup>
    <Content Include="Default.aspx" />
  </ItemGroup>
</Project>
"#;

const DOTNET_PACKAGES: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<packages>
  <package id="Newtonsoft.Json" version="12.0.3" targetFramework="net48" />
  <package id="EntityFramework" version="6.4.4" targetFramework="net48" />
</packages>
"#;

const DOTNET_WEB_CONFIG: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<configuration>
  <appSettings>
    <add key="SmtpHost" value="mail.acme.internal" />
  </appSettings>
  <connectionStrings>
    <add name="Intranet" connectionString="Server=sql01;Database=Intranet;Password=SUPERSECRET" providerName="System.Data.SqlClient" />
  </connectionStrings>
  <system.web>
    <compilation debug="true" targetFramework="4.8" />
    <httpRuntime targetFramework="4.8" />
  </system.web>
</configuration>
"#;

#[test]
fn dotnet_framework_48_web_forms() {
    let snap = snapshot(
        "acme/intranet",
        &[
            ("Intranet.sln", DOTNET_SLN),
            ("Intranet/Intranet.csproj", DOTNET_CSPROJ),
            ("Intranet/packages.config", DOTNET_PACKAGES),
            ("Intranet/Web.config", DOTNET_WEB_CONFIG),
        ],
        &[
            "Intranet/Default.aspx",
            "Intranet/Default.aspx.cs",
            "Intranet/Site.Master",
            "Intranet/Global.asax",
            "Intranet/Models/Employee.cs",
        ],
    );
    let a = analyze(&snap);
    assert_common_invariants(&a);
    let k = keys(&a);
    for expected in [
        "lang:csharp",
        "package:nuget",
        "framework:aspnet_webforms",
        "dotnet:framework",
        "dotnet:packages_config",
        "config:web_config",
        "pages:aspx",
        "runtime:dotnet_framework:4.8",
    ] {
        assert!(k.contains(&expected), "missing fact {expected}; got {k:?}");
    }
    assert!(!k.contains(&"framework:aspnet"), "{k:?}");
    assert!(
        !k.contains(&"runtime:eol:dotnet_framework"),
        "4.8 is still supported: {k:?}"
    );
    let rt = fact(&a, "runtime:dotnet_framework:4.8").unwrap();
    assert_eq!(rt.label, ".NET Framework 4.8");
    assert_eq!(rt.source_path, "Intranet/Intranet.csproj");
    let fw = fact(&a, "dotnet:framework").unwrap();
    assert_eq!(fw.value, "v4.8");
    assert!(
        a.dependencies
            .iter()
            .any(|d| d.name == "EntityFramework" && d.ecosystem == "nuget")
    );
    assert!(a.chunks.iter().all(|c| !c.content.contains("SUPERSECRET")));
    assert!(
        a.capabilities
            .contains(&"framework:aspnet_webforms".to_string())
    );
    assert!(has_edge(
        &a,
        "repository:acme/intranet",
        "CONTAINS",
        "service:acme.intranet"
    ));
    assert!(
        a.understanding.tldr.contains("ASP.NET Web Forms"),
        "{}",
        a.understanding.tldr
    );
    assert!(a.understanding.architecture.contains(".NET Framework"));
    assert!(
        axis(&a, "secrets")
            .findings
            .iter()
            .any(|f| f.contains("web.config"))
    );
    assert_eq!(a.maturity.stage, "legacy", "{:#?}", a.maturity);
}

const WP_COMPOSER: &str = r#"{
  "name": "acme/blog",
  "require": {
    "php": "^7.4",
    "johnpbloch/wordpress": "^5.8",
    "wpackagist-plugin/akismet": "^4.2"
  },
  "require-dev": { "phpunit/phpunit": "^9" }
}"#;

const WP_DOCKERFILE: &str = r#"FROM php:7.4-apache
COPY . /var/www/html
EXPOSE 80
"#;

#[test]
fn php_74_wordpress() {
    let snap = snapshot(
        "acme/blog",
        &[
            ("composer.json", WP_COMPOSER),
            ("Dockerfile", WP_DOCKERFILE),
            (
                "wp-config.php",
                "<?php define('DB_PASSWORD', 'SUPERSECRET');",
            ),
        ],
        &[
            "wp-content/themes/acme/style.css",
            "wp-content/themes/acme/functions.php",
            "index.php",
        ],
    );
    let a = analyze(&snap);
    assert_common_invariants(&a);
    let k = keys(&a);
    for expected in [
        "lang:php",
        "package:composer",
        "framework:wordpress",
        "runtime:php:7.4",
        "runtime:eol:php",
        "container:dockerfile:Dockerfile",
        "testing:phpunit",
    ] {
        assert!(k.contains(&expected), "missing fact {expected}; got {k:?}");
    }
    assert!(!k.contains(&"php:plain"), "composer is present: {k:?}");
    let php = fact(&a, "runtime:php:7.4").unwrap();
    assert_eq!(php.label, "PHP 7.4");
    // composer.json is read before the Dockerfile, so it is the cited source.
    assert_eq!(php.source_path, "composer.json");
    assert!(php.evidence.contains("require.php"), "{}", php.evidence);
    let eol = fact(&a, "runtime:eol:php").unwrap();
    assert_eq!(eol.value, "2022-11-28");
    assert!(a.chunks.iter().all(|c| !c.content.contains("SUPERSECRET")));
    assert!(
        a.understanding.tldr.contains("WordPress"),
        "{}",
        a.understanding.tldr
    );
    assert!(axis(&a, "runtime").findings[0].contains("PHP 7.4 reached end of life 2022-11-28"));
    assert!(axis(&a, "containerization").score >= 50);
}

const RAILS_GEMFILE: &str = r#"source 'https://rubygems.org'
ruby '2.6.6'

gem 'rails', '~> 5.2.4'
gem 'pg', '>= 0.18'
gem 'puma', '~> 3.11'

group :development, :test do
  gem 'rspec-rails', '~> 3.9'
end
"#;

#[test]
fn rails_5_on_ruby_26() {
    let snap = snapshot(
        "acme/crm",
        &[
            ("Gemfile", RAILS_GEMFILE),
            (".ruby-version", "2.6.6\n"),
            (
                "config.ru",
                "require_relative 'config/environment'\nrun Rails.application\n",
            ),
        ],
        &[
            "Gemfile.lock",
            "app/models/lead.rb",
            "spec/models/lead_spec.rb",
            "db/migrate/20200101_create_leads.rb",
        ],
    );
    let a = analyze(&snap);
    assert_common_invariants(&a);
    let k = keys(&a);
    for expected in [
        "lang:ruby",
        "package:bundler",
        "framework:rails",
        "runtime:ruby:2.6",
        "runtime:eol:ruby",
        "testing:rspec",
        "tests:unit",
        "security:lockfile",
    ] {
        assert!(k.contains(&expected), "missing fact {expected}; got {k:?}");
    }
    let ruby = fact(&a, "runtime:ruby:2.6").unwrap();
    assert_eq!(ruby.label, "Ruby 2.6");
    assert_eq!(ruby.source_path, ".ruby-version");
    assert_eq!(fact(&a, "runtime:eol:ruby").unwrap().value, "2022-04-12");
    assert!(
        a.understanding.tldr.contains("Ruby on Rails"),
        "{}",
        a.understanding.tldr
    );
    assert_eq!(a.maturity.stage, "legacy", "{:#?}", a.maturity);
    assert!(axis(&a, "runtime").findings[0].contains("Ruby 2.6 reached end of life"));
}

#[test]
fn node_16_react_with_nvmrc() {
    let snap = snapshot(
        "acme/dashboard",
        &[
            (
                "package.json",
                r#"{"name":"dashboard","private":true,"engines":{"node":">=16"},"scripts":{"build":"react-scripts build","test":"react-scripts test"},"dependencies":{"react":"^17.0.2","react-dom":"^17.0.2"},"devDependencies":{"jest":"^27"}}"#,
            ),
            (".nvmrc", "v16.20.2\n"),
            (
                "Dockerfile",
                "FROM node:16-alpine3.18\nWORKDIR /app\nCOPY . .\nRUN npm ci && npm run build\nHEALTHCHECK CMD wget -qO- http://localhost:3000/ || exit 1\nUSER node\nCMD [\"npx\", \"serve\", \"build\"]\n",
            ),
            ("README.md", "# dashboard\n"),
        ],
        &["package-lock.json", "src/index.js", "src/App.test.js"],
    );
    let a = analyze(&snap);
    assert_common_invariants(&a);
    let k = keys(&a);
    for expected in [
        "lang:javascript",
        "framework:react",
        "runtime:node:16",
        "runtime:eol:node",
        "runtime:alpine:3.18",
        "runtime:eol:alpine",
        "container:healthcheck:Dockerfile",
        "testing:jest",
    ] {
        assert!(k.contains(&expected), "missing fact {expected}; got {k:?}");
    }
    let node = fact(&a, "runtime:node:16").unwrap();
    assert_eq!(node.label, "Node 16");
    assert_eq!(node.value, "16");
    assert_eq!(node.source_path, ".nvmrc");
    assert_eq!(node.evidence, ".nvmrc: v16.20.2");
    let eol = fact(&a, "runtime:eol:node").unwrap();
    assert_eq!(eol.value, "2023-09-11");
    assert_eq!(
        eol.evidence,
        "Node 16 reached end of life 2023-09-11 (.nvmrc: v16.20.2)"
    );
    // Two runtimes (node, alpine), both end of life → runtime axis 0.
    assert_eq!(axis(&a, "runtime").score, 0, "{:#?}", axis(&a, "runtime"));
    assert!(
        axis(&a, "runtime")
            .findings
            .iter()
            .any(|f| f.starts_with("Node 16 reached end of life 2023-09-11"))
    );
    // Non-root user and a healthcheck earn containerization points.
    assert!(!k.iter().any(|k| k.starts_with("security:dockerfile_root")));
    assert_eq!(
        axis(&a, "containerization").score,
        80,
        "{:#?}",
        axis(&a, "containerization")
    );
    assert_eq!(a.maturity.stage, "transitional", "{:#?}", a.maturity);
    assert!(
        a.understanding
            .architecture
            .contains("Node 16 past end of life")
    );
}

#[test]
fn python_2_django_111() {
    let snap = snapshot(
        "acme/portal",
        &[
            ("requirements.txt", "Django==1.11.29\npsycopg2==2.8.6\n"),
            ("runtime.txt", "python-2.7.18\n"),
            (
                "setup.py",
                "from setuptools import setup\nsetup(name='portal')\n",
            ),
            (
                "app.py",
                "import os\nprint \"starting portal\"\ntry:\n    main()\nexcept Exception, e:\n    print e\n",
            ),
        ],
        &["portal/manage.py", "portal/settings.py"],
    );
    let a = analyze(&snap);
    assert_common_invariants(&a);
    let k = keys(&a);
    for expected in [
        "lang:python",
        "framework:django",
        "python:2",
        "dependency:eol:django",
        "runtime:python:2.7",
        "runtime:eol:python",
        "datastore:postgres",
    ] {
        assert!(k.contains(&expected), "missing fact {expected}; got {k:?}");
    }
    let py2 = fact(&a, "python:2").unwrap();
    assert_eq!(py2.kind, FactKind::Detected);
    assert!(py2.evidence.contains("Django"), "{}", py2.evidence);
    assert!(py2.evidence.contains("app.py"), "{}", py2.evidence);
    assert_eq!(fact(&a, "runtime:eol:python").unwrap().value, "2020-01-01");
    let django = fact(&a, "dependency:eol:django").unwrap();
    assert_eq!(django.source_path, "requirements.txt");
    assert!(
        axis(&a, "dependencies")
            .findings
            .iter()
            .any(|f| f.contains("Django"))
    );
    assert_eq!(axis(&a, "runtime").score, 0);
    assert_eq!(a.maturity.stage, "legacy", "{:#?}", a.maturity);
    assert!(a.understanding.architecture.contains("Python 2 code"));
}

const SDI_VALID: &str = r#"version: 1
deploy:
  target: azure
  environments: [staging, production]
  region: westeurope
budget:
  monthly_usd: 20
test:
  command: "cargo test"
  health_path: "/health"
  port: 8080
scan:
  exclude: ["docs/legacy/**", "vendor/**"]
notes: this key is not in the contract
"#;

#[test]
fn sdi_yml_valid_answers_questions() {
    let files = [
        (".sdi.yml", SDI_VALID),
        (
            "Cargo.toml",
            "[package]\nname = \"svc\"\nedition = \"2024\"\nrust-version = \"1.85\"\n[dependencies]\naxum = \"0.8\"\n",
        ),
        (
            "docs/legacy/old.md",
            "# Old\n\nShould be excluded by scan.exclude.\n",
        ),
    ];
    let tree: Vec<TreeEntry> = files
        .iter()
        .map(|(p, c)| TreeEntry {
            path: p.to_string(),
            size: c.len() as u64,
            is_dir: false,
        })
        .chain([TreeEntry {
            path: "vendor/x/package.json".into(),
            size: 10,
            is_dir: false,
        }])
        .collect();
    // Two-phase scan: read the config first, then rank with its excludes.
    let first = wanted_files(&tree);
    assert_eq!(first[0], ".sdi.yml");
    let cfg = parse_config(SDI_VALID);
    assert!(config_is_usable(&cfg));
    let wanted = wanted_files_with(&tree, &cfg.scan_exclude);
    assert!(
        !wanted.iter().any(|w| w.starts_with("docs/legacy/")),
        "{wanted:?}"
    );
    assert!(
        !wanted.iter().any(|w| w.starts_with("vendor/")),
        "{wanted:?}"
    );
    let snap = RepoSnapshot {
        full_name: "acme/svc".into(),
        tree,
        files: files
            .iter()
            .filter(|(p, _)| wanted.iter().any(|w| w == p))
            .map(|(p, c)| (p.to_string(), c.to_string()))
            .collect(),
        ..Default::default()
    };
    let a = analyze(&snap);
    assert_common_invariants(&a);
    assert!(a.config.present);
    assert_eq!(a.config.path, ".sdi.yml");
    assert_eq!(a.config.deploy_target, "azure");
    assert_eq!(a.config.environments, vec!["staging", "production"]);
    assert_eq!(a.config.region, "westeurope");
    assert_eq!(a.config.budget_monthly_usd, 20.0);
    assert_eq!(a.config.test_command, "cargo test");
    assert_eq!(a.config.health_path, "/health");
    assert_eq!(a.config.port, 8080);
    assert_eq!(a.config.scan_exclude, vec!["docs/legacy/**", "vendor/**"]);
    assert_eq!(a.config.errors.len(), 1, "{:?}", a.config.errors);
    assert!(
        a.config.errors[0].contains("`notes`"),
        "{:?}",
        a.config.errors
    );

    let k = keys(&a);
    // A manifest that pins both the MSRV and the edition yields both facts.
    assert!(k.contains(&"runtime:rust:1.85"), "{k:?}");
    assert!(k.contains(&"runtime:rust:edition-2024"), "{k:?}");
    for expected in [
        "config:deploy.target",
        "config:deploy.environments",
        "config:deploy.region",
        "config:budget.monthly_usd",
        "config:test.command",
        "config:test.health_path",
        "config:test.port",
        "config:scan.exclude",
    ] {
        assert!(k.contains(&expected), "missing fact {expected}; got {k:?}");
    }
    let target = fact(&a, "config:deploy.target").unwrap();
    assert_eq!(target.kind, FactKind::Configured);
    assert_eq!(target.category, FactCategory::Configuration);
    assert_eq!(target.value, "azure");
    assert_eq!(target.confidence, 1.0);
    assert_eq!(target.source_path, ".sdi.yml");
    assert!(
        target.evidence.contains("target: azure"),
        "{}",
        target.evidence
    );
    assert_eq!(fact(&a, "config:budget.monthly_usd").unwrap().value, "20");
    assert_eq!(fact(&a, "config:test.port").unwrap().value, "8080");

    let q = question(&a, "deployment_target").expect("question still listed, pre-answered");
    assert_eq!(q.answer, "Azure");
    assert_eq!(q.answered_by, "config");
    let e = question(&a, "environments").expect("question still listed, pre-answered");
    assert_eq!(e.answer, "Staging and production");
    assert_eq!(e.answered_by, "config");
    assert!(k.contains(&"runtime:rust:1.85"), "{k:?}");
    assert!(
        a.understanding
            .sections
            .iter()
            .find(|s| s.key == "deployment")
            .unwrap()
            .summary
            .contains("configured target Azure"),
        "{:#?}",
        a.understanding.sections
    );
    assert!(
        axis(&a, "tests").summary.contains("cargo test"),
        "{:#?}",
        axis(&a, "tests")
    );
}

#[test]
fn sdi_yml_invalid_is_reported_and_ignored() {
    for (content, needle) in [
        ("version: 2\ndeploy:\n  target: vm\n", "`version` must be 1"),
        ("deploy:\n  target: vm\n", "`version: 1` is required"),
        (
            "version: 1\ndeploy:\n  target: [vm]\n",
            "`deploy.target` must be a string",
        ),
        (
            "version: 1\ndeploy:\n  target: mainframe\n",
            "`deploy.target` must be one of",
        ),
        ("version: 1\nbudget: twenty\n", "`budget` must be a mapping"),
        (
            "version: 1\ntest:\n  port: \"eighty\"\n",
            "`test.port` must be a port",
        ),
        ("{{{ not yaml", "does not parse"),
    ] {
        let snap = snapshot(
            "acme/svc",
            &[
                (".sdi.yml", content),
                ("package.json", "{\"name\":\"svc\"}"),
            ],
            &[],
        );
        let a = analyze(&snap);
        assert_common_invariants(&a);
        assert!(a.config.present, "{content}");
        assert!(a.config.deploy_target.is_empty(), "{content}");
        assert!(
            a.config.errors.iter().any(|e| e.contains(needle)),
            "{content}: {:?}",
            a.config.errors
        );
        assert!(a.config.errors.iter().any(|e| e.starts_with("error:")));
        assert!(!config_is_usable(&a.config));
        assert!(
            !keys(&a).iter().any(|k| k.starts_with("config:")),
            "{content}"
        );
        let q = question(&a, "deployment_target").unwrap();
        assert!(q.answer.is_empty(), "{content}");
        assert!(q.answered_by.is_empty(), "{content}");
    }
}

#[test]
fn deploy_to_cloud_is_not_a_release() {
    let workflow = r#"name: Deploy
on: { push: { branches: [main] } }
jobs:
  deploy:
    runs-on: ubuntu-latest
    environment: production
    steps:
      - uses: actions/checkout@v4
      - run: docker build -t app .
      - uses: azure/login@v2
      - uses: azure/container-apps-deploy-action@v2
"#;
    let snap = snapshot(
        "acme/api",
        &[
            ("Dockerfile", "FROM python:3.12-slim\n"),
            ("requirements.txt", "fastapi\n"),
            (".github/workflows/deploy.yml", workflow),
        ],
        &[],
    );
    let a = analyze(&snap);
    let d = fact(&a, "deploy:pipeline:.github/workflows/deploy.yml").unwrap();
    assert_eq!(d.value, "azure");
    assert!(
        a.understanding
            .tldr
            .contains("built and deployed by GitHub Actions"),
        "{}",
        a.understanding.tldr
    );
    assert!(keys(&a).contains(&"runtime:python:3.12"));
    assert!(!keys(&a).iter().any(|k| k.starts_with("runtime:eol:")));
}

#[test]
fn release_only_pipeline_is_worded_as_release() {
    let workflow = r#"name: Release
on:
  push:
    tags: ['v*']
jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --release
      - run: cargo test
      - uses: softprops/action-gh-release@v2
"#;
    let snap = snapshot(
        "acme/cli",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"cli\"\nedition = \"2021\"\n",
            ),
            (".github/workflows/release.yml", workflow),
        ],
        &["src/main.rs"],
    );
    let a = analyze(&snap);
    let d = fact(&a, "deploy:pipeline:.github/workflows/release.yml").unwrap();
    assert_eq!(d.value, "release");
    assert_eq!(d.label, "Releases from CI");
    assert!(
        a.understanding
            .tldr
            .contains("built and released by GitHub Actions"),
        "{}",
        a.understanding.tldr
    );
    assert!(
        a.understanding
            .architecture
            .contains("releases every change")
    );
    assert!(keys(&a).contains(&"runtime:rust:edition-2021"));
    assert!(
        axis(&a, "ci").summary.contains("releases"),
        "{:#?}",
        axis(&a, "ci")
    );
}

#[test]
fn mixed_legacy_signals_go_c_perl_asp_static() {
    let snap = snapshot(
        "acme/misc",
        &[
            (
                "go.mod",
                "module acme/misc\n\ngo 1.20\n\nrequire github.com/gorilla/mux v1.8.0\n",
            ),
            (
                "CMakeLists.txt",
                "cmake_minimum_required(VERSION 3.10)\nproject(misc C CXX)\n",
            ),
            ("cpanfile", "requires 'Mojolicious', '9.0';\n"),
            (".ftpconfig", "{\"host\":\"ftp.acme.internal\"}"),
        ],
        &[
            "native/main.cpp",
            "scripts/report.pl",
            "legacy/default.asp",
            "legacy/orders.asp",
        ],
    );
    let a = analyze(&snap);
    assert_common_invariants(&a);
    let k = keys(&a);
    for expected in [
        "lang:go",
        "framework:gorilla",
        "runtime:go:1.20",
        "runtime:eol:go",
        "build:cmake",
        "lang:perl",
        "package:cpan",
        "framework:mojolicious",
        "pages:classic_asp",
        "deploy:ftp_script",
    ] {
        assert!(k.contains(&expected), "missing fact {expected}; got {k:?}");
    }
    assert_eq!(fact(&a, "pages:classic_asp").unwrap().value, "2 pages");
    assert_eq!(
        fact(&a, "deploy:ftp_script").unwrap().source_path,
        ".ftpconfig"
    );
}

#[test]
fn plain_php_without_composer() {
    let snap = snapshot(
        "acme/shop",
        &[
            ("index.php", "<?php echo 'hi';"),
            ("deploy.bat", "ftp -s:upload.txt ftp.acme.internal\n"),
        ],
        &["inc/db.php", "admin/orders.php"],
    );
    let a = analyze(&snap);
    let k = keys(&a);
    assert!(k.contains(&"lang:php"), "{k:?}");
    assert!(k.contains(&"php:plain"), "{k:?}");
    assert!(k.contains(&"deploy:ftp_script"), "{k:?}");
    assert!(
        a.understanding
            .architecture
            .contains("plain PHP without Composer")
    );
    assert_eq!(a.maturity.stage, "legacy");
}
