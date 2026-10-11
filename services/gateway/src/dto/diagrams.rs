//! Diagrams (CONTRACT.md §16): the spec is the source of truth for the
//! editorial renderer; `mermaid` is rendered from it or user-edited.

use chrono::{DateTime, Utc};
use sdi_proto::intelligence::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{proto_enum, timestamp};

proto_enum! {
    /// What a diagram shows.
    DiagramKind => proto::DiagramKind {
        Architecture, HighLevel, DataFlow, Deployment, Dependency, Sequence, ModuleMap, DbSchema,
        Custom
    }
}

/// A tinted region of related nodes.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagramGroup {
    /// Group id, unique within the diagram.
    pub id: String,
    /// Label.
    pub label: String,
    /// Longer description.
    #[serde(default)]
    pub description: String,
    /// Repository directory the group stands for, when there is one.
    #[serde(default)]
    pub path: String,
}

impl From<proto::DiagramGroup> for DiagramGroup {
    fn from(group: proto::DiagramGroup) -> Self {
        Self {
            id: group.id,
            label: group.label,
            description: group.description,
            path: group.path,
        }
    }
}

impl From<DiagramGroup> for proto::DiagramGroup {
    fn from(group: DiagramGroup) -> Self {
        Self {
            id: group.id,
            label: group.label,
            description: group.description,
            path: group.path,
        }
    }
}

/// One component on the diagram.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagramNode {
    /// Node id, unique within the diagram.
    pub id: String,
    /// 2–4 word label.
    pub label: String,
    /// `actor` | `service` | `module` | `ui` | `datastore` | `queue` | `external` | `infra` | `job`.
    pub kind: String,
    /// Group the node sits in; empty for none.
    #[serde(default)]
    pub group_id: String,
    /// Repository path the node stands for; the code browser opens it. Empty for actors and external systems.
    #[serde(default)]
    pub path: String,
    /// Longer description.
    #[serde(default)]
    pub description: String,
    /// Short type caption under the label, e.g. "axum", "PostgreSQL".
    #[serde(default)]
    pub caption: String,
}

impl From<proto::DiagramNode> for DiagramNode {
    fn from(node: proto::DiagramNode) -> Self {
        Self {
            id: node.id,
            label: node.label,
            kind: node.kind,
            group_id: node.group_id,
            path: node.path,
            description: node.description,
            caption: node.caption,
        }
    }
}

impl From<DiagramNode> for proto::DiagramNode {
    fn from(node: DiagramNode) -> Self {
        Self {
            id: node.id,
            label: node.label,
            kind: node.kind,
            group_id: node.group_id,
            path: node.path,
            description: node.description,
            caption: node.caption,
        }
    }
}

/// A relationship between two nodes.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagramEdge {
    /// Edge id.
    #[serde(default)]
    pub id: String,
    /// Source node id.
    pub source: String,
    /// Target node id.
    pub target: String,
    /// 1–3 words, e.g. "calls", "reads", "publishes".
    #[serde(default)]
    pub label: String,
    /// `call` | `data` | `async` | `returns` | `contains`.
    #[serde(default)]
    pub style: String,
    /// Inferred rather than observed; drawn dashed.
    #[serde(default)]
    pub dashed: bool,
    /// File that shows the relationship; empty when inferred.
    #[serde(default)]
    pub evidence_path: String,
}

impl From<proto::DiagramEdge> for DiagramEdge {
    fn from(edge: proto::DiagramEdge) -> Self {
        Self {
            id: edge.id,
            source: edge.source,
            target: edge.target,
            label: edge.label,
            style: edge.style,
            dashed: edge.dashed,
            evidence_path: edge.evidence_path,
        }
    }
}

impl From<DiagramEdge> for proto::DiagramEdge {
    fn from(edge: DiagramEdge) -> Self {
        Self {
            id: edge.id,
            source: edge.source,
            target: edge.target,
            label: edge.label,
            style: edge.style,
            dashed: edge.dashed,
            evidence_path: edge.evidence_path,
        }
    }
}

/// The diagram's groups, nodes and edges: the single source of truth for rendering.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagramSpec {
    /// `LR` or `TB`.
    #[serde(default = "default_direction")]
    pub direction: String,
    #[serde(default)]
    pub groups: Vec<DiagramGroup>,
    #[serde(default)]
    pub nodes: Vec<DiagramNode>,
    #[serde(default)]
    pub edges: Vec<DiagramEdge>,
}

fn default_direction() -> String {
    "LR".to_owned()
}

impl From<proto::DiagramSpec> for DiagramSpec {
    fn from(spec: proto::DiagramSpec) -> Self {
        Self {
            direction: spec.direction,
            groups: spec.groups.into_iter().map(Into::into).collect(),
            nodes: spec.nodes.into_iter().map(Into::into).collect(),
            edges: spec.edges.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<DiagramSpec> for proto::DiagramSpec {
    fn from(spec: DiagramSpec) -> Self {
        Self {
            direction: spec.direction,
            groups: spec.groups.into_iter().map(Into::into).collect(),
            nodes: spec.nodes.into_iter().map(Into::into).collect(),
            edges: spec.edges.into_iter().map(Into::into).collect(),
        }
    }
}

/// A generated or edited diagram.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Diagram {
    /// Diagram id.
    pub id: String,
    /// Project it belongs to.
    pub project_id: String,
    /// Kind.
    pub kind: DiagramKind,
    /// Title.
    pub title: String,
    /// 60–150 words: what the diagram shows and its evidence limits.
    pub explanation: String,
    /// The spec. `null` in list responses.
    pub spec: Option<DiagramSpec>,
    /// Mermaid rendered from the spec, or the user's edited source. `null` in list responses.
    pub mermaid: Option<String>,
    /// `generated` | `edited` | `mermaid` (the Mermaid source is authoritative).
    pub source: String,
    /// Commit the diagram was generated from.
    pub commit_sha: String,
    /// Bumped on every edit.
    pub revision: u32,
    /// `heuristic` | `claude`.
    pub reasoner: String,
    /// When it was generated.
    pub created_at: DateTime<Utc>,
    /// When it was last edited.
    pub updated_at: DateTime<Utc>,
}

impl From<proto::Diagram> for Diagram {
    fn from(diagram: proto::Diagram) -> Self {
        Self {
            id: diagram.id,
            project_id: diagram.project_id,
            kind: diagram.kind.into(),
            title: diagram.title,
            explanation: diagram.explanation,
            spec: diagram.spec.map(Into::into),
            mermaid: if diagram.mermaid.is_empty() {
                None
            } else {
                Some(diagram.mermaid)
            },
            source: diagram.source,
            commit_sha: diagram.commit_sha,
            revision: diagram.revision,
            reasoner: diagram.reasoner,
            created_at: timestamp(diagram.created_at.as_ref()),
            updated_at: timestamp(diagram.updated_at.as_ref()),
        }
    }
}

/// Body of `POST /projects/{id}/diagrams`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateDiagramRequest {
    /// Kind of diagram to build.
    pub kind: DiagramKind,
    /// Free text for `custom` (a prompt, or Mermaid source to import), or a focus hint for other kinds.
    #[serde(default)]
    pub prompt: Option<String>,
}

/// Body of `PATCH /diagrams/{id}`: every field is optional, at least one must be set.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDiagramRequest {
    /// New title.
    #[serde(default)]
    pub title: Option<String>,
    /// Replaces the spec and regenerates the Mermaid (`source` becomes `edited`).
    #[serde(default)]
    pub spec: Option<DiagramSpec>,
    /// Replaces the Mermaid source (`source` becomes `mermaid`); the spec is kept as is.
    #[serde(default)]
    pub mermaid: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_shape_hides_spec_and_mermaid() {
        let full = Diagram::from(proto::Diagram {
            id: "d1".into(),
            kind: proto::DiagramKind::DataFlow as i32,
            spec: Some(proto::DiagramSpec {
                direction: "LR".into(),
                ..Default::default()
            }),
            mermaid: "flowchart LR".into(),
            ..Default::default()
        });
        assert_eq!(full.kind, DiagramKind::DataFlow);
        assert!(full.spec.is_some());
        assert_eq!(full.mermaid.as_deref(), Some("flowchart LR"));
        let listed = Diagram::from(proto::Diagram::default());
        assert!(listed.spec.is_none());
        assert!(listed.mermaid.is_none());
        let json = serde_json::to_value(&listed).unwrap();
        assert!(json["spec"].is_null());
        assert_eq!(json["kind"], "unspecified");
        assert!(json.get("projectId").is_some());
    }

    #[test]
    fn spec_round_trips_and_tolerates_sparse_input() {
        let spec: DiagramSpec = serde_json::from_str(
            r#"{"nodes":[{"id":"a","label":"A","kind":"service"}],"edges":[{"source":"a","target":"a"}]}"#,
        )
        .unwrap();
        assert_eq!(spec.direction, "LR");
        let proto_spec: proto::DiagramSpec = spec.into();
        assert_eq!(proto_spec.nodes[0].group_id, "");
        let back: DiagramSpec = proto_spec.into();
        assert_eq!(back.edges[0].style, "");
        let json = serde_json::to_value(&back).unwrap();
        assert!(json["edges"][0].get("evidencePath").is_some());
    }
}
