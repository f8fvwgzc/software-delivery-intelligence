//! The final database schema from every migration and ORM model in the
//! sample: SQL `CREATE TABLE` / `ALTER TABLE ADD|DROP|RENAME COLUMN` /
//! `RENAME TO` / `DROP TABLE`, Prisma models, SQLAlchemy and Django models,
//! Diesel `table!` macros, sea-orm entities, TypeORM entities and
//! ActiveRecord migrations. Files are applied in path order, so numbered
//! migrations replay chronologically and the result is the schema as it
//! stands after the last one.

use std::{collections::BTreeMap, sync::LazyLock};

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Column {
    pub name: String,
    pub kind: String,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    /// `(table, column)` the column references, when it is a foreign key.
    pub references: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ForeignKey {
    pub column: String,
    pub table: String,
    pub target_column: String,
    /// File that declared it.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Table {
    pub name: String,
    pub schema: String,
    pub columns: Vec<Column>,
    pub foreign_keys: Vec<ForeignKey>,
    /// File that created the table.
    pub path: String,
}

impl Table {
    #[cfg(test)]
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    fn column_mut(&mut self, name: &str) -> Option<&mut Column> {
        self.columns.iter_mut().find(|c| c.name == name)
    }

    fn add_column(&mut self, column: Column) {
        if let Some(existing) = self.column_mut(&column.name) {
            // A re-declaration (IF NOT EXISTS, or a model repeating a
            // migration) keeps the richer description.
            if existing.kind.is_empty() {
                existing.kind = column.kind;
            }
            existing.primary_key |= column.primary_key;
            existing.not_null |= column.not_null;
            existing.unique |= column.unique;
            if existing.references.is_none() {
                existing.references = column.references;
            }
        } else {
            self.columns.push(column);
        }
    }

    fn add_foreign_key(&mut self, fk: ForeignKey) {
        if let Some(column) = self.column_mut(&fk.column)
            && column.references.is_none()
        {
            column.references = Some((fk.table.clone(), fk.target_column.clone()));
        }
        if !self
            .foreign_keys
            .iter()
            .any(|f| f.column == fk.column && f.table == fk.table)
        {
            self.foreign_keys.push(fk);
        }
    }
}

/// The schema after every file has been applied.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Schema {
    pub tables: BTreeMap<String, Table>,
}

impl Schema {
    /// Applies `files` (path → text) in path order.
    pub fn from_files<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut ordered: Vec<(&str, &str)> = files.into_iter().collect();
        ordered.sort_by(|a, b| a.0.cmp(b.0));
        let mut schema = Schema::default();
        for (path, text) in ordered {
            schema.apply(path, text);
        }
        schema
    }

    pub fn apply(&mut self, path: &str, text: &str) {
        let lower = path.to_lowercase();
        if lower.ends_with(".sql") {
            self.apply_sql(path, text);
        } else if lower.ends_with(".prisma") {
            self.apply_prisma(path, text);
        } else if lower.ends_with(".py") {
            self.apply_python(path, text);
        } else if lower.ends_with(".rs") {
            self.apply_rust(path, text);
            // sqlx migrations embedded in Rust and `sql!` strings are rare; raw SQL in
            // Rust string literals is still picked up below.
            if text.contains("CREATE TABLE") || text.contains("create table") {
                self.apply_sql(path, text);
            }
        } else if lower.ends_with(".ts") || lower.ends_with(".js") {
            self.apply_typeorm(path, text);
            if text.contains("CREATE TABLE") || text.contains("create table") {
                self.apply_sql(path, text);
            }
        } else if lower.ends_with(".rb") {
            self.apply_active_record(path, text);
        }
    }

    fn table_mut(&mut self, name: &str, path: &str) -> &mut Table {
        let (schema, bare) = split_schema(name);
        self.tables.entry(bare.clone()).or_insert_with(|| Table {
            name: bare,
            schema,
            path: path.to_owned(),
            ..Default::default()
        })
    }

    // -- SQL ------------------------------------------------------------------

    fn apply_sql(&mut self, path: &str, text: &str) {
        for statement in split_statements(&strip_sql_comments(text)) {
            let head = statement.trim_start().to_uppercase();
            if head.starts_with("CREATE TABLE")
                || head.starts_with("CREATE UNLOGGED TABLE")
                || head.starts_with("CREATE TEMP")
            {
                self.create_table(path, &statement);
            } else if head.starts_with("ALTER TABLE") {
                self.alter_table(path, &statement);
            } else if head.starts_with("DROP TABLE") {
                static DROP: LazyLock<Regex> = LazyLock::new(|| {
                    Regex::new(r#"(?i)DROP\s+TABLE\s+(?:IF\s+EXISTS\s+)?([`"\[]?[\w.]+[`"\]]?)"#)
                        .expect("drop table")
                });
                if let Some(cap) = DROP.captures(&statement) {
                    let (_, bare) = split_schema(&unquote(&cap[1]));
                    self.tables.remove(&bare);
                }
            }
        }
    }

    fn create_table(&mut self, path: &str, statement: &str) {
        static NAME: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?is)CREATE\s+(?:UNLOGGED\s+|TEMP(?:ORARY)?\s+)?TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?([`"\[]?[\w.]+[`"\]]?(?:\.[`"\[]?\w+[`"\]]?)?)\s*\("#)
                .expect("create table")
        });
        let Some(cap) = NAME.captures(statement) else {
            return;
        };
        let name = unquote(&cap[1]);
        let open = cap.get(0).map(|m| m.end() - 1).unwrap_or(0);
        let Some(body) = balanced(statement, open) else {
            return;
        };
        let table_path = path.to_owned();
        let table = self.table_mut(&name, path);
        if table.path.is_empty() {
            table.path = table_path.clone();
        }
        let mut pending_fks: Vec<ForeignKey> = Vec::new();
        let mut pending_pk: Vec<String> = Vec::new();
        let mut pending_unique: Vec<String> = Vec::new();
        for item in split_top_level(body, ',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            let upper = item.to_uppercase();
            let constraint = upper.starts_with("CONSTRAINT")
                || upper.starts_with("PRIMARY KEY")
                || upper.starts_with("FOREIGN KEY")
                || upper.starts_with("UNIQUE")
                || upper.starts_with("CHECK")
                || upper.starts_with("INDEX")
                || upper.starts_with("KEY ")
                || upper.starts_with("EXCLUDE")
                || upper.starts_with("LIKE ");
            if constraint {
                if let Some(cols) = constraint_columns(item, "PRIMARY KEY") {
                    pending_pk.extend(cols);
                }
                if let Some(cols) = constraint_columns(item, "UNIQUE") {
                    pending_unique.extend(cols);
                }
                if let Some(fk) = foreign_key_clause(item, &table_path) {
                    pending_fks.push(fk);
                }
                continue;
            }
            if let Some(column) = parse_column(item, &table_path) {
                if let Some((fk_table, fk_column)) = &column.references {
                    pending_fks.push(ForeignKey {
                        column: column.name.clone(),
                        table: fk_table.clone(),
                        target_column: fk_column.clone(),
                        path: table_path.clone(),
                    });
                }
                table.add_column(column);
            }
        }
        for name in pending_pk {
            if let Some(column) = table.column_mut(&name) {
                column.primary_key = true;
                column.not_null = true;
            }
        }
        for name in pending_unique {
            if let Some(column) = table.column_mut(&name) {
                column.unique = true;
            }
        }
        for fk in pending_fks {
            table.add_foreign_key(fk);
        }
    }

    fn alter_table(&mut self, path: &str, statement: &str) {
        static ALTER: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?is)ALTER\s+TABLE\s+(?:ONLY\s+)?(?:IF\s+EXISTS\s+)?([`"\[]?[\w.]+[`"\]]?)\s+(.*)"#)
                .expect("alter table")
        });
        let Some(cap) = ALTER.captures(statement) else {
            return;
        };
        let name = unquote(&cap[1]);
        let (_, bare) = split_schema(&name);
        let actions = cap[2].to_owned();
        // RENAME TO: the table moves under a new name.
        static RENAME_TABLE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?is)^\s*RENAME\s+TO\s+([`"\[]?[\w.]+[`"\]]?)"#).expect("rename table")
        });
        if let Some(r) = RENAME_TABLE.captures(&actions) {
            let new_name = split_schema(&unquote(&r[1])).1;
            if let Some(mut table) = self.tables.remove(&bare) {
                table.name = new_name.clone();
                self.tables.insert(new_name.clone(), table);
                for table in self.tables.values_mut() {
                    for fk in &mut table.foreign_keys {
                        if fk.table == bare {
                            fk.table = new_name.clone();
                        }
                    }
                    for column in &mut table.columns {
                        if let Some((t, _)) = &mut column.references
                            && *t == bare
                        {
                            *t = new_name.clone();
                        }
                    }
                }
            }
            return;
        }
        if !self.tables.contains_key(&bare) {
            // Altering a table the sample never created: create a stub so
            // its columns still show.
            self.table_mut(&bare, path);
        }
        for action in split_top_level(&actions, ',') {
            let action = action.trim();
            let table_path = path.to_owned();
            let Some(table) = self.tables.get_mut(&bare) else {
                return;
            };
            if let Some(rest) =
                strip_prefix_ci(action, "ADD COLUMN").or_else(|| strip_prefix_ci(action, "ADD"))
            {
                let rest = strip_prefix_ci(rest, "IF NOT EXISTS")
                    .unwrap_or(rest)
                    .trim();
                let rest_upper = rest.to_uppercase();
                if rest_upper.starts_with("CONSTRAINT")
                    || rest_upper.starts_with("FOREIGN KEY")
                    || rest_upper.starts_with("PRIMARY KEY")
                    || rest_upper.starts_with("UNIQUE")
                {
                    if let Some(fk) = foreign_key_clause(rest, &table_path) {
                        table.add_foreign_key(fk);
                    }
                    if let Some(cols) = constraint_columns(rest, "PRIMARY KEY") {
                        for c in cols {
                            if let Some(column) = table.column_mut(&c) {
                                column.primary_key = true;
                            }
                        }
                    }
                    if let Some(cols) = constraint_columns(rest, "UNIQUE") {
                        for c in cols {
                            if let Some(column) = table.column_mut(&c) {
                                column.unique = true;
                            }
                        }
                    }
                } else if let Some(column) = parse_column(rest, &table_path) {
                    let fk = column.references.clone().map(|(t, c)| ForeignKey {
                        column: column.name.clone(),
                        table: t,
                        target_column: c,
                        path: table_path.clone(),
                    });
                    table.add_column(column);
                    if let Some(fk) = fk {
                        table.add_foreign_key(fk);
                    }
                }
            } else if let Some(rest) = drop_column_rest(action) {
                let rest = strip_prefix_ci(rest, "IF EXISTS").unwrap_or(rest);
                let column = first_identifier(rest);
                table.columns.retain(|c| c.name != column);
                table.foreign_keys.retain(|f| f.column != column);
            } else if let Some(rest) = strip_prefix_ci(action, "RENAME COLUMN")
                .or_else(|| strip_prefix_ci(action, "RENAME"))
            {
                static RENAME: LazyLock<Regex> = LazyLock::new(|| {
                    Regex::new(r#"(?is)^\s*([`"\[]?\w+[`"\]]?)\s+TO\s+([`"\[]?\w+[`"\]]?)"#)
                        .expect("rename column")
                });
                if let Some(r) = RENAME.captures(rest) {
                    let (from, to) = (unquote(&r[1]), unquote(&r[2]));
                    if let Some(column) = table.column_mut(&from) {
                        column.name = to.clone();
                    }
                    for fk in &mut table.foreign_keys {
                        if fk.column == from {
                            fk.column = to.clone();
                        }
                    }
                }
            } else if let Some(rest) = strip_prefix_ci(action, "ALTER COLUMN")
                .or_else(|| strip_prefix_ci(action, "MODIFY COLUMN"))
                .or_else(|| strip_prefix_ci(action, "MODIFY"))
            {
                let column_name = first_identifier(rest);
                let rest_upper = rest.to_uppercase();
                if let Some(column) = table.column_mut(&column_name) {
                    if rest_upper.contains("SET NOT NULL") {
                        column.not_null = true;
                    }
                    if rest_upper.contains("DROP NOT NULL") {
                        column.not_null = false;
                    }
                    static TYPE: LazyLock<Regex> = LazyLock::new(|| {
                        Regex::new(
                            r"(?is)(?:SET\s+DATA\s+)?TYPE\s+([\w\[\]().,\s]+?)(?:\s+USING|\s*$)",
                        )
                        .expect("type")
                    });
                    if let Some(t) = TYPE.captures(rest) {
                        column.kind = normalise_type(t[1].trim());
                    }
                }
            }
        }
    }

    // -- Prisma ---------------------------------------------------------------

    fn apply_prisma(&mut self, path: &str, text: &str) {
        static MODEL: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?s)\bmodel\s+(\w+)\s*\{(.*?)\n\}").expect("prisma model")
        });
        static FIELD: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?m)^[ \t]*(\w+)[ \t]+(\w+)(\[\])?(\?)?[ \t]*(.*)$").expect("prisma field")
        });
        static RELATION: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"@relation\([^)]*fields:\s*\[(\w+)\][^)]*references:\s*\[(\w+)\]")
                .expect("relation")
        });
        static MAP: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r#"@@map\("(\w+)"\)"#).expect("map"));
        let models: Vec<(String, String)> = MODEL
            .captures_iter(text)
            .map(|c| (c[1].to_owned(), c[2].to_owned()))
            .collect();
        let table_names: BTreeMap<String, String> = models
            .iter()
            .map(|(name, body)| {
                let mapped = MAP
                    .captures(body)
                    .map(|m| m[1].to_owned())
                    .unwrap_or_else(|| name.to_lowercase());
                (name.clone(), mapped)
            })
            .collect();
        for (name, body) in &models {
            let table_name = table_names[name].clone();
            let mut fks: Vec<ForeignKey> = Vec::new();
            let mut columns: Vec<Column> = Vec::new();
            for field in FIELD.captures_iter(body) {
                let field_name = field[1].to_owned();
                if field_name.starts_with("@@") || field_name == "@@map" {
                    continue;
                }
                let field_type = field[2].to_owned();
                let list = field.get(3).is_some();
                let optional = field.get(4).is_some();
                let attrs = field.get(5).map(|m| m.as_str()).unwrap_or("");
                if let Some(rel) = RELATION.captures(attrs) {
                    fks.push(ForeignKey {
                        column: rel[1].to_owned(),
                        table: table_names
                            .get(&field_type)
                            .cloned()
                            .unwrap_or_else(|| field_type.to_lowercase()),
                        target_column: rel[2].to_owned(),
                        path: path.to_owned(),
                    });
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
                        | "Unsupported"
                ) || !table_names.contains_key(&field_type);
                if !scalar || list && table_names.contains_key(&field_type) {
                    continue;
                }
                columns.push(Column {
                    name: field_name,
                    kind: if list {
                        format!("{field_type}[]")
                    } else {
                        field_type.clone()
                    }
                    .to_lowercase(),
                    primary_key: attrs.contains("@id"),
                    not_null: !optional,
                    unique: attrs.contains("@unique") || attrs.contains("@id"),
                    references: None,
                });
            }
            let table = self.table_mut(&table_name, path);
            for column in columns {
                table.add_column(column);
            }
            for fk in fks {
                table.add_foreign_key(fk);
            }
        }
    }

    // -- SQLAlchemy / Django --------------------------------------------------

    fn apply_python(&mut self, path: &str, text: &str) {
        static CLASS: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?m)^class\s+(\w+)\s*\(([^)]*)\)\s*:").expect("py class")
        });
        static TABLENAME: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"__tablename__\s*=\s*['"](\w+)['"]"#).expect("tablename")
        });
        static COLUMN: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?m)^\s+(\w+)\s*(?::\s*Mapped\[([^\]]*)\])?\s*=\s*(?:(?:sa\.|db\.|sqlalchemy\.)?(Column|mapped_column|relationship)|models\.(\w+))\(")
                .expect("py column")
        });
        static FK: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"ForeignKey\(\s*['"](\w+)(?:\.(\w+))?['"]|ForeignKey\(\s*(?:to\s*=\s*)?['"]?([A-Z]\w*)['"]?\s*[,)]"#)
                .expect("py fk")
        });
        let classes: Vec<(usize, String, String)> = CLASS
            .captures_iter(text)
            .map(|c| (c.get(0).unwrap().start(), c[1].to_owned(), c[2].to_owned()))
            .collect();
        for (i, (start, class_name, bases)) in classes.iter().enumerate() {
            let end = classes.get(i + 1).map(|(s, _, _)| *s).unwrap_or(text.len());
            let body = &text[*start..end];
            let django = bases.contains("models.Model") || bases.contains("Model)");
            let table_name = TABLENAME
                .captures(body)
                .map(|c| c[1].to_owned())
                .or_else(|| django.then(|| class_name.to_lowercase()));
            let Some(table_name) = table_name else {
                continue;
            };
            let mut columns: Vec<Column> = Vec::new();
            let mut fks: Vec<ForeignKey> = Vec::new();
            for line in body.lines() {
                let Some(cap) = COLUMN.captures(line) else {
                    continue;
                };
                let name = cap[1].to_owned();
                let mapped = cap.get(2).map(|m| m.as_str().to_owned());
                let sa_kind = cap.get(3).map(|m| m.as_str()).unwrap_or("");
                let django_field = cap.get(4).map(|m| m.as_str()).unwrap_or("");
                if sa_kind == "relationship"
                    || matches!(django_field, "ManyToManyField" | "OneToManyField")
                {
                    continue;
                }
                let args: String = line
                    .split_once('(')
                    .map(|(_, a)| a.to_owned())
                    .unwrap_or_default();
                let lower = args.to_lowercase();
                let fk = FK.captures(line).map(|f| {
                    if let Some(t) = f.get(1) {
                        (
                            t.as_str().to_owned(),
                            f.get(2)
                                .map(|m| m.as_str().to_owned())
                                .unwrap_or_else(|| "id".to_owned()),
                        )
                    } else {
                        (
                            f.get(3)
                                .map(|m| m.as_str().to_lowercase())
                                .unwrap_or_default(),
                            "id".to_owned(),
                        )
                    }
                });
                let column_name = if django
                    && (django_field == "ForeignKey" || django_field == "OneToOneField")
                {
                    format!("{name}_id")
                } else {
                    name
                };
                let kind = if !django_field.is_empty() {
                    django_type(django_field)
                } else if let Some(m) = mapped {
                    m.replace("Optional[", "").replace(']', "").to_lowercase()
                } else {
                    args.split(',')
                        .next()
                        .map(|t| t.trim().trim_end_matches(')').to_lowercase())
                        .filter(|t| !t.starts_with("foreignkey"))
                        .unwrap_or_default()
                };
                let primary = lower.contains("primary_key=true");
                columns.push(Column {
                    name: column_name.clone(),
                    kind,
                    primary_key: primary,
                    not_null: primary
                        || lower.contains("nullable=false")
                        || (django && !lower.contains("null=true")),
                    unique: lower.contains("unique=true") || primary,
                    references: fk.clone(),
                });
                if let Some((table, target_column)) = fk {
                    fks.push(ForeignKey {
                        column: column_name,
                        table,
                        target_column,
                        path: path.to_owned(),
                    });
                }
            }
            let table = self.table_mut(&table_name, path);
            for column in columns {
                table.add_column(column);
            }
            for fk in fks {
                table.add_foreign_key(fk);
            }
        }
    }

    // -- Diesel / sea-orm ----------------------------------------------------

    fn apply_rust(&mut self, path: &str, text: &str) {
        static DIESEL: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?s)\b(\w+)\s*\(\s*([\w,\s]+?)\s*\)\s*\{(.*?)\n\s*\}")
                .expect("diesel table")
        });
        static DIESEL_COLUMN: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?m)^\s*(\w+)\s*->\s*(Nullable<)?([\w<>]+)").expect("diesel col")
        });
        static JOINABLE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"joinable!\s*\(\s*(\w+)\s*->\s*(\w+)\s*\(\s*(\w+)\s*\)\s*\)")
                .expect("joinable")
        });
        if text.contains("table!") || text.contains("diesel::table!") {
            for cap in DIESEL.captures_iter(text) {
                let name = cap[1].to_owned();
                if matches!(
                    name.as_str(),
                    "table" | "joinable" | "allow_tables_to_appear_in_same_query"
                ) {
                    continue;
                }
                let pks: Vec<String> = cap[2].split(',').map(|s| s.trim().to_owned()).collect();
                let table = self.table_mut(&name, path);
                for col in DIESEL_COLUMN.captures_iter(&cap[3]) {
                    let col_name = col[1].to_owned();
                    table.add_column(Column {
                        primary_key: pks.contains(&col_name),
                        not_null: col.get(2).is_none(),
                        unique: pks.contains(&col_name),
                        kind: col[3].to_lowercase(),
                        name: col_name,
                        references: None,
                    });
                }
            }
            for cap in JOINABLE.captures_iter(text) {
                let table = self.table_mut(&cap[1], path);
                table.add_foreign_key(ForeignKey {
                    column: cap[3].to_owned(),
                    table: cap[2].to_owned(),
                    target_column: "id".to_owned(),
                    path: path.to_owned(),
                });
            }
        }
        static SEA_TABLE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r#"#\[sea_orm\(table_name\s*=\s*"(\w+)"\)\]\s*pub\s+struct\s+Model\s*\{([^}]*)\}"#,
            )
            .expect("sea table")
        });
        static SEA_FIELD: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r"(?m)^\s*(?:#\[sea_orm\(([^)]*)\)\]\s*)?pub\s+(\w+)\s*:\s*(Option<)?([\w:<>]+)",
            )
            .expect("sea field")
        });
        for cap in SEA_TABLE.captures_iter(text) {
            let table = self.table_mut(&cap[1], path);
            for field in SEA_FIELD.captures_iter(&cap[2]) {
                let attrs = field.get(1).map(|m| m.as_str()).unwrap_or("");
                table.add_column(Column {
                    name: field[2].to_owned(),
                    kind: field[4].rsplit("::").next().unwrap_or("").to_lowercase(),
                    primary_key: attrs.contains("primary_key"),
                    not_null: field.get(3).is_none(),
                    unique: attrs.contains("unique") || attrs.contains("primary_key"),
                    references: None,
                });
            }
        }
    }

    // -- TypeORM -------------------------------------------------------------

    fn apply_typeorm(&mut self, path: &str, text: &str) {
        static ENTITY: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?s)@Entity\((?:\s*['"](\w+)['"])?[^)]*\)\s*(?:export\s+)?class\s+(\w+)[^{]*\{(.*?)\n\}"#).expect("entity")
        });
        static TS_COLUMN: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?s)@(PrimaryGeneratedColumn|PrimaryColumn|Column|CreateDateColumn|UpdateDateColumn|ManyToOne|OneToOne)\(([^)]*)\)[^;\n]*?\n?\s*(?:@JoinColumn\([^)]*\)\s*)?(\w+)\s*[?!]?\s*:\s*([\w<>\[\]]+)")
                .expect("ts column")
        });
        for cap in ENTITY.captures_iter(text) {
            let class = cap[2].to_owned();
            let name = cap
                .get(1)
                .map(|m| m.as_str().to_owned())
                .unwrap_or_else(|| class.to_lowercase());
            let mut columns = Vec::new();
            let mut fks = Vec::new();
            for col in TS_COLUMN.captures_iter(&cap[3]) {
                let decorator = &col[1];
                let args = col[2].to_lowercase();
                let field = col[3].to_owned();
                let ts_type = col[4].to_owned();
                if decorator == "ManyToOne" || decorator == "OneToOne" {
                    let target = ts_type.trim_end_matches("[]").to_lowercase();
                    let column_name = format!("{field}_id");
                    columns.push(Column {
                        name: column_name.clone(),
                        kind: "uuid".to_owned(),
                        references: Some((target.clone(), "id".to_owned())),
                        ..Default::default()
                    });
                    fks.push(ForeignKey {
                        column: column_name,
                        table: target,
                        target_column: "id".to_owned(),
                        path: path.to_owned(),
                    });
                    continue;
                }
                let primary = decorator.starts_with("Primary");
                columns.push(Column {
                    name: field,
                    kind: ts_type.to_lowercase(),
                    primary_key: primary,
                    not_null: primary || !args.contains("nullable: true"),
                    unique: primary || args.contains("unique: true"),
                    references: None,
                });
            }
            let table = self.table_mut(&name, path);
            for column in columns {
                table.add_column(column);
            }
            for fk in fks {
                table.add_foreign_key(fk);
            }
        }
    }

    // -- ActiveRecord --------------------------------------------------------

    fn apply_active_record(&mut self, path: &str, text: &str) {
        static CREATE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?s)create_table\s+:(\w+)[^\n]*do\s*\|t\|(.*?)\n\s*end")
                .expect("ar create")
        });
        static FIELD: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?m)^\s*t\.(\w+)\s+:(\w+)(.*)$").expect("ar field"));
        static ADD: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"add_column\s+:(\w+),\s*:(\w+),\s*:(\w+)(.*)").expect("ar add")
        });
        static REMOVE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"remove_column\s+:(\w+),\s*:(\w+)").expect("ar remove"));
        static RENAME: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"rename_column\s+:(\w+),\s*:(\w+),\s*:(\w+)").expect("ar rename")
        });
        static DROP: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"drop_table\s+:(\w+)").expect("ar drop"));
        static ADD_REF: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"add_(?:reference|foreign_key)\s+:(\w+),\s*:(\w+)").expect("ar ref")
        });
        for cap in CREATE.captures_iter(text) {
            let name = cap[1].to_owned();
            let table = self.table_mut(&name, path);
            table.add_column(Column {
                name: "id".to_owned(),
                kind: "bigint".to_owned(),
                primary_key: true,
                not_null: true,
                unique: true,
                references: None,
            });
            for field in FIELD.captures_iter(&cap[2]) {
                let kind = field[1].to_owned();
                let col = field[2].to_owned();
                let opts = field[3].to_lowercase();
                if kind == "timestamps" {
                    continue;
                }
                if kind == "references" || kind == "belongs_to" {
                    let column = format!("{col}_id");
                    let target = plural(&col);
                    table.add_column(Column {
                        name: column.clone(),
                        kind: "bigint".to_owned(),
                        not_null: opts.contains("null: false"),
                        references: Some((target.clone(), "id".to_owned())),
                        ..Default::default()
                    });
                    table.add_foreign_key(ForeignKey {
                        column,
                        table: target,
                        target_column: "id".to_owned(),
                        path: path.to_owned(),
                    });
                    continue;
                }
                table.add_column(Column {
                    name: col,
                    kind,
                    primary_key: false,
                    not_null: opts.contains("null: false"),
                    unique: opts.contains("unique: true"),
                    references: None,
                });
            }
        }
        for cap in ADD.captures_iter(text) {
            let table = self.table_mut(&cap[1], path);
            let opts = cap[4].to_lowercase();
            table.add_column(Column {
                name: cap[2].to_owned(),
                kind: cap[3].to_owned(),
                not_null: opts.contains("null: false"),
                unique: opts.contains("unique: true"),
                ..Default::default()
            });
        }
        for cap in REMOVE.captures_iter(text) {
            if let Some(table) = self.tables.get_mut(&cap[1]) {
                table.columns.retain(|c| c.name != cap[2]);
            }
        }
        for cap in RENAME.captures_iter(text) {
            if let Some(table) = self.tables.get_mut(&cap[1])
                && let Some(column) = table.column_mut(&cap[2])
            {
                column.name = cap[3].to_owned();
            }
        }
        for cap in ADD_REF.captures_iter(text) {
            let target = plural(&cap[2]);
            let table = self.table_mut(&cap[1], path);
            table.add_foreign_key(ForeignKey {
                column: format!("{}_id", &cap[2]),
                table: target,
                target_column: "id".to_owned(),
                path: path.to_owned(),
            });
        }
        for cap in DROP.captures_iter(text) {
            self.tables.remove(&cap[1]);
        }
    }
}

// -- SQL helpers ---------------------------------------------------------------

/// The SQL type names a column, cut at the first constraint keyword.
const COLUMN_KEYWORDS: [&str; 16] = [
    "NOT",
    "NULL",
    "PRIMARY",
    "REFERENCES",
    "DEFAULT",
    "UNIQUE",
    "CHECK",
    "GENERATED",
    "COLLATE",
    "AUTOINCREMENT",
    "AUTO_INCREMENT",
    "CONSTRAINT",
    "COMMENT",
    "ON",
    "IDENTITY",
    "AS",
];

fn parse_column(item: &str, path: &str) -> Option<Column> {
    let item = item.trim();
    let name = first_identifier(item);
    if name.is_empty() || name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    let upper = name.to_uppercase();
    let rest = item[identifier_span(item)..].trim();
    // `KEY idx (a)` is an index; `key text PRIMARY KEY` is a column called key.
    if matches!(
        upper.as_str(),
        "CONSTRAINT" | "PRIMARY" | "FOREIGN" | "CHECK"
    ) || (matches!(upper.as_str(), "UNIQUE" | "INDEX" | "KEY")
        && (rest.starts_with('(')
            || rest.to_uppercase().starts_with("KEY")
            || rest.to_uppercase().starts_with("INDEX")))
    {
        return None;
    }
    // The type runs up to the first constraint keyword at depth 0.
    let mut kind_tokens: Vec<String> = Vec::new();
    for token in split_top_level(rest, ' ') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if COLUMN_KEYWORDS.contains(&token.to_uppercase().as_str()) {
            break;
        }
        kind_tokens.push(token.to_owned());
    }
    let kind = normalise_type(&kind_tokens.join(" "));
    let upper_rest = rest.to_uppercase();
    let primary = upper_rest.contains("PRIMARY KEY");
    let _ = path;
    Some(Column {
        name,
        kind,
        primary_key: primary,
        not_null: primary || upper_rest.contains("NOT NULL"),
        unique: primary || upper_rest.contains("UNIQUE"),
        references: inline_reference(rest),
    })
}

/// `REFERENCES other (id)` inside a column definition.
fn inline_reference(text: &str) -> Option<(String, String)> {
    static REF: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"(?i)REFERENCES\s+([`"\[]?[\w.]+[`"\]]?)\s*(?:\(\s*([`"\[]?\w+[`"\]]?)\s*\))?"#,
        )
        .expect("references")
    });
    let cap = REF.captures(text)?;
    let table = split_schema(&unquote(&cap[1])).1;
    let column = cap
        .get(2)
        .map(|m| unquote(m.as_str()))
        .unwrap_or_else(|| "id".to_owned());
    Some((table, column))
}

/// `FOREIGN KEY (a) REFERENCES t (b)` as a table constraint (possibly
/// named with `CONSTRAINT x`).
fn foreign_key_clause(text: &str, path: &str) -> Option<ForeignKey> {
    static FK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)FOREIGN\s+KEY\s*\(\s*([`"\[]?\w+[`"\]]?)[^)]*\)\s*REFERENCES\s+([`"\[]?[\w.]+[`"\]]?)\s*(?:\(\s*([`"\[]?\w+[`"\]]?)[^)]*\))?"#)
            .expect("foreign key")
    });
    let cap = FK.captures(text)?;
    Some(ForeignKey {
        column: unquote(&cap[1]),
        table: split_schema(&unquote(&cap[2])).1,
        target_column: cap
            .get(3)
            .map(|m| unquote(m.as_str()))
            .unwrap_or_else(|| "id".to_owned()),
        path: path.to_owned(),
    })
}

/// Columns of a `PRIMARY KEY (a, b)` / `UNIQUE (a)` constraint.
fn constraint_columns(text: &str, keyword: &str) -> Option<Vec<String>> {
    let upper = text.to_uppercase();
    let at = upper.find(keyword)?;
    let after = &text[at + keyword.len()..];
    let open = after.find('(')?;
    let body = balanced(after, open)?;
    Some(
        body.split(',')
            .map(|c| unquote(first_identifier(c.trim()).as_str()))
            .filter(|c| !c.is_empty())
            .collect(),
    )
}

/// The rest of a `DROP [COLUMN] x` action; constraint and index drops are not columns.
fn drop_column_rest(action: &str) -> Option<&str> {
    if let Some(rest) = strip_prefix_ci(action, "DROP COLUMN") {
        return Some(rest);
    }
    let upper = action.trim_start().to_uppercase();
    let other = [
        "DROP CONSTRAINT",
        "DROP INDEX",
        "DROP PRIMARY",
        "DROP FOREIGN",
        "DROP CHECK",
        "DROP KEY",
    ];
    if upper.starts_with("DROP ") && !other.iter().any(|p| upper.starts_with(p)) {
        strip_prefix_ci(action, "DROP")
    } else {
        None
    }
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let trimmed = text.trim_start();
    if trimmed.len() >= prefix.len() && trimmed[..prefix.len()].eq_ignore_ascii_case(prefix) {
        let rest = &trimmed[prefix.len()..];
        if rest.is_empty() || rest.starts_with(char::is_whitespace) || rest.starts_with('(') {
            return Some(rest);
        }
    }
    None
}

fn identifier_span(text: &str) -> usize {
    let text = text.trim_start();
    if let Some(close) = text.strip_prefix('"').and_then(|r| r.find('"')) {
        return close + 2;
    }
    if let Some(close) = text.strip_prefix('`').and_then(|r| r.find('`')) {
        return close + 2;
    }
    if let Some(close) = text.strip_prefix('[').and_then(|r| r.find(']')) {
        return close + 2;
    }
    text.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
        .unwrap_or(text.len())
}

fn first_identifier(text: &str) -> String {
    let text = text.trim_start();
    let span = identifier_span(text);
    unquote(&text[..span])
}

pub fn unquote(text: &str) -> String {
    text.trim()
        .trim_matches(|c| c == '"' || c == '`' || c == '[' || c == ']' || c == '\'')
        .to_lowercase()
}

fn split_schema(name: &str) -> (String, String) {
    let clean = unquote(name);
    match clean.rsplit_once('.') {
        Some((schema, bare)) => (schema.to_owned(), bare.to_owned()),
        None => (String::new(), clean),
    }
}

fn normalise_type(kind: &str) -> String {
    let lower = kind.trim().to_lowercase();
    lower.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn django_type(field: &str) -> String {
    match field {
        "CharField" | "SlugField" | "EmailField" | "URLField" => "varchar",
        "TextField" => "text",
        "IntegerField" | "PositiveIntegerField" | "SmallIntegerField" => "integer",
        "BigIntegerField" | "AutoField" | "BigAutoField" => "bigint",
        "BooleanField" => "boolean",
        "DateTimeField" => "timestamp",
        "DateField" => "date",
        "DecimalField" => "decimal",
        "FloatField" => "float",
        "JSONField" => "jsonb",
        "UUIDField" => "uuid",
        "ForeignKey" | "OneToOneField" => "bigint",
        other => return other.trim_end_matches("Field").to_lowercase(),
    }
    .to_owned()
}

fn plural(word: &str) -> String {
    if word.ends_with('s') {
        word.to_owned()
    } else if let Some(stem) = word.strip_suffix('y') {
        format!("{stem}ies")
    } else {
        format!("{word}s")
    }
}

/// Text inside the parentheses that open at `open`.
fn balanced(text: &str, open: usize) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut depth = 0;
    let mut in_quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match in_quote {
            Some(q) => {
                if b == q {
                    in_quote = None;
                }
            }
            None => match b {
                b'\'' | b'"' | b'`' => in_quote = Some(b),
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&text[open + 1..i]);
                    }
                }
                _ => {}
            },
        }
    }
    None
}

/// Splits on `sep` outside parentheses and quotes.
fn split_top_level(text: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut in_quote: Option<char> = None;
    for c in text.chars() {
        match in_quote {
            Some(q) => {
                current.push(c);
                if c == q {
                    in_quote = None;
                }
            }
            None => match c {
                '\'' | '"' | '`' => {
                    in_quote = Some(c);
                    current.push(c);
                }
                '(' => {
                    depth += 1;
                    current.push(c);
                }
                ')' => {
                    depth -= 1;
                    current.push(c);
                }
                c if c == sep && depth <= 0 => {
                    out.push(std::mem::take(&mut current));
                }
                c => current.push(c),
            },
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

fn split_statements(text: &str) -> Vec<String> {
    split_top_level(text, ';')
        .into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn strip_sql_comments(text: &str) -> String {
    static LINE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)--.*$").expect("sql line comment"));
    static BLOCK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").expect("sql block comment"));
    let no_block = BLOCK.replace_all(text, " ");
    LINE.replace_all(&no_block, " ").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_in_order_with_alters_and_drops() {
        let schema = Schema::from_files(
            crate::domain::diagrams::fixtures::schema_files()
                .iter()
                .map(|(p, t)| (p.as_str(), t.as_str())),
        );
        let names: Vec<&str> = schema.tables.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            vec![
                "projects",
                "run_events",
                "runs",
                "sessions",
                "tasks",
                "users"
            ],
            "{names:?}"
        );
        // Column added, column dropped, column renamed, table dropped.
        let tasks = &schema.tables["tasks"];
        assert!(tasks.column("latest_run_id").is_some(), "{tasks:?}");
        assert!(tasks.column("priority").is_none(), "{tasks:?}");
        assert!(tasks.column("title").is_none());
        assert!(tasks.column("name").is_some(), "{tasks:?}");
        assert!(!schema.tables.contains_key("agents"));
        // Types, keys and constraints.
        let runs = &schema.tables["runs"];
        let id = runs.column("id").unwrap();
        assert!(id.primary_key && id.not_null && id.unique);
        assert_eq!(id.kind, "uuid");
        let project = runs.column("project_id").unwrap();
        assert_eq!(
            project.references.as_ref().unwrap(),
            &("projects".to_owned(), "id".to_owned())
        );
        assert!(project.not_null);
        let status = runs.column("status").unwrap();
        assert_eq!(status.kind, "text");
        assert!(status.not_null);
        // `WITHOUT ROWID` and a composite primary key.
        let events = &schema.tables["run_events"];
        assert!(events.column("run_id").unwrap().primary_key);
        assert!(events.column("seq").unwrap().primary_key);
        assert_eq!(events.column("payload").unwrap().kind, "text");
        let fks: Vec<(&str, &str, &str)> = schema
            .tables
            .values()
            .flat_map(|t| {
                t.foreign_keys
                    .iter()
                    .map(move |f| (t.name.as_str(), f.table.as_str(), f.path.as_str()))
            })
            .collect();
        assert!(
            fks.contains(&("runs", "projects", "db/migrations/0001_init.sql")),
            "{fks:?}"
        );
        assert!(
            fks.contains(&("run_events", "runs", "db/migrations/0001_init.sql")),
            "{fks:?}"
        );
        assert!(
            fks.contains(&("sessions", "users", "db/migrations/0002_users.sql")),
            "{fks:?}"
        );
        // The ALTER TABLE ... ADD CONSTRAINT foreign key cites its own migration.
        assert!(
            fks.contains(&("tasks", "users", "db/migrations/0003_tasks_owner.sql")),
            "{fks:?}"
        );
        let users = &schema.tables["users"];
        assert!(users.column("email").unwrap().unique);
    }

    #[test]
    fn prisma_python_diesel_and_rails_models_are_read() {
        let prisma = "model User {\n  id Int @id @default(autoincrement())\n  email String @unique\n  posts Post[]\n}\n\nmodel Post {\n  id Int @id\n  title String\n  author User @relation(fields: [authorId], references: [id])\n  authorId Int\n  body String?\n}\n";
        let schema = Schema::from_files([("prisma/schema.prisma", prisma)]);
        let post = &schema.tables["post"];
        assert_eq!(
            post.columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["id", "title", "authorId", "body"]
        );
        assert!(post.column("id").unwrap().primary_key);
        assert!(!post.column("body").unwrap().not_null);
        assert_eq!(post.foreign_keys[0].table, "user");
        assert_eq!(post.foreign_keys[0].column, "authorId");
        assert!(schema.tables["user"].column("email").unwrap().unique);

        let python = "class User(Base):\n    __tablename__ = 'users'\n    id = Column(Integer, primary_key=True)\n    email: Mapped[str] = mapped_column(String, unique=True, nullable=False)\n\nclass Order(models.Model):\n    total = models.DecimalField(max_digits=8)\n    user = models.ForeignKey(User, on_delete=models.CASCADE)\n";
        let schema = Schema::from_files([("app/models.py", python)]);
        let users = &schema.tables["users"];
        assert_eq!(users.column("id").unwrap().kind, "integer");
        assert!(users.column("id").unwrap().primary_key);
        assert!(users.column("email").unwrap().unique);
        let order = &schema.tables["order"];
        assert_eq!(order.column("total").unwrap().kind, "decimal");
        assert_eq!(
            order
                .column("user_id")
                .unwrap()
                .references
                .as_ref()
                .unwrap()
                .0,
            "user"
        );

        let diesel = "diesel::table! {\n    users (id) {\n        id -> Integer,\n        name -> Text,\n    }\n}\n\ndiesel::table! {\n    posts (id) {\n        id -> Integer,\n        user_id -> Integer,\n        body -> Nullable<Text>,\n    }\n}\n\ndiesel::joinable!(posts -> users (user_id));\n";
        let schema = Schema::from_files([("src/schema.rs", diesel)]);
        assert!(schema.tables["users"].column("id").unwrap().primary_key);
        assert!(!schema.tables["posts"].column("body").unwrap().not_null);
        assert_eq!(schema.tables["posts"].foreign_keys[0].table, "users");

        let rails = "create_table :accounts do |t|\n  t.string :name, null: false\n  t.references :user, foreign_key: true\n  t.timestamps\nend\nadd_column :accounts, :plan, :string\nrename_column :accounts, :plan, :tier\n";
        let schema = Schema::from_files([("db/migrate/20240101_accounts.rb", rails)]);
        let accounts = &schema.tables["accounts"];
        assert!(accounts.column("id").unwrap().primary_key);
        assert!(accounts.column("tier").is_some());
        assert!(accounts.column("plan").is_none());
        assert_eq!(accounts.foreign_keys[0].table, "users");
    }

    #[test]
    fn sql_helpers_respect_parentheses_and_quotes() {
        assert_eq!(
            split_top_level("a, b(c, d), 'x,y'", ','),
            vec!["a", " b(c, d)", " 'x,y'"]
        );
        assert_eq!(balanced("t (a (b) c) d", 2), Some("a (b) c"));
        assert_eq!(first_identifier("\"Weird Name\" text"), "weird name");
        assert_eq!(unquote("`id`"), "id");
        let col = parse_column(
            "status TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('a', 'b'))",
            "m.sql",
        )
        .unwrap();
        assert_eq!(
            (col.name.as_str(), col.kind.as_str(), col.not_null),
            ("status", "text", true)
        );
        let col = parse_column(
            "owner_id uuid REFERENCES users (id) ON DELETE CASCADE",
            "m.sql",
        )
        .unwrap();
        assert_eq!(
            col.references.unwrap(),
            ("users".to_owned(), "id".to_owned())
        );
        let col = parse_column("amount numeric(10, 2) NOT NULL", "m.sql").unwrap();
        assert_eq!(col.kind, "numeric(10, 2)");
    }
}
