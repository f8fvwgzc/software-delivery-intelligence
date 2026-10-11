//! Architecture: the product's services, libraries, data stores and
//! external APIs with the calls between them. Groups are the top-level
//! directories (or workspace members) that hold the packages, plus "Data"
//! and "External".

use std::collections::HashSet;

use sdi_proto::knowledge::v1::NodeKind;

use super::{
    DiagramInput, add_evidence_edges, add_graph_edges,
    catalog::{Catalog, humanize_dir, node_id},
    model::{DiagramEdge, DiagramGroup, DiagramSpec, kind, style},
};

pub const GROUP_DATA: &str = "data";
pub const GROUP_EXTERNAL: &str = "external";

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let catalog = Catalog::new(input);
    let mut spec = DiagramSpec::new("LR");
    add_packages(&catalog, &mut spec);
    add_datastores(&catalog, &mut spec);
    add_externals(&catalog, &mut spec);
    // Sampled files first: a call seen in code beats the manifest the graph cites.
    add_evidence_edges(&catalog, &mut spec);
    add_graph_edges(&catalog, &mut spec);
    add_inferred_ui_calls(&mut spec);
    spec.prune_groups();
    spec
}

/// The architecture map narrowed to what the prompt names: nodes whose
/// label, path or description mention a prompt word, plus their direct
/// neighbours. An empty or unmatched prompt yields the full map.
pub fn build_focused(input: &DiagramInput<'_>, prompt: &str) -> DiagramSpec {
    let mut spec = build(input);
    let words: Vec<String> = prompt
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(|w| w.to_lowercase())
        .filter(|w| {
            !matches!(
                w.as_str(),
                "the" | "and" | "for" | "with" | "show" | "flow" | "diagram" | "map"
            )
        })
        .collect();
    if words.is_empty() {
        return spec;
    }
    let matched: HashSet<String> = spec
        .nodes
        .iter()
        .filter(|n| {
            let hay =
                format!("{} {} {} {}", n.label, n.path, n.description, n.caption).to_lowercase();
            words.iter().any(|w| hay.contains(w))
        })
        .map(|n| n.id.clone())
        .collect();
    if matched.is_empty() {
        return spec;
    }
    let keep: HashSet<String> = spec
        .edges
        .iter()
        .filter(|e| matched.contains(&e.source) || matched.contains(&e.target))
        .flat_map(|e| [e.source.clone(), e.target.clone()])
        .chain(matched.iter().cloned())
        .collect();
    spec.nodes.retain(|n| keep.contains(&n.id));
    spec.edges
        .retain(|e| keep.contains(&e.source) && keep.contains(&e.target));
    spec.prune_groups();
    spec
}

/// Product packages grouped by their top-level directory.
pub fn add_packages(catalog: &Catalog<'_>, spec: &mut DiagramSpec) {
    for (dir, nodes) in catalog.packages_by_top_dir() {
        let group_id = if dir.is_empty() {
            "root".to_owned()
        } else {
            format!("dir_{}", node_id(&dir))
        };
        spec.add_group(DiagramGroup {
            id: group_id.clone(),
            label: humanize_dir(&dir),
            description: if dir.is_empty() {
                "Packages at the repository root".to_owned()
            } else {
                format!("Packages under {dir}/")
            },
            path: dir.clone(),
        });
        for node in nodes {
            if let Some(diagram_node) = catalog.diagram_node(node, &group_id) {
                spec.add_node(diagram_node);
            }
        }
    }
}

pub fn add_datastores(catalog: &Catalog<'_>, spec: &mut DiagramSpec) {
    let stores: Vec<_> = catalog.of_kind(NodeKind::Datastore).collect();
    if stores.is_empty() {
        return;
    }
    spec.add_group(DiagramGroup {
        id: GROUP_DATA.to_owned(),
        label: "Data".to_owned(),
        description: "Databases, caches and queues the services use".to_owned(),
        path: String::new(),
    });
    for node in stores {
        if let Some(diagram_node) = catalog.diagram_node(node, GROUP_DATA) {
            spec.add_node(diagram_node);
        }
    }
}

pub fn add_externals(catalog: &Catalog<'_>, spec: &mut DiagramSpec) {
    let externals: Vec<_> = catalog.of_kind(NodeKind::ExternalApi).collect();
    if externals.is_empty() {
        return;
    }
    spec.add_group(DiagramGroup {
        id: GROUP_EXTERNAL.to_owned(),
        label: "External".to_owned(),
        description: "Third-party APIs the code talks to".to_owned(),
        path: String::new(),
    });
    for node in externals {
        if let Some(diagram_node) = catalog.diagram_node(node, GROUP_EXTERNAL) {
            spec.add_node(diagram_node);
        }
    }
}

/// A UI with no observed call to a backend almost certainly talks to the
/// only one there is; drawn dashed because nothing in the sample shows it.
pub fn add_inferred_ui_calls(spec: &mut DiagramSpec) {
    let backends: Vec<String> = spec
        .nodes
        .iter()
        .filter(|n| n.kind == kind::SERVICE)
        .map(|n| n.id.clone())
        .collect();
    let [only] = backends.as_slice() else {
        return;
    };
    let uis: Vec<String> = spec
        .nodes
        .iter()
        .filter(|n| n.kind == kind::UI)
        .map(|n| n.id.clone())
        .collect();
    for ui in uis {
        let has_call = spec
            .edges
            .iter()
            .any(|e| e.source == ui && e.style == style::CALL);
        if !has_call {
            spec.add_edge(DiagramEdge {
                id: String::new(),
                source: ui,
                target: only.clone(),
                label: "calls".to_owned(),
                style: style::CALL.to_owned(),
                dashed: true,
                evidence_path: String::new(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::diagrams::{
        fixtures::{self, input, tree},
        model::MAX_GROUPS,
    };

    #[test]
    fn groups_come_from_top_level_dirs_plus_data_and_external() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        let labels: Vec<&str> = spec.groups.iter().map(|g| g.label.as_str()).collect();
        assert!(labels.contains(&"Services"), "{labels:?}");
        assert!(labels.contains(&"Web"), "{labels:?}");
        assert!(labels.contains(&"Crates"), "{labels:?}");
        assert!(labels.contains(&"Data"), "{labels:?}");
        assert!(labels.contains(&"External"), "{labels:?}");
        assert!((3..=MAX_GROUPS).contains(&spec.groups.len()), "{labels:?}");
        // Groups carry the directory they stand for.
        assert_eq!(spec.group("dir_services").unwrap().path, "services");

        // The e2e package under tests/ is machinery, not product.
        assert!(!spec.has_node("library_e2e"));
        assert!(!spec.nodes.iter().any(|n| n.kind == kind::JOB));

        // Kinds and captions from facts.
        let web = spec.node("service_web").unwrap();
        assert_eq!(web.kind, kind::UI);
        assert_eq!(web.caption, "React");
        assert_eq!(web.path, "web/src/main.tsx");
        let api = spec.node("service_api").unwrap();
        assert_eq!(api.kind, kind::SERVICE);
        assert_eq!(api.caption, "axum");
        assert_eq!(api.path, "services/api/src/main.rs");
        let quant = spec.node("service_quant").unwrap();
        assert_eq!(quant.caption, "FastAPI");
        assert_eq!(quant.path, "services/quant/app/main.py");
        let pg = spec.node("datastore_postgres").unwrap();
        assert_eq!(pg.kind, kind::DATASTORE);
        assert_eq!(
            pg.caption, "postgres:16",
            "the image, since the label is the engine"
        );
        assert_eq!(spec.node("datastore_redis").unwrap().caption, "");
        assert_eq!(spec.node("datastore_kafka").unwrap().kind, kind::QUEUE);
        assert_eq!(
            spec.node("external_api_stripe").unwrap().kind,
            kind::EXTERNAL
        );
        assert_eq!(spec.node("library_shared").unwrap().kind, kind::MODULE);
        assert_eq!(
            spec.node("library_shared").unwrap().path,
            "crates/shared/src/lib.rs"
        );
    }

    #[test]
    fn edges_cite_the_callers_file_and_inferred_ones_are_dashed() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        let find = |s: &str, t: &str| spec.edges.iter().find(|e| e.source == s && e.target == t);

        // Graph edge api -> postgres upgraded by the sqlx evidence in main.rs.
        let api_pg = find("service_api", "datastore_postgres").expect("api -> postgres");
        assert!(!api_pg.dashed);
        assert_eq!(api_pg.style, style::DATA);
        assert!(
            api_pg.evidence_path.starts_with("services/api/"),
            "{}",
            api_pg.evidence_path
        );

        // Regex evidence: the API calls quant by its base URL and imports shared.
        let api_quant = find("service_api", "service_quant").expect("api -> quant");
        assert_eq!(api_quant.evidence_path, "services/api/src/main.rs");
        assert_eq!(api_quant.label, "calls");
        let api_shared = find("service_api", "library_shared").expect("api -> shared");
        assert_eq!(api_shared.label, "imports");
        assert_eq!(api_shared.evidence_path, "services/api/src/main.rs");

        // The UI's fetch('/api') names the only backend... there are two, so
        // it stays a graph-free inference: no edge at all from the sample.
        // Redis via the cache module.
        let api_redis = find("service_api", "datastore_redis").expect("api -> redis");
        assert!(!api_redis.dashed);
        // Kafka: graph edge, manifest as evidence.
        let api_kafka = find("service_api", "datastore_kafka").expect("api -> kafka");
        assert_eq!(api_kafka.label, "publishes");
        assert_eq!(api_kafka.style, style::ASYNC);
        // quant -> postgres from asyncpg in main.py.
        let quant_pg = find("service_quant", "datastore_postgres").expect("quant -> postgres");
        assert_eq!(quant_pg.evidence_path, "services/quant/app/main.py");
        // Stripe from the graph: manifest cited.
        let stripe = find("service_api", "external_api_stripe").expect("api -> stripe");
        assert_eq!(stripe.evidence_path, "services/api/Cargo.toml");
        // The expired web -> kafka edge never appears.
        assert!(find("service_web", "datastore_kafka").is_none());
        // Structural CONTAINS edges never appear.
        assert!(spec.edges.iter().all(|e| e.label != "contains"));
    }

    #[test]
    fn without_files_graph_edges_cite_manifests_and_a_lone_ui_call_is_dashed() {
        let files = BTreeMap::new();
        let tree = tree();
        let mut ctx = fixtures::context();
        // Drop quant so the web app has exactly one backend to talk to.
        if let Some(graph) = ctx.graph.as_mut() {
            graph.nodes.retain(|n| n.id != "n-quant");
            graph.edges.retain(|e| e.source_id != "n-quant");
        }
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        let web_api = spec
            .edges
            .iter()
            .find(|e| e.source == "service_web" && e.target == "service_api")
            .expect("web -> api inferred");
        assert!(web_api.dashed);
        assert!(web_api.evidence_path.is_empty());
        let api_pg = spec
            .edges
            .iter()
            .find(|e| e.source == "service_api" && e.target == "datastore_postgres")
            .unwrap();
        assert_eq!(api_pg.evidence_path, "services/api/Cargo.toml");
        assert!(!api_pg.dashed);
    }

    #[test]
    fn focused_build_keeps_the_named_nodes_and_their_neighbours() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build_focused(
            &input(&ctx, &tree, &files, "the quant pricing"),
            "the quant pricing",
        );
        spec.validate().unwrap();
        assert!(spec.has_node("service_quant"));
        assert!(spec.has_node("datastore_postgres"));
        assert!(spec.has_node("service_api"));
        assert!(!spec.has_node("datastore_redis"));
        let full = build_focused(&input(&ctx, &tree, &files, ""), "");
        assert!(full.nodes.len() > spec.nodes.len());
    }
}
