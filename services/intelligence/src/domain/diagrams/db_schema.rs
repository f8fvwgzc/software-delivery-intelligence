//! Database schema: tables and their columns read from the sampled
//! migrations and ORM models (SQL `CREATE TABLE`, Prisma models,
//! SQLAlchemy / Django models), with foreign keys as edges.

use std::{collections::BTreeMap, sync::LazyLock};

use regex::Regex;

use super::{
    DiagramInput,
    catalog::{Catalog, engine_display, humanize_dir, node_id},
    model::{DiagramEdge, DiagramGroup, DiagramNode, DiagramSpec, kind, style},
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Table {
    pub name: String,
    pub columns: Vec<String>,
    /// Referenced table names.
    pub references: Vec<String>,
    pub path: String,
}

static CREATE_TABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?[`"']?(?:\w+\.)?(\w+)[`"']?\s*\((.*?)\)\s*;"#)
        .expect("create table regex")
});
static COLUMN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*[`"']?([a-zA-Z_]\w*)[`"']?\s+[a-zA-Z]"#).expect("column regex")
});
static REFERENCES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)REFERENCES\s+[`"']?(?:\w+\.)?(\w+)[`"']?"#).expect("references regex")
});
static PRISMA_MODEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)model\s+(\w+)\s*\{(.*?)\}").expect("prisma regex"));
static PRISMA_FIELD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*(\w+)\s+(\w+)(\[\])?\??").expect("prisma field regex"));
static PY_TABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"__tablename__\s*=\s*['"](\w+)['"]|class\s+(\w+)\s*\([^)]*models\.Model[^)]*\)"#)
        .expect("py table regex")
});
static PY_COLUMN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s+(\w+)\s*(?::\s*Mapped\[[^\]]*\])?\s*=\s*(?:Column|mapped_column|models\.\w+)\(",
    )
    .expect("py column regex")
});
static PY_FK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"ForeignKey\(\s*['"](\w+)(?:\.\w+)?['"]|ForeignKey\(\s*(\w+)\s*,"#)
        .expect("py fk regex")
});

const SQL_KEYWORDS: [&str; 12] = [
    "primary",
    "constraint",
    "foreign",
    "unique",
    "index",
    "check",
    "key",
    "create",
    "references",
    "on",
    "not",
    "default",
];

/// Tables found in one file.
pub fn tables_in(path: &str, text: &str) -> Vec<Table> {
    let mut tables = Vec::new();
    for cap in CREATE_TABLE.captures_iter(text) {
        let name = cap[1].to_lowercase();
        let body = &cap[2];
        let columns: Vec<String> = COLUMN
            .captures_iter(body)
            .map(|c| c[1].to_lowercase())
            .filter(|c| !SQL_KEYWORDS.contains(&c.as_str()))
            .collect();
        let references: Vec<String> = REFERENCES
            .captures_iter(body)
            .map(|c| c[1].to_lowercase())
            .filter(|r| r != &name)
            .collect();
        tables.push(Table {
            name,
            columns: dedupe(columns),
            references: dedupe(references),
            path: path.to_owned(),
        });
    }
    if path.ends_with(".prisma") {
        for cap in PRISMA_MODEL.captures_iter(text) {
            let name = cap[1].to_lowercase();
            let body = &cap[2];
            let mut columns = Vec::new();
            let mut references = Vec::new();
            for field in PRISMA_FIELD.captures_iter(body) {
                let field_name = field[1].to_lowercase();
                let field_type = field[2].to_string();
                if field_name.starts_with("@@") {
                    continue;
                }
                let scalar = matches!(
                    field_type.as_str(),
                    "String"
                        | "Int"
                        | "BigInt"
                        | "Float"
                        | "Decimal"
                        | "Boolean"
                        | "DateTime"
                        | "Json"
                        | "Bytes"
                );
                if scalar {
                    columns.push(field_name);
                } else {
                    references.push(field_type.to_lowercase());
                }
            }
            tables.push(Table {
                name,
                columns: dedupe(columns),
                references: dedupe(references),
                path: path.to_owned(),
            });
        }
    }
    if path.ends_with(".py") {
        let mut starts: Vec<(usize, String)> = PY_TABLE
            .captures_iter(text)
            .map(|c| {
                let name = c
                    .get(1)
                    .or_else(|| c.get(2))
                    .map(|m| m.as_str())
                    .unwrap_or("");
                (
                    c.get(0).map(|m| m.start()).unwrap_or(0),
                    name.to_lowercase(),
                )
            })
            .collect();
        starts.sort();
        for (i, (start, name)) in starts.iter().enumerate() {
            let end = starts.get(i + 1).map(|(s, _)| *s).unwrap_or(text.len());
            let body = &text[*start..end];
            let columns: Vec<String> = PY_COLUMN
                .captures_iter(body)
                .map(|c| c[1].to_lowercase())
                .collect();
            let references: Vec<String> = PY_FK
                .captures_iter(body)
                .map(|c| {
                    c.get(1)
                        .or_else(|| c.get(2))
                        .map(|m| m.as_str().to_lowercase())
                        .unwrap_or_default()
                })
                .filter(|r| !r.is_empty() && r != name)
                .collect();
            tables.push(Table {
                name: name.clone(),
                columns: dedupe(columns),
                references: dedupe(references),
                path: path.to_owned(),
            });
        }
    }
    tables
}

fn dedupe(items: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let catalog = Catalog::new(input);
    let mut spec = DiagramSpec::new("LR");

    // Tables merged by name across migrations (later files add columns).
    let mut tables: BTreeMap<String, Table> = BTreeMap::new();
    let mut paths: Vec<&String> = input.files.keys().collect();
    paths.sort();
    for path in paths {
        for table in tables_in(path, &input.files[path]) {
            let entry = tables.entry(table.name.clone()).or_insert_with(|| Table {
                name: table.name.clone(),
                path: table.path.clone(),
                ..Default::default()
            });
            for column in table.columns {
                if !entry.columns.contains(&column) {
                    entry.columns.push(column);
                }
            }
            for reference in table.references {
                if !entry.references.contains(&reference) {
                    entry.references.push(reference);
                }
            }
        }
    }

    // Group per migration/model directory, captioned with the datastore engine.
    let engine = catalog
        .nodes()
        .find(|n| {
            n.kind() == sdi_proto::knowledge::v1::NodeKind::Datastore && !Catalog::is_queue(n)
        })
        .map(|n| engine_display(&Catalog::engine_of(n)))
        .unwrap_or_else(|| "SQL".to_owned());
    for table in tables.values() {
        let dir = table.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let group_id = if dir.is_empty() {
            "root".to_owned()
        } else {
            format!("dir_{}", node_id(dir))
        };
        spec.add_group(DiagramGroup {
            id: group_id.clone(),
            label: humanize_dir(dir),
            description: format!(
                "Tables defined under {}",
                if dir.is_empty() { "the root" } else { dir }
            ),
            path: dir.to_owned(),
        });
        let shown: Vec<&str> = table.columns.iter().take(6).map(String::as_str).collect();
        let more = table.columns.len().saturating_sub(shown.len());
        let description = if more > 0 {
            format!("{} (+{more} more)", shown.join(", "))
        } else {
            shown.join(", ")
        };
        spec.add_node(DiagramNode {
            id: format!("table_{}", node_id(&table.name)),
            label: table.name.replace('_', " "),
            kind: kind::DATASTORE.to_owned(),
            group_id,
            path: if !catalog.has_tree() || catalog.file_exists(&table.path) {
                table.path.clone()
            } else {
                String::new()
            },
            description,
            caption: format!("{engine} · {} columns", table.columns.len()),
        });
    }
    for table in tables.values() {
        for reference in &table.references {
            spec.add_edge(DiagramEdge {
                id: String::new(),
                source: format!("table_{}", node_id(&table.name)),
                target: format!("table_{}", node_id(reference)),
                label: "references".to_owned(),
                style: style::DATA.to_owned(),
                dashed: false,
                evidence_path: table.path.clone(),
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
        assert_eq!(views.description, "id, name, owner_id, created_at");
        assert_eq!(views.caption, "PostgreSQL · 4 columns");
        assert_eq!(views.path, "services/api/migrations/0001_init.sql");
        assert_eq!(spec.groups.len(), 1);
        assert_eq!(spec.groups[0].path, "services/api/migrations");
        let fks: Vec<(&str, &str)> = spec
            .edges
            .iter()
            .map(|e| (e.source.as_str(), e.target.as_str()))
            .collect();
        assert!(fks.contains(&("table_views", "table_users")));
        assert!(fks.contains(&("table_sessions", "table_users")));
        assert!(
            spec.edges
                .iter()
                .all(|e| !e.dashed && !e.evidence_path.is_empty())
        );
    }

    #[test]
    fn prisma_and_python_models_are_read_too() {
        let prisma = "model User {\n  id Int @id\n  email String\n  posts Post[]\n}\nmodel Post {\n  id Int @id\n  title String\n  author User @relation(fields: [authorId], references: [id])\n  authorId Int\n}\n";
        let tables = tables_in("prisma/schema.prisma", prisma);
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[1].name, "post");
        assert_eq!(tables[1].columns, vec!["id", "title", "authorid"]);
        assert_eq!(tables[1].references, vec!["user"]);

        let python = "class User(Base):\n    __tablename__ = 'users'\n    id = Column(Integer, primary_key=True)\n    email: Mapped[str] = mapped_column(String)\n\nclass Order(models.Model):\n    total = models.DecimalField()\n    user = models.ForeignKey(User, on_delete=models.CASCADE)\n";
        let tables = tables_in("app/models.py", python);
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].name, "users");
        assert_eq!(tables[0].columns, vec!["id", "email"]);
        assert_eq!(tables[1].name, "order");
        assert_eq!(tables[1].columns, vec!["total", "user"]);
        assert_eq!(tables[1].references, vec!["user"]);
    }
}
