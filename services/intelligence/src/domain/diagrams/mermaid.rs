//! Spec → Mermaid (CONTRACT.md §16): `flowchart LR|TB`, one `subgraph` per
//! group, shapes by node kind, `-.->` for dashed edges, a `click` directive
//! for every node with a path and a `%% evidence:` comment per edge.

use std::collections::{HashMap, HashSet};

use sdi_proto::intelligence::v1::DiagramKind;

use super::model::{DiagramNode, DiagramSpec, kind, style};

/// Mermaid source for a spec of the given kind: `erDiagram` for the
/// database schema, `sequenceDiagram` for the sequence, a flowchart for
/// everything else (CONTRACT.md §16).
pub fn render_kind(kind: DiagramKind, spec: &DiagramSpec, project_id: &str) -> String {
    match kind {
        DiagramKind::DbSchema if !spec.nodes.is_empty() => render_er(spec),
        DiagramKind::Sequence if !spec.nodes.is_empty() => render_sequence(spec),
        _ => render(spec, project_id),
    }
}

/// `erDiagram`: one entity per table with typed attributes and PK/FK/UK
/// markers (read back from the node's description lines), and one
/// `child }o--|| parent : fk_column` relationship per foreign key.
pub fn render_er(spec: &DiagramSpec) -> String {
    let mut out = String::from("erDiagram\n");
    let mut taken = HashSet::new();
    let mut names: HashMap<&str, String> = HashMap::new();
    for node in &spec.nodes {
        let base = sanitize(if node.label.is_empty() {
            &node.id
        } else {
            &node.label
        });
        names.insert(node.id.as_str(), unique(&base, &mut taken));
    }
    for node in &spec.nodes {
        if !node.path.is_empty() {
            out.push_str(&format!(
                "    %% {}: {}\n",
                names[node.id.as_str()],
                comment_safe(&node.path)
            ));
        }
        out.push_str(&format!("    {} {{\n", names[node.id.as_str()]));
        for line in node.description.lines() {
            let Some(column) = parse_column_line(line) else {
                continue;
            };
            out.push_str("        ");
            out.push_str(&column.kind);
            out.push(' ');
            out.push_str(&column.name);
            if !column.keys.is_empty() {
                out.push(' ');
                out.push_str(&column.keys.join(", "));
            }
            if !column.comment.is_empty() {
                out.push_str(&format!(" \"{}\"", escape(&column.comment)));
            }
            out.push('\n');
        }
        out.push_str("    }\n");
    }
    for edge in &spec.edges {
        let (Some(child), Some(parent)) = (
            names.get(edge.source.as_str()),
            names.get(edge.target.as_str()),
        ) else {
            continue;
        };
        let column = spec
            .node(&edge.source)
            .and_then(|n| {
                fk_column(
                    &n.description,
                    &spec
                        .node(&edge.target)
                        .map(|p| p.label.clone())
                        .unwrap_or_default(),
                )
            })
            .unwrap_or_else(|| edge.label.clone());
        if !edge.evidence_path.is_empty() {
            out.push_str(&format!(
                "    %% evidence: {}\n",
                comment_safe(&edge.evidence_path)
            ));
        }
        let line = if edge.dashed { "}o..||" } else { "}o--||" };
        out.push_str(&format!(
            "    {child} {line} {parent} : {}\n",
            sanitize(&column)
        ));
    }
    out
}

struct ErColumn {
    name: String,
    kind: String,
    keys: Vec<&'static str>,
    comment: String,
}

/// "owner_id uuid [FK → users.id] [NOT NULL]" → type `uuid`, keys `FK`, comment "not null".
fn parse_column_line(line: &str) -> Option<ErColumn> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (head, flags) = match line.find('[') {
        Some(i) => (&line[..i], &line[i..]),
        None => (line, ""),
    };
    let mut parts = head.split_whitespace();
    let name = sanitize(parts.next()?);
    let kind_text = parts.collect::<Vec<_>>().join("_").replace(",_", ",");
    let kind = if kind_text.is_empty() {
        "unknown".to_owned()
    } else {
        kind_text
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '(' | ')' | '[' | ']') {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
    };
    let mut keys = Vec::new();
    let mut comment = Vec::new();
    if flags.contains("[PK]") {
        keys.push("PK");
    }
    if flags.contains("[FK") {
        keys.push("FK");
    }
    if flags.contains("[UNIQUE]") {
        keys.push("UK");
    }
    if flags.contains("[NOT NULL]") {
        comment.push("not null");
    }
    Some(ErColumn {
        name,
        kind,
        keys,
        comment: comment.join(", "),
    })
}

/// The column whose `[FK → parent.col]` names `parent`.
fn fk_column(description: &str, parent: &str) -> Option<String> {
    let needle = format!("[FK → {parent}");
    description
        .lines()
        .find(|l| l.contains(&needle))
        .and_then(|l| l.split_whitespace().next())
        .map(str::to_owned)
}

/// `sequenceDiagram`: participants in node order, messages in `edge.id`
/// order; calls as `->>`, returns as `-->>`, with a note naming the workflow.
pub fn render_sequence(spec: &DiagramSpec) -> String {
    let ids = Ids::new(spec);
    let mut out = String::from("sequenceDiagram\n");
    for node in &spec.nodes {
        let keyword = if node.kind == kind::ACTOR {
            "actor"
        } else {
            "participant"
        };
        let mut label = node.label.trim().to_owned();
        if !node.caption.trim().is_empty() && node.kind != kind::ACTOR {
            label.push_str(&format!(" ({})", node.caption.trim()));
        }
        out.push_str(&format!(
            "    {keyword} {} as {}\n",
            ids.node(&node.id),
            message_safe(&label)
        ));
    }
    if let (Some(first), Some(last)) = (spec.nodes.first(), spec.nodes.last()) {
        let workflow = spec
            .edges
            .iter()
            .find(|e| e.style != style::RETURNS)
            .map(|e| e.label.clone())
            .unwrap_or_else(|| "principal workflow".to_owned());
        out.push_str(&format!(
            "    Note over {},{}: Principal workflow: {}\n",
            ids.node(&first.id),
            ids.node(&last.id),
            message_safe(&workflow)
        ));
    }
    let mut edges: Vec<&super::model::DiagramEdge> = spec.edges.iter().collect();
    edges.sort_by(|a, b| a.id.cmp(&b.id));
    for edge in edges {
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
        let arrow = if edge.style == style::RETURNS {
            "-->>"
        } else {
            "->>"
        };
        out.push_str(&format!(
            "    {}{arrow}{}: {}\n",
            ids.node(&edge.source),
            ids.node(&edge.target),
            message_safe(edge.label.trim())
        ));
    }
    for node in spec.nodes.iter().filter(|n| !n.path.is_empty()) {
        out.push_str(&format!(
            "    %% {}: {}\n",
            ids.node(&node.id),
            comment_safe(&node.path)
        ));
    }
    out
}

/// Message and alias text: no line breaks or the characters that end a statement.
fn message_safe(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
        .replace(';', ",")
        .replace(':', " -")
}

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
    use crate::domain::diagrams::model::{DiagramEdge, DiagramGroup};

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
    fn er_and_sequence_renderers_follow_the_kind() {
        let mut er = DiagramSpec::new("LR");
        er.add_node(DiagramNode {
            id: "table_users".into(),
            label: "users".into(),
            kind: kind::DATASTORE.into(),
            path: "db/0001.sql".into(),
            description: "id uuid [PK]\nemail text [NOT NULL] [UNIQUE]".into(),
            caption: "2 columns".into(),
            ..Default::default()
        });
        er.add_node(DiagramNode {
            id: "table_views".into(),
            label: "views".into(),
            kind: kind::DATASTORE.into(),
            description:
                "id uuid [PK]\nowner_id uuid [FK → users.id] [NOT NULL]\namount numeric(10, 2)"
                    .into(),
            ..Default::default()
        });
        er.add_edge(DiagramEdge {
            source: "table_views".into(),
            target: "table_users".into(),
            label: "references".into(),
            style: style::DATA.into(),
            evidence_path: "db/0001.sql".into(),
            ..Default::default()
        });
        let source = render_kind(DiagramKind::DbSchema, &er, "p");
        check(&source).unwrap();
        assert!(source.starts_with("erDiagram\n"));
        assert!(
            source.contains(
                "    users {\n        uuid id PK\n        text email UK \"not null\"\n    }"
            ),
            "{source}"
        );
        assert!(source.contains("uuid owner_id FK \"not null\""), "{source}");
        assert!(source.contains("numeric(10_2) amount"), "{source}");
        assert!(source.contains("views }o--|| users : owner_id"), "{source}");
        // An empty schema falls back to a flowchart so the renderer has something to draw.
        assert!(
            render_kind(DiagramKind::DbSchema, &DiagramSpec::new("LR"), "p")
                .starts_with("flowchart")
        );

        let mut seq = DiagramSpec::new("TB");
        seq.add_node(DiagramNode {
            id: "actor_user".into(),
            label: "User".into(),
            kind: kind::ACTOR.into(),
            ..Default::default()
        });
        seq.add_node(DiagramNode {
            id: "service_api".into(),
            label: "api".into(),
            kind: kind::SERVICE.into(),
            caption: "axum".into(),
            path: "src/main.rs".into(),
            ..Default::default()
        });
        seq.add_node(DiagramNode {
            id: "pg".into(),
            label: "PostgreSQL".into(),
            kind: kind::DATASTORE.into(),
            ..Default::default()
        });
        for (id, s, t, l, st) in [
            ("m02", "service_api", "pg", "select rows", style::DATA),
            (
                "m01",
                "actor_user",
                "service_api",
                "GET /api/views",
                style::CALL,
            ),
            ("m03", "pg", "service_api", "rows", style::RETURNS),
            ("m04", "service_api", "actor_user", "200 OK", style::RETURNS),
        ] {
            seq.add_edge(DiagramEdge {
                id: id.into(),
                source: s.into(),
                target: t.into(),
                label: l.into(),
                style: st.into(),
                dashed: st == style::RETURNS,
                evidence_path: if st == style::RETURNS {
                    String::new()
                } else {
                    "src/main.rs".into()
                },
            });
        }
        let source = render_kind(DiagramKind::Sequence, &seq, "p");
        check(&source).unwrap();
        assert!(source.starts_with("sequenceDiagram\n    actor n_actor_user as User\n    participant n_service_api as api (axum)\n"), "{source}");
        assert!(
            source.contains("Note over n_actor_user,n_pg: Principal workflow"),
            "{source}"
        );
        let m1 = source
            .find("n_actor_user->>n_service_api: GET /api/views")
            .unwrap();
        let m2 = source.find("n_service_api->>n_pg: select rows").unwrap();
        let m3 = source.find("n_pg-->>n_service_api: rows").unwrap();
        let m4 = source
            .find("n_service_api-->>n_actor_user: 200 OK")
            .unwrap();
        assert!(m1 < m2 && m2 < m3 && m3 < m4, "{source}");
        assert!(render_kind(DiagramKind::Architecture, &seq, "p").starts_with("flowchart TB"));
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
