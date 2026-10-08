//! Graph hygiene: one node per key, edges only between nodes that exist,
//! and every service, library and pipeline reachable from the repository.

use std::collections::{BTreeSet, HashMap};

use crate::model::*;

pub(crate) fn finalize(
    repo_key: &str,
    nodes: Vec<NodeDraft>,
    edges: Vec<EdgeDraft>,
) -> (Vec<NodeDraft>, Vec<EdgeDraft>) {
    let mut by_key: HashMap<String, usize> = HashMap::new();
    let mut merged: Vec<NodeDraft> = Vec::new();
    for n in nodes {
        match by_key.get(&n.key) {
            Some(&i) => {
                let m = &mut merged[i];
                for (k, v) in n.attributes {
                    m.attributes.entry(k).or_insert(v);
                }
                if m.description.is_empty() {
                    m.description = n.description;
                }
                if m.source_path.is_empty() {
                    m.source_path = n.source_path;
                }
            }
            None => {
                by_key.insert(n.key.clone(), merged.len());
                merged.push(n);
            }
        }
    }

    let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
    let mut kept: Vec<EdgeDraft> = edges
        .into_iter()
        .filter(|e| e.source_key != e.target_key)
        .filter(|e| by_key.contains_key(&e.source_key) && by_key.contains_key(&e.target_key))
        .filter(|e| {
            seen.insert((
                e.source_key.clone(),
                e.target_key.clone(),
                e.relation.clone(),
            ))
        })
        .collect();

    // Top-level units always hang off the repository so the graph has one root.
    if by_key.contains_key(repo_key) {
        for n in &merged {
            let wants_root = matches!(
                n.kind,
                NodeKind::Service | NodeKind::Library | NodeKind::Pipeline | NodeKind::IacModule
            );
            let has_parent = kept
                .iter()
                .any(|e| e.target_key == n.key && e.relation == "CONTAINS");
            if wants_root && !has_parent && n.key != repo_key {
                kept.push(EdgeDraft {
                    source_key: repo_key.to_string(),
                    target_key: n.key.clone(),
                    relation: "CONTAINS".into(),
                    category: RelationCategory::Structure,
                    attributes: Default::default(),
                });
            }
        }
    }
    (merged, kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(kind: NodeKind, key: &str) -> NodeDraft {
        NodeDraft {
            kind,
            key: key.into(),
            label: key.into(),
            description: String::new(),
            source_path: String::new(),
            attributes: Default::default(),
        }
    }

    fn edge(s: &str, t: &str, r: &str) -> EdgeDraft {
        EdgeDraft {
            source_key: s.into(),
            target_key: t.into(),
            relation: r.into(),
            category: RelationCategory::Structure,
            attributes: Default::default(),
        }
    }

    #[test]
    fn dedupes_and_drops_dangling() {
        let nodes = vec![
            node(NodeKind::Repository, "repository:x"),
            node(NodeKind::Service, "service:a"),
            node(NodeKind::Service, "service:a"),
            node(NodeKind::Pipeline, "pipeline:ci"),
        ];
        let edges = vec![
            edge("service:a", "datastore:missing", "DEPENDS_ON"),
            edge("pipeline:ci", "service:a", "TESTS"),
            edge("pipeline:ci", "service:a", "TESTS"),
        ];
        let (n, e) = finalize("repository:x", nodes, edges);
        assert_eq!(n.len(), 3);
        let rels: Vec<(&str, &str, &str)> = e
            .iter()
            .map(|e| {
                (
                    e.source_key.as_str(),
                    e.target_key.as_str(),
                    e.relation.as_str(),
                )
            })
            .collect();
        assert_eq!(
            rels,
            vec![
                ("pipeline:ci", "service:a", "TESTS"),
                ("repository:x", "service:a", "CONTAINS"),
                ("repository:x", "pipeline:ci", "CONTAINS"),
            ]
        );
    }
}
