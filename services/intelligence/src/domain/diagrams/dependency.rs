//! Dependency: the project's own packages per ecosystem and the runtime
//! third-party packages they pull in, ranked by fan-in across manifests.

use std::collections::BTreeMap;

use super::{
    DiagramInput,
    catalog::{Catalog, concise_label, ecosystem_display, node_id},
    model::{DiagramEdge, DiagramGroup, DiagramNode, DiagramSpec, MAX_NODES, kind, style},
};

/// Third-party packages shown per ecosystem (the most depended-on first).
const MAX_EXTERNAL_PER_ECOSYSTEM: usize = 8;

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let catalog = Catalog::new(input);
    let mut spec = DiagramSpec::new("LR");

    // Manifests that belong to product packages; the rest is tooling.
    let packages: Vec<_> = catalog
        .nodes()
        .filter(|n| catalog.is_product_node(n))
        .collect();
    let package_names: BTreeMap<String, String> = packages
        .iter()
        .map(|p| (normalise(&p.label), node_id(&p.key)))
        .collect();

    let mut by_ecosystem: BTreeMap<String, Vec<&sdi_proto::knowledge::v1::Dependency>> =
        BTreeMap::new();
    for dependency in &catalog.context().dependencies {
        if dependency.scope != "runtime" && !dependency.scope.is_empty() {
            continue;
        }
        if catalog
            .owner_of_path(&dependency.manifest_path)
            .is_none_or(|o| !catalog.is_product_node(o))
        {
            continue;
        }
        by_ecosystem
            .entry(dependency.ecosystem.clone())
            .or_default()
            .push(dependency);
    }

    for (ecosystem, dependencies) in &by_ecosystem {
        let group_id = format!("eco_{}", node_id(ecosystem));
        spec.add_group(DiagramGroup {
            id: group_id.clone(),
            label: format!("{} packages", ecosystem_display(ecosystem)),
            description: format!("{ecosystem} manifests and their runtime dependencies"),
            path: String::new(),
        });
        // Own packages of this ecosystem.
        for package in packages.iter().filter(|p| {
            p.attributes
                .get("ecosystem")
                .is_some_and(|e| e == ecosystem)
        }) {
            if let Some(mut diagram_node) = catalog.diagram_node(package, &group_id) {
                diagram_node.kind = kind::MODULE.to_owned();
                let manifest = package.source_path.clone();
                if !manifest.is_empty() && (!catalog.has_tree() || catalog.file_exists(&manifest)) {
                    diagram_node.path = manifest;
                }
                spec.add_node(diagram_node);
            }
        }
        // Fan-in: how many manifests depend on each name.
        let mut fan_in: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for dependency in dependencies {
            let manifests = fan_in.entry(dependency.name.as_str()).or_default();
            if !manifests.contains(&dependency.manifest_path.as_str()) {
                manifests.push(&dependency.manifest_path);
            }
        }
        let mut ranked: Vec<(&str, &Vec<&str>)> = fan_in.iter().map(|(n, m)| (*n, m)).collect();
        ranked.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
        let mut shown = 0;
        for (name, manifests) in ranked {
            let internal = package_names.get(&normalise(name)).cloned();
            let target_id = match &internal {
                Some(id) => id.clone(),
                None => {
                    if shown >= MAX_EXTERNAL_PER_ECOSYSTEM || spec.nodes.len() >= MAX_NODES {
                        continue;
                    }
                    shown += 1;
                    let id = format!("dep_{}_{}", node_id(ecosystem), node_id(name));
                    spec.add_node(DiagramNode {
                        id: id.clone(),
                        label: concise_label(name),
                        kind: kind::EXTERNAL.to_owned(),
                        group_id: group_id.clone(),
                        path: String::new(),
                        description: format!(
                            "{name} ({ecosystem}), used by {} manifest(s)",
                            manifests.len()
                        ),
                        caption: if manifests.len() > 1 {
                            format!("used by {}", manifests.len())
                        } else {
                            ecosystem.clone()
                        },
                    });
                    id
                }
            };
            for manifest in manifests {
                let Some(owner) = catalog.owner_of_path(manifest) else {
                    continue;
                };
                let evidence = if !catalog.has_tree() || catalog.file_exists(manifest) {
                    (*manifest).to_owned()
                } else {
                    String::new()
                };
                spec.add_edge(DiagramEdge {
                    id: String::new(),
                    source: node_id(&owner.key),
                    target: target_id.clone(),
                    label: if internal.is_some() {
                        "imports"
                    } else {
                        "depends on"
                    }
                    .to_owned(),
                    style: style::CALL.to_owned(),
                    dashed: evidence.is_empty(),
                    evidence_path: evidence,
                });
            }
        }
    }
    spec.prune_groups();
    spec
}

fn normalise(name: &str) -> String {
    name.rsplit('/')
        .next()
        .unwrap_or(name)
        .to_lowercase()
        .replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagrams::fixtures::{self, input, tree};

    #[test]
    fn packages_per_ecosystem_with_fan_in_and_workspace_imports() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        let groups: Vec<&str> = spec.groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(
            groups,
            vec!["Rust packages", "Node.js packages", "Python packages"]
        );
        // Dev dependencies and the e2e tooling are out.
        assert!(
            !spec
                .nodes
                .iter()
                .any(|n| n.label == "vite" || n.label == "pytest")
        );
        assert!(!spec.has_node("library_e2e"));
        // Own packages link to their manifests.
        assert_eq!(
            spec.node("service_api").unwrap().path,
            "services/api/Cargo.toml"
        );
        assert_eq!(spec.node("service_api").unwrap().kind, kind::MODULE);
        // shared is a workspace crate: api imports it rather than depending on a copy.
        let api_shared = spec
            .edges
            .iter()
            .find(|e| e.source == "service_api" && e.target == "library_shared")
            .expect("api imports shared");
        assert_eq!(api_shared.label, "imports");
        assert_eq!(api_shared.evidence_path, "services/api/Cargo.toml");
        // serde is used by two manifests: fan-in caption.
        let serde = spec.node("dep_cargo_serde").unwrap();
        assert_eq!(serde.caption, "used by 2");
        assert_eq!(
            spec.edges
                .iter()
                .filter(|e| e.target == "dep_cargo_serde")
                .count(),
            2
        );
        assert!(spec.edges.iter().all(|e| !e.dashed));
    }
}
