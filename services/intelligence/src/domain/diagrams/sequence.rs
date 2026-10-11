//! Sequence: the principal workflow as a chain of calls — user → interface
//! → entry point → downstream services → data store — and the returns back
//! up the chain. The understanding's `request_flow` orders the hops when
//! the scan wrote one; the graph's call edges are the fallback.

use sdi_proto::knowledge::v1::{GraphNode, NodeKind};

use super::{
    DiagramInput,
    catalog::{Catalog, node_id},
    evidence::{self, routes_in},
    model::{DiagramEdge, DiagramGroup, DiagramNode, DiagramSpec, kind, style},
};

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let catalog = Catalog::new(input);
    let mut spec = DiagramSpec::new("TB");
    for (id, label, description) in [
        ("client", "Client", "Who starts the request"),
        (
            "services",
            "Services",
            "Entry point and the services it calls",
        ),
        ("data", "Data", "Where the request ends"),
    ] {
        spec.add_group(DiagramGroup {
            id: id.to_owned(),
            label: label.to_owned(),
            description: description.to_owned(),
            path: String::new(),
        });
    }
    spec.add_node(DiagramNode {
        id: "actor_user".to_owned(),
        label: "User".to_owned(),
        kind: kind::ACTOR.to_owned(),
        group_id: "client".to_owned(),
        path: String::new(),
        description: "A person using the product".to_owned(),
        caption: "browser".to_owned(),
    });

    // Hops: ui, then backends ordered by the request flow or the call graph.
    let found = evidence::detect(&catalog);
    let mut hops: Vec<&GraphNode> = Vec::new();
    let uis: Vec<&GraphNode> = catalog
        .nodes()
        .filter(|n| n.kind() == NodeKind::Service && catalog.is_product_node(n) && catalog.is_ui(n))
        .collect();
    if let Some(ui) = uis.first() {
        hops.push(ui);
    }
    let backends: Vec<&GraphNode> = catalog
        .nodes()
        .filter(|n| catalog.is_product_node(n) && catalog.is_backend_service(n))
        .collect();
    let flow: Vec<String> = catalog
        .context()
        .understanding
        .as_ref()
        .map(|u| u.request_flow.iter().map(|h| h.to_lowercase()).collect())
        .unwrap_or_default();
    let mut ordered: Vec<&GraphNode> = backends.clone();
    ordered.sort_by_key(|n| {
        let label = n.label.to_lowercase();
        let position = flow
            .iter()
            .position(|hop| hop == &label || hop.contains(&label) || label.contains(hop.as_str()))
            .unwrap_or(usize::MAX);
        // Then the one most called from the UI / least called by others first.
        let fan_in = found
            .iter()
            .filter(|f| f.target == node_id(&n.key) && f.label == "calls")
            .count();
        (position, fan_in, n.label.clone())
    });
    // Walk the calls from the first backend to keep a chain, not a star.
    if let Some(first) = ordered.first() {
        hops.push(first);
        let mut current = *first;
        while hops.len() < 6 {
            let next = ordered.iter().copied().find(|candidate| {
                !hops.iter().any(|h| h.key == candidate.key)
                    && (found.iter().any(|f| {
                        f.source == node_id(&current.key) && f.target == node_id(&candidate.key)
                    }) || catalog.edges().any(|e| {
                        e.source_id == current.id
                            && e.target_id == candidate.id
                            && matches!(e.relation.as_str(), "DEPENDS_ON" | "USES" | "CALLS")
                    }))
            });
            match next {
                Some(node) => {
                    hops.push(node);
                    current = node;
                }
                None => break,
            }
        }
    }
    for node in &hops {
        if let Some(diagram_node) = catalog.diagram_node(node, "services") {
            spec.add_node(diagram_node);
        }
    }
    // The store the last backend touches (most depended-on as a tiebreak).
    let last = hops
        .iter()
        .rev()
        .find(|n| catalog.is_backend_service(n))
        .copied();
    let store = last.and_then(|service| {
        let service_id = node_id(&service.key);
        let mut stores: Vec<(&GraphNode, usize, bool)> = catalog
            .of_kind(NodeKind::Datastore)
            .filter(|d| !Catalog::is_queue(d))
            .map(|d| {
                let id = node_id(&d.key);
                let direct = found
                    .iter()
                    .any(|f| f.source == service_id && f.target == id)
                    || catalog
                        .edges()
                        .any(|e| e.source_id == service.id && e.target_id == d.id);
                let fan_in = catalog.edges().filter(|e| e.target_id == d.id).count();
                (d, fan_in, direct)
            })
            .collect();
        stores.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then(b.1.cmp(&a.1))
                .then(a.0.label.cmp(&b.0.label))
        });
        stores.first().map(|(d, _, direct)| (*d, *direct))
    });
    if let Some((store_node, _)) = store
        && let Some(diagram_node) = catalog.diagram_node(store_node, "data")
    {
        spec.add_node(diagram_node);
    }

    // Forward edges along the chain, cited where the sample or graph shows them.
    let mut chain: Vec<String> = vec!["actor_user".to_owned()];
    chain.extend(hops.iter().map(|n| node_id(&n.key)));
    if let Some((store_node, _)) = store {
        chain.push(node_id(&store_node.key));
    }
    for pair in chain.windows(2) {
        let (source, target) = (&pair[0], &pair[1]);
        let (label, style, evidence_path) = describe_hop(&catalog, &found, &hops, source, target);
        let dashed = evidence_path.is_empty();
        spec.add_edge(DiagramEdge {
            id: String::new(),
            source: source.clone(),
            target: target.clone(),
            label,
            style,
            dashed,
            evidence_path,
        });
    }
    // Returns, inferred: every call comes back.
    for pair in chain.windows(2).rev() {
        spec.add_edge(DiagramEdge {
            id: String::new(),
            source: pair[1].clone(),
            target: pair[0].clone(),
            label: "returns".to_owned(),
            style: style::RETURNS.to_owned(),
            dashed: true,
            evidence_path: String::new(),
        });
    }
    spec.prune_groups();
    spec
}

/// Label, style and evidence for one hop of the chain.
fn describe_hop(
    catalog: &Catalog<'_>,
    found: &[evidence::Found],
    hops: &[&GraphNode],
    source: &str,
    target: &str,
) -> (String, String, String) {
    if source == "actor_user" {
        let node = hops.first();
        let label = node
            .filter(|n| catalog.is_ui(n))
            .map(|_| "opens".to_owned())
            .unwrap_or_else(|| "requests".to_owned());
        return (label, style::CALL.to_owned(), String::new());
    }
    if let Some(hit) = found
        .iter()
        .find(|f| f.source == source && f.target == target)
    {
        let label = if hit.style == style::DATA {
            hit.label.clone()
        } else {
            route_label(catalog, target).unwrap_or_else(|| hit.label.clone())
        };
        return (label, hit.style.clone(), hit.evidence_path.clone());
    }
    let source_node = catalog.node_by_key(&key_of(catalog, source));
    let target_node = catalog.node_by_key(&key_of(catalog, target));
    if let (Some(s), Some(t)) = (source_node, target_node)
        && let Some(edge) = catalog
            .edges()
            .find(|e| e.source_id == s.id && e.target_id == t.id)
        && let Some((label, style)) = super::graph_verb(catalog, edge)
    {
        let evidence = super::graph_evidence(catalog, edge);
        return (label.to_owned(), style.to_owned(), evidence);
    }
    let style = if target_node.is_some_and(|t| t.kind() == NodeKind::Datastore) {
        style::DATA
    } else {
        style::CALL
    };
    let label = route_label(catalog, target).unwrap_or_else(|| {
        if style == style::DATA {
            "queries"
        } else {
            "calls"
        }
        .to_owned()
    });
    (label, style.to_owned(), String::new())
}

/// "GET /api/views" from the target's sampled router or entry point.
fn route_label(catalog: &Catalog<'_>, target: &str) -> Option<String> {
    let node = catalog.node_by_key(&key_of(catalog, target))?;
    let dir = catalog.dir_of(node);
    let mut paths: Vec<&String> = catalog
        .input
        .files
        .keys()
        .filter(|p| super::catalog::within(p, &dir))
        .collect();
    paths.sort();
    for path in paths {
        // The first business route; probes are not what the user asked for.
        if let Some(route) = routes_in(&catalog.input.files[path], 6)
            .into_iter()
            .find(|r| {
                let lower = r.to_lowercase();
                r.split_whitespace().count() <= 3
                    && !["health", "ready", "live", "metrics", "ping"]
                        .iter()
                        .any(|probe| lower.contains(probe))
            })
        {
            return Some(route);
        }
    }
    None
}

/// Graph key for a diagram node id (`service_api` -> `service:api`).
fn key_of(catalog: &Catalog<'_>, id: &str) -> String {
    catalog
        .nodes()
        .find(|n| node_id(&n.key) == id)
        .map(|n| n.key.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagrams::fixtures::{self, input, tree};

    #[test]
    fn user_to_ui_to_api_to_store_and_back() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        assert_eq!(spec.direction, "TB");
        assert_eq!(spec.groups.len(), 3);
        let forward: Vec<(&str, &str, &str)> = spec
            .edges
            .iter()
            .filter(|e| e.style != style::RETURNS)
            .map(|e| (e.source.as_str(), e.target.as_str(), e.label.as_str()))
            .collect();
        assert_eq!(forward[0], ("actor_user", "service_web", "opens"));
        assert_eq!(forward[1].0, "service_web");
        assert_eq!(forward[1].1, "service_api");
        // api -> quant is shown by main.rs; its label is quant's first route.
        let api_quant = forward
            .iter()
            .find(|e| e.0 == "service_api" && e.1 == "service_quant")
            .unwrap();
        assert_eq!(api_quant.2, "GET /price");
        let last = forward.last().unwrap();
        assert_eq!(last.1, "datastore_postgres");
        let returns = spec
            .edges
            .iter()
            .filter(|e| e.style == style::RETURNS)
            .count();
        assert_eq!(returns, forward.len());
        assert!(
            spec.edges
                .iter()
                .filter(|e| e.style == style::RETURNS)
                .all(|e| e.dashed)
        );
        // The hop the sample does not show is dashed; the one it shows is cited.
        assert!(spec.edges[0].dashed);
        let cited = spec
            .edges
            .iter()
            .find(|e| e.source == "service_api" && e.target == "service_quant")
            .unwrap();
        assert_eq!(cited.evidence_path, "services/api/src/main.rs");
    }
}
