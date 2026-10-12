//! Schema migrations that cannot be rolled out without care: dropping
//! tables or columns, changing a column's type in place, and adding a
//! NOT NULL column without a default to a table that already exists.
//! SQL and the common ORM DSLs (Rails, Alembic, Knex, Prisma, Django).

use std::sync::LazyLock;

use regex::Regex;

use super::{Finding, Severity, evidence};
use crate::domain::review::input::{ChangedFile, ReviewInput};

static DROP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\bDROP\s+(?:TABLE|COLUMN)\b|\bdrop_table\b|\bremove_column\b|\bop\.drop_(?:table|column)\(|\.dropColumn\(|\.dropTable\(|\bDeleteModel\(|\bRemoveField\(",
    )
    .expect("drop pattern")
});

static ALTER_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\bALTER\s+(?:COLUMN\s+)?\S+\s+(?:SET\s+DATA\s+)?TYPE\b|\bMODIFY\s+COLUMN\b|\bchange_column\b|\bop\.alter_column\([^)]*type_\s*=|\.alterColumn\(|\bAlterField\(",
    )
    .expect("alter type pattern")
});

static NOT_NULL_ADD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\bADD\s+(?:COLUMN\s+)?(?:IF\s+NOT\s+EXISTS\s+)?\S+\s+[^,;]*\bNOT\s+NULL\b|\bALTER\s+(?:COLUMN\s+)?\S+\s+SET\s+NOT\s+NULL\b|\badd_column\b[^\n]*null:\s*false|\bop\.add_column\([^\n]*nullable\s*=\s*False|\.notNullable\(\)|\bAddField\([^\n]*null=False",
    )
    .expect("not null pattern")
});

static DEFAULT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bDEFAULT\b|\bdefault:|server_default\s*=|\.defaultTo\(|\bdefault\s*=")
        .expect("default pattern")
});

static CREATE_TABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bCREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?["`]?([\w.]+)["`]?|\bcreate_table\s*[:(]?\s*["':]?(\w+)|\.createTable\(\s*["'](\w+)|\bCreateModel\(\s*name\s*=\s*["'](\w+)"#)
        .expect("create table pattern")
});

static TARGET_TABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bALTER\s+TABLE\s+(?:IF\s+EXISTS\s+)?(?:ONLY\s+)?["`]?([\w.]+)["`]?|\badd_column\s*[:(]?\s*["':]?(\w+)|\bop\.add_column\(\s*["'](\w+)|\.(?:alterTable|table)\(\s*["'](\w+)|\bmodel_name\s*=\s*["'](\w+)"#)
        .expect("target table pattern")
});

pub fn is_migration(file: &ChangedFile) -> bool {
    let lower = file.path.to_ascii_lowercase();
    let name = file.file_name().to_ascii_lowercase();
    lower.contains("migrations/")
        || lower.contains("migrate/")
        || lower.contains("alembic/versions/")
        || lower.contains("/schema/")
        || lower.ends_with(".sql")
        || (name.starts_with('v') && name.contains("__") && name.ends_with(".sql"))
}

fn table_of(line: &str, captures: &Regex) -> Option<String> {
    captures
        .captures(line)
        .and_then(|c| (1..c.len()).find_map(|i| c.get(i)))
        .map(|m| {
            m.as_str()
                .trim_start_matches("public.")
                .to_ascii_lowercase()
        })
}

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in input
        .files
        .iter()
        .filter(|f| f.is_reviewable() && is_migration(f))
    {
        let lines = file.added_lines();
        // Tables this very change creates: a NOT NULL column on them is safe.
        let created: Vec<String> = lines
            .iter()
            .filter_map(|(_, l)| table_of(l, &CREATE_TABLE))
            .collect();
        let mut current_table: Option<String> = None;
        for (line_no, line) in &lines {
            let trimmed = line.trim_start();
            if trimmed.starts_with("--") || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }
            if let Some(t) = table_of(line, &TARGET_TABLE) {
                current_table = Some(t);
            }
            if DROP.is_match(line) {
                findings.push(Finding::new(
                    "migrations",
                    "drop",
                    Severity::High,
                    &file.path,
                    *line_no,
                    "Migration drops data",
                    format!(
                        "Line {line_no} of `{}` drops a table or column: {}. Running code may still read it and the \
                         data is gone after rollback; deploy the code that stops using it first, then drop in a \
                         later migration.",
                        file.path,
                        evidence(line)
                    ),
                ));
                continue;
            }
            if ALTER_TYPE.is_match(line) {
                findings.push(Finding::new(
                    "migrations",
                    "alter_type",
                    Severity::Medium,
                    &file.path,
                    *line_no,
                    "Column type changed in place",
                    format!(
                        "Line {line_no} of `{}` changes a column's type: {}. On a large table this rewrites every row \
                         under a lock and the old code may not read the new type; add a new column, backfill, then \
                         switch.",
                        file.path,
                        evidence(line)
                    ),
                ));
                continue;
            }
            if NOT_NULL_ADD.is_match(line) && !DEFAULT.is_match(line) {
                let on_new_table = current_table
                    .as_ref()
                    .is_some_and(|t| created.iter().any(|c| c == t));
                if on_new_table {
                    continue;
                }
                findings.push(Finding::new(
                    "migrations",
                    "not_null_without_default",
                    Severity::High,
                    &file.path,
                    *line_no,
                    "NOT NULL column added without a default",
                    format!(
                        "Line {line_no} of `{}` adds a NOT NULL column (or constraint) with no default: {}. The \
                         migration fails on any existing row, and inserts from running code fail until they set it; \
                         add a default or backfill first.",
                        file.path,
                        evidence(line)
                    ),
                ));
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;

    #[test]
    fn unsafe_sql_migrations() {
        let findings = analyze(&input(vec![added(
            "migrations/0007_users.sql",
            &[
                "-- DROP TABLE comment",
                "ALTER TABLE users ADD COLUMN email text NOT NULL;",
                "ALTER TABLE users ADD COLUMN plan text NOT NULL DEFAULT 'free';",
                "ALTER TABLE users ALTER COLUMN age TYPE bigint;",
                "ALTER TABLE users ALTER COLUMN name SET NOT NULL;",
                "DROP TABLE sessions;",
                "ALTER TABLE orders DROP COLUMN legacy_id;",
                "CREATE TABLE audit (id uuid PRIMARY KEY, actor text NOT NULL);",
                "ALTER TABLE audit ADD COLUMN detail text NOT NULL;",
            ],
        )]));
        let hits: Vec<(&str, u32)> = findings.iter().map(|f| (f.rule.as_str(), f.line)).collect();
        assert_eq!(
            hits,
            vec![
                ("migrations.not_null_without_default", 2),
                ("migrations.alter_type", 4),
                ("migrations.not_null_without_default", 5),
                ("migrations.drop", 6),
                ("migrations.drop", 7),
            ],
            "{findings:?}"
        );
    }

    #[test]
    fn orm_dsls_and_non_migration_files() {
        let findings = analyze(&input(vec![
            added(
                "db/migrate/20240101_add_flag.rb",
                &[
                    "add_column :users, :flag, :boolean, null: false",
                    "add_column :users, :tier, :string, null: false, default: 'basic'",
                    "remove_column :users, :legacy",
                    "change_column :users, :age, :bigint",
                ],
            ),
            added(
                "alembic/versions/abc_add.py",
                &[
                    "op.add_column('users', sa.Column('x', sa.Integer(), nullable=False))",
                    "op.add_column('users', sa.Column('y', sa.Integer(), nullable=False, server_default='0'))",
                    "op.drop_column('users', 'z')",
                ],
            ),
            added("src/app.rs", &["// DROP TABLE users"]),
            added("README.md", &["DROP TABLE users;"]),
        ]));
        let hits: Vec<(&str, &str, u32)> = findings
            .iter()
            .map(|f| (f.rule.as_str(), f.path.as_str(), f.line))
            .collect();
        assert_eq!(
            hits,
            vec![
                (
                    "migrations.not_null_without_default",
                    "db/migrate/20240101_add_flag.rb",
                    1
                ),
                ("migrations.drop", "db/migrate/20240101_add_flag.rb", 3),
                (
                    "migrations.alter_type",
                    "db/migrate/20240101_add_flag.rb",
                    4
                ),
                (
                    "migrations.not_null_without_default",
                    "alembic/versions/abc_add.py",
                    1
                ),
                ("migrations.drop", "alembic/versions/abc_add.py", 3),
            ],
            "{findings:?}"
        );
    }
}
