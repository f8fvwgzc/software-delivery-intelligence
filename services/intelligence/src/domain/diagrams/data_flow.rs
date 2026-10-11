//! Data flow: the services that read, write and publish, the stores and
//! queues they touch, and nothing else.

use sdi_proto::knowledge::v1::NodeKind;

use super::{
    DiagramInput, add_evidence_edges, add_graph_edges,
    architecture::add_packages,
    catalog::Catalog,
    model::{DiagramGroup, DiagramSpec, kind, style},
};

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let catalog = Catalog::new(input);
    let mut spec = DiagramSpec::new("LR");
    add_packages(&catalog, &mut spec);

    let stores: Vec<_> = catalog.of_kind(NodeKind::Datastore).collect();
    if stores.iter().any(|n| !Catalog::is_queue(n)) {
        spec.add_group(DiagramGroup {
            id: "stores".to_owned(),
            label: "Stores".to_owned(),
            description: "Databases and caches".to_owned(),
            path: String::new(),
        });
    }
    if stores.iter().any(|n| Catalog::is_queue(n)) {
        spec.add_group(DiagramGroup {
            id: "queues".to_owned(),
            label: "Queues".to_owned(),
            description: "Message brokers and streams".to_owned(),
            path: String::new(),
        });
    }
    for node in stores {
        let group = if Catalog::is_queue(node) {
            "queues"
        } else {
            "stores"
        };
        if let Some(diagram_node) = catalog.diagram_node(node, group) {
            spec.add_node(diagram_node);
        }
    }

    add_evidence_edges(&catalog, &mut spec);
    add_graph_edges(&catalog, &mut spec);

    // Only data edges belong here; then only components that touch data.
    spec.edges
        .retain(|e| e.style == style::DATA || e.style == style::ASYNC);
    let touched: std::collections::HashSet<String> = spec
        .edges
        .iter()
        .flat_map(|e| [e.source.clone(), e.target.clone()])
        .collect();
    spec.nodes
        .retain(|n| touched.contains(&n.id) || n.kind == kind::DATASTORE || n.kind == kind::QUEUE);
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
    }
}
