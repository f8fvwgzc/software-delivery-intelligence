//! High level: actors → product → external systems, at most nine nodes.

use sdi_proto::knowledge::v1::NodeKind;

use super::{
    DiagramInput, add_evidence_edges, add_graph_edges,
    catalog::{Catalog, provider_display},
    model::{DiagramEdge, DiagramGroup, DiagramNode, DiagramSpec, kind, style},
};

pub const MAX_HIGH_LEVEL_NODES: usize = 9;

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let catalog = Catalog::new(input);
    let mut spec = DiagramSpec::new("LR");
    spec.add_group(DiagramGroup {
        id: "actors".to_owned(),
        label: "Actors".to_owned(),
        description: "Who or what drives the product".to_owned(),
        path: String::new(),
    });
    spec.add_group(DiagramGroup {
        id: "product".to_owned(),
        label: "Product".to_owned(),
        description: format!("The {} services and interfaces", catalog.project_name()),
        path: String::new(),
    });

    // Product: interfaces first, then backends, then libraries never (too fine).
    let mut packages: Vec<_> = catalog
        .nodes()
        .filter(|n| n.kind() == NodeKind::Service && catalog.is_product_node(n))
        .collect();
    packages.sort_by_key(|n| (!catalog.is_ui(n), n.label.clone()));
    let budget_product = MAX_HIGH_LEVEL_NODES - 1 - 2 - 1; // users, two stores, one external
    for node in packages.iter().take(budget_product.max(2)) {
        if let Some(diagram_node) = catalog.diagram_node(node, "product") {
            spec.add_node(diagram_node);
        }
    }

    spec.add_node(DiagramNode {
        id: "actor_users".to_owned(),
        label: "Users".to_owned(),
        kind: kind::ACTOR.to_owned(),
        group_id: "actors".to_owned(),
        path: String::new(),
        description: "People using the product through its interfaces".to_owned(),
        caption: "browser".to_owned(),
    });

    // Data stores: the two most depended-on.
    let mut stores: Vec<_> = catalog
        .of_kind(NodeKind::Datastore)
        .map(|n| {
            let fan_in = catalog.edges().filter(|e| e.target_id == n.id).count();
            (n, fan_in)
        })
        .collect();
    stores.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.label.cmp(&b.0.label)));
    if !stores.is_empty() {
        spec.add_group(DiagramGroup {
            id: "data".to_owned(),
            label: "Data".to_owned(),
            description: "Where the product keeps its state".to_owned(),
            path: String::new(),
        });
        for (node, _) in stores.iter().take(2) {
            if let Some(diagram_node) = catalog.diagram_node(node, "data") {
                spec.add_node(diagram_node);
            }
        }
    }

    // External systems: APIs the code uses, plus CI when the scan found it.
    let externals: Vec<_> = catalog.of_kind(NodeKind::ExternalApi).collect();
    let pipeline = catalog.of_kind(NodeKind::Pipeline).next();
    if !externals.is_empty() || pipeline.is_some() {
        spec.add_group(DiagramGroup {
            id: "external".to_owned(),
            label: "External systems".to_owned(),
            description: "Third parties and automation around the product".to_owned(),
            path: String::new(),
        });
    }
    let room =
        MAX_HIGH_LEVEL_NODES.saturating_sub(spec.nodes.len() + usize::from(pipeline.is_some()));
    for node in externals.iter().take(room.max(1)) {
        if let Some(diagram_node) = catalog.diagram_node(node, "external") {
            spec.add_node(diagram_node);
        }
    }
    if let Some(pipeline) = pipeline {
        let provider = pipeline
            .attributes
            .get("provider")
            .map(|p| provider_display(p))
            .unwrap_or_else(|| "CI".to_owned());
        spec.add_node(DiagramNode {
            id: "job_ci".to_owned(),
            label: "CI pipeline".to_owned(),
            kind: kind::JOB.to_owned(),
            group_id: "external".to_owned(),
            path: catalog.path_of(pipeline),
            description: pipeline.description.clone(),
            caption: provider,
        });
    }

    add_evidence_edges(&catalog, &mut spec);
    add_graph_edges(&catalog, &mut spec);

    // Users reach the interfaces (or the only service): inferred, so dashed.
    let entry: Vec<String> = {
        let uis: Vec<String> = spec
            .nodes
            .iter()
            .filter(|n| n.kind == kind::UI)
            .map(|n| n.id.clone())
            .collect();
        if uis.is_empty() {
            spec.nodes
                .iter()
                .filter(|n| n.kind == kind::SERVICE)
                .take(1)
                .map(|n| n.id.clone())
                .collect()
        } else {
            uis
        }
    };
    for target in entry {
        spec.add_edge(DiagramEdge {
            id: String::new(),
            source: "actor_users".to_owned(),
            target,
            label: "uses".to_owned(),
            style: style::CALL.to_owned(),
            dashed: true,
            evidence_path: String::new(),
        });
    }
    // CI builds and tests the product: the workflow file is the evidence.
    if let Some(pipeline) = pipeline {
        let workflow = catalog.path_of(pipeline);
        let targets: Vec<String> = spec
            .nodes
            .iter()
            .filter(|n| n.kind == kind::SERVICE || n.kind == kind::UI)
            .map(|n| n.id.clone())
            .collect();
        for target in targets {
            spec.add_edge(DiagramEdge {
                id: String::new(),
                source: "job_ci".to_owned(),
                target,
                label: "builds & tests".to_owned(),
                style: style::CALL.to_owned(),
                dashed: workflow.is_empty(),
                evidence_path: workflow.clone(),
            });
        }
    }
    super::architecture::add_inferred_ui_calls(&mut spec);
    truncate(&mut spec);
    spec.prune_groups();
    spec
}

/// Keeps the nine most important nodes: actors and product first, then
/// data, then external.
fn truncate(spec: &mut DiagramSpec) {
    if spec.nodes.len() <= MAX_HIGH_LEVEL_NODES {
        return;
    }
    let rank = |n: &DiagramNode| match n.kind.as_str() {
        kind::ACTOR => 0,
        kind::UI => 1,
        kind::SERVICE => 2,
        kind::DATASTORE | kind::QUEUE => 3,
        kind::JOB => 4,
        _ => 5,
    };
    let mut ordered: Vec<(usize, usize)> = spec
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (rank(n), i))
        .collect();
    ordered.sort();
    let keep: std::collections::HashSet<usize> = ordered
        .iter()
        .take(MAX_HIGH_LEVEL_NODES)
        .map(|(_, i)| *i)
        .collect();
    let mut index = 0;
    spec.nodes.retain(|_| {
        let kept = keep.contains(&index);
        index += 1;
        kept
    });
    let ids: std::collections::HashSet<&str> = spec.nodes.iter().map(|n| n.id.as_str()).collect();
    spec.edges
        .retain(|e| ids.contains(e.source.as_str()) && ids.contains(e.target.as_str()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagrams::fixtures::{self, input, tree};

    #[test]
    fn one_screen_with_actors_product_data_and_external() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        assert!(spec.nodes.len() <= MAX_HIGH_LEVEL_NODES);
        assert_eq!(spec.groups.len(), 4);
        assert!(spec.has_node("actor_users"));
        assert!(spec.has_node("service_web"));
        assert!(spec.has_node("service_api"));
        assert!(spec.has_node("datastore_postgres"));
        assert!(spec.has_node("job_ci"));
        assert!(!spec.has_node("library_shared"));
        let users_web = spec
            .edges
            .iter()
            .find(|e| e.source == "actor_users" && e.target == "service_web")
            .unwrap();
        assert!(users_web.dashed);
        let ci = spec
            .edges
            .iter()
            .find(|e| e.source == "job_ci" && e.target == "service_api")
            .unwrap();
        assert_eq!(ci.evidence_path, ".github/workflows/ci.yml");
        assert!(!ci.dashed);
        assert_eq!(spec.node("job_ci").unwrap().caption, "GitHub Actions");
    }
}
