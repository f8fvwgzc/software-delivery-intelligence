//! Sequence: the principal workflow as an ordered exchange of messages.
//! Participants are the actor, the interface, the entry point and the
//! components the import graph reaches from it (depth 6), ending at the
//! store and its database when one is reached. Edge ids `m01`, `m02`, …
//! carry the order: every call first, then the returns back up the chain
//! (dashed, inferred). CONTRACT.md §16.

use std::collections::HashSet;

use sdi_proto::knowledge::v1::{GraphNode, NodeKind};

use super::{
    DiagramInput,
    catalog::{Catalog, node_id},
    components::{Component, Components, Role, split_words},
    evidence::{self, routes_in},
    model::{DiagramEdge, DiagramGroup, DiagramNode, DiagramSpec, kind, style},
};

/// Components after the entry point, at most.
pub const MAX_DEPTH: usize = 6;

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let catalog = Catalog::new(input);
    let components = Components::build(&catalog);
    let found = evidence::detect(&catalog);
    let mut spec = DiagramSpec::new("TB");
    for (id, label, description) in [
        ("client", "Client", "Who starts the workflow"),
        (
            "services",
            "Services",
            "Entry point and the components it calls, in order",
        ),
        ("data", "Data", "Where the workflow ends"),
    ] {
        spec.add_group(DiagramGroup {
            id: id.to_owned(),
            label: label.to_owned(),
            description: description.to_owned(),
            path: String::new(),
        });
    }

    // 1. Interface and entry point.
    let uis: Vec<&GraphNode> = catalog
        .nodes()
        .filter(|n| n.kind() == NodeKind::Service && catalog.is_product_node(n) && catalog.is_ui(n))
        .collect();
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
        let fan_in = found
            .iter()
            .filter(|f| f.target == node_id(&n.key) && f.label == "calls")
            .count();
        let has_entry = catalog.entry_point(&catalog.dir_of(n)).is_none();
        (position, has_entry, fan_in, n.label.clone())
    });

    // A CLI binary is the principal entry even when a dashboard exists; a
    // UI fronting an HTTP service is the entry otherwise.
    let cli = super::architecture::principal_service(&components)
        .filter(|s| !super::architecture::serves_http(&catalog, &s.package))
        .map(|s| s.id.clone());
    let uis: Vec<&GraphNode> = if cli.is_some() { Vec::new() } else { uis };
    let actor_label = if uis.is_empty() { "Operator" } else { "User" };
    spec.add_node(DiagramNode {
        id: "actor_user".to_owned(),
        label: actor_label.to_owned(),
        kind: kind::ACTOR.to_owned(),
        group_id: "client".to_owned(),
        path: String::new(),
        description: if uis.is_empty() {
            "Whoever starts the product".to_owned()
        } else {
            "A person using the product".to_owned()
        },
        caption: if uis.is_empty() { "CLI" } else { "browser" }.to_owned(),
    });

    // Chain of component ids, in call order.
    let mut chain: Vec<String> = vec!["actor_user".to_owned()];
    let mut visited: HashSet<String> = HashSet::new();
    if let Some(ui) = uis.first() {
        push_component(
            &catalog,
            &components,
            &mut spec,
            &mut chain,
            &mut visited,
            &node_id(&ui.key),
        );
        // The UI's own client module is the hop that talks to the backend.
        let current = chain.last().cloned().unwrap_or_default();
        if let Some(client) = next_hop(&components, &current, &visited, |c| {
            let lower = c.label.to_lowercase();
            lower.contains("client") || lower.contains("api")
        }) {
            push_component(
                &catalog,
                &components,
                &mut spec,
                &mut chain,
                &mut visited,
                &client,
            );
        }
    }
    if let Some(first) = ordered.first() {
        push_component(
            &catalog,
            &components,
            &mut spec,
            &mut chain,
            &mut visited,
            &node_id(&first.key),
        );
    }
    // 2. Follow the import graph from the entry point: the busiest
    // execution components first, configuration and UI last.
    let mut depth = 0;
    while depth < MAX_DEPTH {
        let Some(current) = chain.last().cloned() else {
            break;
        };
        // A library's root is a detour once one of its modules is on the chain.
        let packages_on_chain: HashSet<String> = chain
            .iter()
            .filter_map(|id| components.get(id).map(|c| c.package.clone()))
            .collect();
        let Some(next) = next_hop(&components, &current, &visited, |c| {
            c.role != Role::State
                && !(c.is_root && c.kind == kind::MODULE && packages_on_chain.contains(&c.package))
        }) else {
            break;
        };
        push_component(
            &catalog,
            &components,
            &mut spec,
            &mut chain,
            &mut visited,
            &next,
        );
        depth += 1;
    }
    // 3. The store the chain reaches (a data edge to a store-like component
    // beats any other state module), then its database.
    let store_hop = {
        let mut candidates: Vec<(usize, bool, bool, String)> = Vec::new();
        for (position, id) in chain.iter().enumerate() {
            for edge in components.edges.iter().filter(|e| e.source == *id) {
                let Some(target) = components.get(&edge.target) else {
                    continue;
                };
                if target.role != Role::State || visited.contains(&target.id) {
                    continue;
                }
                let store_like = split_words(&target.label).iter().any(|w| {
                    matches!(
                        w.as_str(),
                        "store"
                            | "db"
                            | "database"
                            | "repo"
                            | "repository"
                            | "persistence"
                            | "storage"
                    )
                });
                candidates.push((
                    position,
                    edge.style != style::DATA,
                    !store_like,
                    target.id.clone(),
                ));
            }
        }
        candidates.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then(a.2.cmp(&b.2))
                .then(b.0.cmp(&a.0))
                .then(a.3.cmp(&b.3))
        });
        candidates.first().map(|c| c.3.clone())
    };
    if let Some(store) = store_hop {
        push_component(
            &catalog,
            &components,
            &mut spec,
            &mut chain,
            &mut visited,
            &store,
        );
    }
    let datastore = chain
        .iter()
        .rev()
        .find_map(|id| datastore_for(&catalog, &components, &found, id));
    if let Some((node, verb, evidence_path)) = &datastore
        && let Some(diagram_node) = catalog.diagram_node(node, "data")
    {
        let id = diagram_node.id.clone();
        spec.add_node(diagram_node);
        chain.push(id);
        debug_assert!(!verb.is_empty() || evidence_path.is_empty() || true);
    }

    // 4. Messages: calls in order, then returns back up the chain.
    let mut messages: Vec<DiagramEdge> = Vec::new();
    for pair in chain.windows(2) {
        let (source, target) = (&pair[0], &pair[1]);
        let (label, edge_style, evidence_path) = if let Some((node, verb, path)) = &datastore
            && *target == node_id(&node.key)
        {
            (data_message(verb), style::DATA.to_owned(), path.clone())
        } else {
            describe_hop(&catalog, &components, &found, &chain, source, target)
        };
        messages.push(DiagramEdge {
            id: String::new(),
            source: source.clone(),
            target: target.clone(),
            label,
            style: edge_style,
            dashed: evidence_path.is_empty(),
            evidence_path,
        });
    }
    for (i, pair) in chain.windows(2).rev().enumerate() {
        let from_store = i == 0 && datastore.is_some();
        let to_actor = pair[0] == "actor_user";
        let label = if from_store {
            "rows"
        } else if to_actor {
            "200 OK"
        } else {
            "returns"
        };
        messages.push(DiagramEdge {
            id: String::new(),
            source: pair[1].clone(),
            target: pair[0].clone(),
            label: label.to_owned(),
            style: style::RETURNS.to_owned(),
            dashed: true,
            evidence_path: String::new(),
        });
    }
    for (i, mut message) in messages.into_iter().enumerate() {
        message.id = format!("m{:02}", i + 1);
        spec.edges.push(message);
    }
    spec.prune_groups();
    spec
}

fn push_component(
    catalog: &Catalog<'_>,
    components: &Components,
    spec: &mut DiagramSpec,
    chain: &mut Vec<String>,
    visited: &mut HashSet<String>,
    id: &str,
) {
    if visited.contains(id) {
        return;
    }
    let node = if let Some(component) = components.get(id) {
        DiagramNode {
            id: component.id.clone(),
            label: component.label.clone(),
            kind: component.kind.to_owned(),
            group_id: "services".to_owned(),
            path: component.path.clone(),
            description: component.description.clone(),
            caption: component.caption.clone(),
        }
    } else if let Some(graph_node) = catalog.nodes().find(|n| node_id(&n.key) == id)
        && let Some(node) = catalog.diagram_node(graph_node, "services")
    {
        node
    } else {
        return;
    };
    spec.add_node(node);
    visited.insert(id.to_owned());
    chain.push(id.to_owned());
}

/// The best unvisited component `from` imports: execution first, then
/// integrations, monitoring, bootstrap; busier components first.
fn next_hop(
    components: &Components,
    from: &str,
    visited: &HashSet<String>,
    accept: impl Fn(&Component) -> bool,
) -> Option<String> {
    let mut candidates: Vec<&Component> = components
        .edges
        .iter()
        .filter(|e| e.source == from && !visited.contains(&e.target))
        .filter_map(|e| components.get(&e.target))
        .filter(|c| accept(c))
        .collect();
    candidates.sort_by_key(|c| {
        let role_rank = match c.role {
            Role::Execution => 0,
            Role::Integrations => 1,
            Role::State => 2,
            Role::Monitoring => 3,
            Role::Other => 4,
            Role::Bootstrap => 5,
        };
        let out_degree = components
            .edges
            .iter()
            .filter(|e| e.source == c.id && !visited.contains(&e.target))
            .count();
        (role_rank, std::cmp::Reverse(out_degree), c.label.clone())
    });
    candidates.first().map(|c| c.id.clone())
}

/// The datastore a component reaches (sampled driver evidence first, then
/// the graph), with the data verb and the file that shows it.
fn datastore_for<'a>(
    catalog: &Catalog<'a>,
    components: &Components,
    found: &[evidence::Found],
    id: &str,
) -> Option<(&'a GraphNode, String, String)> {
    let component = components.get(id);
    let package = component.map(|c| c.package.clone()).or_else(|| {
        catalog
            .nodes()
            .find(|n| node_id(&n.key) == id)
            .map(|n| n.key.clone())
    })?;
    let files: Vec<&String> = component
        .map(|c| c.files.iter().collect())
        .unwrap_or_default();
    for hit in found.iter().filter(|f| f.style == style::DATA) {
        let from_component = files.iter().any(|f| **f == hit.evidence_path);
        let from_package = hit.source == node_id(&package);
        let hits = from_component || (component.is_none_or(|c| c.is_root) && from_package);
        if hits
            && let Some(node) = catalog.nodes().find(|n| node_id(&n.key) == hit.target)
            && node.kind() == NodeKind::Datastore
            && !Catalog::is_queue(node)
        {
            return Some((node, hit.label.clone(), hit.evidence_path.clone()));
        }
    }
    let owner = catalog.node_by_key(&package)?;
    let edge = catalog.edges().find(|e| {
        e.source_id == owner.id
            && catalog
                .node_by_id(&e.target_id)
                .is_some_and(|t| t.kind() == NodeKind::Datastore && !Catalog::is_queue(t))
    })?;
    let node = catalog.node_by_id(&edge.target_id)?;
    Some((
        node,
        "reads/writes".to_owned(),
        super::graph_evidence(catalog, edge),
    ))
}

fn data_message(verb: &str) -> String {
    match verb {
        "reads" => "select rows",
        "writes" => "insert rows",
        _ => "query rows",
    }
    .to_owned()
}

/// Label, style and evidence for one hop of the chain.
fn describe_hop(
    catalog: &Catalog<'_>,
    components: &Components,
    found: &[evidence::Found],
    chain: &[String],
    source: &str,
    target: &str,
) -> (String, String, String) {
    let target_component = components.get(target);
    if source == "actor_user" {
        let label = match target_component {
            Some(c) if c.kind == kind::UI => "open dashboard".to_owned(),
            Some(c) if c.kind == kind::SERVICE => route_label(catalog, target)
                .unwrap_or_else(|| format!("run {}", object_of(&c.label))),
            _ => "request".to_owned(),
        };
        return (label, style::CALL.to_owned(), String::new());
    }
    // The direct edge, else the edge from whichever earlier participant
    // reaches the target (a store hop is appended after the chain's end).
    let edge = components
        .edges
        .iter()
        .find(|e| e.source == source && e.target == target)
        .or_else(|| {
            chain.iter().rev().find_map(|id| {
                components
                    .edges
                    .iter()
                    .find(|e| e.source == *id && e.target == target)
            })
        });
    if let Some(edge) = edge {
        let object = target_component
            .map(|c| object_of(&c.label))
            .unwrap_or_default();
        return (
            message(&edge.label, &object),
            edge.style.clone(),
            edge.evidence_path.clone(),
        );
    }
    if let Some(hit) = found
        .iter()
        .find(|f| f.source == source && f.target == target)
    {
        let label = route_label(catalog, target).unwrap_or_else(|| {
            let object = target_component
                .map(|c| object_of(&c.label))
                .unwrap_or_default();
            message(&hit.label, &object)
        });
        return (label, hit.style.clone(), hit.evidence_path.clone());
    }
    let label = route_label(catalog, target).unwrap_or_else(|| {
        let object = target_component
            .map(|c| object_of(&c.label))
            .unwrap_or_default();
        message("calls", &object)
    });
    (label, style::CALL.to_owned(), String::new())
}

/// "load workflow", "spawn runner", "insert run": the verb in the
/// imperative plus the target's object noun, three words at most.
pub fn message(verb: &str, object: &str) -> String {
    let imperative = match verb {
        "loads" => "load",
        "starts" => "start",
        "runs" => "spawn",
        "dispatches" => "dispatch",
        "records" => "insert",
        "reads" => "read",
        "writes" => "write",
        "reads/writes" => "query",
        "calls" => "call",
        "streams" => "stream",
        "validates" => "validate",
        "serves" => "route to",
        "renders" => "render",
        "configures" => "configure",
        "publishes" => "publish",
        "consumes" => "consume",
        "imports" | "uses" => "use",
        other => other,
    };
    let text = if object.is_empty() {
        imperative.to_owned()
    } else {
        format!("{imperative} {object}")
    };
    text.split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The noun of a component label: "Workflow store" → "workflow",
/// "Agent runner" → "agent", "App" → "app".
pub fn object_of(label: &str) -> String {
    let words = split_words(label);
    let kept: Vec<&str> = words
        .iter()
        .map(String::as_str)
        .filter(|w| {
            !matches!(
                *w,
                "store" | "settings" | "runner" | "ui" | "client" | "server" | "the" | "module"
            )
        })
        .collect();
    match kept.as_slice() {
        [] => words.first().cloned().unwrap_or_default(),
        [one] => (*one).to_owned(),
        [a, b, ..] => format!("{a} {b}"),
    }
}

/// "GET /api/views" from the target's sampled router or entry point.
fn route_label(catalog: &Catalog<'_>, target: &str) -> Option<String> {
    let node = catalog.nodes().find(|n| node_id(&n.key) == target)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagrams::fixtures::{self, input, tree};

    fn ordered(spec: &DiagramSpec) -> Vec<(&str, &str, &str, &str)> {
        let mut edges: Vec<&DiagramEdge> = spec.edges.iter().collect();
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        edges
            .iter()
            .map(|e| {
                (
                    e.id.as_str(),
                    e.source.as_str(),
                    e.label.as_str(),
                    e.target.as_str(),
                )
            })
            .collect()
    }

    #[test]
    fn user_to_ui_to_api_to_store_and_back() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        assert_eq!(spec.direction, "TB");
        assert_eq!(spec.groups.len(), 3);
        let messages = ordered(&spec);
        assert_eq!(messages[0].0, "m01");
        assert_eq!(
            (messages[0].1, messages[0].2, messages[0].3),
            ("actor_user", "open dashboard", "service_web")
        );
        // Every id is sequential and unique.
        for (i, m) in messages.iter().enumerate() {
            assert_eq!(m.0, format!("m{:02}", i + 1));
        }
        let calls: Vec<_> = spec
            .edges
            .iter()
            .filter(|e| e.style != style::RETURNS)
            .collect();
        let returns: Vec<_> = spec
            .edges
            .iter()
            .filter(|e| e.style == style::RETURNS)
            .collect();
        assert_eq!(calls.len(), returns.len());
        assert!(
            returns
                .iter()
                .all(|e| e.dashed && e.evidence_path.is_empty())
        );
        // The chain passes the API client, the API, its routes and store, and ends at PostgreSQL.
        let participants: Vec<&str> = spec.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(
            participants.contains(&"file_web_src_lib_api_ts"),
            "{participants:?}"
        );
        assert!(participants.contains(&"service_api"), "{participants:?}");
        assert!(
            participants.contains(&"dir_services_api_src_db"),
            "{participants:?}"
        );
        assert_eq!(calls.last().unwrap().target, "datastore_postgres");
        assert!(
            calls.last().unwrap().label.ends_with("rows"),
            "{}",
            calls.last().unwrap().label
        );
        // The UI -> API hop is the first business route.
        let to_api = calls.iter().find(|e| e.target == "service_api").unwrap();
        assert_eq!(
            to_api.label, "GET /api/views",
            "axum routes carry their method"
        );
        // Labels are 1–3 words; the return to the user is a status.
        for edge in &spec.edges {
            let words = edge.label.split_whitespace().count();
            assert!((1..=3).contains(&words), "{:?}", edge.label);
        }
        assert_eq!(returns.last().unwrap().label, "200 OK");
        assert_eq!(returns[0].label, "rows");
    }

    #[test]
    fn symphony_chain_follows_the_import_graph_from_the_binary() {
        let files = fixtures::symphony_files();
        let tree = fixtures::symphony_tree();
        let ctx = fixtures::symphony_context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        let messages = ordered(&spec);
        assert_eq!(
            (messages[0].1, messages[0].3),
            ("actor_user", "service_symphony")
        );
        assert_eq!(spec.node("actor_user").unwrap().label, "Operator");
        let labels: Vec<&str> = messages.iter().map(|m| m.2).collect();
        assert!(
            labels
                .iter()
                .any(|l| l.starts_with("start") || l.starts_with("spawn")),
            "{labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|l| l.starts_with("insert") || l.starts_with("read")),
            "{labels:?}"
        );
        assert!(spec.has_node("datastore_sqlite"));
        assert!(spec.nodes.len() <= 2 + MAX_DEPTH + 3);
    }

    #[test]
    fn messages_are_short() {
        assert_eq!(message("loads", "workflow"), "load workflow");
        assert_eq!(message("runs", "agent"), "spawn agent");
        assert_eq!(message("records", "run history"), "insert run history");
        assert_eq!(object_of("Workflow store"), "workflow");
        assert_eq!(object_of("Agent runner"), "agent");
        assert_eq!(object_of("Store"), "store");
    }
}
