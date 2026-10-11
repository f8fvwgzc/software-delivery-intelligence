//! What the builders read: the knowledge graph indexed by id and key, the
//! repository tree as a path set, and the classification rules that turn a
//! graph node into a diagram node (kind, caption, implementing file).

use std::collections::{HashMap, HashSet};

use sdi_proto::knowledge::v1::{FactCategory, GraphEdge, GraphNode, NodeKind, ProjectContext};

use super::{
    DiagramInput,
    model::{DiagramNode, kind},
};

/// Directories whose contents never appear in a runtime map (CONTRACT.md §16).
pub const EXCLUDED_DIRS: [&str; 24] = [
    "test",
    "tests",
    "__tests__",
    "spec",
    "specs",
    "fixtures",
    "fixture",
    "testdata",
    "docs",
    "doc",
    "examples",
    "example",
    ".github",
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "xtask",
    "scripts",
    "hack",
    "benches",
    "benchmarks",
    "ci",
];

/// Entry points, most specific first, relative to a package directory.
pub const ENTRY_POINTS: [&str; 34] = [
    "src/main.rs",
    "src/main.ts",
    "src/main.tsx",
    "src/index.ts",
    "src/index.tsx",
    "src/server.ts",
    "src/app.ts",
    "src/App.tsx",
    "src/main.js",
    "src/index.js",
    "src/server.js",
    "src/app.js",
    "src/main.py",
    "src/app.py",
    "app/main.py",
    "main.py",
    "app.py",
    "manage.py",
    "main.go",
    "cmd/main.go",
    "server.ts",
    "index.ts",
    "app.ts",
    "server.js",
    "index.js",
    "app.js",
    "Program.cs",
    "src/lib.rs",
    "src/index.html",
    "index.html",
    "src/main.java",
    "lib/main.dart",
    "src/bin/main.rs",
    "bin/server.rs",
];

/// Frameworks that render a browser UI; a package with only these is `ui`.
const UI_FRAMEWORKS: [&str; 14] = [
    "react",
    "vue",
    "angular",
    "svelte",
    "sveltekit",
    "next",
    "nuxt",
    "vite",
    "astro",
    "remix",
    "gatsby",
    "solid",
    "preact",
    "ember",
];

const BACKEND_FRAMEWORKS: [&str; 24] = [
    "express",
    "fastify",
    "nest",
    "nestjs",
    "koa",
    "hono",
    "hapi",
    "axum",
    "actix",
    "rocket",
    "warp",
    "fastapi",
    "django",
    "flask",
    "starlette",
    "spring",
    "rails",
    "laravel",
    "gin",
    "echo",
    "fiber",
    "aspnet",
    "phoenix",
    "sinatra",
];

const QUEUE_ENGINES: [&str; 6] = [
    "kafka",
    "rabbitmq",
    "nats",
    "sqs",
    "pubsub",
    "redis-streams",
];

pub struct Catalog<'a> {
    pub input: &'a DiagramInput<'a>,
    nodes: Vec<&'a GraphNode>,
    by_id: HashMap<&'a str, &'a GraphNode>,
    by_key: HashMap<&'a str, &'a GraphNode>,
    edges: Vec<&'a GraphEdge>,
    /// File path → size in bytes as the tree reports it.
    files: HashMap<&'a str, u64>,
    dirs: HashSet<String>,
}

impl<'a> Catalog<'a> {
    pub fn new(input: &'a DiagramInput<'a>) -> Self {
        let graph = input.context.graph.as_ref();
        let nodes: Vec<&GraphNode> = graph.map(|g| g.nodes.iter().collect()).unwrap_or_default();
        let by_id = nodes.iter().map(|n| (n.id.as_str(), *n)).collect();
        let by_key = nodes.iter().map(|n| (n.key.as_str(), *n)).collect();
        // Temporal edges: only what is still true.
        let edges = graph
            .map(|g| g.edges.iter().filter(|e| e.valid_to.is_none()).collect())
            .unwrap_or_default();
        let mut dirs = HashSet::new();
        let mut files = HashMap::new();
        for entry in input.tree {
            if entry.is_dir {
                dirs.insert(entry.path.trim_end_matches('/').to_owned());
            } else {
                files.insert(entry.path.as_str(), entry.size);
                // Trees from GitHub list directories, but a flat fixture may not.
                let mut parent = entry.path.rsplit_once('/').map(|(d, _)| d);
                while let Some(dir) = parent {
                    if !dirs.insert(dir.to_owned()) {
                        break;
                    }
                    parent = dir.rsplit_once('/').map(|(d, _)| d);
                }
            }
        }
        Self {
            input,
            nodes,
            by_id,
            by_key,
            edges,
            files,
            dirs,
        }
    }

    pub fn context(&self) -> &'a ProjectContext {
        self.input.context
    }

    pub fn project_name(&self) -> String {
        self.context()
            .project
            .as_ref()
            .map(|p| {
                if p.name.is_empty() {
                    p.repo_full_name
                        .rsplit('/')
                        .next()
                        .unwrap_or("project")
                        .to_owned()
                } else {
                    p.name.clone()
                }
            })
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "project".to_owned())
    }

    pub fn commit_sha(&self) -> String {
        self.context()
            .understanding
            .as_ref()
            .map(|u| u.commit_sha.clone())
            .unwrap_or_default()
    }

    pub fn has_tree(&self) -> bool {
        !self.input.tree.is_empty()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &'a GraphNode> + '_ {
        self.nodes.iter().copied()
    }

    pub fn edges(&self) -> impl Iterator<Item = &'a GraphEdge> + '_ {
        self.edges.iter().copied()
    }

    pub fn node_by_id(&self, id: &str) -> Option<&'a GraphNode> {
        self.by_id.get(id).copied()
    }

    pub fn node_by_key(&self, key: &str) -> Option<&'a GraphNode> {
        self.by_key.get(key).copied()
    }

    pub fn of_kind(&self, kind: NodeKind) -> impl Iterator<Item = &'a GraphNode> + '_ {
        self.nodes().filter(move |n| n.kind() == kind)
    }

    // -- tree ---------------------------------------------------------------

    pub fn file_exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn dir_exists(&self, dir: &str) -> bool {
        !dir.is_empty() && self.dirs.contains(dir)
    }

    /// Size the tree reports for a file (0 when unknown).
    pub fn file_size(&self, path: &str) -> u64 {
        self.files.get(path).copied().unwrap_or(0)
    }

    pub fn all_files(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.files.keys().copied()
    }

    /// Files directly or transitively under `dir` (all files for "").
    pub fn files_under(&self, dir: &str) -> impl Iterator<Item = &'a str> + '_ {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        self.files
            .keys()
            .copied()
            .filter(move |f| f.starts_with(&prefix))
    }

    /// Immediate child directories of `dir`, sorted.
    pub fn child_dirs(&self, dir: &str) -> Vec<String> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut out: Vec<String> = self
            .dirs
            .iter()
            .filter(|d| d.starts_with(&prefix) && !d[prefix.len()..].contains('/'))
            .filter(|d| !d.is_empty())
            .cloned()
            .collect();
        out.sort();
        out
    }

    /// A path a node can link to: the entry point under `dir` when there is
    /// one, else `fallback` when it exists in the tree. When no tree was
    /// available the fallback is trusted as is.
    pub fn implementing_file(&self, dir: &str, fallback: &str) -> String {
        if !self.has_tree() {
            return fallback.to_owned();
        }
        if let Some(entry) = self.entry_point(dir) {
            return entry;
        }
        if !fallback.is_empty() && self.file_exists(fallback) {
            return fallback.to_owned();
        }
        String::new()
    }

    pub fn entry_point(&self, dir: &str) -> Option<String> {
        let join = |rel: &str| {
            if dir.is_empty() {
                rel.to_owned()
            } else {
                format!("{dir}/{rel}")
            }
        };
        for candidate in ENTRY_POINTS {
            let path = join(candidate);
            if self.file_exists(&path) {
                return Some(path);
            }
        }
        // cmd/<name>/main.go, src/<name>/main.py and the like.
        let mut globbed: Vec<&str> = self
            .files_under(dir)
            .filter(|f| {
                let rel = if dir.is_empty() {
                    *f
                } else {
                    &f[dir.len() + 1..]
                };
                let depth = rel.matches('/').count();
                depth <= 2
                    && (rel.ends_with("/main.go")
                        || rel.ends_with("/main.py")
                        || rel.ends_with("/__main__.py")
                        || rel.ends_with("/main.rs"))
            })
            .collect();
        globbed.sort();
        globbed.first().map(|f| (*f).to_owned())
    }

    /// Files excluded from runtime maps: tests, CI, docs, fixtures.
    pub fn is_excluded_path(path: &str) -> bool {
        path.split('/').any(|part| EXCLUDED_DIRS.contains(&part))
            || path.ends_with(".test.ts")
            || path.ends_with(".spec.ts")
            || path.ends_with("_test.go")
            || path.ends_with("_test.py")
    }

    // -- node classification ------------------------------------------------

    /// The directory a graph node stands for (attribute, else the directory
    /// of its source path). "" is the repository root.
    pub fn dir_of(&self, node: &GraphNode) -> String {
        if let Some(dir) = node.attributes.get("directory") {
            return if dir == "." {
                String::new()
            } else {
                dir.trim_matches('/').to_owned()
            };
        }
        node.source_path
            .rsplit_once('/')
            .map(|(d, _)| d.to_owned())
            .unwrap_or_default()
    }

    /// First path component of the node's directory ("" at the root).
    pub fn top_dir(&self, node: &GraphNode) -> String {
        let dir = self.dir_of(node);
        dir.split('/').next().unwrap_or("").to_owned()
    }

    pub fn is_repository_node(node: &GraphNode) -> bool {
        matches!(node.kind(), NodeKind::Service | NodeKind::Library)
    }

    /// Whether a service or library is part of the product rather than its
    /// test, docs or CI machinery.
    pub fn is_product_node(&self, node: &GraphNode) -> bool {
        Self::is_repository_node(node) && !Self::is_excluded_path(&self.dir_of(node))
    }

    /// Framework names the scan attached to a package, lowercased.
    pub fn frameworks_of(&self, node: &GraphNode) -> Vec<String> {
        let mut out: Vec<String> = node
            .attributes
            .get("frameworks")
            .map(|f| f.split(',').map(|s| s.trim().to_lowercase()).collect())
            .unwrap_or_default();
        if out.is_empty() {
            let dir = self.dir_of(node);
            for fact in &self.context().facts {
                if fact.category() == FactCategory::Framework
                    && fact.key.starts_with("framework:")
                    && within(&fact.source_path, &dir)
                {
                    out.push(fact.key.trim_start_matches("framework:").to_lowercase());
                }
            }
        }
        out.retain(|f| !f.is_empty());
        out
    }

    /// A browser UI: a UI framework without a backend one, or (when the
    /// scan attached no framework) a package whose entry point is a
    /// component file or that is built by Vite.
    pub fn is_ui(&self, node: &GraphNode) -> bool {
        let frameworks = self.frameworks_of(node);
        let ui = frameworks
            .iter()
            .any(|f| UI_FRAMEWORKS.iter().any(|u| f.contains(u)));
        let backend = frameworks
            .iter()
            .any(|f| BACKEND_FRAMEWORKS.iter().any(|b| f.contains(b)));
        if backend {
            return false;
        }
        if ui {
            return true;
        }
        if node.kind() != NodeKind::Service || !self.has_tree() {
            return false;
        }
        let dir = self.dir_of(node);
        let join = |rel: &str| {
            if dir.is_empty() {
                rel.to_owned()
            } else {
                format!("{dir}/{rel}")
            }
        };
        let entry = self.entry_point(&dir).unwrap_or_default();
        entry.ends_with(".tsx")
            || entry.ends_with(".jsx")
            || entry.ends_with(".vue")
            || entry.ends_with(".svelte")
            || [
                "vite.config.ts",
                "vite.config.js",
                "vite.config.mts",
                "next.config.js",
                "next.config.mjs",
                "next.config.ts",
            ]
            .iter()
            .any(|f| self.file_exists(&join(f)))
    }

    pub fn is_backend_service(&self, node: &GraphNode) -> bool {
        node.kind() == NodeKind::Service && !self.is_ui(node)
    }

    pub fn engine_of(node: &GraphNode) -> String {
        node.attributes
            .get("engine")
            .cloned()
            .unwrap_or_else(|| node.key.trim_start_matches("datastore:").to_owned())
            .to_lowercase()
    }

    pub fn is_queue(node: &GraphNode) -> bool {
        node.kind() == NodeKind::Datastore
            && QUEUE_ENGINES.contains(&Self::engine_of(node).as_str())
    }

    /// Diagram kind for a graph node, or `None` when it has no place on a map.
    pub fn diagram_kind(&self, node: &GraphNode) -> Option<&'static str> {
        Some(match node.kind() {
            NodeKind::Service => {
                if self.is_ui(node) {
                    kind::UI
                } else {
                    kind::SERVICE
                }
            }
            NodeKind::Library => kind::MODULE,
            NodeKind::Datastore => {
                if Self::is_queue(node) {
                    kind::QUEUE
                } else {
                    kind::DATASTORE
                }
            }
            NodeKind::ExternalApi => kind::EXTERNAL,
            NodeKind::Infrastructure
            | NodeKind::Environment
            | NodeKind::ContainerImage
            | NodeKind::IacModule => kind::INFRA,
            NodeKind::Pipeline | NodeKind::Deployment => kind::JOB,
            NodeKind::Owner => kind::ACTOR,
            NodeKind::Repository
            | NodeKind::TestSuite
            | NodeKind::Document
            | NodeKind::Migration
            | NodeKind::Unspecified => return None,
        })
    }

    /// Short type caption: framework, engine, runtime or provider.
    pub fn caption_of(&self, node: &GraphNode) -> String {
        match node.kind() {
            NodeKind::Service | NodeKind::Library => {
                if let Some(framework) = self.frameworks_of(node).first() {
                    return display_framework(framework);
                }
                let ecosystem = node
                    .attributes
                    .get("ecosystem")
                    .map(String::as_str)
                    .unwrap_or("");
                ecosystem_display(ecosystem).to_owned()
            }
            NodeKind::Datastore => {
                // The label already names the engine; the image adds the version.
                let engine = engine_display(&Self::engine_of(node));
                if engine.eq_ignore_ascii_case(node.label.trim()) {
                    node.attributes.get("image").cloned().unwrap_or_default()
                } else {
                    engine
                }
            }
            NodeKind::ExternalApi => node
                .attributes
                .get("dependency")
                .cloned()
                .unwrap_or_else(|| "external API".to_owned()),
            NodeKind::ContainerImage => node
                .attributes
                .get("base_image")
                .cloned()
                .unwrap_or_else(|| "container image".to_owned()),
            NodeKind::Infrastructure => node
                .attributes
                .get("resource_type")
                .cloned()
                .or_else(|| node.attributes.get("tool").cloned())
                .unwrap_or_else(|| "infrastructure".to_owned()),
            NodeKind::Environment => node
                .attributes
                .get("defined_by")
                .map(|d| d.replace('_', " "))
                .unwrap_or_else(|| "environment".to_owned()),
            NodeKind::IacModule => node
                .attributes
                .get("tool")
                .cloned()
                .unwrap_or_else(|| "IaC".to_owned()),
            NodeKind::Pipeline => node
                .attributes
                .get("provider")
                .map(|p| provider_display(p))
                .unwrap_or_else(|| "pipeline".to_owned()),
            NodeKind::Deployment => node
                .attributes
                .get("target")
                .cloned()
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "deployment".to_owned()),
            NodeKind::Owner => "owner".to_owned(),
            _ => String::new(),
        }
    }

    /// 2–4 word label: the node's own label, shortened.
    pub fn label_of(&self, node: &GraphNode) -> String {
        concise_label(&node.label)
    }

    /// Path the node links to, which exists in the tree (or is empty).
    pub fn path_of(&self, node: &GraphNode) -> String {
        match node.kind() {
            NodeKind::Service | NodeKind::Library => {
                self.implementing_file(&self.dir_of(node), &node.source_path)
            }
            NodeKind::ExternalApi | NodeKind::Owner => String::new(),
            _ => {
                if !self.has_tree() || self.file_exists(&node.source_path) {
                    node.source_path.clone()
                } else {
                    String::new()
                }
            }
        }
    }

    /// A diagram node for a graph node, in the given group.
    pub fn diagram_node(&self, node: &GraphNode, group_id: &str) -> Option<DiagramNode> {
        let kind = self.diagram_kind(node)?;
        Some(DiagramNode {
            id: node_id(&node.key),
            label: self.label_of(node),
            kind: kind.to_owned(),
            group_id: group_id.to_owned(),
            path: self.path_of(node),
            description: node.description.clone(),
            caption: self.caption_of(node),
        })
    }

    /// The repository node (service or library) whose directory contains `path`.
    pub fn owner_of_path(&self, path: &str) -> Option<&'a GraphNode> {
        let mut best: Option<(&GraphNode, usize)> = None;
        for node in self.nodes() {
            if !Self::is_repository_node(node) {
                continue;
            }
            let dir = self.dir_of(node);
            if within(path, &dir) {
                let depth = dir.len();
                if best.is_none_or(|(_, d)| depth > d) {
                    best = Some((node, depth));
                }
            }
        }
        best.map(|(n, _)| n)
    }

    /// Excerpted files owned by product packages: `(path, text, owner)`.
    pub fn excerpts(&self) -> Vec<(&'a str, &'a str, &'a GraphNode)> {
        self.input
            .files
            .iter()
            .filter_map(|(path, text)| {
                let owner = self.owner_of_path(path)?;
                Some((path.as_str(), text.as_str(), owner))
            })
            .collect()
    }
}

/// Whether `path` sits inside `dir` ("" means the root, which holds everything).
pub fn within(path: &str, dir: &str) -> bool {
    dir.is_empty() || path == dir || path.starts_with(&format!("{dir}/"))
}

/// Stable, Mermaid-safe node id from a graph key such as `service:api`.
pub fn node_id(key: &str) -> String {
    let mut id = String::with_capacity(key.len());
    for c in key.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
        } else if !id.ends_with('_') {
            id.push('_');
        }
    }
    id.trim_matches('_').to_owned()
}

/// Keeps a label to at most four words and strips manifest noise.
pub fn concise_label(label: &str) -> String {
    let cleaned = label
        .replace(['_'], " ")
        .replace(" → ", " to ")
        .trim()
        .to_owned();
    let words: Vec<&str> = cleaned.split_whitespace().take(4).collect();
    if words.is_empty() {
        "component".to_owned()
    } else {
        words.join(" ")
    }
}

pub fn humanize_dir(dir: &str) -> String {
    if dir.is_empty() {
        return "Root".to_owned();
    }
    let name = dir
        .rsplit('/')
        .next()
        .unwrap_or(dir)
        .replace(['-', '_'], " ");
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Root".to_owned(),
    }
}

pub fn engine_display(engine: &str) -> String {
    match engine {
        "postgres" | "postgresql" => "PostgreSQL",
        "mysql" => "MySQL",
        "mariadb" => "MariaDB",
        "redis" => "Redis",
        "mongodb" | "mongo" => "MongoDB",
        "sqlite" => "SQLite",
        "kafka" => "Kafka",
        "rabbitmq" => "RabbitMQ",
        "nats" => "NATS",
        "elasticsearch" => "Elasticsearch",
        "opensearch" => "OpenSearch",
        "dynamodb" => "DynamoDB",
        "s3" => "S3",
        "cassandra" => "Cassandra",
        "clickhouse" => "ClickHouse",
        "sqs" => "SQS",
        "pubsub" => "Pub/Sub",
        "memcached" => "Memcached",
        other => return capitalise(other),
    }
    .to_owned()
}

pub fn display_framework(framework: &str) -> String {
    match framework {
        "react" => "React",
        "vue" => "Vue",
        "angular" => "Angular",
        "svelte" => "Svelte",
        "next" | "nextjs" => "Next.js",
        "nuxt" => "Nuxt",
        "vite" => "Vite",
        "express" => "Express",
        "fastify" => "Fastify",
        "nest" | "nestjs" => "NestJS",
        "axum" => "axum",
        "actix" => "Actix",
        "rocket" => "Rocket",
        "fastapi" => "FastAPI",
        "django" => "Django",
        "flask" => "Flask",
        "spring" => "Spring",
        "rails" => "Rails",
        "laravel" => "Laravel",
        "gin" => "Gin",
        "aspnet" => "ASP.NET",
        other => return capitalise(other),
    }
    .to_owned()
}

pub fn ecosystem_display(ecosystem: &str) -> &'static str {
    match ecosystem {
        "cargo" => "Rust",
        "npm" | "pnpm" | "yarn" | "bun" => "Node.js",
        "pip" | "poetry" | "uv" | "pipenv" => "Python",
        "go" => "Go",
        "maven" | "gradle" => "Java",
        "nuget" => ".NET",
        "composer" => "PHP",
        "bundler" | "rubygems" => "Ruby",
        _ => "package",
    }
}

pub fn provider_display(provider: &str) -> String {
    match provider {
        "github_actions" => "GitHub Actions".to_owned(),
        "gitlab_ci" => "GitLab CI".to_owned(),
        "circleci" => "CircleCI".to_owned(),
        "jenkins" => "Jenkins".to_owned(),
        other => capitalise(&other.replace('_', " ")),
    }
}

pub fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Tree entries are what builders receive; this keeps fixtures short.
#[cfg(test)]
pub fn tree_entry(path: &str) -> sdi_proto::knowledge::v1::RepoTreeEntry {
    tree_entry_sized(path, 0)
}

#[cfg(test)]
pub fn tree_entry_sized(path: &str, size: u64) -> sdi_proto::knowledge::v1::RepoTreeEntry {
    let is_dir = path.ends_with('/');
    let clean = path.trim_end_matches('/');
    sdi_proto::knowledge::v1::RepoTreeEntry {
        path: clean.to_owned(),
        name: clean.rsplit('/').next().unwrap_or(clean).to_owned(),
        is_dir,
        size,
        language: String::new(),
    }
}
