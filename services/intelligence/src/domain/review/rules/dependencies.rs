//! Manifest changes: new dependencies without the lockfile, runtimes past
//! their end of life, and version pins that got looser.

use std::sync::LazyLock;

use regex::Regex;

use super::{Finding, Severity, evidence};
use crate::domain::review::input::{ChangedFile, ReviewInput};

/// Manifest file name → the lockfiles that must move with it.
fn lockfiles_for(name: &str) -> &'static [&'static str] {
    match name {
        "package.json" => &[
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "bun.lockb",
            "bun.lock",
        ],
        "Cargo.toml" => &["Cargo.lock"],
        "pyproject.toml" => &["poetry.lock", "uv.lock", "pdm.lock"],
        "Pipfile" => &["Pipfile.lock"],
        "go.mod" => &["go.sum"],
        "Gemfile" => &["Gemfile.lock"],
        "composer.json" => &["composer.lock"],
        "mix.exs" => &["mix.lock"],
        _ => &[],
    }
}

/// `"name": "spec"` (package.json), `name = "spec"` (Cargo/pyproject),
/// `name==spec` (requirements), `gem 'name', 'spec'`, `require x vY` (go).
static JSON_DEP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*"(@?[\w./-]+)"\s*:\s*"([^"]*)"\s*,?\s*$"#).expect("json dep")
});
static TOML_DEP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*([\w.-]+)\s*=\s*(?:"([^"]*)"|\{[^}]*version\s*=\s*"([^"]*)")"#)
        .expect("toml dep")
});
static PIP_DEP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*([A-Za-z0-9_.\[\]-]+)\s*(==|>=|~=|<=|>|<|!=)?\s*([\w.*+-]*)").expect("pip dep")
});
static GO_DEP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*([\w./-]+)\s+(v[\w.+-]+)").expect("go dep"));

/// Keys that are not dependencies in the manifests above.
fn json_meta_key(key: &str) -> bool {
    matches!(
        key,
        "name"
            | "version"
            | "description"
            | "main"
            | "module"
            | "types"
            | "license"
            | "author"
            | "private"
            | "type"
            | "homepage"
            | "repository"
            | "packageManager"
            | "node"
            | "npm"
            | "pnpm"
    )
}

fn toml_meta_key(key: &str) -> bool {
    matches!(
        key,
        "name"
            | "version"
            | "edition"
            | "license"
            | "description"
            | "authors"
            | "publish"
            | "resolver"
            | "rust-version"
            | "path"
            | "readme"
            | "repository"
            | "homepage"
            | "keywords"
            | "categories"
            | "default-run"
            | "build"
            | "requires-python"
            | "python"
    )
}

/// Whether an added manifest line declares a dependency.
fn is_dependency_line(file: &ChangedFile, line: &str) -> bool {
    let name = file.file_name();
    match name {
        "package.json" => JSON_DEP
            .captures(line)
            .is_some_and(|c| !json_meta_key(&c[1]) && looks_like_version_spec(&c[2])),
        "Cargo.toml" | "pyproject.toml" => TOML_DEP.captures(line).is_some_and(|c| {
            !toml_meta_key(&c[1])
                && c.get(2)
                    .or(c.get(3))
                    .is_some_and(|m| looks_like_version_spec(m.as_str()))
        }),
        "go.mod" => {
            GO_DEP.is_match(line)
                && !line.trim_start().starts_with("module")
                && !line.trim_start().starts_with("go ")
        }
        "Gemfile" => line.trim_start().starts_with("gem "),
        "Pipfile" => TOML_DEP.is_match(line),
        "composer.json" => JSON_DEP.captures(line).is_some_and(|c| c[1].contains('/')),
        "mix.exs" => line.contains("{:") && line.contains('"'),
        _ => false,
    }
}

fn looks_like_version_spec(spec: &str) -> bool {
    let s = spec.trim();
    !s.is_empty()
        && (s.starts_with(|c: char| c.is_ascii_digit())
            || s.starts_with(['^', '~', '*', '>', '<', '=', 'v'])
            || s == "latest"
            || s.starts_with("workspace:")
            || s.starts_with("file:")
            || s.starts_with("git")
            || s.starts_with("npm:")
            || s.starts_with("link:"))
}

/// Runtime pins that reached end of life: `(family, version-prefix)` pairs
/// matched against Dockerfile `FROM`, workflow `*-version:`, `engines`,
/// `.nvmrc`, `.python-version`, `runtime.txt`, `.tool-versions`.
const EOL: &[(&str, &[&str])] = &[
    ("python", &["2", "2.7", "3.5", "3.6", "3.7", "3.8", "3.9"]),
    ("node", &["10", "12", "14", "16", "18", "20"]),
    ("ruby", &["2.5", "2.6", "2.7", "3.0", "3.1"]),
    (
        "php",
        &["5", "7.0", "7.1", "7.2", "7.3", "7.4", "8.0", "8.1"],
    ),
    (
        "go",
        &["1.17", "1.18", "1.19", "1.20", "1.21", "1.22", "1.23"],
    ),
    ("dotnet", &["2.1", "3.1", "5.0", "6.0", "7.0"]),
    ("ubuntu", &["16.04", "18.04", "20.04"]),
    (
        "debian",
        &[
            "8", "9", "10", "11", "jessie", "stretch", "buster", "bullseye",
        ],
    ),
    (
        "alpine",
        &["3.14", "3.15", "3.16", "3.17", "3.18", "3.19", "3.20"],
    ),
];

static RUNTIME_PIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:FROM\s+(?:[\w./-]+/)?(python|node|ruby|php|golang|go|ubuntu|debian|alpine)(?::|@)(?P<tag>[\w.-]+)|(python|node|ruby|php|go|dotnet)[-_]version\s*:\s*['"]?(?P<ver>[\w.]+)|"(node|python|ruby|php|go)"\s*:\s*"(?:[>=^~]*)(?P<eng>[\d.]+)|^\s*(python|nodejs|ruby|php|golang|dotnet-core)\s+(?P<tool>[\d.]+)|python-(?P<rt>[\d.]+))"#,
    )
    .expect("runtime pin")
});

fn eol_runtime(line: &str, file: &ChangedFile) -> Option<(String, String)> {
    let name = file.file_name();
    let bare = matches!(
        name,
        ".nvmrc" | ".node-version" | ".python-version" | ".ruby-version" | ".go-version"
    );
    if bare {
        let version = line.trim().trim_start_matches('v');
        let family = match name {
            ".nvmrc" | ".node-version" => "node",
            ".python-version" => "python",
            ".ruby-version" => "ruby",
            _ => "go",
        };
        return eol_match(family, version).map(|v| (family.to_owned(), v));
    }
    let caps = RUNTIME_PIN.captures(line)?;
    let family = [1, 3, 5, 7]
        .iter()
        .find_map(|i| caps.get(*i))
        .map(|m| m.as_str().to_ascii_lowercase())
        .unwrap_or_else(|| "python".to_owned());
    let family = match family.as_str() {
        "golang" => "go".to_owned(),
        "nodejs" => "node".to_owned(),
        "dotnet-core" => "dotnet".to_owned(),
        other => other.to_owned(),
    };
    let version = ["tag", "ver", "eng", "tool", "rt"]
        .iter()
        .find_map(|n| caps.name(n))
        .map(|m| m.as_str())?;
    eol_match(&family, version).map(|v| (family, v))
}

fn eol_match(family: &str, version: &str) -> Option<String> {
    let (_, prefixes) = EOL.iter().find(|(f, _)| *f == family)?;
    // "3.7-slim" → "3.7"; "14-alpine" → "14"; "buster-slim" → "buster".
    let core: String = version
        .split(['-', '_'])
        .next()
        .unwrap_or(version)
        .to_owned();
    let hit = prefixes
        .iter()
        .any(|p| core == *p || core.starts_with(&format!("{p}.")));
    hit.then_some(core)
}

/// Pins that got looser: a removed exact version became a range, a
/// wildcard, `latest`, or a bare name.
fn loosened(removed: &str, added: &str, file: &ChangedFile) -> Option<(String, String, String)> {
    let (name, old) = dependency_of(removed, file)?;
    let (new_name, new) = dependency_of(added, file)?;
    if name != new_name {
        return None;
    }
    // Cargo's bare "1.2" already means "^1.2", so there a caret is not looser.
    let cargo = file.file_name() == "Cargo.toml";
    let strict = |spec: &str| {
        let s = spec.trim();
        (s.starts_with(|c: char| c.is_ascii_digit()) && !s.contains(['*', 'x', 'X']))
            || s.starts_with("==")
            || (s.starts_with('=') && !s.starts_with("=>"))
    };
    let loose = |spec: &str| {
        let s = spec.trim();
        s.is_empty()
            || s == "latest"
            || s.contains('*')
            || s.starts_with(['>', '<'])
            || (!cargo && s.starts_with(['^', '~']))
    };
    (strict(&old) && loose(&new)).then_some((name, old, new))
}

fn dependency_of(line: &str, file: &ChangedFile) -> Option<(String, String)> {
    match file.file_name() {
        "package.json" | "composer.json" => {
            let c = JSON_DEP.captures(line)?;
            (!json_meta_key(&c[1])).then(|| (c[1].to_owned(), c[2].to_owned()))
        }
        "Cargo.toml" | "pyproject.toml" | "Pipfile" => {
            let c = TOML_DEP.captures(line)?;
            let spec = c.get(2).or(c.get(3))?.as_str().to_owned();
            (!toml_meta_key(&c[1])).then(|| (c[1].to_owned(), spec))
        }
        "requirements.txt" | "requirements-dev.txt" | "constraints.txt" => {
            let c = PIP_DEP.captures(line)?;
            let op = c.get(2).map_or("", |m| m.as_str());
            let ver = c.get(3).map_or("", |m| m.as_str());
            if c[1].starts_with('-') || c[1].starts_with('#') {
                return None;
            }
            Some((c[1].to_ascii_lowercase(), format!("{op}{ver}")))
        }
        _ => None,
    }
}

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in input.files.iter().filter(|f| !f.is_removed()) {
        let name = file.file_name();
        let locks = lockfiles_for(name);
        if !locks.is_empty() {
            let added_deps: Vec<(u32, &str)> = file
                .added_lines()
                .into_iter()
                .filter(|(_, l)| is_dependency_line(file, l))
                .collect();
            let lock_changed = input.has_file(|f| locks.contains(&f.file_name()));
            if let Some((line_no, line)) = added_deps.first()
                && !lock_changed
            {
                findings.push(Finding::new(
                    "dependencies",
                    "lockfile_missing",
                    Severity::Medium,
                    &file.path,
                    *line_no,
                    "Dependency added without its lockfile",
                    format!(
                        "`{}` gains {} dependency line{} (first: {}) but none of {} changed in this pull request. \
                         Run the package manager's install and commit the lockfile so every build resolves the same versions.",
                        file.path,
                        added_deps.len(),
                        if added_deps.len() == 1 { "" } else { "s" },
                        evidence(line),
                        locks.iter().map(|l| format!("`{l}`")).collect::<Vec<_>>().join(", ")
                    ),
                ));
            }
        }

        let runtime_file = file.is_dockerfile()
            || file.is_workflow()
            || matches!(
                name,
                "package.json"
                    | ".nvmrc"
                    | ".node-version"
                    | ".python-version"
                    | ".ruby-version"
                    | ".go-version"
                    | ".tool-versions"
                    | "runtime.txt"
                    | "pyproject.toml"
                    | "docker-compose.yml"
                    | "docker-compose.yaml"
                    | "compose.yml"
                    | "compose.yaml"
            );
        if runtime_file {
            for (line_no, line) in file.added_lines() {
                if let Some((family, version)) = eol_runtime(line, file) {
                    findings.push(Finding::new(
                        "dependencies",
                        "eol_runtime",
                        Severity::High,
                        &file.path,
                        line_no,
                        format!("End-of-life runtime: {family} {version}"),
                        format!(
                            "Line {line_no} of `{}` pins {family} {version}, which no longer receives security fixes: {}. \
                             Move to a supported release line and run the test suite against it.",
                            file.path,
                            evidence(line)
                        ),
                    ));
                }
            }
        }

        let removed = file.removed_lines();
        if !removed.is_empty() {
            for (line_no, added_line) in file.added_lines() {
                for (_, removed_line) in &removed {
                    if let Some((dep, old, new)) = loosened(removed_line, added_line, file) {
                        findings.push(
                            Finding::new(
                                "dependencies",
                                "pin_loosened",
                                Severity::Low,
                                &file.path,
                                line_no,
                                format!("Version pin loosened: {dep}"),
                                format!(
                                    "`{dep}` moves from `{old}` to `{new}` in `{}`. A range resolves differently over time; \
                                     keep the exact pin (or let the lockfile carry it) unless the range is intended.",
                                    file.path
                                ),
                            )
                            .with_suggestion(removed_line.to_owned()),
                        );
                        break;
                    }
                }
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
    fn new_dependency_without_lockfile() {
        let findings = analyze(&input(vec![modified(
            "web/package.json",
            &["    \"left-pad\": \"^1.3.0\",", "    \"name\": \"web\","],
        )]));
        assert_eq!(rules(&findings), vec!["dependencies.lockfile_missing"]);
        assert_eq!(findings[0].line, 1);
        assert!(findings[0].detail.contains("pnpm-lock.yaml"));

        let cargo = analyze(&input(vec![modified(
            "Cargo.toml",
            &["serde = { version = \"1\", features = [\"derive\"] }"],
        )]));
        assert_eq!(rules(&cargo), vec!["dependencies.lockfile_missing"]);
    }

    #[test]
    fn eol_runtimes_in_dockerfiles_workflows_and_pins() {
        let findings = analyze(&input(vec![
            added(
                "Dockerfile",
                &["FROM python:3.7-slim", "FROM node:22-alpine"],
            ),
            added(
                ".github/workflows/ci.yml",
                &["        node-version: 14", "        python-version: '3.12'"],
            ),
            added(".nvmrc", &["v16.20.0"]),
        ]));
        assert_eq!(
            rules(&findings),
            vec![
                "dependencies.eol_runtime",
                "dependencies.eol_runtime",
                "dependencies.eol_runtime"
            ]
        );
        assert!(findings[0].title.contains("python 3.7"));
        assert!(findings[1].title.contains("node 14"));
        assert!(findings[2].title.contains("node 16"));
    }

    #[test]
    fn loosened_pins_get_a_suggestion() {
        let findings = analyze(&input(vec![
            file(
                "package.json",
                "modified",
                &["    \"react\": \"*\","],
                &["    \"react\": \"18.3.1\","],
            ),
            file("package-lock.json", "modified", &["{}"], &["{}"]),
            file(
                "requirements.txt",
                "modified",
                &["requests>=2.0"],
                &["requests==2.31.0"],
            ),
            file(
                "Cargo.toml",
                "modified",
                &["tokio = \"*\""],
                &["tokio = \"1.40\""],
            ),
            file("Cargo.lock", "modified", &["x"], &["y"]),
        ]));
        assert_eq!(
            rules(&findings),
            vec![
                "dependencies.pin_loosened",
                "dependencies.pin_loosened",
                "dependencies.pin_loosened"
            ]
        );
        assert_eq!(
            findings[0].suggestion.as_deref(),
            Some("    \"react\": \"18.3.1\",")
        );
        assert!(findings[1].title.contains("requests"));
    }

    #[test]
    fn lockfile_present_supported_runtimes_and_tightened_pins_are_clean() {
        let findings = analyze(&input(vec![
            modified("package.json", &["    \"zod\": \"^3.23.0\","]),
            modified("pnpm-lock.yaml", &["zod@3.23.0:"]),
            added("Dockerfile", &["FROM python:3.12-slim", "FROM node:22"]),
            file(
                "requirements.txt",
                "modified",
                &["requests==2.32.0"],
                &["requests>=2.0"],
            ),
            file(
                "Cargo.toml",
                "modified",
                &["tokio = \"1.41\""],
                &["tokio = \"1.40\""],
            ),
            file("Cargo.lock", "modified", &["x"], &["y"]),
        ]));
        assert!(findings.is_empty(), "{findings:?}");
    }
}
