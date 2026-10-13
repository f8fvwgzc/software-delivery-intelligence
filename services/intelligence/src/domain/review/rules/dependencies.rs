//! Manifest changes: new dependencies without the lockfile, runtimes past
//! their end of life, and version pins that got looser.

use std::sync::LazyLock;

use regex::Regex;

use sdi_analyzer::{Day, eol};

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

/// Runtime pins matched against Dockerfile `FROM`, workflow `*-version:`,
/// `engines`, `.nvmrc`, `.python-version`, `runtime.txt`, `.tool-versions`;
/// whether a pin is past end of life comes from the analyzer's curated
/// table (`sdi_analyzer::eol`) and the review's `today`, never from a list
/// baked into this rule.
static RUNTIME_PIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:FROM\s+(?:[\w./-]+/)?(python|node|ruby|php|golang|go|ubuntu|debian|alpine)(?::|@)(?P<tag>[\w.-]+)|(python|node|ruby|php|go|dotnet)[-_]version\s*:\s*['"]?(?P<ver>[\w.]+)|"(node|python|ruby|php|go)"\s*:\s*"(?:[>=^~]*)(?P<eng>[\d.]+)|^\s*(python|nodejs|ruby|php|golang|dotnet-core)\s+(?P<tool>[\d.]+)|python-(?P<rt>[\d.]+))"#,
    )
    .expect("runtime pin")
});

fn eol_runtime(line: &str, file: &ChangedFile, today: Day) -> Option<(String, String)> {
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
        return eol_match(family, version, today).map(|v| (family.to_owned(), v));
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
    eol_match(&family, version, today).map(|v| (family, v))
}

/// The release line of `version` for `family` when it is past its end of
/// life on `today`. "3.7-slim" → "3.7"; "14-alpine" → "14"; "buster-slim"
/// → "10".
fn eol_match(family: &str, version: &str, today: Day) -> Option<String> {
    let core: String = version
        .split(['-', '_'])
        .next()
        .unwrap_or(version)
        .to_owned();
    let (runtime, line) = match family {
        "python" => (eol::PYTHON, eol::python_minor(&core)?),
        "node" => (eol::NODE, eol::node_major(&core)?),
        "ruby" => (eol::RUBY, eol::ruby_minor(&core)?),
        "php" => (eol::PHP, eol::php_minor(&core)?),
        "go" => (eol::GO, eol::go_minor(&core)?),
        "dotnet" => {
            let major = core.split('.').next().unwrap_or(&core);
            match major {
                "1" | "2" | "3" => (eol::DOTNET_CORE, eol::python_minor(&core)?),
                _ => (eol::DOTNET, major.to_owned()),
            }
        }
        "ubuntu" => (eol::UBUNTU, eol::ubuntu_version(&core)?),
        "debian" => (eol::DEBIAN, eol::debian_version(&core)?),
        "alpine" => (eol::ALPINE, eol::alpine_version(&core)?),
        _ => return None,
    };
    eol::is_eol(runtime, &line, today).map(|_| line)
}

/// A dependency added by the pull request, in OSV's vocabulary
/// (`ecosystem`, `name`, `version`), with where it was added. Only exact
/// versions are reported: a range cannot be looked up.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AddedDependency {
    pub ecosystem: &'static str,
    pub name: String,
    pub version: String,
    pub path: String,
    pub line: u32,
}

fn osv_ecosystem(file: &ChangedFile) -> Option<&'static str> {
    Some(match file.file_name() {
        "package.json" => "npm",
        "Cargo.toml" => "crates.io",
        "pyproject.toml"
        | "Pipfile"
        | "requirements.txt"
        | "requirements-dev.txt"
        | "constraints.txt" => "PyPI",
        "go.mod" => "Go",
        "Gemfile" => "RubyGems",
        "composer.json" => "Packagist",
        "mix.exs" => "Hex",
        _ => return None,
    })
}

/// The single version `spec` pins in `ecosystem` (an OSV ecosystem name),
/// or `None` when it names a range. What pins a version differs by package
/// manager, and getting it wrong reports vulnerabilities a project does
/// not have: `regex = "1"` in a `Cargo.toml` is the range `>=1.0.0, <2.0.0`
/// and resolves to the newest 1.x, not to 1.0.0.
///
/// | ecosystem | pins | does not |
/// |---|---|---|
/// | crates.io | `=1.2.3` | `1.2.3`, `1`, `^1.2`, `~1.2`, `=1.2` (all ranges) |
/// | npm | `1.2.3`, `=1.2.3`, `v1.2.3` | `1`, `1.2` (x-ranges), `^`, `~`, `>=`, `*`, `latest` |
/// | PyPI | `==1.2.3` | a bare `1.2.3` (a caret in Poetry), `>=`, `~=`, `==1.2.*` |
/// | Go | `1.2.3` (go.mod names the selected version) | anything shorter |
/// | RubyGems, Packagist, Hex, Maven, NuGet | a bare or `=` version | `~>`, `^`, `>=`, ranges |
///
/// An ecosystem that is not listed is never looked up.
pub(crate) fn exact_version(ecosystem: &str, spec: &str) -> Option<String> {
    let spec = spec.trim();
    let (pinned, rest) = match spec
        .strip_prefix("===")
        .or_else(|| spec.strip_prefix("=="))
        .or_else(|| spec.strip_prefix('='))
    {
        Some(rest) => (true, rest.trim()),
        None => (false, spec),
    };
    let version = rest.strip_prefix('v').unwrap_or(rest);
    let parts = numeric_parts(version)?;
    let exact = match ecosystem {
        "crates.io" => pinned && parts == 3,
        "npm" | "Go" => parts == 3,
        "PyPI" => pinned,
        "RubyGems" | "Packagist" | "Hex" | "Maven" | "NuGet" => true,
        _ => false,
    };
    exact.then(|| version.to_owned())
}

/// How many dot-separated numbers open `version` (`1.2.3-rc.1` has three),
/// or `None` when it is not one plain version: empty, a wildcard (`1.x`,
/// `1.*`), an operator, a list, or anything with a character a version
/// never has.
fn numeric_parts(version: &str) -> Option<usize> {
    if !version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
    {
        return None;
    }
    let core = version.split(['-', '+']).next().unwrap_or_default();
    let mut parts = 0;
    for part in core.split('.') {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        parts += 1;
    }
    (parts > 0).then_some(parts)
}

/// Every exactly pinned dependency the pull request adds (sorted, unique).
pub fn added_dependencies(input: &ReviewInput) -> Vec<AddedDependency> {
    let mut out: Vec<AddedDependency> = Vec::new();
    for file in input.files.iter().filter(|f| f.is_reviewable()) {
        let Some(ecosystem) = osv_ecosystem(file) else {
            continue;
        };
        for (line_no, line) in file.added_lines() {
            let coords = match file.file_name() {
                "go.mod" => GO_DEP
                    .captures(line)
                    .filter(|_| is_dependency_line(file, line))
                    .and_then(|c| exact_version(ecosystem, &c[2]).map(|v| (c[1].to_owned(), v))),
                "Gemfile" => {
                    let parts: Vec<&str> = line
                        .trim()
                        .trim_start_matches("gem")
                        .split(',')
                        .map(|p| p.trim().trim_matches(['\'', '"']))
                        .collect();
                    match parts.as_slice() {
                        [name, version, ..] if !name.is_empty() => {
                            exact_version(ecosystem, version).map(|v| ((*name).to_owned(), v))
                        }
                        _ => None,
                    }
                }
                _ => dependency_of(line, file)
                    .filter(|_| {
                        is_dependency_line(file, line)
                            || file.file_name().starts_with("requirements")
                    })
                    .and_then(|(name, spec)| exact_version(ecosystem, &spec).map(|v| (name, v))),
            };
            if let Some((name, version)) = coords {
                out.push(AddedDependency {
                    ecosystem,
                    name,
                    version,
                    path: file.path.clone(),
                    line: line_no,
                });
            }
        }
    }
    out.sort();
    out.dedup_by(|a, b| a.ecosystem == b.ecosystem && a.name == b.name && a.version == b.version);
    out
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
    for file in input.files.iter().filter(|f| f.is_reviewable()) {
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
                if let Some((family, version)) = eol_runtime(line, file, input.today) {
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
                            // Putting the removed line back is the whole fix.
                            .with_replacement(removed_line.to_owned()),
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
    fn eol_follows_the_review_date_not_a_list() {
        let mut old = input(vec![added("Dockerfile", &["FROM node:22"])]);
        old.today = (2027, 6, 1);
        assert_eq!(rules(&analyze(&old)), vec!["dependencies.eol_runtime"]);
        let mut early = input(vec![added("Dockerfile", &["FROM node:16"])]);
        early.today = (2022, 1, 1);
        assert!(analyze(&early).is_empty());
    }

    #[test]
    fn added_dependencies_are_exact_pins_in_osv_terms() {
        let deps = added_dependencies(&input(vec![
            modified(
                "package.json",
                &[
                    "    \"left-pad\": \"1.3.0\",",
                    "    \"lodash\": \"^4.17.21\",",
                    "    \"name\": \"web\",",
                ],
            ),
            modified("Cargo.toml", &["serde = \"=1.0.200\"", "tokio = \"1\""]),
            modified("requirements.txt", &["requests==2.31.0", "flask>=2"]),
            modified("go.mod", &["\tgithub.com/gin-gonic/gin v1.9.0"]),
            modified("Gemfile", &["gem 'rails', '7.0.0'", "gem 'puma'"]),
            modified("package-lock.json", &["{}"]),
        ]));
        let coords: Vec<(&str, &str, &str)> = deps
            .iter()
            .map(|d| (d.ecosystem, d.name.as_str(), d.version.as_str()))
            .collect();
        assert_eq!(
            coords,
            vec![
                ("Go", "github.com/gin-gonic/gin", "1.9.0"),
                ("PyPI", "requests", "2.31.0"),
                ("RubyGems", "rails", "7.0.0"),
                ("crates.io", "serde", "1.0.200"),
                ("npm", "left-pad", "1.3.0"),
            ]
        );
        assert_eq!(deps.iter().find(|d| d.name == "requests").unwrap().line, 1);
    }

    /// What pins one version depends on the package manager. A bare
    /// version in a `Cargo.toml` is a caret range, a short one in a
    /// `package.json` an x-range, a bare one for PyPI no pin at all: asking
    /// OSV about a range's lower bound reports vulnerabilities the project
    /// very likely does not have.
    #[test]
    fn exact_versions_are_judged_per_package_manager() {
        let exact = |ecosystem: &str, spec: &str| exact_version(ecosystem, spec);
        let pinned = |v: &str| Some(v.to_owned());

        // Cargo: only `=x.y.z`.
        assert_eq!(exact("crates.io", "=1.0.200"), pinned("1.0.200"));
        assert_eq!(exact("crates.io", " = 1.0.200 "), pinned("1.0.200"));
        for range in [
            "1", "1.0", "1.0.200", "^1.2.3", "~1.2", ">=1.2.3", "=1.2", "=1", "*", "",
        ] {
            assert_eq!(exact("crates.io", range), None, "cargo {range:?}");
        }

        // npm: a full version with or without `=` / `v`; anything shorter is an x-range.
        for (spec, version) in [
            ("1.3.0", "1.3.0"),
            ("=1.3.0", "1.3.0"),
            ("v1.3.0", "1.3.0"),
            ("19.0.0-rc.1", "19.0.0-rc.1"),
            ("1.0.0-next.3+build.7", "1.0.0-next.3+build.7"),
        ] {
            assert_eq!(exact("npm", spec), pinned(version), "npm {spec:?}");
        }
        for range in [
            "1",
            "1.2",
            "^1.3.0",
            "~1.3.0",
            ">=1.3.0",
            "1.x",
            "1.2.x",
            "1.2.*",
            "*",
            "latest",
            "1.2.3 - 2.0.0",
            "1.2.3 || 2.0.0",
            "<2",
            "workspace:*",
            "npm:other@1.0.0",
            "github:owner/repo",
            "file:../x",
        ] {
            assert_eq!(exact("npm", range), None, "npm {range:?}");
        }

        // PyPI: only `==` (PEP 440); a bare version is a caret in Poetry.
        assert_eq!(exact("PyPI", "==2.31.0"), pinned("2.31.0"));
        assert_eq!(exact("PyPI", "===2.31.0"), pinned("2.31.0"));
        assert_eq!(exact("PyPI", "==8"), pinned("8"));
        for range in [
            "2.31.0", ">=2.31.0", "~=2.31", "==2.31.*", "<3,>=2", "^2.31", "*", "",
        ] {
            assert_eq!(exact("PyPI", range), None, "pip {range:?}");
        }

        // Go: go.mod names the selected version, with or without the `v`.
        assert_eq!(exact("Go", "v1.9.0"), pinned("1.9.0"));
        assert_eq!(exact("Go", "1.9.0"), pinned("1.9.0"));
        assert_eq!(
            exact("Go", "v0.0.0-20240102150405-abcdef123456"),
            pinned("0.0.0-20240102150405-abcdef123456")
        );
        assert_eq!(exact("Go", "v1.9"), None);

        // Bundler, Composer, Hex, Maven: a bare version means exactly that one.
        for ecosystem in ["RubyGems", "Packagist", "Hex", "Maven"] {
            assert_eq!(exact(ecosystem, "7.0.0"), pinned("7.0.0"), "{ecosystem}");
            assert_eq!(exact(ecosystem, "== 7.0.0"), pinned("7.0.0"), "{ecosystem}");
            for range in [
                "~> 7.0",
                "^7.0",
                ">= 7.0.0",
                "7.0.*",
                "[1.0,2.0)",
                "7.0 || 8.0",
                "",
            ] {
                assert_eq!(exact(ecosystem, range), None, "{ecosystem} {range:?}");
            }
        }

        // An ecosystem OSV is not asked about is never looked up.
        assert_eq!(exact("Docker", "22.0.0"), None);
        assert_eq!(exact("", "1.2.3"), None);
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
