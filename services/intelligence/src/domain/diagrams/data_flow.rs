//! Data flow: the components that read, write and publish, the stores and
//! queues they touch, and nothing else. Built from the architecture map
//! (packages, module components, import-graph store edges) by keeping only
//! the data and async edges and the nodes on them.

use std::collections::HashSet;

use super::{
    DiagramInput, architecture,
    model::{DiagramGroup, DiagramSpec, kind, style},
};

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let mut spec = architecture::build(input);
    // Only data edges belong here; then only components that touch data.
    spec.edges
        .retain(|e| e.style == style::DATA || e.style == style::ASYNC);
    let touched: HashSet<String> = spec
        .edges
        .iter()
        .flat_map(|e| [e.source.clone(), e.target.clone()])
        .collect();
    spec.nodes
        .retain(|n| touched.contains(&n.id) || n.kind == kind::DATASTORE || n.kind == kind::QUEUE);

    // Stores and queues get their own groups whatever the architecture used.
    let has_store = spec.nodes.iter().any(|n| n.kind == kind::DATASTORE);
    let has_queue = spec.nodes.iter().any(|n| n.kind == kind::QUEUE);
    if has_store {
        spec.add_group(DiagramGroup {
            id: "stores".to_owned(),
            label: "Stores".to_owned(),
            description: "Databases and caches".to_owned(),
            path: String::new(),
        });
    }
    if has_queue {
        spec.add_group(DiagramGroup {
            id: "queues".to_owned(),
            label: "Queues".to_owned(),
            description: "Message brokers and streams".to_owned(),
            path: String::new(),
        });
    }
    for node in &mut spec.nodes {
        if node.kind == kind::DATASTORE {
            node.group_id = "stores".to_owned();
        } else if node.kind == kind::QUEUE {
            node.group_id = "queues".to_owned();
        }
    }
    spec.prune_groups();
    spec
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagrams::fixtures::{self, input, tree};

    #[test]
    fn only_data_edges_and_the_components_that_use_them() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        assert!(
            spec.edges
                .iter()
                .all(|e| e.style == style::DATA || e.style == style::ASYNC)
        );
        assert!(spec.has_node("service_api"));
        assert!(spec.has_node("service_quant"));
        assert!(spec.has_node("datastore_postgres"));
        assert!(spec.has_node("datastore_kafka"));
        // The web app and the shared crate touch no data store.
        assert!(!spec.has_node("service_web"));
        assert!(!spec.has_node("library_shared"));
        assert!(spec.groups.iter().any(|g| g.id == "queues"));
        let quant_pg = spec
            .edges
            .iter()
            .find(|e| e.source == "service_quant" && e.target == "datastore_postgres")
            .unwrap();
        assert_eq!(quant_pg.label, "reads");
        let api_pg = spec
            .edges
            .iter()
            .find(|e| e.source == "service_api" && e.target == "datastore_postgres")
            .unwrap();
        assert_eq!(api_pg.label, "reads/writes");
        assert!(!api_pg.evidence_path.is_empty());
        // The import graph adds the store module and the routes that read it.
        assert!(spec.has_node("dir_services_api_src_db"));
        assert!(
            spec.edges
                .iter()
                .any(|e| e.source == "dir_services_api_src_routes"
                    && e.target == "dir_services_api_src_db")
        );
    }

    #[test]
    fn symphony_store_edges_reach_sqlite() {
        let files = fixtures::symphony_files();
        let tree = fixtures::symphony_tree();
        let ctx = fixtures::symphony_context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        assert!(spec.has_node("datastore_sqlite"));
        assert!(
            spec.edges
                .iter()
                .any(|e| e.source.contains("store") && e.target == "datastore_sqlite"),
            "{:?}",
            spec.edges
        );
        assert!(spec.edges.iter().any(|e| e.label == "records"));
        assert!(!spec.has_node("external_linear"));
    }
}
