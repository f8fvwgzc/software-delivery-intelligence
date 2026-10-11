//! Spec → Mermaid (CONTRACT.md §16): `flowchart LR|TB`, one `subgraph` per
//! group, shapes by node kind, `-.->` for dashed edges, a `click` directive
//! for every node with a path and a `%% evidence:` comment per edge.

use std::collections::{HashMap, HashSet};

use super::model::{DiagramNode, DiagramSpec};

/// Mermaid source for a spec. `project_id` builds the code-browser links.
pub fn render(spec: &DiagramSpec, project_id: &str) -> String {
    let ids = Ids::new(spec);
    let direction = if spec.direction == "TB" { "TB" } else { "LR" };
    let mut out = format!("flowchart {direction}\n");

    let grouped: HashSet<&str> = spec.groups.iter().map(|g| g.id.as_str()).collect();
    for group in &spec.groups {
        out.push_str(&format!(
            "    subgraph {}[\"{}\"]\n",
            ids.group(&group.id),
            escape(&group.label)
        ));
        for node in spec.nodes.iter().filter(|n| n.group_id == group.id) {
            out.push_str("        ");
            out.push_str(&declare(node, &ids));
            out.push('\n');
        }
        out.push_str("    end\n");
    }
    for node in spec
        .nodes
        .iter()
        .filter(|n| !grouped.contains(n.group_id.as_str()))
    {
        out.push_str("    ");
        out.push_str(&declare(node, &ids));
        out.push('\n');
    }

    for edge in &spec.edges {
        if !spec.has_node(&edge.source) || !spec.has_node(&edge.target) {
            continue;
        }
        if edge.evidence_path.is_empty() {
            out.push_str("    %% evidence: none (inferred)\n");
        } else {
            out.push_str(&format!(
                "    %% evidence: {}\n",
                comment_safe(&edge.evidence_path)
            ));
        }
        let arrow = if edge.dashed { "-.->" } else { "-->" };
        let label = edge.label.trim();
        if label.is_empty() {
            out.push_str(&format!(
                "    {} {arrow} {}\n",
                ids.node(&edge.source),
                ids.node(&edge.target)
            ));
        } else {
            out.push_str(&format!(
                "    {} {arrow}|\"{}\"| {}\n",
                ids.node(&edge.source),
                escape(label),
                ids.node(&edge.target)
            ));
        }
    }

    for node in spec.nodes.iter().filter(|n| !n.path.is_empty()) {
        out.push_str(&format!(
            "    click {} \"/projects/{}/code/{}\" \"{}\"\n",
            ids.node(&node.id),
            url_path(project_id),
            url_path(&node.path),
            escape(&node.path)
        ));
    }
    out
}

fn declare(node: &DiagramNode, ids: &Ids) -> String {
    let mut text = escape(&node.label);
    if !node.caption.trim().is_empty() {
        text.push_str("<br/><small>");
        text.push_str(&escape(node.caption.trim()));
        text.push_str("</small>");
    }
    let id = ids.node(&node.id);
    match node.kind.as_str() {
        "datastore" => format!("{id}[(\"{text}\")]"),
        "actor" => format!("{id}([\"{text}\"])"),
        "infra" => format!("{id}[[\"{text}\"]]"),
        "external" => format!("{id}>\"{text}\"]"),
        "queue" => format!("{id}[/\"{text}\"/]"),
        "job" => format!("{id}{{{{\"{text}\"}}}}"),
        _ => format!("{id}[\"{text}\"]"),
    }
}

/// Unique, Mermaid-safe identifiers for nodes and groups: letters, digits
/// and underscores only, prefixed so a node and a group never collide and
/// no id is a Mermaid keyword.
struct Ids {
    nodes: HashMap<String, String>,
    groups: HashMap<String, String>,
}

impl Ids {
    fn new(spec: &DiagramSpec) -> Self {
        let mut taken = HashSet::new();
        let mut nodes = HashMap::new();
        for node in &spec.nodes {
            let id = unique(&format!("n_{}", sanitize(&node.id)), &mut taken);
            nodes.insert(node.id.clone(), id);
        }
        let mut groups = HashMap::new();
        for group in &spec.groups {
            let id = unique(&format!("g_{}", sanitize(&group.id)), &mut taken);
            groups.insert(group.id.clone(), id);
        }
        Self { nodes, groups }
    }

    fn node(&self, id: &str) -> &str {
        self.nodes
            .get(id)
            .map(String::as_str)
            .unwrap_or("n_unknown")
    }

    fn group(&self, id: &str) -> &str {
        self.groups
            .get(id)
            .map(String::as_str)
            .unwrap_or("g_unknown")
    }
}

fn unique(base: &str, taken: &mut HashSet<String>) -> String {
    let mut candidate = base.to_owned();
    let mut n = 2;
    while !taken.insert(candidate.clone()) {
        candidate = format!("{base}_{n}");
        n += 1;
    }
    candidate
}

pub fn sanitize(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for c in id.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "x".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Label text inside `"..."`: Mermaid's own entity escapes, no raw quotes
/// or line breaks.
pub fn escape(text: &str) -> String {
    text.replace('"', "#quot;")
        .replace('`', "#96;")
        .replace(['\n', '\r'], " ")
}

fn comment_safe(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

/// Repository paths are shown as they are except for the characters that
/// would end the quoted href.
fn url_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            segment
                .chars()
                .map(|c| match c {
                    '"' => "%22".to_owned(),
                    ' ' => "%20".to_owned(),
                    '#' => "%23".to_owned(),
                    '?' => "%3F".to_owned(),
                    _ => c.to_string(),
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Diagram types Mermaid renders; only flowcharts get the structural check.
const OTHER_HEADERS: [&str; 14] = [
    "sequenceDiagram",
    "classDiagram",
    "stateDiagram",
    "erDiagram",
    "gantt",
    "pie",
    "journey",
    "gitGraph",
    "mindmap",
    "timeline",
    "quadrantChart",
    "C4Context",
    "C4Container",
    "block-beta",
];

/// A structural check of Mermaid source: a known diagram header and, for
/// flowcharts, balanced `subgraph`/`end` and every edge endpoint declared
/// somewhere. Used by the tests and by `UpdateDiagram` to refuse obviously
/// broken edits early. Front matter and `%%{init}%%` directives are skipped.
pub fn check(source: &str) -> Result<(), String> {
    let mut lines = source
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("%%"))
        .peekable();
    // YAML front matter (`---` ... `---`) carries the title and config.
    if lines.peek() == Some(&"---") {
        lines.next();
        for line in lines.by_ref() {
            if line == "---" {
                break;
            }
        }
    }
    let header = lines.next().unwrap_or("");
    if OTHER_HEADERS.iter().any(|h| {
        header == *h || header.starts_with(&format!("{h} ")) || header.starts_with(&format!("{h}-"))
    }) {
        return Ok(());
    }
    if !(header.starts_with("flowchart ") || header.starts_with("graph ") || header == "flowchart")
    {
        return Err(format!("expected a Mermaid diagram header, got {header:?}"));
    }
    let mut depth = 0i32;
    let mut declared: HashSet<String> = HashSet::new();
    let mut endpoints: Vec<String> = Vec::new();
    for line in lines {
        if line.starts_with("click ")
            || line.starts_with("style ")
            || line.starts_with("classDef ")
            || line.starts_with("class ")
            || line.starts_with("linkStyle ")
            || line.starts_with("direction ")
        {
            continue;
        }
        if let Some(rest) = line.strip_prefix("subgraph ") {
            depth += 1;
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            declared.insert(id);
            continue;
        }
        if line == "end" {
            depth -= 1;
            if depth < 0 {
                return Err("`end` without a subgraph".to_owned());
            }
            continue;
        }
        if let Some((source, target)) = split_edge(line) {
            // `a[A] --> b[B]` declares its endpoints inline, as Mermaid allows.
            for (id, shaped) in [&source, &target] {
                if *shaped {
                    declared.insert(id.clone());
                }
            }
            endpoints.push(source.0);
            endpoints.push(target.0);
        } else {
            let id: String = line
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !id.is_empty() {
                declared.insert(id);
            }
        }
    }
    if depth != 0 {
        return Err(format!("{depth} subgraph(s) without end"));
    }
    for endpoint in endpoints {
        if !declared.contains(&endpoint) {
            return Err(format!("edge endpoint {endpoint} is never declared"));
        }
    }
    Ok(())
}

/// The two endpoint ids of an edge line and whether each carries an inline
/// shape (`a[A]`), which declares it.
#[allow(clippy::type_complexity)]
fn split_edge(line: &str) -> Option<((String, bool), (String, bool))> {
    let arrow = ["-.->", "-->", "---", "==>", "-.-"]
        .iter()
        .filter_map(|a| line.find(a).map(|i| (i, *a)))
        .min_by_key(|(i, _)| *i)?;
    let source = endpoint(line[..arrow.0].trim());
    let mut rest = &line[arrow.0 + arrow.1.len()..];
    if let Some(stripped) = rest.trim_start().strip_prefix('|') {
        let close = stripped.find('|')?;
        rest = &stripped[close + 1..];
    }
    let target = endpoint(rest.trim());
    if source.0.is_empty() || target.0.is_empty() {
        return None;
    }
    Some((source, target))
}

fn endpoint(text: &str) -> (String, bool) {
    let id: String = text
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let shaped = text[id.len()..]
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '[' | '(' | '{' | '>'));
    (id, shaped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagrams::model::{DiagramEdge, DiagramGroup, kind, style};

    fn fixture() -> DiagramSpec {
        let mut spec = DiagramSpec::new("LR");
        spec.add_group(DiagramGroup {
            id: "dir_services".into(),
            label: "Services".into(),
            path: "services".into(),
            ..Default::default()
        });
        spec.add_group(DiagramGroup {
            id: "data".into(),
            label: "Data \"stores\"".into(),
            ..Default::default()
        });
        spec.add_node(DiagramNode {
            id: "service:api".into(),
            label: "api".into(),
            kind: kind::SERVICE.into(),
            group_id: "dir_services".into(),
            path: "services/api/src/main.rs".into(),
            caption: "axum".into(),
            ..Default::default()
        });
        spec.add_node(DiagramNode {
            id: "service api".into(),
            label: "api \"two\"".into(),
            kind: kind::UI.into(),
            group_id: "dir_services".into(),
            path: "web/src/main.tsx".into(),
            ..Default::default()
        });
        spec.add_node(DiagramNode {
            id: "pg".into(),
            label: "PostgreSQL".into(),
            kind: kind::DATASTORE.into(),
            group_id: "data".into(),
            path: "docker-compose.yml".into(),
            ..Default::default()
        });
        spec.add_node(DiagramNode {
            id: "users".into(),
            label: "Users".into(),
            kind: kind::ACTOR.into(),
            ..Default::default()
        });
        spec.add_node(DiagramNode {
            id: "stripe".into(),
            label: "Stripe".into(),
            kind: kind::EXTERNAL.into(),
            group_id: "nope".into(),
            ..Default::default()
        });
        spec.add_node(DiagramNode {
            id: "img".into(),
            label: "api image".into(),
            kind: kind::INFRA.into(),
            ..Default::default()
        });
        spec.add_edge(DiagramEdge {
            source: "service:api".into(),
            target: "pg".into(),
            label: "reads/writes".into(),
            style: style::DATA.into(),
            evidence_path: "services/api/src/main.rs".into(),
            ..Default::default()
        });
        spec.add_edge(DiagramEdge {
            source: "users".into(),
            target: "service api".into(),
            label: "uses".into(),
            style: style::CALL.into(),
            dashed: true,
            ..Default::default()
        });
        spec.add_edge(DiagramEdge {
            source: "service:api".into(),
            target: "stripe".into(),
            label: String::new(),
            style: style::CALL.into(),
            evidence_path: "services/api/Cargo.toml".into(),
            ..Default::default()
        });
        spec
    }

    #[test]
    fn renders_per_contract_and_parses_structurally() {
        let source = render(&fixture(), "proj-1");
        check(&source).unwrap();
        assert!(source.starts_with("flowchart LR\n"));
        assert_eq!(source.matches("subgraph ").count(), 2);
        assert_eq!(source.matches("\n    end\n").count(), 2);
        // Shapes by kind.
        assert!(source.contains("n_pg[(\"PostgreSQL\")]"));
        assert!(source.contains("n_users([\"Users\"])"));
        assert!(source.contains("n_img[[\"api image\"]]"));
        assert!(source.contains("n_stripe>\"Stripe\"]"));
        assert!(source.contains("n_service_api[\"api<br/><small>axum</small>\"]"));
        // Ids are sanitised and kept unique; labels are escaped.
        assert!(source.contains("n_service_api_2[\"api #quot;two#quot;\"]"));
        assert!(source.contains("subgraph g_data[\"Data #quot;stores#quot;\"]"));
        // Dashed and labelled edges, with their evidence comments.
        assert!(source.contains(
            "%% evidence: services/api/src/main.rs\n    n_service_api -->|\"reads/writes\"| n_pg"
        ));
        assert!(
            source.contains(
                "%% evidence: none (inferred)\n    n_users -.->|\"uses\"| n_service_api_2"
            )
        );
        assert!(source.contains("n_service_api --> n_stripe"));
        // Click directives only for nodes with a path.
        assert!(
            source
                .contains("click n_service_api \"/projects/proj-1/code/services/api/src/main.rs\"")
        );
        assert!(source.contains("click n_pg \"/projects/proj-1/code/docker-compose.yml\""));
        assert!(!source.contains("click n_users"));
        assert!(!source.contains("click n_stripe"));
        // A node in an unknown group is declared at the top level.
        assert!(source.contains("\n    n_stripe>"));
    }

    #[test]
    fn direction_tb_and_empty_spec() {
        let spec = DiagramSpec::new("TB");
        let source = render(&spec, "p");
        assert_eq!(source, "flowchart TB\n");
        check(&source).unwrap();
        let odd = DiagramSpec::new("sideways");
        assert!(render(&odd, "p").starts_with("flowchart LR"));
    }

    #[test]
    fn check_rejects_broken_sources() {
        assert!(check("sequenceDiagram\n  A->>B: hi").is_ok());
        assert!(check("not a diagram\n  A->>B: hi").is_err());
        assert!(check("").is_err());
        assert!(check("flowchart LR\n  subgraph a\n  x[X]\n").is_err());
        assert!(check("flowchart LR\n  end\n").is_err());
        assert!(check("flowchart LR\n  a --> b\n").is_err());
        check("flowchart TB\n  a[A] --> b[B]\n  b --> c(C)\n").unwrap();
        assert!(check("flowchart TB\n  a[A] --> b\n").is_err());
        check("flowchart LR\n  a[A]\n  b[B]\n  a -->|calls| b\n  a -.-> b\n").unwrap();
        check("graph TD\n  subgraph s[S]\n    a[A]\n  end\n  b[B]\n  a --> b\n  click a \"/x\"\n")
            .unwrap();
        check("---\ntitle: T\n---\n%%{init: {}}%%\nflowchart TB\n  a[A]\n").unwrap();
    }
}
