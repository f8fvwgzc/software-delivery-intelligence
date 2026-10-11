//! Database schema: every table with its columns, as the migrations and ORM
//! models leave it after being applied in order (CONTRACT.md §16). One node
//! per table (`datastore`, caption "N columns", one column per description
//! line), one edge per foreign key child → parent labelled "references"
//! and cited by the migration that declared it. Tables group by schema when
//! there is more than one.

use std::collections::BTreeSet;

use super::{
    DiagramInput, FileClass,
    catalog::{Catalog, node_id},
    classify,
    imports::parent_dir,
    model::{DiagramEdge, DiagramGroup, DiagramNode, DiagramSpec, kind, style},
    schema::{Column, Schema, Table},
};

/// Whether a sampled file feeds the schema.
pub fn is_schema_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".sql")
        || lower.ends_with(".prisma")
        || lower.ends_with("schema.rb")
        || matches!(
            classify(path),
            Some(FileClass::Migration | FileClass::Model)
        )
}

/// "name type [PK] [FK → table.column] [NOT NULL] [UNIQUE]".
pub fn column_line(column: &Column) -> String {
    let mut line = column.name.clone();
    if !column.kind.is_empty() {
        line.push(' ');
        line.push_str(&column.kind);
    }
    if column.primary_key {
        line.push_str(" [PK]");
    }
    if let Some((table, target)) = &column.references {
        if target.is_empty() {
            line.push_str(&format!(" [FK → {table}]"));
        } else {
            line.push_str(&format!(" [FK → {table}.{target}]"));
        }
    }
    if column.not_null && !column.primary_key {
        line.push_str(" [NOT NULL]");
    }
    if column.unique && !column.primary_key {
        line.push_str(" [UNIQUE]");
    }
    line
}

pub fn table_node_id(table: &Table) -> String {
    format!("table_{}", node_id(&table.name))
}

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let catalog = Catalog::new(input);
    let mut spec = DiagramSpec::new("LR");
    let schema = Schema::from_files(
        input
            .files
            .iter()
            .filter(|(path, _)| is_schema_file(path))
            .map(|(path, text)| (path.as_str(), text.as_str())),
    );

    let schemas: BTreeSet<&str> = schema.tables.values().map(|t| t.schema.as_str()).collect();
    let by_schema = schemas.len() > 1;
    let migrations_dir = schema
        .tables
        .values()
        .map(|t| parent_dir(&t.path).to_owned())
        .min()
        .unwrap_or_default();

    for table in schema.tables.values() {
        let group_id = if by_schema {
            let name = if table.schema.is_empty() {
                "public"
            } else {
                table.schema.as_str()
            };
            let id = format!("schema_{}", node_id(name));
            spec.add_group(DiagramGroup {
                id: id.clone(),
                label: name.to_owned(),
                description: format!("Tables in the {name} schema"),
                path: parent_dir(&table.path).to_owned(),
            });
            id
        } else {
            spec.add_group(DiagramGroup {
                id: "tables".to_owned(),
                label: "Tables".to_owned(),
                description: format!(
                    "Tables after applying the migrations under {}",
                    if migrations_dir.is_empty() {
                        "the root"
                    } else {
                        migrations_dir.as_str()
                    }
                ),
                path: migrations_dir.clone(),
            });
            "tables".to_owned()
        };
        let description = table
            .columns
            .iter()
            .map(column_line)
            .collect::<Vec<_>>()
            .join("\n");
        spec.add_node(DiagramNode {
            id: table_node_id(table),
            label: table.name.clone(),
            kind: kind::DATASTORE.to_owned(),
            group_id,
            path: if !catalog.has_tree() || catalog.file_exists(&table.path) {
                table.path.clone()
            } else {
                String::new()
            },
            description,
            caption: format!("{} columns", table.columns.len()),
        });
    }
    for table in schema.tables.values() {
        for fk in &table.foreign_keys {
            spec.add_edge(DiagramEdge {
                id: String::new(),
                source: table_node_id(table),
                target: format!("table_{}", node_id(&fk.table)),
                label: "references".to_owned(),
                style: style::DATA.to_owned(),
                dashed: false,
                evidence_path: fk.path.clone(),
            });
        }
    }
    spec.prune_groups();
    spec
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagrams::fixtures::{self, input, tree};

    #[test]
    fn sql_migrations_yield_tables_columns_and_foreign_keys() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        let names: Vec<&str> = spec.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(names, vec!["table_sessions", "table_users", "table_views"]);
        let views = spec.node("table_views").unwrap();
        assert_eq!(
            views.description,
            "id uuid [PK]\nname text [NOT NULL]\nowner_id uuid [FK → users.id] [NOT NULL]\ncreated_at timestamptz [NOT NULL]"
        );
        assert_eq!(views.caption, "4 columns");
        assert_eq!(views.label, "views");
        assert_eq!(views.path, "services/api/migrations/0001_init.sql");
        assert_eq!(spec.groups.len(), 1);
        assert_eq!(spec.groups[0].path, "services/api/migrations");
        let fks: Vec<(&str, &str, &str)> = spec
            .edges
            .iter()
            .map(|e| {
                (
                    e.source.as_str(),
                    e.target.as_str(),
                    e.evidence_path.as_str(),
                )
            })
            .collect();
        assert!(
            fks.contains(&(
                "table_views",
                "table_users",
                "services/api/migrations/0001_init.sql"
            )),
            "{fks:?}"
        );
        assert!(
            fks.contains(&(
                "table_sessions",
                "table_users",
                "services/api/migrations/0002_sessions.sql"
            )),
            "{fks:?}"
        );
        assert!(
            spec.edges
                .iter()
                .all(|e| !e.dashed && e.label == "references")
        );
    }

    #[test]
    fn six_table_fixture_applies_alters_and_drops() {
        let files = fixtures::schema_files();
        let tree = fixtures::schema_tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        let names: Vec<&str> = spec.nodes.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "projects",
                "run_events",
                "runs",
                "sessions",
                "tasks",
                "users"
            ]
        );
        assert!(!spec.has_node("table_agents"));
        let tasks = spec.node("table_tasks").unwrap();
        assert!(
            tasks.description.contains("latest_run_id uuid"),
            "{}",
            tasks.description
        );
        assert!(
            tasks.description.contains("name text [NOT NULL]"),
            "{}",
            tasks.description
        );
        assert!(
            !tasks.description.contains("priority"),
            "{}",
            tasks.description
        );
        assert!(
            tasks.description.contains("owner_id uuid [FK → users.id]"),
            "{}",
            tasks.description
        );
        assert_eq!(tasks.caption, "6 columns");
        let runs = spec.node("table_runs").unwrap();
        assert!(
            runs.description.starts_with("id uuid [PK]\n"),
            "{}",
            runs.description
        );
        let events = spec.node("table_run_events").unwrap();
        assert!(
            events
                .description
                .contains("run_id integer [PK] [FK → runs.id]"),
            "{}",
            events.description
        );
        let fks: Vec<(&str, &str, &str)> = spec
            .edges
            .iter()
            .map(|e| {
                (
                    e.source.as_str(),
                    e.target.as_str(),
                    e.evidence_path.as_str(),
                )
            })
            .collect();
        assert_eq!(fks.len(), 6, "{fks:?}");
        assert!(
            fks.contains(&(
                "table_tasks",
                "table_users",
                "db/migrations/0003_tasks_owner.sql"
            )),
            "{fks:?}"
        );
        assert!(
            fks.contains(&("table_runs", "table_tasks", "db/migrations/0001_init.sql")),
            "{fks:?}"
        );
        for node in &spec.nodes {
            assert!(tree.iter().any(|e| e.path == node.path), "{}", node.path);
        }
    }
}
