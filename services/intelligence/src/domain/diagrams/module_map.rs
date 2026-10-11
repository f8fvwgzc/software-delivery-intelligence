//! Module map: directories and packages as a tree read from the repository
//! tree. Groups are the top-level directories; nodes are their immediate
//! subdirectories (and the directory itself); edges are containment.

use super::{
    DiagramInput,
    catalog::{Catalog, humanize_dir, node_id},
    model::{DiagramEdge, DiagramGroup, DiagramNode, DiagramSpec, MAX_NODES, kind, style},
};

/// Top-level directories shown (the ones with the most files win).
const MAX_TOP_DIRS: usize = 6;
/// Subdirectories per top-level directory.
const MAX_CHILDREN: usize = 6;

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let catalog = Catalog::new(input);
    let mut spec = DiagramSpec::new("TB");
    let root_path = [
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
        "README.md",
    ]
    .iter()
    .find(|p| catalog.file_exists(p))
    .map(|p| (*p).to_owned())
    .unwrap_or_default();
    spec.add_node(DiagramNode {
        id: "root".to_owned(),
        label: catalog.project_name(),
        kind: kind::MODULE.to_owned(),
        group_id: String::new(),
        path: root_path,
        description: "Repository root".to_owned(),
        caption: "repository".to_owned(),
    });

    let mut top_dirs: Vec<(String, usize)> = catalog
        .child_dirs("")
        .into_iter()
        .filter(|d| !Catalog::is_excluded_path(d) && !d.starts_with('.'))
        .map(|d| {
            let files = catalog.files_under(&d).count();
            (d, files)
        })
        .collect();
    top_dirs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (dir, files) in top_dirs.iter().take(MAX_TOP_DIRS) {
        let group_id = format!("dir_{}", node_id(dir));
        spec.add_group(DiagramGroup {
            id: group_id.clone(),
            label: humanize_dir(dir),
            description: format!("{dir}/ with {files} files"),
            path: dir.clone(),
        });
        let dir_node = add_dir_node(&catalog, &mut spec, dir, &group_id);
        spec.add_edge(contains("root", &dir_node, &catalog, dir));
        // Package directories get their src/ children instead of just "src".
        let mut children: Vec<String> = catalog
            .child_dirs(dir)
            .into_iter()
            .filter(|d| !Catalog::is_excluded_path(d))
            .collect();
        if let [only] = children.as_slice()
            && (only.ends_with("/src") || only.ends_with("/app") || only.ends_with("/lib"))
        {
            let inner: Vec<String> = catalog
                .child_dirs(only)
                .into_iter()
                .filter(|d| !Catalog::is_excluded_path(d))
                .collect();
            if !inner.is_empty() {
                children = inner;
            }
        }
        let mut sized: Vec<(String, usize)> = children
            .into_iter()
            .map(|c| {
                let n = catalog.files_under(&c).count();
                (c, n)
            })
            .collect();
        sized.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for (child, _) in sized.into_iter().take(MAX_CHILDREN) {
            if spec.nodes.len() >= MAX_NODES {
                break;
            }
            let child_node = add_dir_node(&catalog, &mut spec, &child, &group_id);
            spec.add_edge(contains(&dir_node, &child_node, &catalog, &child));
        }
    }
    spec.prune_groups();
    spec
}

fn add_dir_node(
    catalog: &Catalog<'_>,
    spec: &mut DiagramSpec,
    dir: &str,
    group_id: &str,
) -> String {
    let id = format!("mod_{}", node_id(dir));
    let is_package = catalog
        .nodes()
        .any(|n| Catalog::is_repository_node(n) && catalog.dir_of(n) == dir);
    let files = catalog.files_under(dir).count();
    spec.add_node(DiagramNode {
        id: id.clone(),
        label: humanize_dir(dir),
        kind: kind::MODULE.to_owned(),
        group_id: group_id.to_owned(),
        path: representative_file(catalog, dir),
        description: format!("{dir}/ with {files} files"),
        caption: if is_package {
            "package".to_owned()
        } else {
            format!("{files} files")
        },
    });
    id
}

/// The entry point under a directory, else its manifest or index file, else
/// its first file (at any depth, so a directory of packages still links).
fn representative_file(catalog: &Catalog<'_>, dir: &str) -> String {
    if let Some(entry) = catalog.entry_point(dir) {
        return entry;
    }
    let mut files: Vec<&str> = catalog.files_under(dir).collect();
    files.sort_by_key(|f| {
        let rel = &f[dir.len() + 1..];
        let name = rel.rsplit('/').next().unwrap_or(rel);
        (
            rel.matches('/').count(),
            !matches!(
                name,
                "mod.rs"
                    | "index.ts"
                    | "index.js"
                    | "__init__.py"
                    | "package.json"
                    | "Cargo.toml"
                    | "pyproject.toml"
            ),
            name.to_owned(),
        )
    });
    files.first().map(|f| (*f).to_owned()).unwrap_or_default()
}

/// Containment read from the tree: the child's own path is the evidence.
fn contains(parent: &str, child: &str, catalog: &Catalog<'_>, dir: &str) -> DiagramEdge {
    let evidence = representative_file(catalog, dir);
    DiagramEdge {
        id: String::new(),
        source: parent.to_owned(),
        target: child.to_owned(),
        label: "contains".to_owned(),
        style: style::CONTAINS.to_owned(),
        dashed: evidence.is_empty(),
        evidence_path: evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagrams::fixtures::{self, input, tree};

    #[test]
    fn top_level_dirs_become_groups_with_their_subdirectories() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        assert_eq!(spec.direction, "TB");
        let groups: Vec<&str> = spec.groups.iter().map(|g| g.path.as_str()).collect();
        assert!(groups.contains(&"services"), "{groups:?}");
        assert!(groups.contains(&"web"), "{groups:?}");
        assert!(groups.contains(&"crates"), "{groups:?}");
        assert!(!groups.contains(&"docs"), "{groups:?}");
        assert!(!groups.contains(&"tests"), "{groups:?}");
        assert!(!groups.contains(&".github"), "{groups:?}");
        assert_eq!(spec.node("root").unwrap().path, "Cargo.toml");
        assert_eq!(
            spec.node("mod_services_api").unwrap().path,
            "services/api/src/main.rs"
        );
        assert_eq!(spec.node("mod_services_api").unwrap().caption, "package");
        // web/ has only src/, so its children are src/'s directories.
        assert!(spec.has_node("mod_web_src_features"));
        assert!(
            spec.edges
                .iter()
                .all(|e| e.label == "contains" && !e.dashed)
        );
        assert!(
            spec.edges
                .iter()
                .any(|e| e.source == "root" && e.target == "mod_services")
        );
    }
}
