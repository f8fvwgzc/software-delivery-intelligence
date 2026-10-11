//! The reasoner's narrow role on diagrams (CONTRACT.md §16): write the
//! explanation and, optionally, rename labels or move nodes between the
//! existing groups. It can never add nodes or edges: the reply is JSON and
//! only the whitelisted fields are applied, after validation.

use std::collections::BTreeMap;

use sdi_proto::intelligence::v1::DiagramKind;
use serde::Deserialize;

use super::Reasoner;
use crate::domain::diagrams::{Generated, kind_label, word_count};

pub const MAX_TOKENS: u32 = 1200;

pub const SYSTEM_PROMPT: &str = "You refine an automatically generated software architecture diagram for an engineer. \
You receive the diagram as JSON (groups, nodes with kind/label/caption/path, edges with their evidence file) and a \
templated explanation. Reply with ONE JSON object and nothing else: \
{\"explanation\": string, \"labels\": {nodeId: label}, \"groups\": {nodeId: groupId}, \"group_labels\": {groupId: label}}. \
The explanation is 60-150 words of plain text (no Markdown) that says what the diagram shows, which parts are backed by \
cited files and which edges are inferred (dashed), and what the sample may have missed; mention only components, \
paths and technologies present in the JSON. Labels are 2-4 words. Only rename or regroup when it makes the map clearer; \
otherwise leave those maps empty. Never invent nodes, edges, files or technologies; never add keys other than the four above.";

/// What the reasoner is shown.
pub struct RefineRequest {
    pub kind: DiagramKind,
    pub title: String,
    pub diagram: serde_json::Value,
    pub draft_explanation: String,
}

impl RefineRequest {
    pub fn new(kind: DiagramKind, generated: &Generated) -> Self {
        Self {
            kind,
            title: generated.title.clone(),
            diagram: serde_json::to_value(&generated.spec).unwrap_or_default(),
            draft_explanation: generated.explanation.clone(),
        }
    }

    pub fn user_message(&self) -> String {
        format!(
            "Diagram kind: {}\nTitle: {}\n\nDiagram (JSON):\n{}\n\nTemplated explanation:\n{}\n\nReply with the JSON object.",
            kind_label(self.kind),
            self.title,
            self.diagram,
            self.draft_explanation
        )
    }
}

/// The whitelisted reply. Unknown keys (`nodes`, `edges`, ...) are ignored.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Refinement {
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub groups: BTreeMap<String, String>,
    #[serde(default)]
    pub group_labels: BTreeMap<String, String>,
}

/// Extracts the JSON object from a reply that may wrap it in prose or a
/// code fence.
pub fn parse(reply: &str) -> Option<Refinement> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&reply[start..=end]).ok()
}

fn is_short_label(label: &str) -> bool {
    let words = word_count(label);
    (1..=4).contains(&words) && label.chars().count() <= 40 && !label.contains(['\n', '"', '`'])
}

/// Applies what passes validation. Returns whether the explanation was
/// taken from the reasoner, which is what makes the diagram "claude".
pub fn apply(generated: &mut Generated, refinement: Refinement) -> bool {
    let explanation = refinement.explanation.trim();
    let took_explanation = (60..=150).contains(&word_count(explanation))
        && !explanation.contains("```")
        && !explanation.contains('#');
    if took_explanation {
        generated.explanation = explanation.to_owned();
    }
    for (id, label) in &refinement.labels {
        let label = label.trim();
        if is_short_label(label)
            && let Some(node) = generated.spec.node_mut(id)
        {
            node.label = label.to_owned();
        }
    }
    for (id, group) in &refinement.groups {
        let group = group.trim();
        if generated.spec.group(group).is_some()
            && let Some(node) = generated.spec.node_mut(id)
        {
            node.group_id = group.to_owned();
        }
    }
    for (id, label) in &refinement.group_labels {
        let label = label.trim();
        if is_short_label(label)
            && let Some(group) = generated.spec.groups.iter_mut().find(|g| &g.id == id)
        {
            group.label = label.to_owned();
        }
    }
    generated.spec.prune_groups();
    took_explanation
}

/// Asks the reasoner and applies its validated reply; silent on failure.
pub async fn refine(reasoner: &dyn Reasoner, kind: DiagramKind, generated: &mut Generated) -> bool {
    let request = RefineRequest::new(kind, generated);
    let Some(reply) = reasoner.refine_diagram(&request).await else {
        return false;
    };
    let Some(refinement) = parse(&reply) else {
        tracing::warn!("diagram refinement was not JSON; keeping the templated text");
        return false;
    };
    apply(generated, refinement)
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::domain::diagrams::{
        DiagramSpec,
        model::{DiagramEdge, DiagramGroup, DiagramNode, kind, style},
    };

    fn generated() -> Generated {
        let mut spec = DiagramSpec::new("LR");
        spec.add_group(DiagramGroup {
            id: "g1".into(),
            label: "Services".into(),
            ..Default::default()
        });
        spec.add_group(DiagramGroup {
            id: "g2".into(),
            label: "Data".into(),
            ..Default::default()
        });
        spec.add_node(DiagramNode {
            id: "api".into(),
            label: "api".into(),
            kind: kind::SERVICE.into(),
            group_id: "g1".into(),
            ..Default::default()
        });
        spec.add_node(DiagramNode {
            id: "pg".into(),
            label: "PostgreSQL".into(),
            kind: kind::DATASTORE.into(),
            group_id: "g2".into(),
            ..Default::default()
        });
        spec.add_edge(DiagramEdge {
            source: "api".into(),
            target: "pg".into(),
            label: "reads".into(),
            style: style::DATA.into(),
            ..Default::default()
        });
        Generated {
            title: "t".into(),
            explanation: "draft".into(),
            spec,
        }
    }

    fn long_text() -> String {
        std::iter::repeat_n("word", 80)
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn applies_only_whitelisted_validated_fields() {
        let mut g = generated();
        let reply = format!(
            "Here you go:\n```json\n{{\"explanation\": \"{}\", \"labels\": {{\"api\": \"HTTP API\", \"ghost\": \"x\", \"pg\": \"\"}}, \
             \"groups\": {{\"pg\": \"g1\", \"api\": \"nope\"}}, \"group_labels\": {{\"g1\": \"Backend\"}}, \
             \"nodes\": [{{\"id\": \"evil\"}}], \"edges\": [{{\"source\": \"api\", \"target\": \"evil\"}}]}}\n```",
            long_text()
        );
        let refinement = parse(&reply).unwrap();
        assert!(apply(&mut g, refinement));
        assert_eq!(word_count(&g.explanation), 80);
        assert_eq!(g.spec.node("api").unwrap().label, "HTTP API");
        assert_eq!(g.spec.node("pg").unwrap().label, "PostgreSQL");
        assert_eq!(g.spec.node("pg").unwrap().group_id, "g1");
        assert_eq!(g.spec.node("api").unwrap().group_id, "g1");
        assert_eq!(g.spec.groups.len(), 1, "empty Data group pruned");
        assert_eq!(g.spec.groups[0].label, "Backend");
        assert_eq!(g.spec.nodes.len(), 2);
        assert_eq!(g.spec.edges.len(), 1);
        g.spec.validate().unwrap();
    }

    #[test]
    fn short_or_markdown_explanations_are_ignored() {
        let mut g = generated();
        assert!(!apply(
            &mut g,
            parse("{\"explanation\": \"too short\"}").unwrap()
        ));
        assert_eq!(g.explanation, "draft");
        let md = format!("{{\"explanation\": \"# Title {}\"}}", long_text());
        assert!(!apply(&mut g, parse(&md).unwrap()));
        assert!(parse("no json here").is_none());
        assert!(parse("{ broken").is_none());
    }

    struct Scripted(Option<String>);

    #[async_trait]
    impl Reasoner for Scripted {
        fn name(&self) -> &'static str {
            "claude"
        }
        async fn write_reply(&self, draft: &str, _: &serde_json::Value) -> String {
            draft.to_owned()
        }
        async fn refine_diagram(&self, _: &RefineRequest) -> Option<String> {
            self.0.clone()
        }
    }

    #[tokio::test]
    async fn refine_is_silent_when_the_reasoner_declines_or_babbles() {
        let mut g = generated();
        assert!(
            !refine(
                &super::super::HeuristicReasoner,
                DiagramKind::Architecture,
                &mut g
            )
            .await
        );
        assert!(!refine(&Scripted(None), DiagramKind::Architecture, &mut g).await);
        assert!(
            !refine(
                &Scripted(Some("sure!".into())),
                DiagramKind::Architecture,
                &mut g
            )
            .await
        );
        let ok = format!("{{\"explanation\": \"{}\"}}", long_text());
        assert!(refine(&Scripted(Some(ok)), DiagramKind::Architecture, &mut g).await);
        let request = RefineRequest::new(DiagramKind::DataFlow, &g);
        assert!(request.user_message().contains("data flow"));
        assert!(request.user_message().contains("\"pg\""));
    }
}
