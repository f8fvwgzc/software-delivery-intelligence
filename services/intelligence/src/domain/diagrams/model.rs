//! The diagram spec: groups, nodes and edges (CONTRACT.md §16). Stored as
//! JSON on the row and the single source of truth for every renderer.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

pub const MAX_NODES: usize = 34;
pub const MAX_EDGES: usize = 48;
pub const MAX_GROUPS: usize = 6;

/// Node kinds (`DiagramNode.kind`).
pub mod kind {
    pub const ACTOR: &str = "actor";
    pub const SERVICE: &str = "service";
    pub const MODULE: &str = "module";
    pub const UI: &str = "ui";
    pub const DATASTORE: &str = "datastore";
    pub const QUEUE: &str = "queue";
    pub const EXTERNAL: &str = "external";
    pub const INFRA: &str = "infra";
    pub const JOB: &str = "job";

    pub const ALL: [&str; 9] = [
        ACTOR, SERVICE, MODULE, UI, DATASTORE, QUEUE, EXTERNAL, INFRA, JOB,
    ];
}

/// Edge styles (`DiagramEdge.style`).
pub mod style {
    pub const CALL: &str = "call";
    pub const DATA: &str = "data";
    pub const ASYNC: &str = "async";
    pub const RETURNS: &str = "returns";
    pub const CONTAINS: &str = "contains";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiagramSpec {
    /// "LR" | "TB"
    pub direction: String,
    pub groups: Vec<DiagramGroup>,
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
}

impl Default for DiagramSpec {
    fn default() -> Self {
        Self {
            direction: "LR".to_owned(),
            groups: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DiagramGroup {
    pub id: String,
    pub label: String,
    pub description: String,
    /// Repository directory the group stands for, when there is one.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DiagramNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub group_id: String,
    /// Repository path the node stands for; empty for actors and external systems.
    pub path: String,
    pub description: String,
    /// Short type caption under the label, e.g. "axum", "PostgreSQL".
    pub caption: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DiagramEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    /// 1–3 words, e.g. "calls", "reads", "publishes".
    pub label: String,
    pub style: String,
    /// Optional or unverified relationship.
    pub dashed: bool,
    /// File that shows the relationship; empty when inferred.
    pub evidence_path: String,
}

impl DiagramSpec {
    pub fn new(direction: &str) -> Self {
        Self {
            direction: direction.to_owned(),
            ..Default::default()
        }
    }

    pub fn node(&self, id: &str) -> Option<&DiagramNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: &str) -> Option<&mut DiagramNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn group(&self, id: &str) -> Option<&DiagramGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn has_node(&self, id: &str) -> bool {
        self.node(id).is_some()
    }

    /// Adds a group once; a repeated id keeps the first definition.
    pub fn add_group(&mut self, group: DiagramGroup) -> String {
        if self.group(&group.id).is_none() {
            self.groups.push(group.clone());
        }
        group.id
    }

    /// Adds a node once; a repeated id keeps the first definition.
    pub fn add_node(&mut self, node: DiagramNode) -> String {
        if !self.has_node(&node.id) {
            self.nodes.push(node.clone());
        }
        node.id
    }

    /// Adds an edge when both endpoints exist and it is not a self loop or a
    /// duplicate (same endpoints and style). The first edge between two
    /// nodes wins, so callers add the best-evidenced edges first; a
    /// duplicate with evidence still upgrades a dashed one in place.
    pub fn add_edge(&mut self, edge: DiagramEdge) -> bool {
        if edge.source == edge.target
            || !self.has_node(&edge.source)
            || !self.has_node(&edge.target)
        {
            return false;
        }
        if let Some(existing) = self
            .edges
            .iter_mut()
            .find(|e| e.source == edge.source && e.target == edge.target && e.style == edge.style)
        {
            if existing.dashed && !edge.dashed {
                existing.dashed = false;
                existing.evidence_path = edge.evidence_path;
                existing.label = edge.label;
            }
            return false;
        }
        let mut edge = edge;
        if edge.id.is_empty() {
            edge.id = format!("e{}", self.edges.len() + 1);
        }
        self.edges.push(edge);
        true
    }

    pub fn degree(&self, id: &str) -> usize {
        self.edges
            .iter()
            .filter(|e| e.source == id || e.target == id)
            .count()
    }

    /// Drops groups nobody is in and clears group ids that point nowhere.
    pub fn prune_groups(&mut self) {
        let used: HashSet<&str> = self.nodes.iter().map(|n| n.group_id.as_str()).collect();
        self.groups.retain(|g| used.contains(g.id.as_str()));
        let known: HashSet<String> = self.groups.iter().map(|g| g.id.clone()).collect();
        for node in &mut self.nodes {
            if !known.contains(&node.group_id) {
                node.group_id.clear();
            }
        }
    }

    /// Enforces the §16 limits: at most 34 nodes (the best connected win),
    /// 48 edges (evidence-backed first) and 6 groups (the smallest merge
    /// into "Other"). Deterministic for the same input.
    pub fn enforce_limits(&mut self) {
        if self.nodes.len() > MAX_NODES {
            let mut ranked: Vec<(usize, usize)> = self
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (i, self.degree(&n.id)))
                .collect();
            // Highest degree first, original order as the tiebreak.
            ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let keep: HashSet<usize> = ranked.iter().take(MAX_NODES).map(|(i, _)| *i).collect();
            let mut index = 0;
            self.nodes.retain(|_| {
                let kept = keep.contains(&index);
                index += 1;
                kept
            });
            let ids: HashSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
            self.edges
                .retain(|e| ids.contains(e.source.as_str()) && ids.contains(e.target.as_str()));
        }
        if self.edges.len() > MAX_EDGES {
            let mut ranked: Vec<(usize, &DiagramEdge)> = self.edges.iter().enumerate().collect();
            ranked.sort_by_key(|(i, e)| (e.dashed, e.evidence_path.is_empty(), *i));
            let keep: HashSet<usize> = ranked.iter().take(MAX_EDGES).map(|(i, _)| *i).collect();
            let mut index = 0;
            self.edges.retain(|_| {
                let kept = keep.contains(&index);
                index += 1;
                kept
            });
        }
        self.prune_groups();
        if self.groups.len() > MAX_GROUPS {
            let mut sizes: BTreeMap<&str, usize> = BTreeMap::new();
            for node in &self.nodes {
                *sizes.entry(node.group_id.as_str()).or_default() += 1;
            }
            let mut by_size: Vec<(&str, usize)> = self
                .groups
                .iter()
                .map(|g| {
                    (
                        g.id.as_str(),
                        sizes.get(g.id.as_str()).copied().unwrap_or(0),
                    )
                })
                .collect();
            by_size.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let kept: HashSet<String> = by_size
                .iter()
                .take(MAX_GROUPS - 1)
                .map(|(id, _)| (*id).to_owned())
                .collect();
            let other = "other".to_owned();
            for node in &mut self.nodes {
                if !kept.contains(&node.group_id) {
                    node.group_id = other.clone();
                }
            }
            self.groups.retain(|g| kept.contains(&g.id));
            self.groups.push(DiagramGroup {
                id: other,
                label: "Other".to_owned(),
                description: "Components from the smaller groups, merged to stay within six groups"
                    .to_owned(),
                path: String::new(),
            });
            self.prune_groups();
        }
    }

    /// Every endpoint names a node, no self loops, no duplicate ids.
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashMap::new();
        for node in &self.nodes {
            if node.id.trim().is_empty() {
                return Err("node without an id".to_owned());
            }
            if ids.insert(node.id.as_str(), ()).is_some() {
                return Err(format!("duplicate node id {}", node.id));
            }
            if !kind::ALL.contains(&node.kind.as_str()) {
                return Err(format!("node {} has unknown kind {:?}", node.id, node.kind));
            }
        }
        let groups: HashSet<&str> = self.groups.iter().map(|g| g.id.as_str()).collect();
        if groups.len() != self.groups.len() {
            return Err("duplicate group id".to_owned());
        }
        for node in &self.nodes {
            if !node.group_id.is_empty() && !groups.contains(node.group_id.as_str()) {
                return Err(format!(
                    "node {} is in unknown group {}",
                    node.id, node.group_id
                ));
            }
        }
        for edge in &self.edges {
            if !ids.contains_key(edge.source.as_str()) || !ids.contains_key(edge.target.as_str()) {
                return Err(format!(
                    "edge {} -> {} names an unknown node",
                    edge.source, edge.target
                ));
            }
            if edge.source == edge.target {
                return Err(format!("edge {} loops on itself", edge.source));
            }
        }
        if self.direction != "LR" && self.direction != "TB" {
            return Err(format!(
                "direction must be LR or TB, not {:?}",
                self.direction
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, group: &str) -> DiagramNode {
        DiagramNode {
            id: id.into(),
            label: id.into(),
            kind: kind::SERVICE.into(),
            group_id: group.into(),
            ..Default::default()
        }
    }

    fn edge(s: &str, t: &str, dashed: bool) -> DiagramEdge {
        DiagramEdge {
            source: s.into(),
            target: t.into(),
            label: "calls".into(),
            style: style::CALL.into(),
            dashed,
            evidence_path: if dashed {
                String::new()
            } else {
                "src/main.rs".into()
            },
            ..Default::default()
        }
    }

    #[test]
    fn edges_need_both_endpoints_and_dedupe_with_upgrade() {
        let mut spec = DiagramSpec::new("LR");
        spec.add_node(node("a", ""));
        spec.add_node(node("b", ""));
        assert!(!spec.add_edge(edge("a", "zzz", false)));
        assert!(!spec.add_edge(edge("a", "a", false)));
        assert!(spec.add_edge(edge("a", "b", true)));
        assert!(!spec.add_edge(edge("a", "b", false)));
        assert_eq!(spec.edges.len(), 1);
        assert!(!spec.edges[0].dashed);
        assert_eq!(spec.edges[0].evidence_path, "src/main.rs");
        assert_eq!(spec.edges[0].id, "e1");
        spec.validate().unwrap();
    }

    #[test]
    fn limits_keep_the_best_connected_nodes_and_merge_small_groups() {
        let mut spec = DiagramSpec::new("LR");
        for g in 0..8 {
            spec.add_group(DiagramGroup {
                id: format!("g{g}"),
                label: format!("G{g}"),
                ..Default::default()
            });
        }
        for i in 0..40 {
            spec.add_node(node(&format!("n{i}"), &format!("g{}", i % 8)));
        }
        // n0 is the hub; everything else only touches it.
        for i in 1..40 {
            spec.add_edge(edge("n0", &format!("n{i}"), i % 2 == 0));
        }
        for i in 1..20 {
            spec.add_edge(edge(&format!("n{i}"), &format!("n{}", i + 1), true));
        }
        spec.enforce_limits();
        assert!(spec.nodes.len() <= MAX_NODES);
        assert!(spec.edges.len() <= MAX_EDGES);
        assert!(spec.groups.len() <= MAX_GROUPS);
        assert!(spec.has_node("n0"));
        assert!(spec.groups.iter().any(|g| g.id == "other"));
        spec.validate().unwrap();
    }

    #[test]
    fn json_round_trip_tolerates_missing_fields() {
        let spec: DiagramSpec =
            serde_json::from_str(r#"{"nodes":[{"id":"a","label":"A","kind":"service"}]}"#).unwrap();
        assert_eq!(spec.direction, "LR");
        assert_eq!(spec.nodes[0].group_id, "");
        let text = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<DiagramSpec>(&text).unwrap(), spec);
    }
}
