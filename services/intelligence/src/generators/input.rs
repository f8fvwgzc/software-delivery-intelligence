//! Everything a generator may read, resolved once from the project's facts,
//! `.sdi.yml` and the negotiated parameters. Generators are pure functions of
//! this struct, so the same input always yields the same files.

use std::collections::{BTreeMap, BTreeSet};

use sdi_proto::knowledge::v1::{Fact, FactCategory, NodeKind, ProjectConfig, ProjectContext};

use crate::domain::model::PlanStepData;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Stack {
    Node,
    Rust,
    Python,
    Go,
    Java,
    DotNet,
    Php,
    Ruby,
    Static,
    #[default]
    Unknown,
}

impl Stack {
    pub fn label(self) -> &'static str {
        match self {
            Stack::Node => "Node.js",
            Stack::Rust => "Rust",
            Stack::Python => "Python",
            Stack::Go => "Go",
            Stack::Java => "Java",
            Stack::DotNet => ".NET",
            Stack::Php => "PHP",
            Stack::Ruby => "Ruby",
            Stack::Static => "static site",
            Stack::Unknown => "unknown",
        }
    }

    /// Runtime version to use when the scan found none: the current LTS line.
    pub fn default_runtime_version(self) -> &'static str {
        match self {
            Stack::Node | Stack::Static => "22",
            Stack::Rust => "1.85",
            Stack::Python => "3.12",
            Stack::Go => "1.23",
            Stack::Java => "21",
            Stack::DotNet => "8.0",
            Stack::Php => "8.3",
            Stack::Ruby => "3.3",
            Stack::Unknown => "",
        }
    }
}

/// One workspace package of a monorepo, as the graph's service and library
/// nodes describe it. Generators that run per package (the CI job per
/// package) read the stack from here instead of the repository-wide one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Package {
    /// Directory relative to the repository root; empty for a workspace
    /// whose manifest sits at the root (a Cargo or pnpm workspace builds and
    /// tests from there, never per member).
    pub dir: String,
    /// Package name from its manifest (the crate, npm or pyproject name).
    pub name: String,
    pub stack: Stack,
    pub package_manager: String,
    /// Lowercase framework name, or empty.
    pub framework: String,
    /// A library node: tested in CI, never the package an image is built from.
    pub is_library: bool,
}

impl Package {
    /// Frameworks that answer HTTP themselves: the package to containerise
    /// in a monorepo that also has a UI bundle or a library.
    pub fn is_server(&self) -> bool {
        matches!(
            self.framework.as_str(),
            "axum"
                | "actix"
                | "rocket"
                | "express"
                | "nestjs"
                | "fastify"
                | "koa"
                | "hono"
                | "nextjs"
                | "nuxt"
                | "fastapi"
                | "django"
                | "flask"
                | "spring"
                | "rails"
                | "laravel"
                | "phoenix"
                | "gin"
                | "echo"
                | "fiber"
                | "aspnet"
                | "aspnet_core"
        )
    }

    /// GitHub Actions job id: `backend/rlcd` -> `test-backend-rlcd`, the
    /// root workspace -> `test`.
    pub fn job_id(&self) -> String {
        if self.dir.is_empty() {
            return "test".to_owned();
        }
        let slug: String = self
            .dir
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        format!("test-{}", slug.trim_matches('-'))
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GenInput {
    pub repo_name: String,
    pub repo_full_name: String,
    pub owner: String,
    pub default_branch: String,
    /// Lowercase image / service name derived from the repository name.
    pub image: String,
    /// Name of the package the image is built from: the Rust binary, the npm
    /// package. Falls back to `image` when the scan recorded no package.
    pub app_name: String,
    /// Directory of the service the image is built from, relative to the
    /// repository root; empty when the application lives at the root. In a
    /// monorepo this is the server package (`backend`), so the Dockerfile,
    /// the compose build context and the image job all point at it.
    pub app_dir: String,
    pub stack: Stack,
    /// npm | pnpm | yarn | bun | cargo | pip | poetry | uv | go | maven | gradle | dotnet | composer | bundler
    pub package_manager: String,
    pub framework: String,
    pub runtime_version: String,
    pub port: u16,
    pub health_path: String,
    pub test_command: String,
    /// "azure" | "aws" | "gcp" | "" (none chosen)
    pub cloud: String,
    pub region: String,
    /// Engines: postgres | mysql | redis | mongodb | sqlite
    pub datastores: Vec<String>,
    pub env_vars: Vec<String>,
    /// Dependabot ecosystem names found in the repository.
    pub ecosystems: Vec<String>,
    /// Repository paths that exist (every fact's source path plus the keyed ones).
    pub existing_paths: BTreeSet<String>,
    pub is_private: bool,
    pub is_monorepo: bool,
    /// Workspace packages of a monorepo, each with its own stack (empty
    /// when the repository is a single package).
    pub packages: Vec<Package>,
    /// Build output directory of a static site.
    pub static_dir: String,
    pub replicas: u32,
    /// Service names from the graph, for the runbook.
    pub services: Vec<String>,
    pub plan: Vec<PlanStepData>,
    pub option_title: String,
    pub option_summary: String,
    /// Facts the input was built from; documents count them as cited.
    pub fact_ids: Vec<String>,
}

const DEFAULT_REGION: &[(&str, &str)] = &[
    ("azure", "westeurope"),
    ("aws", "eu-west-1"),
    ("gcp", "europe-west1"),
];

pub fn default_region(cloud: &str) -> &'static str {
    DEFAULT_REGION
        .iter()
        .find(|(c, _)| *c == cloud)
        .map(|(_, r)| *r)
        .unwrap_or("westeurope")
}

fn has_cap(ctx: &ProjectContext, cap: &str) -> bool {
    ctx.capabilities.iter().any(|c| c == cap)
}

fn cap_suffixes<'a>(ctx: &'a ProjectContext, prefix: &str) -> Vec<&'a str> {
    ctx.capabilities
        .iter()
        .filter_map(|c| c.strip_prefix(prefix))
        .collect()
}

fn stack_of_package_manager(package_manager: &str) -> Stack {
    match package_manager {
        "npm" | "pnpm" | "yarn" | "bun" => Stack::Node,
        "cargo" => Stack::Rust,
        "pip" | "poetry" | "uv" => Stack::Python,
        "go" | "gomod" => Stack::Go,
        "maven" | "gradle" => Stack::Java,
        "dotnet" | "nuget" => Stack::DotNet,
        "composer" => Stack::Php,
        "bundler" => Stack::Ruby,
        _ => Stack::Unknown,
    }
}

fn stack_of(ctx: &ProjectContext, package_manager: &str) -> Stack {
    let langs = cap_suffixes(ctx, "lang:");
    let has_lang = |l: &str| langs.contains(&l);
    if has_cap(ctx, "static_site") && !has_lang("rust") && !has_lang("go") {
        return Stack::Static;
    }
    let by_manager = stack_of_package_manager(package_manager);
    if by_manager != Stack::Unknown {
        return by_manager;
    }
    let primary = ctx
        .project
        .as_ref()
        .map(|p| p.primary_language.to_lowercase())
        .unwrap_or_default();
    let candidates: Vec<&str> = std::iter::once(primary.as_str())
        .chain(langs.iter().copied())
        .collect();
    for lang in candidates {
        let stack = match lang {
            "typescript" | "javascript" => Stack::Node,
            "rust" => Stack::Rust,
            "python" => Stack::Python,
            "go" => Stack::Go,
            "java" | "kotlin" => Stack::Java,
            "csharp" | "c#" => Stack::DotNet,
            "php" => Stack::Php,
            "ruby" => Stack::Ruby,
            "html" => Stack::Static,
            _ => continue,
        };
        return stack;
    }
    Stack::Unknown
}

fn package_manager_of(ctx: &ProjectContext) -> String {
    const ORDER: &[&str] = &[
        "pnpm", "yarn", "bun", "npm", "cargo", "uv", "poetry", "pip", "go", "maven", "gradle",
        "dotnet", "composer", "bundler", "mix",
    ];
    let found = cap_suffixes(ctx, "package:");
    ORDER
        .iter()
        .find(|pm| found.iter().any(|f| f == *pm))
        .map(|pm| (*pm).to_owned())
        .unwrap_or_default()
}

fn default_port(stack: Stack, framework: &str) -> u16 {
    match framework {
        "nextjs" | "nuxt" | "express" | "nestjs" | "rails" | "react" | "vue" | "svelte" => 3000,
        "fastapi" | "django" | "laravel" => 8000,
        "flask" => 5000,
        "astro" => 4321,
        _ => match stack {
            Stack::Node => 3000,
            Stack::Python => 8000,
            Stack::Ruby => 3000,
            Stack::Php => 9000,
            _ => 8080,
        },
    }
}

pub fn default_test_command(stack: Stack, package_manager: &str) -> String {
    match stack {
        Stack::Node | Stack::Static => match package_manager {
            "pnpm" => "pnpm test",
            "yarn" => "yarn test",
            "bun" => "bun test",
            _ => "npm test",
        },
        Stack::Rust => "cargo test --locked",
        Stack::Python => "pytest",
        Stack::Go => "go test ./...",
        Stack::Java => {
            if package_manager == "gradle" {
                "./gradlew test"
            } else {
                "mvn -B test"
            }
        }
        Stack::DotNet => "dotnet test",
        Stack::Php => "vendor/bin/phpunit",
        Stack::Ruby => "bundle exec rspec",
        Stack::Unknown => "make test",
    }
    .to_owned()
}

/// The version string a `runtime:*` fact or a negotiated "runtime" parameter
/// carries, e.g. "node 20" -> "20", "python3.11" -> "3.11".
pub fn version_digits(value: &str) -> String {
    let trimmed = value.trim().trim_start_matches('v');
    let start = trimmed.find(|c: char| c.is_ascii_digit());
    match start {
        Some(i) => trimmed[i..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect::<String>()
            .trim_end_matches('.')
            .to_owned(),
        None => String::new(),
    }
}

fn runtime_version_of(ctx: &ProjectContext, stack: Stack, cited: &mut Vec<String>) -> String {
    let wanted = match stack {
        Stack::Node | Stack::Static => "node",
        Stack::Rust => "rust",
        Stack::Python => "python",
        Stack::Go => "go",
        Stack::Java => "java",
        Stack::DotNet => "dotnet",
        Stack::Php => "php",
        Stack::Ruby => "ruby",
        Stack::Unknown => "",
    };
    ctx.facts
        .iter()
        .find(|f| {
            f.key
                .strip_prefix("runtime:")
                .is_some_and(|rest| rest.starts_with(wanted))
                && !wanted.is_empty()
        })
        .map(|f| {
            cited.push(f.id.clone());
            let version = version_digits(&f.value);
            if version.is_empty() {
                version_digits(f.key.trim_start_matches("runtime:"))
            } else {
                version
            }
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| stack.default_runtime_version().to_owned())
}

fn datastores_of(ctx: &ProjectContext, cited: &mut Vec<String>) -> Vec<String> {
    let mut engines: Vec<String> = Vec::new();
    for fact in &ctx.facts {
        let engine = fact
            .key
            .strip_prefix("datastore:")
            .or_else(|| fact.key.strip_prefix("infra:datastore:"));
        let Some(engine) = engine else {
            continue;
        };
        if engine.starts_with("orm:") || engine.starts_with("configured:") || engine.contains(':') {
            continue;
        }
        if fact.confidence < 0.8 {
            continue;
        }
        let engine = engine.to_lowercase();
        if !engines.contains(&engine) {
            engines.push(engine);
            cited.push(fact.id.clone());
        }
    }
    for cap in cap_suffixes(ctx, "database:") {
        let engine = cap.to_lowercase();
        if !engines.contains(&engine) {
            engines.push(engine);
        }
    }
    engines
}

fn env_vars_of(
    ctx: &ProjectContext,
    datastores: &[String],
    cited: &mut Vec<String>,
) -> Vec<String> {
    let mut vars: Vec<String> = vec!["PORT".to_owned()];
    let mut push = |name: &str| {
        let name = name.trim().to_owned();
        if !name.is_empty() && !vars.contains(&name) {
            vars.push(name);
        }
    };
    for fact in &ctx.facts {
        let from_key = fact
            .key
            .strip_prefix("env:")
            .or_else(|| fact.key.strip_prefix("config:env:"))
            .or_else(|| fact.key.strip_prefix("configuration:env:"));
        if let Some(name) = from_key {
            push(name);
            cited.push(fact.id.clone());
            continue;
        }
        if fact.category() == FactCategory::Configuration && is_env_name(&fact.value) {
            push(&fact.value);
            cited.push(fact.id.clone());
        }
    }
    for engine in datastores {
        match engine.as_str() {
            "postgres" | "mysql" | "sqlite" | "mongodb" => push("DATABASE_URL"),
            "redis" => push("REDIS_URL"),
            _ => {}
        }
    }
    vars
}

fn is_env_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && value.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn ecosystems_of(ctx: &ProjectContext, package_manager: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if !name.is_empty() && !out.iter().any(|e| e == name) {
            out.push(name.to_owned());
        }
    };
    let from_pm = |pm: &str| match pm {
        "npm" | "pnpm" | "yarn" | "bun" => "npm",
        "cargo" => "cargo",
        "pip" | "poetry" | "uv" => "pip",
        "go" | "gomod" => "gomod",
        "maven" => "maven",
        "gradle" => "gradle",
        "dotnet" | "nuget" => "nuget",
        "composer" => "composer",
        "bundler" => "bundler",
        "mix" => "mix",
        "docker" => "docker",
        "github_actions" | "github-actions" => "github-actions",
        _ => "",
    };
    push(from_pm(package_manager));
    for dep in &ctx.dependencies {
        push(from_pm(&dep.ecosystem));
    }
    for pm in cap_suffixes(ctx, "package:") {
        push(from_pm(pm));
    }
    if has_cap(ctx, "dockerfile") {
        push("docker");
    }
    if has_cap(ctx, "ci:github_actions") {
        push("github-actions");
    }
    out
}

fn existing_paths_of(facts: &[Fact]) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for fact in facts {
        if !fact.source_path.is_empty() {
            paths.insert(fact.source_path.trim_start_matches("./").to_owned());
        }
        for prefix in [
            "container:dockerfile:",
            "container:compose:",
            "ci:workflow:",
            "deploy:pipeline:",
            "iac:terraform:",
            "iac:bicep:",
        ] {
            if let Some(path) = fact.key.strip_prefix(prefix)
                && path.contains(['/', '.'])
            {
                paths.insert(path.to_owned());
            }
        }
    }
    paths
}

/// Packages from the graph's service and library nodes (the knowledge
/// service records `directory`, `package_manager`, `ecosystem` and
/// `frameworks` on each). Sorted by directory so output is stable.
fn packages_of(ctx: &ProjectContext) -> Vec<Package> {
    let mut packages: Vec<Package> = ctx
        .graph
        .as_ref()
        .map(|g| {
            g.nodes
                .iter()
                .filter(|n| matches!(n.kind(), NodeKind::Service | NodeKind::Library))
                .filter_map(|n| {
                    let dir = n.attributes.get("directory")?.trim_matches('/').to_owned();
                    if dir.is_empty() || dir == "." {
                        return None;
                    }
                    let is_library = n.kind() == NodeKind::Library;
                    let attr = |k: &str| n.attributes.get(k).cloned().unwrap_or_default();
                    let package_manager = match attr("package_manager") {
                        pm if !pm.is_empty() => pm,
                        _ => attr("ecosystem"),
                    }
                    .to_lowercase();
                    let framework = attr("frameworks")
                        .split(',')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_lowercase();
                    Some(Package {
                        dir,
                        name: if n.label.is_empty() {
                            n.key.rsplit(':').next().unwrap_or_default().to_owned()
                        } else {
                            n.label.clone()
                        },
                        stack: stack_of_package_manager(&package_manager),
                        package_manager,
                        framework,
                        is_library,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    packages.sort_by(|a, b| a.dir.cmp(&b.dir));
    packages.dedup_by(|a, b| a.dir == b.dir);
    collapse_workspaces(ctx, packages)
}

/// Root manifests that make every package of their stack one workspace:
/// `cargo test` in `crates/foo` of a Cargo workspace is not how the project
/// is built, and a pnpm workspace installs once at the root.
const WORKSPACE_MANIFESTS: &[(&str, Stack)] = &[
    ("Cargo.toml", Stack::Rust),
    ("Cargo.lock", Stack::Rust),
    ("package.json", Stack::Node),
    ("pnpm-workspace.yaml", Stack::Node),
    ("pnpm-lock.yaml", Stack::Node),
    ("package-lock.json", Stack::Node),
    ("yarn.lock", Stack::Node),
    ("go.work", Stack::Go),
    ("pom.xml", Stack::Java),
    ("settings.gradle", Stack::Java),
    ("settings.gradle.kts", Stack::Java),
];

/// Replaces the member packages of each stack that has a root manifest with
/// one root package (dir empty) named after its first service, keeping the
/// package manager and the first server framework among the members.
fn collapse_workspaces(ctx: &ProjectContext, packages: Vec<Package>) -> Vec<Package> {
    let root_manifests: BTreeSet<&str> = ctx
        .facts
        .iter()
        .filter(|f| f.key.starts_with("package:"))
        .map(|f| f.source_path.trim_start_matches("./"))
        .filter(|p| !p.contains('/'))
        .collect();
    let root_stacks: BTreeSet<Stack> = WORKSPACE_MANIFESTS
        .iter()
        .filter(|(manifest, _)| root_manifests.contains(manifest))
        .map(|(_, stack)| *stack)
        .collect();
    let mut out: Vec<Package> = Vec::new();
    let mut roots: Vec<Package> = Vec::new();
    for package in packages {
        let in_workspace = root_stacks.contains(&package.stack)
            || (package.stack == Stack::Static && root_stacks.contains(&Stack::Node));
        if !in_workspace {
            out.push(package);
            continue;
        }
        match roots.iter_mut().find(|r| r.stack == package.stack) {
            Some(root) => {
                if root.is_library && !package.is_library {
                    root.name = package.name.clone();
                    root.is_library = false;
                }
                if root.framework.is_empty() || (!root.is_server() && package.is_server()) {
                    root.framework = package.framework.clone();
                }
            }
            None => roots.push(Package {
                dir: String::new(),
                ..package
            }),
        }
    }
    roots.extend(out);
    roots
}

/// The service node at the repository root, for the name of a single-package
/// repository's binary.
fn root_package_name(ctx: &ProjectContext) -> Option<String> {
    ctx.graph.as_ref()?.nodes.iter().find_map(|n| {
        let dir = n.attributes.get("directory").map(|d| d.trim_matches('/'));
        (n.kind() == NodeKind::Service
            && matches!(dir, None | Some("") | Some("."))
            && !n.label.is_empty())
        .then(|| n.label.clone())
    })
}

/// First port the scanned Dockerfile of `app_dir` exposes (`ports` on the
/// container_image node is a comma-separated list); when no image lives
/// there but the repository has exactly one image (texc-symphony keeps its
/// Dockerfile under `docker/`), that one's port is what the app listens on.
fn existing_image_port(ctx: &ProjectContext, app_dir: &str) -> Option<u16> {
    let images: Vec<_> = ctx
        .graph
        .as_ref()?
        .nodes
        .iter()
        .filter(|n| n.kind() == NodeKind::ContainerImage)
        .collect();
    let port_of = |n: &sdi_proto::knowledge::v1::GraphNode| {
        n.attributes
            .get("ports")?
            .split(',')
            .find_map(|p| p.trim().parse::<u16>().ok())
            .filter(|p| *p > 0)
    };
    let in_app = images.iter().find(|n| {
        let dir = n
            .attributes
            .get("directory")
            .map(|d| d.trim_matches('/'))
            .unwrap_or("");
        dir == app_dir || (app_dir.is_empty() && dir == ".")
    });
    match (in_app, images.as_slice()) {
        (Some(image), _) => port_of(image),
        (None, [only]) => port_of(only),
        _ => None,
    }
}

/// The package the container image is built from: the server (an API
/// answers health checks, a UI bundle does not), else the first package
/// with a known stack that is not a plain Node bundle, else the first.
fn primary_package(packages: &[Package]) -> Option<&Package> {
    let apps = || packages.iter().filter(|p| !p.is_library);
    apps()
        .find(|p| p.is_server())
        .or_else(|| {
            apps().find(|p| !matches!(p.stack, Stack::Unknown | Stack::Node | Stack::Static))
        })
        .or_else(|| apps().find(|p| p.stack != Stack::Unknown))
        .or_else(|| packages.iter().find(|p| p.is_server()))
}

/// The project's `.sdi.yml` when its values may be used (CONTRACT.md §12):
/// present and nothing fatal in it. The analyzer reports fatal problems as
/// `error: …` and skipped unknown keys as `warning: …`, and a warning must
/// not discard the keys that did parse.
pub fn usable_config(ctx: &ProjectContext) -> Option<&ProjectConfig> {
    ctx.config
        .as_ref()
        .filter(|c| c.present && !c.errors.iter().any(|e| e.starts_with("error:")))
}

/// Negotiated parameters (`region`, `port`, `runtime`) as parsed from the
/// proposal's constraints; `cloud_hint` is the pattern's cloud, if any.
pub fn build(
    ctx: &ProjectContext,
    params: &BTreeMap<String, String>,
    cloud_hint: &str,
) -> GenInput {
    let mut cited: Vec<String> = Vec::new();
    let project = ctx.project.as_ref();
    let config = usable_config(ctx);

    let repo_full_name = project
        .map(|p| p.repo_full_name.clone())
        .unwrap_or_default();
    let (owner, repo_name) = match repo_full_name.split_once('/') {
        Some((o, r)) => (o.to_owned(), r.to_owned()),
        None => (
            String::new(),
            project.map(|p| p.name.clone()).unwrap_or_default(),
        ),
    };
    let repo_name = if repo_name.is_empty() {
        "app".to_owned()
    } else {
        repo_name
    };
    let image = repo_name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();

    let is_monorepo = has_cap(ctx, "monorepo");
    let packages = if is_monorepo {
        packages_of(ctx)
    } else {
        Vec::new()
    };
    // A monorepo is described by the package that serves traffic, not by
    // whichever package manager sorts first: view-engine is a Rust API plus
    // a React bundle and two Python services, and the image is the API.
    let (app_dir, app_name, package_manager, stack, framework) = match primary_package(&packages) {
        Some(primary) => (
            primary.dir.clone(),
            primary.name.clone(),
            primary.package_manager.clone(),
            primary.stack,
            primary.framework.clone(),
        ),
        None => {
            let package_manager = package_manager_of(ctx);
            let stack = stack_of(ctx, &package_manager);
            let framework = cap_suffixes(ctx, "framework:")
                .first()
                .map(|f| (*f).to_owned())
                .unwrap_or_default();
            (
                String::new(),
                root_package_name(ctx).unwrap_or_else(|| image.clone()),
                package_manager,
                stack,
                framework,
            )
        }
    };
    for fact in &ctx.facts {
        if fact.key.starts_with("lang:")
            || fact.key.starts_with("framework:")
            || fact.key.starts_with("package:")
        {
            cited.push(fact.id.clone());
        }
    }

    let runtime_version = params
        .get("runtime")
        .map(|v| version_digits(v))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| runtime_version_of(ctx, stack, &mut cited));

    // The port an existing image of this directory already exposes beats
    // the framework convention: the health check must hit what the app
    // listens on, not what Express usually uses.
    let port = params
        .get("port")
        .and_then(|p| p.parse::<u16>().ok())
        .or_else(|| {
            config
                .map(|c| c.port)
                .filter(|p| *p > 0)
                .and_then(|p| u16::try_from(p).ok())
        })
        .or_else(|| existing_image_port(ctx, &app_dir))
        .unwrap_or_else(|| default_port(stack, &framework));

    let cloud = match cloud_hint {
        "azure" | "aws" | "gcp" => cloud_hint.to_owned(),
        _ => config
            .map(|c| c.deploy_target.as_str())
            .filter(|t| matches!(*t, "azure" | "aws" | "gcp"))
            .map(str::to_owned)
            .or_else(|| cap_suffixes(ctx, "cloud:").first().map(|c| (*c).to_owned()))
            .unwrap_or_default(),
    };
    let region_cloud = if cloud.is_empty() {
        "azure"
    } else {
        cloud.as_str()
    };
    let region = params
        .get("region")
        .cloned()
        .or_else(|| config.map(|c| c.region.clone()).filter(|r| !r.is_empty()))
        .unwrap_or_else(|| default_region(region_cloud).to_owned());

    let datastores = datastores_of(ctx, &mut cited);
    let env_vars = env_vars_of(ctx, &datastores, &mut cited);
    let ecosystems = ecosystems_of(ctx, &package_manager);
    let services = ctx
        .graph
        .as_ref()
        .map(|g| {
            g.nodes
                .iter()
                .filter(|n| n.kind() == NodeKind::Service)
                .map(|n| {
                    if n.label.is_empty() {
                        n.key.clone()
                    } else {
                        n.label.clone()
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    cited.sort();
    cited.dedup();

    let static_dir = match framework.as_str() {
        "nextjs" => "out".to_owned(),
        "astro" | "vue" | "svelte" | "react" => "dist".to_owned(),
        _ if package_manager.is_empty() => ".".to_owned(),
        _ => "dist".to_owned(),
    };
    let health_path = config
        .map(|c| c.health_path.clone())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| {
            if stack == Stack::Static {
                "/".into()
            } else {
                "/health".into()
            }
        });
    let test_command = config
        .map(|c| c.test_command.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| default_test_command(stack, &package_manager));

    GenInput {
        repo_name,
        repo_full_name,
        owner,
        default_branch: project
            .map(|p| p.default_branch.clone())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "main".to_owned()),
        image,
        app_dir,
        app_name,
        stack,
        package_manager,
        framework,
        runtime_version,
        port,
        health_path,
        test_command,
        cloud,
        region,
        datastores,
        env_vars,
        ecosystems,
        existing_paths: existing_paths_of(&ctx.facts),
        is_private: project.is_some_and(|p| p.is_private),
        is_monorepo,
        packages,
        static_dir,
        replicas: if has_cap(ctx, "tests:e2e") { 2 } else { 1 },
        services,
        plan: Vec::new(),
        option_title: String::new(),
        option_summary: String::new(),
        fact_ids: cited,
    }
}

impl GenInput {
    pub fn exists(&self, path: &str) -> bool {
        self.existing_paths.contains(path)
    }

    /// A path inside the application directory: `Dockerfile` at the root,
    /// `backend/Dockerfile` in a monorepo whose server lives in `backend`.
    pub fn in_app(&self, path: &str) -> String {
        if self.app_dir.is_empty() {
            path.to_owned()
        } else {
            format!("{}/{path}", self.app_dir)
        }
    }

    /// Docker build context relative to the repository root (`.` or `./backend`).
    pub fn context_dir(&self) -> String {
        if self.app_dir.is_empty() {
            ".".to_owned()
        } else {
            format!("./{}", self.app_dir)
        }
    }

    pub fn has_datastore(&self, engine: &str) -> bool {
        self.datastores.iter().any(|d| d == engine)
    }

    /// The relational engine to provision a managed database for, if any.
    pub fn database_engine(&self) -> Option<&str> {
        self.datastores
            .iter()
            .map(String::as_str)
            .find(|d| matches!(*d, "postgres" | "mysql"))
    }

    pub fn image_ref(&self) -> String {
        if self.owner.is_empty() {
            format!("ghcr.io/{}", self.image)
        } else {
            format!("ghcr.io/{}/{}", self.owner.to_lowercase(), self.image)
        }
    }
}

#[cfg(test)]
pub mod fixtures {
    use super::*;
    use sdi_proto::knowledge::v1::{
        Dependency, Fact, FactCategory, FactKind, Project, ProjectConfig,
    };

    pub fn fact(id: &str, key: &str, value: &str, path: &str) -> Fact {
        Fact {
            id: id.to_owned(),
            project_id: "proj-1".to_owned(),
            kind: FactKind::Detected as i32,
            category: FactCategory::Container as i32,
            key: key.to_owned(),
            label: key.to_owned(),
            value: value.to_owned(),
            confidence: 1.0,
            source_path: path.to_owned(),
            evidence: String::new(),
            detected_at: None,
        }
    }

    /// A TypeScript service with pnpm, Postgres and an existing Dockerfile.
    pub fn node_context() -> ProjectContext {
        ProjectContext {
            project: Some(Project {
                id: "proj-1".into(),
                name: "view-engine".into(),
                repo_full_name: "f8fvwgzc/view-engine".into(),
                default_branch: "main".into(),
                primary_language: "TypeScript".into(),
                is_private: false,
                ..Default::default()
            }),
            facts: vec![
                fact("f1", "lang:typescript", "", "package.json"),
                fact("f2", "package:pnpm", "", "pnpm-lock.yaml"),
                fact("f3", "framework:express", "", "package.json"),
                fact(
                    "f4",
                    "container:dockerfile:Dockerfile",
                    "FROM node:18",
                    "Dockerfile",
                ),
                fact("f5", "datastore:postgres", "", "docker-compose.yml"),
                fact("f6", "runtime:node", "18", ".nvmrc"),
                fact(
                    "f7",
                    "ci:workflow:.github/workflows/ci.yml",
                    "",
                    ".github/workflows/ci.yml",
                ),
            ],
            capabilities: vec![
                "lang:typescript".into(),
                "package:pnpm".into(),
                "framework:express".into(),
                "dockerfile".into(),
                "database:postgres".into(),
                "ci:github_actions".into(),
            ],
            dependencies: vec![Dependency {
                ecosystem: "npm".into(),
                name: "express".into(),
                ..Default::default()
            }],
            config: Some(ProjectConfig {
                present: true,
                path: ".sdi.yml".into(),
                port: 3000,
                health_path: "/healthz".into(),
                test_command: "pnpm test".into(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    pub fn rust_context() -> ProjectContext {
        ProjectContext {
            project: Some(Project {
                id: "proj-2".into(),
                name: "texc-symphony".into(),
                repo_full_name: "f8fvwgzc/texc-symphony".into(),
                default_branch: "main".into(),
                primary_language: "Rust".into(),
                is_private: true,
                ..Default::default()
            }),
            facts: vec![
                fact("r1", "lang:rust", "", "Cargo.toml"),
                fact("r2", "package:cargo", "", "Cargo.lock"),
                fact("r3", "datastore:redis", "", "src/cache.rs"),
            ],
            capabilities: vec![
                "lang:rust".into(),
                "package:cargo".into(),
                "lockfile".into(),
            ],
            ..Default::default()
        }
    }

    pub fn python_context() -> ProjectContext {
        ProjectContext {
            project: Some(Project {
                id: "proj-3".into(),
                name: "api".into(),
                repo_full_name: "acme/api".into(),
                default_branch: "main".into(),
                primary_language: "Python".into(),
                ..Default::default()
            }),
            facts: vec![
                fact("p1", "lang:python", "", "pyproject.toml"),
                fact("p2", "package:uv", "", "uv.lock"),
                fact("p3", "framework:fastapi", "", "pyproject.toml"),
            ],
            capabilities: vec![
                "lang:python".into(),
                "package:uv".into(),
                "framework:fastapi".into(),
            ],
            ..Default::default()
        }
    }

    pub fn static_context() -> ProjectContext {
        ProjectContext {
            project: Some(Project {
                id: "proj-4".into(),
                name: "book".into(),
                repo_full_name: "f8fvwgzc/book".into(),
                default_branch: "main".into(),
                primary_language: "HTML".into(),
                is_private: true,
                ..Default::default()
            }),
            facts: vec![fact("s1", "repository:static_site", "", "index.html")],
            capabilities: vec!["static_site".into()],
            ..Default::default()
        }
    }

    pub fn input(ctx: &ProjectContext, cloud: &str) -> GenInput {
        let mut input = build(ctx, &BTreeMap::new(), cloud);
        input.option_title = "Test option".into();
        input.option_summary = "An option used by the generator tests.".into();
        input.plan = vec![
            PlanStepData {
                key: "build".into(),
                title: "Build image".into(),
                action_type: "container.build".into(),
                adapter: "docker".into(),
                required_permissions: vec!["container:build".into()],
                verification: "image exists".into(),
                ..Default::default()
            },
            PlanStepData {
                key: "deploy".into(),
                title: "Deploy to staging".into(),
                action_type: "deploy.staging".into(),
                adapter: "cloud".into(),
                depends_on: vec!["build".into()],
                required_permissions: vec!["deploy:staging".into()],
                verification: "health check passes".into(),
                ..Default::default()
            },
        ];
        input
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures::*, *};
    use sdi_proto::knowledge::v1::{Graph, GraphNode};

    fn node(kind: NodeKind, key: &str, label: &str, attrs: &[(&str, &str)]) -> GraphNode {
        GraphNode {
            id: key.to_owned(),
            kind: kind as i32,
            key: key.to_owned(),
            label: label.to_owned(),
            attributes: attrs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..Default::default()
        }
    }

    /// texc-symphony: a Cargo workspace of eight crates plus a pnpm web app.
    /// CI must test the workspace once from the root and the image is the
    /// service crate's binary, not a library's.
    #[test]
    fn cargo_workspace_members_collapse_into_one_root_package() {
        let mut ctx = rust_context();
        ctx.capabilities.push("monorepo".into());
        ctx.facts.push(fact(
            "r4",
            "package:pnpm",
            "18 dependencies",
            "web/package.json",
        ));
        let cargo = [("ecosystem", "cargo"), ("package_manager", "cargo")];
        ctx.graph = Some(Graph {
            nodes: vec![
                node(
                    NodeKind::Library,
                    "library:symphony-codex",
                    "symphony-codex",
                    &[
                        ("directory", "crates/symphony-codex"),
                        ("frameworks", "axum"),
                        cargo[0],
                        cargo[1],
                    ],
                ),
                node(
                    NodeKind::Library,
                    "library:symphony-core",
                    "symphony-core",
                    &[("directory", "crates/symphony-core"), cargo[0], cargo[1]],
                ),
                node(
                    NodeKind::Service,
                    "service:symphony",
                    "symphony",
                    &[("directory", "crates/symphony"), cargo[0], cargo[1]],
                ),
                node(
                    NodeKind::Service,
                    "service:symphony-web",
                    "symphony-web",
                    &[
                        ("directory", "web"),
                        ("ecosystem", "npm"),
                        ("package_manager", "pnpm"),
                    ],
                ),
            ],
            ..Default::default()
        });
        let input = build(&ctx, &BTreeMap::new(), "");
        assert_eq!(input.packages.len(), 2);
        assert_eq!(input.packages[0].dir, "");
        assert_eq!(input.packages[0].name, "symphony");
        assert_eq!(input.packages[0].stack, Stack::Rust);
        assert_eq!(input.packages[0].framework, "axum");
        assert!(!input.packages[0].is_library);
        assert_eq!(input.packages[0].job_id(), "test");
        assert_eq!(input.packages[1].dir, "web");
        assert_eq!(input.app_dir, "");
        assert_eq!(input.app_name, "symphony");
        assert_eq!(input.context_dir(), ".");
        assert_eq!(input.in_app("Dockerfile"), "Dockerfile");
    }

    /// CONTRACT.md §12: an unknown key is a warning, and the keys that did
    /// parse still apply; only a fatal `error:` discards the file.
    #[test]
    fn a_config_warning_keeps_the_parsed_values_and_an_error_drops_them() {
        let mut ctx = node_context();
        ctx.config.as_mut().unwrap().errors =
            vec!["warning: unknown key `test.coverage` was ignored".into()];
        assert!(usable_config(&ctx).is_some());
        let input = build(&ctx, &BTreeMap::new(), "");
        assert_eq!(input.port, 3000);
        assert_eq!(input.health_path, "/healthz");
        assert_eq!(input.test_command, "pnpm test");

        ctx.config.as_mut().unwrap().errors = vec!["error: `test` must be a mapping".into()];
        assert!(usable_config(&ctx).is_none());
        let input = build(&ctx, &BTreeMap::new(), "");
        assert_ne!(input.health_path, "/healthz");
    }

    #[test]
    fn node_input_resolves_from_facts_and_config() {
        let input = build(&node_context(), &BTreeMap::new(), "");
        assert_eq!(input.stack, Stack::Node);
        assert_eq!(input.package_manager, "pnpm");
        assert_eq!(input.framework, "express");
        assert_eq!(input.runtime_version, "18");
        assert_eq!(input.port, 3000);
        assert_eq!(input.health_path, "/healthz");
        assert_eq!(input.test_command, "pnpm test");
        assert_eq!(input.datastores, vec!["postgres"]);
        assert!(input.env_vars.contains(&"DATABASE_URL".to_owned()));
        assert!(input.exists("Dockerfile"));
        assert!(input.exists(".github/workflows/ci.yml"));
        assert_eq!(input.ecosystems, vec!["npm", "docker", "github-actions"]);
        assert_eq!(input.image_ref(), "ghcr.io/f8fvwgzc/view-engine");
        assert_eq!(input.region, "westeurope");
        assert!(input.fact_ids.contains(&"f6".to_owned()));
    }

    #[test]
    fn negotiated_params_override_everything() {
        let params: BTreeMap<String, String> = [
            ("region", "eastus"),
            ("port", "4000"),
            ("runtime", "node 22"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let input = build(&node_context(), &params, "aws");
        assert_eq!(input.region, "eastus");
        assert_eq!(input.port, 4000);
        assert_eq!(input.runtime_version, "22");
        assert_eq!(input.cloud, "aws");
    }

    #[test]
    fn defaults_fill_what_the_scan_did_not_find() {
        let rust = build(&rust_context(), &BTreeMap::new(), "");
        assert_eq!(rust.stack, Stack::Rust);
        assert_eq!(rust.port, 8080);
        assert_eq!(rust.runtime_version, "1.85");
        assert_eq!(rust.test_command, "cargo test --locked");
        assert_eq!(rust.datastores, vec!["redis"]);
        assert!(rust.is_private);
        let py = build(&python_context(), &BTreeMap::new(), "gcp");
        assert_eq!(py.port, 8000);
        assert_eq!(py.region, "europe-west1");
        let site = build(&static_context(), &BTreeMap::new(), "");
        assert_eq!(site.stack, Stack::Static);
        assert_eq!(site.health_path, "/");
        assert_eq!(site.static_dir, ".");
        assert_eq!(version_digits("python3.11"), "3.11");
        assert_eq!(version_digits("v20.11.1"), "20.11.1");
        assert_eq!(version_digits("latest"), "");
    }
}
