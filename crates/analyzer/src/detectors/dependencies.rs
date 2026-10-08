//! Parses every manifest, records dependencies and turns each manifest into
//! an app (service or library) that later detectors attach things to.

use crate::detectors::{App, Ctx, attrs};
use crate::files::{is_dockerfile, is_lock_file, is_manifest_name};
use crate::manifests::{self, Manifest};
use crate::model::*;
use crate::util::{basename, depth, dirname, display, line_with};

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    let manifest_files = ctx.files_where(|p| is_manifest_name(basename(p)));
    for (path, content) in manifest_files {
        let names = ctx.names_under(dirname(path));
        let Some(m) = manifests::parse(path, content, &names) else {
            continue;
        };
        record_manifest(ctx, &m);
        ctx.manifests.push(m);
    }
    // Secondary manifests (requirements-dev.txt, workspace-only Cargo.toml)
    // never make an app of their own; one app per directory is enough.
    let mut dirs_seen = Vec::new();
    for m in ctx.manifests.clone() {
        let dir = dirname(&m.path).to_string();
        if dirs_seen.contains(&dir) || is_secondary(&m) {
            continue;
        }
        dirs_seen.push(dir.clone());
        add_app(ctx, &m, &dir);
    }

    let has_lockfile = ctx.tree_has(is_lock_file);
    if has_lockfile {
        let lock = ctx
            .tree_paths()
            .find(|p| is_lock_file(p))
            .unwrap_or("")
            .to_string();
        ctx.detected(
            FactCategory::Dependency,
            "security:lockfile",
            "Lock file",
            basename(&lock),
            &lock,
            "lock file committed",
        );
    }
    let monorepo = ctx.manifests.iter().any(|m| m.workspace)
        || dirs_seen.iter().filter(|d| !d.is_empty()).count() > 1
        || ctx.tree_has(|p| p == "pnpm-workspace.yaml");
    if monorepo {
        let ev = ctx
            .manifests
            .iter()
            .find(|m| m.workspace)
            .map(|m| m.path.clone())
            .unwrap_or_else(|| "several manifests in subdirectories".into());
        ctx.detected(
            FactCategory::Repository,
            "repository:monorepo",
            "Monorepo",
            "multiple packages",
            "",
            &ev,
        );
    }
}

fn is_secondary(m: &Manifest) -> bool {
    let name = basename(&m.path);
    (name.starts_with("requirements") && name != "requirements.txt")
        || (m.ecosystem == "cargo" && m.workspace && m.name.is_empty())
        || name == "settings.gradle"
        || name == "settings.gradle.kts"
}

fn record_manifest(ctx: &mut Ctx<'_>, m: &Manifest) {
    for d in &m.deps {
        ctx.out.dependencies.push(DependencyDraft {
            ecosystem: m.ecosystem.clone(),
            name: d.name.clone(),
            version: d.version.clone(),
            manifest_path: m.path.clone(),
            scope: d.scope.clone(),
        });
    }
    let key = format!("package:{}", m.package_manager);
    if !ctx.has_fact(&key) {
        let evidence = line_with(ctx.file(&m.path).unwrap_or(""), "name")
            .unwrap_or(basename(&m.path))
            .to_string();
        ctx.detected(
            FactCategory::Dependency,
            &key,
            &display(&m.package_manager),
            &format!("{} dependencies", m.deps.len()),
            &m.path,
            &evidence,
        );
    }
    if depth(&m.path) <= 1 {
        let title = format!("{} ({})", basename(&m.path), m.ecosystem);
        ctx.chunk(
            ChunkKind::Code,
            &m.path,
            &title,
            ctx.file(&m.path).unwrap_or(""),
        );
    }
}

fn add_app(ctx: &mut Ctx<'_>, m: &Manifest, dir: &str) {
    let name = if !m.name.is_empty() {
        m.name.rsplit('/').next().unwrap_or(&m.name).to_string()
    } else if dir.is_empty() {
        ctx.repo_name().to_string()
    } else {
        basename(dir).to_string()
    };
    let has_dockerfile = ctx.tree_has(|p| dirname(p) == dir && is_dockerfile(basename(p)));
    let library = m.library && !has_dockerfile && !m.scripts.contains_key("start");
    let kind = if library {
        NodeKind::Library
    } else {
        NodeKind::Service
    };
    let key = format!(
        "{}:{}",
        if library { "library" } else { "service" },
        name.to_ascii_lowercase()
    );
    if ctx.has_node(&key) {
        return;
    }
    let repo_key = ctx.repo_key.clone();
    ctx.node(
        kind,
        &key,
        &name,
        &format!("{} package at {}", display(&m.ecosystem), m.path),
        &m.path,
        attrs(&[
            ("ecosystem", &m.ecosystem),
            ("package_manager", &m.package_manager),
            ("directory", if dir.is_empty() { "." } else { dir }),
        ]),
    );
    ctx.edge(&repo_key, &key, "CONTAINS", RelationCategory::Structure);
    ctx.apps.push(App {
        key,
        name,
        dir: dir.to_string(),
        manifest_path: m.path.clone(),
        frontend: Vec::new(),
        backend: Vec::new(),
        library,
    });
}
