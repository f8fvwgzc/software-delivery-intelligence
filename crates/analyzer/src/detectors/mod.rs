//! The detector pipeline and the context it accumulates into.
//!
//! Detectors run in a fixed order because later ones read what earlier ones
//! found (frameworks need dependencies, questions need deploy evidence).
//! Each one only appends; nothing is removed until the graph pass.

pub(crate) mod ci;
pub(crate) mod cloud;
pub(crate) mod containers;
pub(crate) mod datastores;
pub(crate) mod dependencies;
pub(crate) mod docs;
pub(crate) mod frameworks;
pub(crate) mod iac;
pub(crate) mod languages;
pub(crate) mod legacy;
pub(crate) mod migrations;
pub(crate) mod runtime;
pub(crate) mod security;
pub(crate) mod testing;

use std::collections::{BTreeMap, BTreeSet};

use crate::config::CONFIG_PATH;
use crate::eol::Day;
use crate::manifests::Manifest;
use crate::model::*;
use crate::util::{basename, dirname, excerpt};

/// A deployable unit found through a manifest. Services and libraries get a
/// graph node each; datastores, tests and images hang off them.
#[derive(Debug, Clone)]
pub(crate) struct App {
    pub key: String,
    pub name: String,
    pub dir: String,
    pub manifest_path: String,
    pub frontend: Vec<String>,
    pub backend: Vec<String>,
    pub library: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ImageRef {
    pub path: String,
    pub image: String,
    pub line: String,
}

pub(crate) struct Ctx<'a> {
    pub snap: &'a RepoSnapshot,
    pub out: Analysis,
    pub repo_key: String,
    pub manifests: Vec<Manifest>,
    pub apps: Vec<App>,
    /// Images declared by compose or Kubernetes manifests.
    pub images: Vec<ImageRef>,
    /// Environment names with evidence, e.g. `production`, `staging`, `local`.
    pub environments: BTreeSet<String>,
    /// Where deploys go, from IaC or CI: `azure`, `aws`, `gcp`, `kubernetes`, `github_pages`, ...
    pub deploy_targets: BTreeSet<String>,
    /// The day the end-of-life table is compared against. Passed in, never read from a clock.
    pub today: Day,
}

pub(crate) fn attrs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

impl<'a> Ctx<'a> {
    pub fn new(snap: &'a RepoSnapshot, today: Day) -> Self {
        let name = if snap.full_name.is_empty() {
            "repository".to_string()
        } else {
            snap.full_name.clone()
        };
        let mut ctx = Self {
            snap,
            out: Analysis::default(),
            repo_key: format!("repository:{name}"),
            manifests: Vec::new(),
            apps: Vec::new(),
            images: Vec::new(),
            environments: BTreeSet::new(),
            deploy_targets: BTreeSet::new(),
            today,
        };
        let key = ctx.repo_key.clone();
        ctx.node(
            NodeKind::Repository,
            &key,
            &name,
            &snap.description,
            "",
            attrs(&[
                ("default_branch", &snap.default_branch),
                ("commit", &snap.commit_sha),
                (
                    "visibility",
                    if snap.is_private { "private" } else { "public" },
                ),
            ]),
        );
        ctx
    }

    pub fn repo_name(&self) -> &str {
        self.snap
            .full_name
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("repository")
    }

    // -- facts ------------------------------------------------------------

    pub fn detected(
        &mut self,
        category: FactCategory,
        key: &str,
        label: &str,
        value: &str,
        source_path: &str,
        evidence: &str,
    ) {
        self.out.facts.push(FactDraft {
            kind: FactKind::Detected,
            category,
            key: key.to_string(),
            label: label.to_string(),
            value: value.to_string(),
            confidence: 1.0,
            source_path: source_path.to_string(),
            evidence: excerpt(evidence, 160),
        });
    }

    pub fn inferred(
        &mut self,
        category: FactCategory,
        key: &str,
        label: &str,
        value: &str,
        confidence: f64,
        evidence: &str,
    ) {
        self.out.facts.push(FactDraft {
            kind: FactKind::Inferred,
            category,
            key: key.to_string(),
            label: label.to_string(),
            value: value.to_string(),
            confidence: confidence.clamp(0.0, 0.99),
            source_path: String::new(),
            evidence: excerpt(evidence, 160),
        });
    }

    /// A value the repository declared in `.sdi.yml`: as trustworthy as a
    /// detected fact, kept apart so the UI can show who said it.
    pub fn configured(&mut self, key: &str, label: &str, value: &str, evidence: &str) {
        self.out.facts.push(FactDraft {
            kind: FactKind::Configured,
            category: FactCategory::Configuration,
            key: key.to_string(),
            label: label.to_string(),
            value: value.to_string(),
            confidence: 1.0,
            source_path: CONFIG_PATH.to_string(),
            evidence: excerpt(evidence, 160),
        });
    }

    pub fn has_fact(&self, key: &str) -> bool {
        self.out.facts.iter().any(|f| f.key == key)
    }

    pub fn has_fact_prefix(&self, prefix: &str) -> bool {
        self.out.facts.iter().any(|f| f.key.starts_with(prefix))
    }

    /// Suffixes of fact keys under `prefix`, in first-seen order, deduped.
    pub fn fact_suffixes(&self, prefix: &str) -> Vec<String> {
        let mut seen = BTreeSet::new();
        self.out
            .facts
            .iter()
            .filter_map(|f| f.key.strip_prefix(prefix))
            .filter(|s| seen.insert(s.to_string()))
            .map(String::from)
            .collect()
    }

    pub fn fact(&self, key: &str) -> Option<&FactDraft> {
        self.out.facts.iter().find(|f| f.key == key)
    }

    // -- graph ------------------------------------------------------------

    pub fn node(
        &mut self,
        kind: NodeKind,
        key: &str,
        label: &str,
        description: &str,
        source_path: &str,
        attributes: BTreeMap<String, String>,
    ) -> String {
        self.out.nodes.push(NodeDraft {
            kind,
            key: key.to_string(),
            label: label.to_string(),
            description: description.to_string(),
            source_path: source_path.to_string(),
            attributes,
        });
        key.to_string()
    }

    pub fn edge(&mut self, source: &str, target: &str, relation: &str, category: RelationCategory) {
        self.out.edges.push(EdgeDraft {
            source_key: source.to_string(),
            target_key: target.to_string(),
            relation: relation.to_string(),
            category,
            attributes: BTreeMap::new(),
        });
    }

    pub fn edge_with(
        &mut self,
        source: &str,
        target: &str,
        relation: &str,
        category: RelationCategory,
        attributes: BTreeMap<String, String>,
    ) {
        self.out.edges.push(EdgeDraft {
            source_key: source.to_string(),
            target_key: target.to_string(),
            relation: relation.to_string(),
            category,
            attributes,
        });
    }

    pub fn has_node(&self, key: &str) -> bool {
        self.out.nodes.iter().any(|n| n.key == key)
    }

    pub fn chunk(&mut self, kind: ChunkKind, path: &str, title: &str, content: &str) {
        // Compose files and workflows legitimately name secrets; their values never travel.
        let content = crate::util::redact(content.trim());
        if content.is_empty() || crate::files::looks_secret(path) {
            return;
        }
        self.out.chunks.push(ChunkDraft {
            kind,
            path: path.to_string(),
            title: title.to_string(),
            content: crate::util::truncate(&content, 4000),
        });
    }

    // -- repository access --------------------------------------------------

    pub fn file(&self, path: &str) -> Option<&'a str> {
        self.snap.files.get(path).map(String::as_str)
    }

    /// Downloaded files whose path satisfies `pred`, sorted by path.
    pub fn files_where(&self, pred: impl Fn(&str) -> bool) -> Vec<(&'a str, &'a str)> {
        self.snap
            .files
            .iter()
            .filter(|(p, _)| pred(p))
            .map(|(p, c)| (p.as_str(), c.as_str()))
            .collect()
    }

    /// Every file path in the tree (downloaded or not).
    pub fn tree_paths(&self) -> impl Iterator<Item = &'a str> {
        let from_tree = self
            .snap
            .tree
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.path.as_str());
        let from_files = self.snap.files.keys().map(String::as_str);
        let mut seen = BTreeSet::new();
        from_tree
            .chain(from_files)
            .filter(move |p| seen.insert(*p))
            .collect::<Vec<_>>()
            .into_iter()
    }

    pub fn tree_has(&self, pred: impl Fn(&str) -> bool) -> bool {
        self.tree_paths().any(pred)
    }

    /// File names (not paths) below `dir`; used to answer "is there a main.rs here?".
    pub fn names_under(&self, dir: &str) -> Vec<&'a str> {
        self.tree_paths()
            .filter(|p| dir.is_empty() || p.starts_with(&format!("{dir}/")))
            .map(basename)
            .collect()
    }

    // -- apps -------------------------------------------------------------

    /// The app whose directory is the longest prefix of `path`.
    pub fn app_for_path(&self, path: &str) -> Option<&App> {
        let dir = dirname(path);
        self.apps
            .iter()
            .filter(|a| a.dir.is_empty() || dir == a.dir || dir.starts_with(&format!("{}/", a.dir)))
            .max_by_key(|a| a.dir.len())
    }

    pub fn app_key_for_path(&self, path: &str) -> Option<String> {
        self.app_for_path(path).map(|a| a.key.clone())
    }

    pub fn app_for_manifest(&self, manifest_path: &str) -> Option<&App> {
        self.apps.iter().find(|a| a.manifest_path == manifest_path)
    }

    /// Services that answer requests: backend first, then anything that is not a library.
    pub fn primary_service_key(&self) -> Option<String> {
        self.apps
            .iter()
            .find(|a| !a.backend.is_empty())
            .or_else(|| self.apps.iter().find(|a| !a.library))
            .or_else(|| self.apps.first())
            .map(|a| a.key.clone())
    }

    pub fn service_keys(&self) -> Vec<String> {
        self.apps
            .iter()
            .filter(|a| !a.library)
            .map(|a| a.key.clone())
            .collect()
    }
}

/// Runs every detector in dependency order. The configuration goes first so
/// its facts exist for everything else; runtime reads base images, so it
/// follows the container and IaC passes that collect them.
pub(crate) fn run_all(ctx: &mut Ctx<'_>) {
    crate::config::detect(ctx);
    dependencies::detect(ctx);
    languages::detect(ctx);
    frameworks::detect(ctx);
    legacy::detect(ctx);
    containers::detect(ctx);
    iac::detect(ctx);
    ci::detect(ctx);
    runtime::detect(ctx);
    datastores::detect(ctx);
    cloud::detect(ctx);
    testing::detect(ctx);
    migrations::detect(ctx);
    docs::detect(ctx);
    security::detect(ctx);
}
