//! Deployment: pipelines, container images, environments and provisioned
//! infrastructure, joined by the scan's DEPLOYED_TO / PROVISIONS / RELEASES
//! / BUILDS / RUNS_ON edges.

use sdi_proto::knowledge::v1::NodeKind;

use super::{
    DiagramInput, add_graph_edges,
    catalog::Catalog,
    model::{DiagramGroup, DiagramSpec},
};

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let catalog = Catalog::new(input);
    let mut spec = DiagramSpec::new("LR");
    let groups = [
        (
            "pipelines",
            "Pipelines",
            "CI/CD workflows and their deploy jobs",
        ),
        (
            "images",
            "Images",
            "Container images and the packages built into them",
        ),
        ("environments", "Environments", "Where releases run"),
        (
            "infrastructure",
            "Infrastructure",
            "Resources provisioned by IaC, and managed data stores",
        ),
    ];
    for (id, label, description) in groups {
        spec.add_group(DiagramGroup {
            id: id.to_owned(),
            label: label.to_owned(),
            description: description.to_owned(),
            path: String::new(),
        });
    }
    for node in catalog.nodes() {
        let group = match node.kind() {
            NodeKind::Pipeline | NodeKind::Deployment => "pipelines",
            NodeKind::ContainerImage => "images",
            NodeKind::Service
                if catalog.is_product_node(node) && is_packaged(&catalog, &node.id) =>
            {
                "images"
            }
            NodeKind::Environment => "environments",
            NodeKind::Infrastructure | NodeKind::IacModule => "infrastructure",
            NodeKind::Datastore
                if node
                    .attributes
                    .get("defined_by")
                    .is_some_and(|d| d != "dependency") =>
            {
                "infrastructure"
            }
            _ => continue,
        };
        if let Some(diagram_node) = catalog.diagram_node(node, group) {
            spec.add_node(diagram_node);
        }
    }
    add_graph_edges(&catalog, &mut spec);
    // A runtime map of "what talks to what" is the architecture's job.
    spec.edges.retain(|e| {
        !matches!(
            e.label.as_str(),
            "calls" | "reads" | "writes" | "reads/writes" | "publishes" | "consumes" | "imports"
        )
    });
    spec.prune_groups();
    spec
}

fn is_packaged(catalog: &Catalog<'_>, service_id: &str) -> bool {
    catalog
        .edges()
        .any(|e| e.relation == "PACKAGED_AS" && e.source_id == service_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagrams::{
        fixtures::{self, input, tree},
        model::kind,
    };

    #[test]
    fn pipelines_images_environments_and_their_edges() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        assert_eq!(spec.groups.len(), 4);
        assert!(spec.has_node("pipeline_ci_yml"));
        assert!(spec.has_node("container_image_api"));
        assert!(spec.has_node("environment_production"));
        assert!(spec.has_node("datastore_postgres"));
        assert!(!spec.has_node("external_api_stripe"));
        assert!(!spec.has_node("service_quant"), "quant has no image");
        assert_eq!(spec.node("container_image_api").unwrap().kind, kind::INFRA);
        assert_eq!(
            spec.node("container_image_api").unwrap().caption,
            "rust:1.85-slim"
        );
        assert_eq!(spec.node("pipeline_ci_yml").unwrap().kind, kind::JOB);
        let labels: Vec<&str> = spec.edges.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"builds"), "{labels:?}");
        assert!(labels.contains(&"deploys to"), "{labels:?}");
        assert!(labels.contains(&"packaged as"), "{labels:?}");
        assert!(labels.contains(&"runs on"), "{labels:?}");
        assert!(!labels.contains(&"reads/writes"), "{labels:?}");
        let builds = spec.edges.iter().find(|e| e.label == "builds").unwrap();
        assert_eq!(builds.evidence_path, ".github/workflows/ci.yml");
    }
}
