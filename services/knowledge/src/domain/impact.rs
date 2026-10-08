//! Change impact: breadth-first walk over the valid edges of the graph.
//!
//! Every edge is followed source → target. Structural and dependency edges
//! are also followed backwards, because "X contains Y" and "X depends on Y"
//! both mean a change on either side matters to the other.

use std::collections::{BTreeMap, HashMap, VecDeque};

pub const DEFAULT_MAX_DEPTH: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub relation: String,
    /// Lowercase category as stored, e.g. `structure`.
    pub category: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Impacted {
    pub id: String,
    pub distance: u32,
    /// Relations walked from the origin, in order.
    pub via: Vec<String>,
}

fn is_bidirectional(edge: &Edge) -> bool {
    edge.category == "structure" || edge.relation.contains("DEPENDS")
}

/// Nodes reachable from `origin` within `max_depth` hops, nearest first.
pub fn analyze(origin: &str, edges: &[Edge], max_depth: u32) -> Vec<Impacted> {
    let mut adjacency: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for edge in edges {
        adjacency
            .entry(edge.source.as_str())
            .or_default()
            .push((edge.target.as_str(), edge.relation.as_str()));
        if is_bidirectional(edge) {
            adjacency
                .entry(edge.target.as_str())
                .or_default()
                .push((edge.source.as_str(), edge.relation.as_str()));
        }
    }

    let mut seen: HashMap<&str, Impacted> = HashMap::new();
    let mut queue = VecDeque::new();
    queue.push_back((origin, 0_u32, Vec::<String>::new()));
    seen.insert(
        origin,
        Impacted {
            id: origin.to_owned(),
            distance: 0,
            via: Vec::new(),
        },
    );

    while let Some((node, depth, via)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        let Some(neighbours) = adjacency.get(node) else {
            continue;
        };
        for (next, relation) in neighbours {
            if seen.contains_key(next) {
                continue;
            }
            let mut path = via.clone();
            path.push((*relation).to_owned());
            seen.insert(
                next,
                Impacted {
                    id: (*next).to_owned(),
                    distance: depth + 1,
                    via: path.clone(),
                },
            );
            queue.push_back((next, depth + 1, path));
        }
    }

    let mut impacted: Vec<Impacted> = seen.into_values().filter(|n| n.distance > 0).collect();
    impacted.sort_by(|a, b| a.distance.cmp(&b.distance).then_with(|| a.id.cmp(&b.id)));
    impacted
}

/// Picks the node whose `source_path` best covers `path`: an exact match,
/// then the longest directory prefix. A node that points at a file (such as
/// `backend/Cargo.toml`) also covers that file's directory, and a node with
/// no source path (the repository) covers everything as a last resort.
pub fn origin_for_path<'a>(nodes: &'a [(String, String)], path: &str) -> Option<&'a str> {
    let path = path.trim_matches('/');
    let mut best: Option<(&str, usize)> = None;
    let mut root: Option<&str> = None;
    for (id, source_path) in nodes {
        let source = source_path.trim_matches('/');
        if source.is_empty() {
            root.get_or_insert(id.as_str());
            continue;
        }
        let mut candidates = vec![(source, source.len() * 2 + 1)];
        if let Some((dir, file)) = source.rsplit_once('/')
            && file.contains('.')
        {
            candidates.push((dir, dir.len() * 2));
        }
        for (prefix, rank) in candidates {
            let covers = prefix == path || path.starts_with(&format!("{prefix}/"));
            if covers && best.is_none_or(|(_, best_rank)| rank > best_rank) {
                best = Some((id.as_str(), rank));
            }
        }
    }
    best.map(|(id, _)| id).or(root)
}

/// Plain-language summary: "Changing X affects 4 nodes within 2 hops: 2 services, 1 datastore, 1 pipeline."
pub fn summary(
    origin_label: &str,
    impacted: &[Impacted],
    kinds: &HashMap<String, String>,
) -> String {
    if impacted.is_empty() {
        return format!("Changing {origin_label} affects nothing else in the graph.");
    }
    let max_distance = impacted.iter().map(|n| n.distance).max().unwrap_or(0);
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for node in impacted {
        let kind = kinds.get(&node.id).map(String::as_str).unwrap_or("node");
        *by_kind.entry(kind).or_default() += 1;
    }
    let breakdown: Vec<String> = by_kind
        .iter()
        .map(|(kind, count)| format!("{count} {}", plural(kind, *count)))
        .collect();
    let hops = if max_distance == 1 {
        "1 hop".to_owned()
    } else {
        format!("{max_distance} hops")
    };
    format!(
        "Changing {origin_label} affects {} node{} within {hops}: {}.",
        impacted.len(),
        if impacted.len() == 1 { "" } else { "s" },
        breakdown.join(", ")
    )
}

fn plural(kind: &str, count: usize) -> String {
    let word = kind.replace('_', " ");
    if count == 1 { word } else { format!("{word}s") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(source: &str, target: &str, relation: &str, category: &str) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            relation: relation.into(),
            category: category.into(),
        }
    }

    #[test]
    fn walks_forward_and_backwards_over_dependencies() {
        let edges = vec![
            edge("api", "db", "DEPENDS_ON", "infrastructure"),
            edge("web", "api", "DEPENDS_ON", "infrastructure"),
            edge("api", "ci", "TESTED_BY", "validation"),
        ];
        let impacted = analyze("api", &edges, 4);
        let ids: Vec<&str> = impacted.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["ci", "db", "web"]);
        assert!(impacted.iter().all(|n| n.distance == 1));
    }

    #[test]
    fn non_structural_edges_are_one_way() {
        let edges = vec![edge("api", "ci", "TESTED_BY", "validation")];
        assert!(analyze("ci", &edges, 4).is_empty());
    }

    #[test]
    fn respects_max_depth_and_records_via() {
        let edges = vec![
            edge("a", "b", "CONTAINS", "structure"),
            edge("b", "c", "DEPLOYED_TO", "execution"),
            edge("c", "d", "RUNS_ON", "infrastructure"),
        ];
        let impacted = analyze("a", &edges, 2);
        assert_eq!(impacted.len(), 2);
        let c = impacted.iter().find(|n| n.id == "c").unwrap();
        assert_eq!(c.distance, 2);
        assert_eq!(c.via, vec!["CONTAINS", "DEPLOYED_TO"]);
    }

    #[test]
    fn cycles_terminate() {
        let edges = vec![
            edge("a", "b", "DEPENDS_ON", "x"),
            edge("b", "a", "DEPENDS_ON", "x"),
        ];
        assert_eq!(analyze("a", &edges, 10).len(), 1);
    }

    #[test]
    fn origin_for_path_prefers_longest_prefix() {
        let nodes = vec![
            ("repo".to_owned(), String::new()),
            ("svc".to_owned(), "services".to_owned()),
            ("api".to_owned(), "services/api".to_owned()),
        ];
        assert_eq!(
            origin_for_path(&nodes, "services/api/src/main.rs"),
            Some("api")
        );
        assert_eq!(origin_for_path(&nodes, "services/web"), Some("svc"));
        assert_eq!(origin_for_path(&nodes, "README.md"), Some("repo"));
        assert_eq!(origin_for_path(&[], "README.md"), None);
    }

    #[test]
    fn origin_for_path_uses_the_directory_of_file_nodes() {
        let nodes = vec![
            ("api".to_owned(), "backend/Cargo.toml".to_owned()),
            (
                "migrations".to_owned(),
                "backend/migrations/0001.sql".to_owned(),
            ),
            ("readme".to_owned(), "README.md".to_owned()),
        ];
        assert_eq!(origin_for_path(&nodes, "backend/src/main.rs"), Some("api"));
        assert_eq!(
            origin_for_path(&nodes, "backend/migrations/0002.sql"),
            Some("migrations")
        );
        assert_eq!(origin_for_path(&nodes, "backend/Cargo.toml"), Some("api"));
        assert_eq!(origin_for_path(&nodes, "README.md"), Some("readme"));
        assert_eq!(origin_for_path(&nodes, "docs/x.md"), None);
    }

    #[test]
    fn summary_counts_kinds() {
        let impacted = vec![
            Impacted {
                id: "a".into(),
                distance: 1,
                via: vec![],
            },
            Impacted {
                id: "b".into(),
                distance: 2,
                via: vec![],
            },
        ];
        let kinds = HashMap::from([
            ("a".to_owned(), "service".to_owned()),
            ("b".to_owned(), "service".to_owned()),
        ]);
        assert_eq!(
            summary("API", &impacted, &kinds),
            "Changing API affects 2 nodes within 2 hops: 2 services."
        );
    }
}
