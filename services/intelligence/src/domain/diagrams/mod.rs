//! Evidence-based diagram specs (CONTRACT.md §16). Every builder is a pure
//! function of a `DiagramInput`: the project context (facts, graph,
//! understanding, dependencies), the repository tree and a bounded sample
//! of key files. Nodes link to the file that implements them, edges cite
//! the file that shows the call, and anything inferred is dashed.

pub mod architecture;
pub mod catalog;
pub mod components;
pub mod data_flow;
pub mod db_schema;
pub mod dependency;
pub mod deployment;
pub mod evidence;
pub mod high_level;
pub mod imports;
pub mod mermaid;
pub mod model;
pub mod module_map;
pub mod schema;
pub mod sequence;

#[cfg(test)]
pub mod fixtures;

use std::collections::BTreeMap;

use sdi_proto::{
    intelligence::v1::DiagramKind,
    knowledge::v1::{NodeKind, ProjectContext, RelationCategory, RepoTreeEntry},
};

use catalog::Catalog;
pub use model::{DiagramEdge, DiagramSpec};

/// Key files read per diagram and the size each is cut to: `key_files`
/// picks up to `PRIMARY_KEY_FILES` by structure (entry points, routers,
/// module hubs, manifests, large sources), `follow_imports` tops the sample
/// up to `MAX_KEY_FILES` with the files those import most.
pub const MAX_KEY_FILES: usize = 60;
pub const PRIMARY_KEY_FILES: usize = 40;
pub const MAX_FILE_BYTES: usize = 24 * 1024;
/// The database schema reads every migration and model file, up to this many.
pub const MAX_SCHEMA_FILES: usize = 200;

pub struct DiagramInput<'a> {
    pub context: &'a ProjectContext,
    pub tree: &'a [RepoTreeEntry],
    /// Path -> excerpt (at most `MAX_FILE_BYTES` each) of the key files.
    pub files: &'a BTreeMap<String, String>,
    /// Focus hint ("the payment flow") or the custom prompt; may be empty.
    pub prompt: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    pub title: String,
    pub explanation: String,
    pub spec: DiagramSpec,
}

pub fn kind_label(kind: DiagramKind) -> &'static str {
    match kind {
        DiagramKind::Architecture => "architecture",
        DiagramKind::HighLevel => "high-level",
        DiagramKind::DataFlow => "data flow",
        DiagramKind::Deployment => "deployment",
        DiagramKind::Dependency => "dependency",
        DiagramKind::Sequence => "sequence",
        DiagramKind::ModuleMap => "module map",
        DiagramKind::DbSchema => "database schema",
        DiagramKind::Custom => "custom",
        DiagramKind::Unspecified => "diagram",
    }
}

/// Builds the spec for a kind, applies the limits and writes the templated
/// title and explanation. `Custom` is the architecture map focused on the
/// prompt (Mermaid imports are handled before reaching here).
pub fn generate(kind: DiagramKind, input: &DiagramInput<'_>) -> Option<Generated> {
    let mut spec = match kind {
        DiagramKind::Architecture => architecture::build(input),
        DiagramKind::HighLevel => high_level::build(input),
        DiagramKind::DataFlow => data_flow::build(input),
        DiagramKind::Deployment => deployment::build(input),
        DiagramKind::Dependency => dependency::build(input),
        DiagramKind::Sequence => sequence::build(input),
        DiagramKind::ModuleMap => module_map::build(input),
        DiagramKind::DbSchema => db_schema::build(input),
        DiagramKind::Custom => architecture::build_focused(input, input.prompt),
        DiagramKind::Unspecified => return None,
    };
    spec.enforce_limits();
    let catalog = Catalog::new(input);
    Some(Generated {
        title: title(kind, &catalog, input.prompt),
        explanation: explanation(kind, &spec, &catalog),
        spec,
    })
}

pub fn title(kind: DiagramKind, catalog: &Catalog<'_>, prompt: &str) -> String {
    let name = catalog.project_name();
    match kind {
        DiagramKind::Architecture => format!("{name} architecture"),
        DiagramKind::HighLevel => format!("{name} at a glance"),
        DiagramKind::DataFlow => format!("{name} data flow"),
        DiagramKind::Deployment => format!("{name} deployment"),
        DiagramKind::Dependency => format!("{name} dependencies"),
        DiagramKind::Sequence => format!("{name} request sequence"),
        DiagramKind::ModuleMap => format!("{name} module map"),
        DiagramKind::DbSchema => format!("{name} database schema"),
        DiagramKind::Custom => {
            let focus: String = prompt
                .split_whitespace()
                .take(6)
                .collect::<Vec<_>>()
                .join(" ");
            if focus.is_empty() {
                format!("{name} custom view")
            } else {
                format!("{name}: {focus}")
            }
        }
        DiagramKind::Unspecified => name,
    }
}

/// 60–150 words that say what is shown and where the evidence stops.
pub fn explanation(kind: DiagramKind, spec: &DiagramSpec, catalog: &Catalog<'_>) -> String {
    let name = catalog.project_name();
    let evidenced = spec
        .edges
        .iter()
        .filter(|e| !e.evidence_path.is_empty())
        .count();
    let inferred = spec.edges.len() - evidenced;
    let linked = spec.nodes.iter().filter(|n| !n.path.is_empty()).count();
    let files = catalog.input.files.len();
    let commit = {
        let sha = catalog.commit_sha();
        if sha.is_empty() {
            "the last scan".to_owned()
        } else {
            format!("commit {}", &sha[..sha.len().min(7)])
        }
    };
    let what = match kind {
        DiagramKind::Architecture | DiagramKind::Custom => {
            "the runtime components of the product and the calls between them, grouped by top-level directory, data stores and external systems"
        }
        DiagramKind::HighLevel => {
            "who uses the product, the few components that make it up and the external systems it depends on, on one screen"
        }
        DiagramKind::DataFlow => {
            "which components read, write and publish to each data store and queue"
        }
        DiagramKind::Deployment => {
            "where the software runs: pipelines, container images, environments and the infrastructure that provisions them"
        }
        DiagramKind::Dependency => {
            "the project's own packages per ecosystem and the third-party packages they depend on, with fan-in across manifests"
        }
        DiagramKind::Sequence => {
            "the principal workflow as ordered messages: from the user through the entry point and the modules it calls to the data store, and the returns back"
        }
        DiagramKind::ModuleMap => {
            "directories and packages as a tree, as the repository is laid out"
        }
        DiagramKind::DbSchema => {
            "every table with its columns as the migrations and ORM models leave it once applied in order, and the foreign keys between tables"
        }
        DiagramKind::Unspecified => "the project",
    };
    let mut text = format!(
        "This {} diagram of {name} shows {what}. It has {} nodes in {} groups and {} edges, built from the knowledge graph and repository tree at {commit}. ",
        kind_label(kind),
        spec.nodes.len(),
        spec.groups.len(),
        spec.edges.len(),
    );
    text.push_str(&format!(
        "{linked} nodes link to the file that implements them; {evidenced} edges cite the file that shows the relationship, and {inferred} are dashed because they are inferred rather than observed. "
    ));
    if files > 0 {
        text.push_str(&format!(
            "Call and data evidence comes from {files} sampled key files (entry points, routers, manifests, migrations and the modules they import), so calls made elsewhere may be missing. "
        ));
    } else {
        text.push_str(
            "No key files could be sampled, so edges rely on the knowledge graph alone and calls it did not detect are missing. ",
        );
    }
    text.push_str(match kind {
        DiagramKind::Deployment => {
            "Pipelines and infrastructure appear only when the scan found workflows, compose files or IaC. "
        }
        DiagramKind::DbSchema => {
            "Columns come from every migration and model file in the scan, applied in path order; tables created in code are not listed. "
        }
        DiagramKind::Dependency => {
            "Only the most used runtime dependencies are shown; development and build tooling is left out. "
        }
        _ => "Tests, CI workflows, documentation and fixtures are left out on purpose. ",
    });
    text.push_str(
        "The map is capped at 34 nodes and 48 edges; verify anything surprising by opening the cited file.",
    );
    text
}

/// The sample of files a kind reads, ranked by relevance to the kind:
/// entry points, routers, module hubs (`lib.rs`, `mod.rs`, `index.ts`,
/// `__init__.py`), manifests, deployment descriptors, large source files
/// and migrations; shallow and large files first; tests, fixtures, docs,
/// CI, generated and vendored files never. The database schema gets every
/// migration and model file (up to `MAX_SCHEMA_FILES`); every other kind
/// at most `PRIMARY_KEY_FILES`. Pure, so the gRPC layer only fetches.
pub fn key_files(kind: DiagramKind, tree: &[RepoTreeEntry]) -> Vec<String> {
    if kind == DiagramKind::DbSchema {
        let mut files: Vec<&str> = tree
            .iter()
            .filter(|e| {
                !e.is_dir && !Catalog::is_excluded_path(&e.path) && !imports::is_generated(&e.path)
            })
            .filter(|e| db_schema::is_schema_file(&e.path))
            .map(|e| e.path.as_str())
            .collect();
        files.sort();
        return files
            .into_iter()
            .take(MAX_SCHEMA_FILES)
            .map(str::to_owned)
            .collect();
    }
    let mut ranked: Vec<(u8, usize, std::cmp::Reverse<u64>, &str)> = tree
        .iter()
        .filter(|e| !e.is_dir)
        .filter(|e| !Catalog::is_excluded_path(&e.path) || is_workflow(&e.path))
        .filter(|e| !imports::is_generated(&e.path))
        .filter_map(|e| {
            let depth = e.path.matches('/').count();
            let class = classify(&e.path).or_else(|| {
                if !imports::is_source(&e.path) {
                    None
                } else if e.size >= components::SIGNIFICANT_BYTES {
                    Some(FileClass::LargeSource)
                } else {
                    Some(FileClass::Source)
                }
            })?;
            let priority = priority(kind, class, depth)?;
            Some((priority, depth, std::cmp::Reverse(e.size), e.path.as_str()))
        })
        .collect();
    ranked.sort();
    // Entry points of every package (manifest directory) always make it
    // for the kinds that read the import graph.
    let mut out: Vec<String> = if matches!(
        kind,
        DiagramKind::Architecture
            | DiagramKind::Custom
            | DiagramKind::DataFlow
            | DiagramKind::Sequence
            | DiagramKind::HighLevel
    ) {
        let mut always = package_entry_points(tree);
        // Manifests name the crates and their lib aliases, which the
        // resolver needs; they are small.
        let mut manifests: Vec<String> = tree
            .iter()
            .filter(|e| !e.is_dir && !Catalog::is_excluded_path(&e.path))
            .filter(|e| {
                matches!(
                    e.name.as_str(),
                    "Cargo.toml" | "package.json" | "pyproject.toml" | "go.mod"
                )
            })
            .map(|e| e.path.clone())
            .collect();
        manifests.sort();
        always.extend(manifests.into_iter().take(PRIMARY_KEY_FILES / 4));
        always
    } else {
        Vec::new()
    };
    for (_, _, _, path) in ranked {
        if out.len() >= PRIMARY_KEY_FILES {
            break;
        }
        if !out.iter().any(|p| p == path) {
            out.push(path.to_owned());
        }
    }
    out.truncate(PRIMARY_KEY_FILES);
    out
}

/// The entry point of every directory holding a manifest, from the tree alone.
fn package_entry_points(tree: &[RepoTreeEntry]) -> Vec<String> {
    let files: std::collections::HashSet<&str> = tree
        .iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.path.as_str())
        .collect();
    let mut dirs: Vec<&str> = tree
        .iter()
        .filter(|e| !e.is_dir && !Catalog::is_excluded_path(&e.path))
        .filter(|e| {
            matches!(
                e.name.as_str(),
                "Cargo.toml"
                    | "package.json"
                    | "pyproject.toml"
                    | "go.mod"
                    | "pom.xml"
                    | "build.gradle"
            )
        })
        .map(|e| e.path.rsplit_once('/').map(|(d, _)| d).unwrap_or(""))
        .collect();
    dirs.sort();
    dirs.dedup();
    let mut out = Vec::new();
    for dir in dirs {
        for candidate in catalog::ENTRY_POINTS {
            let path = if dir.is_empty() {
                candidate.to_owned()
            } else {
                format!("{dir}/{candidate}")
            };
            if files.contains(path.as_str()) && imports::is_source(&path) {
                if !out.contains(&path) {
                    out.push(path);
                }
                break;
            }
        }
    }
    out
}

/// Files the sample imports but has not read, most imported first, so the
/// gRPC layer can top the sample up to `MAX_KEY_FILES`. Entry points of
/// packages the sample missed come first.
pub fn follow_imports(input: &DiagramInput<'_>, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let catalog = Catalog::new(input);
    let resolver = imports::Resolver::new(&catalog);
    let mut out: Vec<String> = Vec::new();
    for node in catalog.nodes().filter(|n| catalog.is_product_node(n)) {
        if let Some(entry) = catalog.entry_point(&catalog.dir_of(node))
            && !input.files.contains_key(&entry)
            && imports::is_source(&entry)
        {
            out.push(entry);
        }
    }
    let graph = imports::FileGraph::build(&catalog, &resolver);
    let mut ranked: Vec<(std::cmp::Reverse<usize>, std::cmp::Reverse<u64>, String)> = graph
        .fan_in()
        .into_iter()
        .filter(|(path, _)| !input.files.contains_key(path) && !out.contains(path))
        .filter(|(path, _)| !Catalog::is_excluded_path(path) && !imports::is_generated(path))
        .map(|(path, fan_in)| {
            (
                std::cmp::Reverse(fan_in),
                std::cmp::Reverse(catalog.file_size(&path)),
                path,
            )
        })
        .collect();
    ranked.sort();
    out.extend(ranked.into_iter().map(|(_, _, p)| p));
    out.truncate(limit);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClass {
    Entry,
    Router,
    Hub,
    Manifest,
    Deploy,
    Workflow,
    Migration,
    Model,
    LargeSource,
    Source,
}

fn is_workflow(path: &str) -> bool {
    path.starts_with(".github/workflows/") && (path.ends_with(".yml") || path.ends_with(".yaml"))
}

pub(crate) fn classify(path: &str) -> Option<FileClass> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = path.to_lowercase();
    if is_workflow(path) {
        return Some(FileClass::Workflow);
    }
    if matches!(
        name,
        "main.rs"
            | "main.ts"
            | "main.tsx"
            | "index.ts"
            | "index.tsx"
            | "server.ts"
            | "app.ts"
            | "App.tsx"
            | "main.js"
            | "index.js"
            | "server.js"
            | "app.js"
            | "main.py"
            | "app.py"
            | "manage.py"
            | "main.go"
            | "Program.cs"
            | "lib.rs"
    ) {
        return Some(FileClass::Entry);
    }
    if matches!(
        name,
        "routes.rs"
            | "router.rs"
            | "routes.ts"
            | "router.ts"
            | "routes.py"
            | "urls.py"
            | "api.ts"
            | "api.rs"
            | "app.rs"
            | "handlers.rs"
            | "routes.js"
            | "router.js"
            | "views.py"
            | "server.rs"
    ) || lower.contains("/routes/mod.rs")
        || lower.contains("/routes/index.")
        || lower.contains("/api/mod.rs")
        || lower.contains("/api/index.")
        || lower.contains("/app/api/")
    {
        return Some(FileClass::Router);
    }
    if matches!(name, "mod.rs" | "__init__.py") {
        return Some(FileClass::Hub);
    }
    if matches!(
        name,
        "package.json"
            | "Cargo.toml"
            | "pyproject.toml"
            | "go.mod"
            | "requirements.txt"
            | "pom.xml"
            | "build.gradle"
            | "pnpm-workspace.yaml"
    ) {
        return Some(FileClass::Manifest);
    }
    if name.starts_with("docker-compose")
        || name == "compose.yml"
        || name == "compose.yaml"
        || name.starts_with("Dockerfile")
        || name.ends_with(".tf")
        || name.ends_with(".bicep")
        || lower.contains("/k8s/")
        || lower.contains("/kubernetes/")
        || lower.contains("/helm/")
        || lower.contains("/deploy/")
        || name == "render.yaml"
        || name == "fly.toml"
        || name == "Procfile"
    {
        return Some(FileClass::Deploy);
    }
    let migration_dir = lower.contains("/migrations/")
        || lower.starts_with("migrations/")
        || lower.contains("/migrate/")
        || lower.starts_with("migrate/")
        || lower.contains("/alembic/versions/")
        || lower.starts_with("alembic/versions/")
        || lower.contains("/db/migrate/")
        || lower.contains("/sql/");
    if migration_dir
        && (name.ends_with(".sql")
            || name.ends_with(".ts")
            || name.ends_with(".js")
            || name.ends_with(".py")
            || name.ends_with(".rb")
            || name.ends_with(".rs"))
        && !name.starts_with("mod.")
        && !name.starts_with("__init__")
    {
        return Some(FileClass::Migration);
    }
    if name == "schema.prisma"
        || name == "schema.sql"
        || name == "schema.rs"
        || name == "schema.rb"
        || name == "models.py"
        || name == "models.rs"
        || name == "entities.ts"
        || name == "entity.ts"
        || lower.contains("/models/")
        || lower.contains("/entities/")
        || lower.contains("/entity/")
        || lower.contains("/drizzle/schema")
        || lower.contains("/prisma/")
    {
        return Some(FileClass::Model);
    }
    None
}

/// Lower is better; `None` means a kind does not read that class at all.
fn priority(kind: DiagramKind, class: FileClass, depth: usize) -> Option<u8> {
    use FileClass::*;
    let base = match kind {
        DiagramKind::DbSchema => match class {
            Migration => 0,
            Model => 1,
            Manifest => 4,
            Deploy => 5,
            _ => return None,
        },
        DiagramKind::Deployment => match class {
            Deploy => 0,
            Workflow => 1,
            Manifest => 2,
            Entry => 3,
            _ => return None,
        },
        DiagramKind::Dependency => match class {
            Manifest => 0,
            Entry => 2,
            _ => return None,
        },
        DiagramKind::ModuleMap => match class {
            Manifest => 0,
            Entry => 1,
            _ => return None,
        },
        DiagramKind::Sequence => match class {
            Entry => 0,
            Router => 0,
            Hub => 2,
            LargeSource => 3,
            Model => 4,
            Manifest => 4,
            Source => 5,
            _ => return None,
        },
        _ => match class {
            Entry => 0,
            Router => 1,
            Hub => 2,
            Manifest => 3,
            Deploy => 4,
            LargeSource => 5,
            Model => 5,
            Source => 6,
            Migration => 7,
            Workflow => return None,
        },
    };
    // Shallow files first: a root manifest beats one buried in a sub-package.
    Some(base * 4 + depth.min(3) as u8)
}

/// Verb and style for a valid, non-structural graph edge, or `None` when
/// the relation has no place on a runtime map (containment, ownership,
/// testing, documentation).
pub fn graph_verb(
    catalog: &Catalog<'_>,
    edge: &sdi_proto::knowledge::v1::GraphEdge,
) -> Option<(&'static str, &'static str)> {
    use model::style;
    let target = catalog.node_by_id(&edge.target_id)?;
    // The one structural relation a map shows: which image a package becomes.
    if edge.relation == "PACKAGED_AS" {
        return Some(("packaged as", style::CONTAINS));
    }
    if edge.category() == RelationCategory::Structure
        || edge.category() == RelationCategory::Ownership
        || edge.category() == RelationCategory::Validation
    {
        return None;
    }
    Some(match edge.relation.as_str() {
        "DEPENDS_ON" | "USES" | "CALLS" | "READS" | "WRITES" | "PUBLISHES" | "CONSUMES" => {
            match target.kind() {
                NodeKind::Datastore if Catalog::is_queue(target) => match edge.relation.as_str() {
                    "CONSUMES" => ("consumes", style::ASYNC),
                    _ => ("publishes", style::ASYNC),
                },
                NodeKind::Datastore => match edge.relation.as_str() {
                    "READS" => ("reads", style::DATA),
                    "WRITES" => ("writes", style::DATA),
                    _ => ("reads/writes", style::DATA),
                },
                _ => ("calls", style::CALL),
            }
        }
        "DEPLOYED_TO" => ("deploys to", style::CONTAINS),
        "DEPLOYS" => ("deploys", style::CALL),
        "PROVISIONS" => ("provisions", style::CONTAINS),
        "RELEASES" => ("releases", style::CALL),
        "BUILDS" => ("builds", style::CALL),
        "RUNS_ON" => ("runs on", style::CONTAINS),
        "PROVIDES" => ("provides", style::CONTAINS),
        _ => return None,
    })
}

/// The file that shows a graph edge: the source node's own source path (the
/// manifest or descriptor the scan read), when it exists in the tree.
pub fn graph_evidence(catalog: &Catalog<'_>, edge: &sdi_proto::knowledge::v1::GraphEdge) -> String {
    let Some(source) = catalog.node_by_id(&edge.source_id) else {
        return String::new();
    };
    if !catalog.has_tree() || catalog.file_exists(&source.source_path) {
        source.source_path.clone()
    } else {
        String::new()
    }
}

/// Adds every valid non-structural graph edge whose endpoints are on the
/// spec, cited by the source's file or dashed when the file is unknown.
pub fn add_graph_edges(catalog: &Catalog<'_>, spec: &mut DiagramSpec) {
    for edge in catalog.edges() {
        let Some((label, style)) = graph_verb(catalog, edge) else {
            continue;
        };
        let (Some(source), Some(target)) = (
            catalog.node_by_id(&edge.source_id),
            catalog.node_by_id(&edge.target_id),
        ) else {
            continue;
        };
        let evidence_path = graph_evidence(catalog, edge);
        spec.add_edge(DiagramEdge {
            id: String::new(),
            source: catalog::node_id(&source.key),
            target: catalog::node_id(&target.key),
            label: label.to_owned(),
            style: style.to_owned(),
            dashed: evidence_path.is_empty(),
            evidence_path,
        });
    }
}

/// Adds the edges the sampled files show (never dashed).
pub fn add_evidence_edges(catalog: &Catalog<'_>, spec: &mut DiagramSpec) {
    for found in evidence::detect(catalog) {
        spec.add_edge(found.edge());
    }
}

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fixtures::{input, tree};

    #[test]
    fn key_files_prefer_entry_points_then_routers_then_manifests() {
        let tree = tree();
        let files = key_files(DiagramKind::Architecture, &tree);
        assert!(files.len() <= PRIMARY_KEY_FILES);
        // Entry points lead, shallowest first.
        assert!(
            files[..4].iter().all(|f| {
                f.ends_with("main.rs")
                    || f.ends_with("lib.rs")
                    || f.ends_with("main.tsx")
                    || f.ends_with("App.tsx")
                    || f.ends_with("main.py")
            }),
            "{files:?}"
        );
        // Module hubs and plain sources follow, never generated or vendored files.
        assert!(
            files.contains(&"services/api/src/routes/mod.rs".to_owned()),
            "{files:?}"
        );
        assert!(
            files.contains(&"services/api/src/cache.rs".to_owned()),
            "{files:?}"
        );
        assert!(files.contains(&"services/api/src/main.rs".to_owned()));
        assert!(files.contains(&"web/src/main.tsx".to_owned()));
        assert!(files.contains(&"services/quant/app/main.py".to_owned()));
        assert!(
            files
                .iter()
                .any(|f| f.ends_with("Cargo.toml") || f.ends_with("package.json"))
        );
        assert!(files.iter().all(|f| !f.contains("/tests/")));
        assert!(files.iter().all(|f| !f.starts_with(".github/")));

        let schema = key_files(DiagramKind::DbSchema, &tree);
        assert!(
            schema.iter().any(|f| f.contains("migrations/")),
            "{schema:?}"
        );
        assert!(
            schema
                .iter()
                .all(|f| f.contains("migrations/") || f.contains("models")),
            "{schema:?}"
        );
        // The symphony tree is bigger: the structural sample stays bounded
        // and the import follow-up fills the rest with what it imports.
        let symphony_tree = fixtures::symphony_tree();
        let primary = key_files(DiagramKind::Architecture, &symphony_tree);
        assert!(primary.len() <= PRIMARY_KEY_FILES);
        assert!(primary.contains(&"crates/symphony/src/main.rs".to_owned()));
        assert!(primary.contains(&"crates/symphony-runtime/src/lib.rs".to_owned()));
        assert!(
            primary
                .iter()
                .all(|f| !f.contains("/tests/") && !f.starts_with("xtask/")),
            "{primary:?}"
        );
        let ctx = fixtures::symphony_context();
        let sampled: BTreeMap<String, String> = fixtures::symphony_files()
            .into_iter()
            .filter(|(p, _)| primary.contains(p))
            .collect();
        let more = follow_imports(
            &input(&ctx, &symphony_tree, &sampled, ""),
            MAX_KEY_FILES - sampled.len(),
        );
        assert!(more.len() <= MAX_KEY_FILES - sampled.len());
        assert!(more.iter().all(|p| !sampled.contains_key(p)));
        assert!(!more.is_empty(), "the sample imports files it has not read");
        let deploy = key_files(DiagramKind::Deployment, &tree);
        assert_eq!(deploy[0], "docker-compose.yml");
        assert!(deploy.iter().any(|f| f.starts_with(".github/workflows/")));
    }

    #[test]
    fn every_kind_generates_within_limits_with_a_sensible_explanation() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let input = input(&ctx, &tree, &files, "");
        for kind in [
            DiagramKind::Architecture,
            DiagramKind::HighLevel,
            DiagramKind::DataFlow,
            DiagramKind::Deployment,
            DiagramKind::Dependency,
            DiagramKind::Sequence,
            DiagramKind::ModuleMap,
            DiagramKind::DbSchema,
            DiagramKind::Custom,
        ] {
            let generated = generate(kind, &input).unwrap();
            generated
                .spec
                .validate()
                .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
            assert!(generated.spec.nodes.len() <= model::MAX_NODES, "{kind:?}");
            assert!(generated.spec.edges.len() <= model::MAX_EDGES, "{kind:?}");
            assert!(generated.spec.groups.len() <= model::MAX_GROUPS, "{kind:?}");
            assert!(!generated.spec.nodes.is_empty(), "{kind:?} has no nodes");
            assert!(
                generated.title.starts_with("view-engine"),
                "{kind:?}: {}",
                generated.title
            );
            let words = word_count(&generated.explanation);
            assert!((60..=150).contains(&words), "{kind:?}: {words} words");
            // Evidence rule: a non-dashed edge always cites a file; a dashed one never does.
            for edge in &generated.spec.edges {
                assert_eq!(
                    edge.dashed,
                    edge.evidence_path.is_empty(),
                    "{kind:?}: {} -> {} ({})",
                    edge.source,
                    edge.target,
                    edge.evidence_path
                );
                assert!(
                    edge.label.split_whitespace().count() <= 3,
                    "{kind:?}: verb {:?}",
                    edge.label
                );
            }
            // Paths exist in the tree; labels are concise.
            for node in &generated.spec.nodes {
                if !node.path.is_empty() {
                    assert!(
                        tree.iter().any(|e| e.path == node.path),
                        "{kind:?}: {} links to {} which is not in the tree",
                        node.id,
                        node.path
                    );
                }
                assert!(
                    node.label.split_whitespace().count() <= 4,
                    "{kind:?}: label {:?}",
                    node.label
                );
            }
        }
        assert!(generate(DiagramKind::Unspecified, &input).is_none());
    }

    #[test]
    fn graph_verbs_follow_the_relation_and_target_kind() {
        let files = BTreeMap::new();
        let tree = tree();
        let ctx = fixtures::context();
        let input = input(&ctx, &tree, &files, "");
        let catalog = Catalog::new(&input);
        let mut seen = std::collections::BTreeSet::new();
        for edge in catalog.edges() {
            if let Some((verb, _)) = graph_verb(&catalog, edge) {
                seen.insert(verb);
            }
        }
        assert!(seen.contains("reads/writes"));
        assert!(seen.contains("calls"));
        assert!(seen.contains("publishes") || seen.contains("deploys to"));
        // CONTAINS never becomes an edge.
        let contains = catalog.edges().find(|e| e.relation == "CONTAINS").unwrap();
        assert!(graph_verb(&catalog, contains).is_none());
    }
}
