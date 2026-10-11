//! Module-level components (CONTRACT.md §16): the sampled files and the
//! files they import collapse into components — a directory with three or
//! more source files, or a single significant file (an entry point, a file
//! three or more others import, a file over 8 KB) — and the import graph
//! becomes component edges whose verbs come from what the importing file
//! does (spawns, parses, writes, streams...). Every package's root
//! directory is a component whose id is the graph node's, so package-level
//! evidence and graph edges land on it unchanged.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::{
    catalog::{Catalog, capitalise, node_id, within},
    evidence::{SQL_READ, SQL_WRITE},
    imports::{
        FileGraph, Resolver, extension, file_name, is_generated, is_source, parent_dir,
        strip_comments,
    },
    model::{kind, style},
};

/// A file at least this large is a component of its own.
pub const SIGNIFICANT_BYTES: u64 = 8 * 1024;
/// A file at least this many sampled files import is a component of its own.
pub const SIGNIFICANT_FAN_IN: usize = 3;
/// A directory with at least this many source files is a component.
pub const DIR_MIN_FILES: usize = 3;

/// The gitdiagram-style roles components are grouped by when enough of
/// them are recognisable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Bootstrap,
    Execution,
    Integrations,
    Monitoring,
    State,
    Other,
}

impl Role {
    pub fn id(self) -> &'static str {
        match self {
            Role::Bootstrap => "role_bootstrap",
            Role::Execution => "role_execution",
            Role::Integrations => "role_integrations",
            Role::Monitoring => "role_monitoring",
            Role::State => "role_state",
            Role::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Bootstrap => "Configuration & bootstrap",
            Role::Execution => "Core execution",
            Role::Integrations => "External integrations",
            Role::Monitoring => "Monitoring & UI",
            Role::State => "State & storage",
            Role::Other => "Other",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Role::Bootstrap => "Entry points, settings and the code that boots the product",
            Role::Execution => "The work the product exists to do",
            Role::Integrations => "Adapters, clients and the external systems they talk to",
            Role::Monitoring => "HTTP API, streaming updates and user interfaces",
            Role::State => "Persistence: stores, records and the databases behind them",
            Role::Other => "Components outside the recognised roles",
        }
    }

    pub const ALL: [Role; 6] = [
        Role::Bootstrap,
        Role::Execution,
        Role::Integrations,
        Role::Monitoring,
        Role::State,
        Role::Other,
    ];
}

const STATE_WORDS: [&str; 14] = [
    "store",
    "stores",
    "db",
    "database",
    "repo",
    "repository",
    "repositories",
    "persist",
    "persistence",
    "migrations",
    "storage",
    "history",
    "recorder",
    "cache",
];
const MONITORING_WORDS: [&str; 24] = [
    "server",
    "http",
    "routes",
    "router",
    "api",
    "web",
    "ui",
    "dashboard",
    "sse",
    "view",
    "views",
    "presenter",
    "frontend",
    "pages",
    "components",
    "layout",
    "features",
    "assets",
    "control",
    "metrics",
    "monitoring",
    "events",
    "live",
    "format",
];
const INTEGRATION_WORDS: [&str; 22] = [
    "tracker",
    "trackers",
    "client",
    "clients",
    "adapter",
    "adapters",
    "integration",
    "integrations",
    "codex",
    "provider",
    "providers",
    "external",
    "github",
    "gitlab",
    "jira",
    "linear",
    "asana",
    "sdk",
    "transport",
    "rest",
    "webhook",
    "ssh",
];
const EXECUTION_WORDS: [&str; 24] = [
    "runtime",
    "orchestrator",
    "orchestration",
    "worker",
    "workers",
    "runner",
    "executor",
    "scheduler",
    "dispatch",
    "workspace",
    "process",
    "engine",
    "job",
    "jobs",
    "task",
    "tasks",
    "pipeline",
    "agent",
    "agents",
    "session",
    "harness",
    "retry",
    "reconcile",
    "snapshot",
];
const BOOTSTRAP_WORDS: [&str; 14] = [
    "config",
    "settings",
    "schema",
    "workflow",
    "bootstrap",
    "cli",
    "core",
    "env",
    "logging",
    "version",
    "prompt",
    "issue",
    "error",
    "path",
];

/// Role from the words of a path (package, directories and file stem).
pub fn role_of(words: &[String]) -> Role {
    let has = |list: &[&str]| words.iter().any(|w| list.contains(&w.as_str()));
    // "workflow_store" and "config/store.rs" are settings, not persistence.
    if has(&["config", "settings", "workflow"]) {
        Role::Bootstrap
    } else if has(&STATE_WORDS) {
        Role::State
    } else if has(&MONITORING_WORDS) {
        Role::Monitoring
    } else if has(&INTEGRATION_WORDS) {
        Role::Integrations
    } else if has(&EXECUTION_WORDS) {
        Role::Execution
    } else if has(&BOOTSTRAP_WORDS)
        || words
            .last()
            .is_some_and(|w| matches!(w.as_str(), "app" | "main" | "bootstrap" | "startup"))
    {
        // `app` and `main` only mean bootstrap as the file itself, not as a
        // directory every module of a package lives in.
        Role::Bootstrap
    } else {
        Role::Other
    }
}

/// Lowercase words of a path: directory names and the file stem split on
/// `-`, `_`, `.` and camel case.
pub fn words_of(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in path.split('/') {
        let stem = part.rsplit_once('.').map(|(s, _)| s).unwrap_or(part);
        out.extend(split_words(stem));
    }
    out
}

pub fn split_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for c in text.chars() {
        if c == '-' || c == '_' || c == '.' || c == ' ' {
            if !current.is_empty() {
                words.push(current.to_lowercase());
                current.clear();
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !current.is_empty() {
            words.push(current.to_lowercase());
            current.clear();
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        current.push(c);
    }
    if !current.is_empty() {
        words.push(current.to_lowercase());
    }
    words
}

/// Sentence case from words: "workflow store".
pub fn sentence(words: &[String]) -> String {
    let joined = words.join(" ");
    if joined.starts_with("API") {
        return joined;
    }
    capitalise(&joined)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub id: String,
    pub label: String,
    pub caption: String,
    pub kind: &'static str,
    pub path: String,
    pub description: String,
    pub role: Role,
    /// Graph key of the package the component belongs to.
    pub package: String,
    /// Component the files fold into when the map is too big.
    pub parent: Option<String>,
    pub is_root: bool,
    /// Never collapsed: package roots with an entry point.
    pub protected: bool,
    /// Lives in a browser UI package.
    pub ui_package: bool,
    pub files: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentEdge {
    pub source: String,
    pub target: String,
    pub label: String,
    pub style: String,
    pub evidence_path: String,
}

#[derive(Debug, Default)]
pub struct Components {
    pub items: Vec<Component>,
    pub edges: Vec<ComponentEdge>,
    by_file: HashMap<String, String>,
    pub file_graph: FileGraph,
}

/// Verb specificity: the most informative verb between two components wins.
fn verb_rank(label: &str) -> usize {
    match label {
        "starts" | "runs" | "dispatches" => 0,
        "loads" | "validates" | "records" | "writes" | "reads" | "streams" => 1,
        "calls" | "serves" | "renders" | "configures" => 2,
        "reads/writes" => 3,
        _ => 9,
    }
}

static SPAWN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"tokio::spawn|thread::spawn|spawn_blocking|JoinSet|Command::new|\.spawn\(|child_process|subprocess\.|multiprocessing|exec\.Command|go\s+func|ProcessBuilder|new Thread\(|Thread\(target|asyncio\.create_task|\bserve\(")
        .expect("spawn")
});
static HTTP_CLIENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"reqwest|hyper::Client|\bfetch\(|axios|http\.Client|HttpClient|requests\.(get|post|put|delete|request)|httpx|urllib|RestTemplate|WebClient|EventSource\(|graphql")
        .expect("http client")
});
static STREAM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"broadcast|watch::|mpsc|\bSse\b|\bsse\b|WebSocket|websocket|subscribe\(|Stream\b|EventSource|ReadableStream")
        .expect("stream")
});
static LOAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"serde_yaml|serde_yml|toml::|from_str\(|read_to_string|yaml\.|json\.load|load_config|dotenv|env::var|parse_args|clap::|argparse|from_env|\bload\(|::load\(|::start\(|::open\(")
        .expect("load")
});
static VALIDATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bvalidat\w*\(|\bValidator\b|\.verify\(|\bcheck_\w+\(|\bpreflight\w*\(")
        .expect("validate")
});
static RUN_CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"run_turn|\.run\(|::start\(|\.start\(|\.execute\(|\.dispatch\(|\.launch\(")
        .expect("run call")
});
static WRITE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(insert|append|save|record|write|persist|start_run|finish_run|update_|mark_|put\()",
    )
    .expect("write")
});

impl Components {
    pub fn build(catalog: &Catalog<'_>) -> Self {
        let resolver = Resolver::new(catalog);
        let file_graph = FileGraph::build(catalog, &resolver);
        let fan_in = file_graph.fan_in();

        // Candidate files: sampled sources and what they import.
        let mut candidates: BTreeSet<String> = BTreeSet::new();
        for path in catalog.input.files.keys() {
            if is_source(path) && !Catalog::is_excluded_path(path) && !is_generated(path) {
                candidates.insert(path.clone());
            }
        }
        for (_, target) in &file_graph.edges {
            candidates.insert(target.clone());
        }

        let short_names = short_package_names(catalog);
        let mut comps = Components {
            file_graph,
            ..Default::default()
        };

        // Package roots first, so every file has somewhere to fold into.
        let mut roots: Vec<(String, String, String)> = Vec::new(); // (key, dir, root_dir)
        for node in catalog.nodes().filter(|n| catalog.is_product_node(n)) {
            let dir = catalog.dir_of(node);
            let entry = catalog.entry_point(&dir);
            let root_dir = entry
                .as_deref()
                .map(parent_dir)
                .map(str::to_owned)
                .or_else(|| {
                    let src = if dir.is_empty() {
                        "src".to_owned()
                    } else {
                        format!("{dir}/src")
                    };
                    catalog.dir_exists(&src).then_some(src)
                })
                .unwrap_or_else(|| dir.clone());
            let Some(mut diagram) = catalog.diagram_node(node, "") else {
                continue;
            };
            if diagram.label.contains('-')
                || diagram.label.contains('_')
                || diagram
                    .label
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_lowercase())
            {
                // "symphony-runtime" reads better as "Symphony runtime".
                diagram.label = sentence(&split_words(&diagram.label));
            }
            let words: Vec<String> = short_names
                .get(&node.key)
                .cloned()
                .into_iter()
                .chain(words_of(&dir))
                .collect();
            let role = role_of(&words);
            let mut files = BTreeSet::new();
            if let Some(entry) = &entry {
                files.insert(entry.clone());
            }
            comps.items.push(Component {
                id: diagram.id.clone(),
                label: diagram.label,
                caption: diagram.caption,
                kind: if diagram.kind == kind::UI {
                    kind::UI
                } else if diagram.kind == kind::MODULE {
                    kind::MODULE
                } else {
                    kind::SERVICE
                },
                path: diagram.path,
                description: diagram.description,
                role,
                package: node.key.clone(),
                parent: None,
                is_root: true,
                protected: entry.is_some(),
                ui_package: diagram.kind == kind::UI,
                files,
            });
            if let Some(entry) = entry {
                comps.by_file.insert(entry, diagram.id.clone());
            }
            roots.push((node.key.clone(), dir, root_dir));
        }

        // Files per directory, for the ≥ 3 rule (tree-wide, not just the sample).
        let mut dir_counts: HashMap<String, usize> = HashMap::new();
        for file in catalog.all_files() {
            if is_source(file) && !Catalog::is_excluded_path(file) && !is_generated(file) {
                *dir_counts.entry(parent_dir(file).to_owned()).or_default() += 1;
            }
        }

        for file in &candidates {
            if comps.by_file.contains_key(file) {
                continue;
            }
            let Some(owner) = catalog.owner_of_path(file) else {
                continue;
            };
            let Some((key, pkg_dir, root_dir)) = roots.iter().find(|(k, _, _)| *k == owner.key)
            else {
                continue;
            };
            let root_id = node_id(key);
            let is_ui = catalog.is_ui(owner);
            let short = short_names.get(key).cloned().unwrap_or_default();
            let name = file_name(file);
            let is_hub = matches!(
                name,
                "mod.rs"
                    | "lib.rs"
                    | "main.rs"
                    | "index.ts"
                    | "index.tsx"
                    | "index.js"
                    | "__init__.py"
            );
            let significant = !is_hub
                && (fan_in.get(file).copied().unwrap_or(0) >= SIGNIFICANT_FAN_IN
                    || catalog.file_size(file) >= SIGNIFICANT_BYTES);
            // The directory component the file sits in, if its directory qualifies.
            let dir_component = {
                let mut dir = parent_dir(file).to_owned();
                let mut found: Option<String> = None;
                while !dir.is_empty() && dir != *root_dir && within(&dir, pkg_dir) {
                    if dir_counts.get(&dir).copied().unwrap_or(0) >= DIR_MIN_FILES {
                        found = Some(dir.clone());
                        break;
                    }
                    dir = parent_dir(&dir).to_owned();
                }
                found
            };
            let dir_id = dir_component.as_ref().map(|dir| {
                let id = format!("dir_{}", node_id(dir));
                if !comps.items.iter().any(|c| c.id == id) {
                    let rel = dir.strip_prefix(root_dir.as_str()).unwrap_or(dir);
                    let rel = rel.trim_start_matches('/');
                    let dir_name = file_name(dir);
                    let rel_words = words_of(rel);
                    let words: Vec<String> = if role_of(&rel_words) == Role::Other {
                        [short.clone()].into_iter().chain(rel_words).collect()
                    } else {
                        rel_words
                    };
                    let (label, comp_kind) = dir_label(dir_name, rel, &short, is_ui);
                    let hub = [
                        "mod.rs",
                        "index.ts",
                        "index.tsx",
                        "index.js",
                        "__init__.py",
                        "lib.rs",
                    ]
                    .iter()
                    .map(|h| format!("{dir}/{h}"))
                    .find(|p| catalog.file_exists(p));
                    comps.items.push(Component {
                        id: id.clone(),
                        label,
                        caption: format!("{}/", rel),
                        kind: comp_kind,
                        path: hub.unwrap_or_default(),
                        description: format!(
                            "{} source files under {dir}/",
                            dir_counts.get(dir).copied().unwrap_or(0)
                        ),
                        role: role_of(&words),
                        package: key.clone(),
                        parent: Some(root_id.clone()),
                        is_root: false,
                        protected: false,
                        ui_package: is_ui,
                        files: BTreeSet::new(),
                    });
                }
                id
            });
            let component_id = if significant {
                let id = format!("file_{}", node_id(file));
                let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
                let mut words = split_words(stem);
                if words.is_empty() {
                    words.push(stem.to_lowercase());
                }
                if is_ui && matches!(stem.to_lowercase().as_str(), "api" | "client" | "http") {
                    words = vec!["API".to_owned(), "client".to_owned()];
                }
                let rel_file = file.strip_prefix(root_dir.as_str()).unwrap_or(file);
                let rel_words = words_of(rel_file.trim_start_matches('/'));
                let role_words: Vec<String> = if role_of(&rel_words) == Role::Other {
                    [short.clone()].into_iter().chain(rel_words).collect()
                } else {
                    rel_words
                };
                comps.items.push(Component {
                    id: id.clone(),
                    label: sentence(&words[..words.len().min(4)]),
                    caption: name.to_owned(),
                    kind: if matches!(extension(file), "tsx" | "jsx" | "vue" | "svelte") {
                        kind::UI
                    } else {
                        kind::MODULE
                    },
                    path: file.clone(),
                    description: format!("{file} ({} KB)", catalog.file_size(file) / 1024),
                    role: role_of(&role_words),
                    package: key.clone(),
                    parent: Some(dir_id.clone().unwrap_or_else(|| root_id.clone())),
                    is_root: false,
                    protected: false,
                    ui_package: is_ui,
                    files: BTreeSet::from([file.clone()]),
                });
                id
            } else {
                dir_id.unwrap_or(root_id)
            };
            if let Some(component) = comps.items.iter_mut().find(|c| c.id == component_id) {
                component.files.insert(file.clone());
            }
            comps.by_file.insert(file.clone(), component_id);
        }

        comps.disambiguate_labels(&short_names);
        comps.rebuild_edges(catalog);
        comps
    }

    /// Two files called `app.rs` in different packages both read "App";
    /// the package name tells them apart ("Server app").
    fn disambiguate_labels(&mut self, short_names: &BTreeMap<String, String>) {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for component in &self.items {
            *counts.entry(component.label.to_lowercase()).or_default() += 1;
        }
        for component in &mut self.items {
            if component.is_root
                || counts
                    .get(&component.label.to_lowercase())
                    .copied()
                    .unwrap_or(0)
                    < 2
            {
                continue;
            }
            let short = short_names
                .get(&component.package)
                .cloned()
                .unwrap_or_default();
            if short.is_empty() {
                continue;
            }
            let lower = component.label.to_lowercase();
            if !lower.starts_with(&short) {
                component.label = sentence(&[short, lower]);
            }
        }
    }

    /// Component edges from the file edges, one per pair with the most
    /// specific verb; the evidence is the importing file.
    fn rebuild_edges(&mut self, catalog: &Catalog<'_>) {
        let mut best: BTreeMap<(String, String), ComponentEdge> = BTreeMap::new();
        let edges = self.file_graph.edges.clone();
        for (importer, imported) in &edges {
            let (Some(source), Some(target)) =
                (self.component_of(importer), self.component_of(imported))
            else {
                continue;
            };
            if source == target {
                continue;
            }
            let text = catalog
                .input
                .files
                .get(importer)
                .map(String::as_str)
                .unwrap_or("");
            let target_component = self.get(&target).expect("component exists");
            let same_package = self
                .get(&source)
                .is_some_and(|s| s.package == target_component.package);
            let (label, edge_style) = if same_package && target_component.is_root {
                // A module reaching back into its own crate root is plumbing, not a call.
                ("uses", style::CALL)
            } else {
                verb_for(&strip_comments(text), target_component)
            };
            let candidate = ComponentEdge {
                source: source.clone(),
                target: target.clone(),
                label: label.to_owned(),
                style: edge_style.to_owned(),
                evidence_path: importer.clone(),
            };
            let key = (source, target);
            match best.get(&key) {
                Some(existing) if verb_rank(&existing.label) <= verb_rank(&candidate.label) => {}
                _ => {
                    best.insert(key, candidate);
                }
            }
        }
        self.edges = best.into_values().collect();
    }

    pub fn get(&self, id: &str) -> Option<&Component> {
        self.items.iter().find(|c| c.id == id)
    }

    pub fn component_of(&self, file: &str) -> Option<String> {
        self.by_file.get(file).cloned()
    }

    pub fn root_of_package(&self, key: &str) -> Option<&Component> {
        self.items.iter().find(|c| c.is_root && c.package == key)
    }

    pub fn degree(&self, id: &str) -> usize {
        self.edges
            .iter()
            .filter(|e| e.source == id || e.target == id)
            .count()
    }

    /// Distinct roles among the components (ignoring `Other`).
    pub fn roles(&self) -> BTreeSet<Role> {
        self.items
            .iter()
            .map(|c| c.role)
            .filter(|r| *r != Role::Other)
            .collect()
    }

    /// Folds the least connected non-root components into their parent
    /// until at most `max` remain. Roots with an entry point are never
    /// folded; the merged component keeps the parent's identity and gains
    /// the child's edges (duplicates and self-loops dropped).
    pub fn collapse_to(&mut self, max: usize) {
        while self.items.len() > max {
            // Least connected first; among equals, the package with the most
            // components gives one up, so a flat package of many large files
            // cannot crowd out the others.
            let mut per_package: HashMap<&str, usize> = HashMap::new();
            for c in &self.items {
                *per_package.entry(c.package.as_str()).or_default() += 1;
            }
            let victim = self
                .items
                .iter()
                .filter(|c| !c.protected && c.parent.is_some())
                .min_by_key(|c| {
                    (
                        self.degree(&c.id),
                        c.is_root,
                        std::cmp::Reverse(
                            per_package.get(c.package.as_str()).copied().unwrap_or(0),
                        ),
                        c.id.clone(),
                    )
                })
                .map(|c| c.id.clone());
            let Some(victim) = victim else { break };
            self.merge_into_parent(&victim);
        }
        // Components nothing touches (and nobody is folded into) are noise,
        // except roots.
        let connected: HashSet<String> = self
            .edges
            .iter()
            .flat_map(|e| [e.source.clone(), e.target.clone()])
            .collect();
        self.items
            .retain(|c| c.is_root || connected.contains(&c.id));
    }

    pub fn merge_into_parent(&mut self, id: &str) {
        let Some(index) = self.items.iter().position(|c| c.id == id) else {
            return;
        };
        let child = self.items.remove(index);
        let parent_id = child.parent.clone().unwrap_or_default();
        let parent_exists = self.items.iter().any(|c| c.id == parent_id);
        let target = if parent_exists {
            parent_id
        } else {
            // The parent went first: fold into the package root.
            match self.root_of_package(&child.package) {
                Some(root) => root.id.clone(),
                None => return,
            }
        };
        if let Some(parent) = self.items.iter_mut().find(|c| c.id == target) {
            parent.files.extend(child.files.iter().cloned());
        }
        for file in &child.files {
            self.by_file.insert(file.clone(), target.clone());
        }
        for other in &mut self.items {
            if other.parent.as_deref() == Some(id) {
                other.parent = Some(target.clone());
            }
        }
        let target_is_root = self.get(&target).is_some_and(|c| c.is_root);
        let child_package = child.package.clone();
        let mut merged: Vec<ComponentEdge> = Vec::new();
        for mut edge in std::mem::take(&mut self.edges) {
            if edge.source == id {
                edge.source = target.clone();
            }
            if edge.target == id {
                edge.target = target.clone();
                // A call into a module that folded into its own crate root is plumbing.
                if target_is_root
                    && self
                        .get(&edge.source)
                        .is_some_and(|s| s.package == child_package)
                {
                    edge.label = "uses".to_owned();
                    edge.style = style::CALL.to_owned();
                }
            }
            if edge.source == edge.target {
                continue;
            }
            match merged
                .iter_mut()
                .find(|e| e.source == edge.source && e.target == edge.target)
            {
                Some(existing) => {
                    if verb_rank(&edge.label) < verb_rank(&existing.label) {
                        *existing = edge;
                    }
                }
                None => merged.push(edge),
            }
        }
        self.edges = merged;
    }
}

/// Verb and style for an import, from what the importing file does and
/// what the imported component is.
pub fn verb_for(text: &str, target: &Component) -> (&'static str, &'static str) {
    // The component's own name: its label, its file stem and (for a
    // directory) the directory name. Parent directories would make every
    // module under `harness/` a runner.
    let mut words: Vec<String> = split_words(&target.label);
    let stem = file_name(&target.path);
    let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
    if matches!(stem, "mod" | "lib" | "index" | "__init__" | "main") {
        words.extend(split_words(file_name(parent_dir(&target.path))));
    } else {
        words.extend(split_words(stem));
    }
    let has = |list: &[&str]| words.iter().any(|w| list.contains(&w.as_str()));
    let is_store = has(&[
        "store",
        "stores",
        "db",
        "database",
        "repo",
        "repository",
        "recorder",
        "history",
        "persistence",
        "storage",
        "cache",
        "ops",
        "migrations",
    ]);
    let is_config = has(&[
        "config", "settings", "schema", "workflow", "env", "cli", "options", "args",
    ]);
    let is_runner = has(&[
        "worker",
        "workers",
        "runner",
        "process",
        "session",
        "launch",
        "agent",
        "agents",
        "runtime",
        "orchestrator",
        "executor",
        "dispatch",
        "job",
        "jobs",
        "task",
        "tasks",
        "spawn",
        "scheduler",
        "harness",
    ]);
    let is_stream = has(&[
        "sse",
        "stream",
        "events",
        "live",
        "subscribe",
        "snapshot",
        "watch",
        "handle",
        "relay",
        "source",
    ]);
    let is_client = has(&[
        "tracker",
        "trackers",
        "client",
        "clients",
        "transport",
        "rest",
        "provider",
        "providers",
        "adapter",
        "adapters",
        "codex",
        "fetch",
        "github",
        "gitlab",
        "jira",
        "linear",
        "asana",
        "ssh",
    ]);
    let is_routes = has(&["routes", "router", "handlers", "endpoints", "api"]);
    let is_server = has(&["server", "http", "control"]);
    let is_ui = target.kind == kind::UI
        || has(&[
            "view",
            "views",
            "presenter",
            "dashboard",
            "format",
            "page",
            "pages",
            "components",
            "layout",
            "panel",
            "table",
            "ui",
        ]);

    if target.ui_package && (is_client || is_routes || has(&["http", "query"])) {
        return ("calls", style::CALL);
    }
    if target.kind == kind::UI {
        return ("renders", style::CALL);
    }
    let bootstrap_file = target.role == Role::Bootstrap
        && !target.is_root
        && words_of(&target.path)
            .last()
            .is_some_and(|w| matches!(w.as_str(), "app" | "main" | "bootstrap" | "startup"));
    if bootstrap_file {
        return ("starts", style::CALL);
    }
    if is_config {
        return if VALIDATE.is_match(text) && has(&["schema", "validate", "validation"]) {
            ("validates", style::CALL)
        } else if LOAD.is_match(text) {
            ("loads", style::CALL)
        } else {
            ("configures", style::CALL)
        };
    }
    if is_store {
        return if SQL_WRITE.is_match(text) || WRITE.is_match(text) {
            ("records", style::DATA)
        } else {
            let _ = SQL_READ.is_match(text);
            ("reads", style::DATA)
        };
    }
    if is_runner {
        return if SPAWN.is_match(text) {
            if has(&["runtime", "orchestrator", "server", "app", "scheduler"]) {
                ("starts", style::CALL)
            } else {
                ("runs", style::CALL)
            }
        } else if RUN_CALL.is_match(text) {
            ("runs", style::CALL)
        } else if has(&["dispatch", "worker", "workers"]) {
            ("dispatches", style::CALL)
        } else {
            ("uses", style::CALL)
        };
    }
    if is_stream && STREAM.is_match(text) {
        return ("streams", style::ASYNC);
    }
    if is_client {
        return ("calls", style::CALL);
    }
    if is_routes {
        return ("serves", style::CALL);
    }
    if is_server {
        return if SPAWN.is_match(text) {
            ("starts", style::CALL)
        } else if HTTP_CLIENT.is_match(text) {
            ("calls", style::CALL)
        } else {
            ("serves", style::CALL)
        };
    }
    if is_ui {
        return ("renders", style::CALL);
    }
    ("uses", style::CALL)
}

/// Label and kind for a directory component from its name and purpose.
pub fn dir_label(
    dir_name: &str,
    rel: &str,
    package: &str,
    ui_package: bool,
) -> (String, &'static str) {
    let name = dir_name.to_lowercase();
    let pkg = capitalise(package);
    let own = sentence(&split_words(dir_name));
    let generic = |suffix: &str| {
        if pkg.is_empty() {
            format!("{own} {suffix}")
        } else {
            format!("{pkg} {suffix}")
        }
    };
    match name.as_str() {
        "routes" | "router" | "http" | "handlers" | "endpoints" | "controllers" => (
            if ui_package {
                generic("client")
            } else {
                "HTTP server".to_owned()
            },
            kind::SERVICE,
        ),
        "api" => (
            if ui_package {
                "API client".to_owned()
            } else {
                "HTTP server".to_owned()
            },
            if ui_package {
                kind::MODULE
            } else {
                kind::SERVICE
            },
        ),
        "store" | "db" | "database" | "repo" | "repository" | "repositories" | "persistence"
        | "storage" | "dao" => (generic("store"), kind::MODULE),
        "worker" | "workers" | "runner" | "runners" | "executor" | "executors" | "jobs"
        | "tasks" | "scheduler" => (generic("runner"), kind::MODULE),
        "config" | "settings" | "schema" | "options" => (generic("settings"), kind::MODULE),
        "ui" | "components" | "pages" | "widgets" | "screens" => (generic("UI"), kind::UI),
        "views" | "features" | "layout" | "live" => (format!("{own} UI"), kind::UI),
        "client" | "clients" | "sdk" => (generic("client"), kind::MODULE),
        "adapters" | "providers" | "integrations" | "trackers" | "connectors" => {
            (own.clone(), kind::MODULE)
        }
        _ => {
            // "features/overview" → "Overview"; keep the directory's own name.
            let _ = rel;
            (own, if ui_package { kind::UI } else { kind::MODULE })
        }
    }
}

/// Package labels with the workspace-wide prefix removed: when three or
/// more packages share `symphony-`, `symphony-core` is just "core".
pub fn short_package_names(catalog: &Catalog<'_>) -> BTreeMap<String, String> {
    let labels: Vec<(String, String)> = catalog
        .nodes()
        .filter(|n| catalog.is_product_node(n))
        .map(|n| (n.key.clone(), n.label.to_lowercase()))
        .collect();
    // Every dash-separated prefix of every label; the longest one three or
    // more packages share wins ("view-engine" over "view").
    let mut prefix_counts: HashMap<String, usize> = HashMap::new();
    for (_, label) in &labels {
        let parts: Vec<&str> = label.split(['-', '_']).collect();
        for end in 1..parts.len() {
            *prefix_counts.entry(parts[..end].join("-")).or_default() += 1;
        }
    }
    let common = prefix_counts
        .into_iter()
        .filter(|(_, n)| *n >= 3)
        .max_by_key(|(p, n)| (p.len(), *n, p.clone()))
        .map(|(p, _)| p);
    labels
        .into_iter()
        .map(|(key, label)| {
            let short = match &common {
                Some(prefix) => {
                    let dashed = label.replace('_', "-");
                    dashed
                        .strip_prefix(&format!("{prefix}-"))
                        .unwrap_or(&label)
                        .to_owned()
                }
                None => label.clone(),
            };
            let short = split_words(&short).join(" ");
            (key, short)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagrams::{DiagramInput, fixtures};

    fn symphony<'a>(
        ctx: &'a sdi_proto::knowledge::v1::ProjectContext,
        tree: &'a [sdi_proto::knowledge::v1::RepoTreeEntry],
        files: &'a BTreeMap<String, String>,
    ) -> DiagramInput<'a> {
        fixtures::input(ctx, tree, files, "")
    }

    #[test]
    fn roles_and_words() {
        assert_eq!(
            role_of(&words_of("crates/symphony-store/src/store.rs")),
            Role::State
        );
        assert_eq!(
            role_of(&words_of("crates/symphony-server/src/sse.rs")),
            Role::Monitoring
        );
        assert_eq!(
            role_of(&words_of("crates/symphony-trackers/src/linear.rs")),
            Role::Integrations
        );
        assert_eq!(
            role_of(&words_of("crates/symphony-runtime/src/orchestrator/mod.rs")),
            Role::Execution
        );
        assert_eq!(
            role_of(&words_of("crates/symphony-core/src/config/schema.rs")),
            Role::Bootstrap
        );
        assert_eq!(role_of(&words_of("quant/app/pricing.py")), Role::Other);
        assert_eq!(
            role_of(&words_of("crates/symphony/src/app.rs")),
            Role::Bootstrap
        );
        assert_eq!(split_words("useLiveState"), vec!["use", "live", "state"]);
        assert_eq!(sentence(&split_words("workflow_store")), "Workflow store");
    }

    #[test]
    fn files_fold_into_directory_file_and_root_components() {
        let files = fixtures::symphony_files();
        let tree = fixtures::symphony_tree();
        let ctx = fixtures::symphony_context();
        let input = symphony(&ctx, &tree, &files);
        let catalog = Catalog::new(&input);
        let comps = Components::build(&catalog);
        // The binary's root is its entry point; app.rs is big enough to stand alone.
        let root = comps.root_of_package("service:symphony").unwrap();
        assert_eq!(root.path, "crates/symphony/src/main.rs");
        assert!(root.protected);
        let app = comps
            .get("file_crates_symphony_src_app_rs")
            .expect("app.rs component");
        // Two `app.rs` files: the package name tells them apart.
        assert_eq!(app.label, "Symphony app");
        assert_eq!(app.caption, "app.rs");
        assert_eq!(
            comps
                .get("file_crates_symphony_server_src_app_rs")
                .unwrap()
                .label,
            "Server app"
        );
        assert_eq!(app.parent.as_deref(), Some("service_symphony"));
        // orchestrator/ has ≥ 3 files: a directory component.
        let orch = comps
            .get("dir_crates_symphony_runtime_src_orchestrator")
            .expect("orchestrator dir");
        assert_eq!(orch.label, "Orchestrator");
        assert_eq!(orch.role, Role::Execution);
        assert_eq!(orch.path, "crates/symphony-runtime/src/orchestrator/mod.rs");
        // config/ is "<package> settings".
        let config = comps
            .get("dir_crates_symphony_core_src_config")
            .expect("config dir");
        assert_eq!(config.label, "Core settings");
        // A small file with little fan-in folds into its root.
        assert_eq!(
            comps
                .component_of("crates/symphony-runtime/src/tracker.rs")
                .as_deref(),
            Some("library_symphony_runtime")
        );
        // Verbs come from the importer.
        let find = |s: &str, t: &str| comps.edges.iter().find(|e| e.source == s && e.target == t);
        let starts = find(
            "file_crates_symphony_src_app_rs",
            "file_crates_symphony_runtime_src_runtime_rs",
        )
        .unwrap();
        assert_eq!(starts.label, "starts");
        assert_eq!(starts.evidence_path, "crates/symphony/src/app.rs");
        let loads = find(
            "file_crates_symphony_src_app_rs",
            "file_crates_symphony_core_src_workflow_store_rs",
        )
        .unwrap();
        assert_eq!(loads.label, "loads");
        let records = find(
            "dir_crates_symphony_runtime_src_orchestrator",
            "file_crates_symphony_store_src_store_rs",
        )
        .unwrap();
        assert_eq!(records.label, "records");
        assert_eq!(records.style, style::DATA);
        assert!(comps.roles().len() >= 4);
    }

    #[test]
    fn collapse_folds_small_components_and_keeps_roots() {
        let files = fixtures::symphony_files();
        let tree = fixtures::symphony_tree();
        let ctx = fixtures::symphony_context();
        let input = symphony(&ctx, &tree, &files);
        let catalog = Catalog::new(&input);
        let mut comps = Components::build(&catalog);
        let before = comps.items.len();
        assert!(before > 10, "{before}");
        comps.collapse_to(10);
        assert!(comps.items.len() <= 10);
        assert!(comps.get("service_symphony").is_some());
        assert!(comps.edges.iter().all(|e| e.source != e.target));
        assert!(
            comps
                .edges
                .iter()
                .all(|e| comps.get(&e.source).is_some() && comps.get(&e.target).is_some())
        );
    }
}
