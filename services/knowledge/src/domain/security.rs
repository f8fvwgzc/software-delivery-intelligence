//! Dependabot alerts, the part that is arithmetic (CONTRACT.md §23.4): how
//! alerts are ordered, and which open Dependabot pull request fixes which
//! alert. The handler reads GitHub; nothing here does.

use std::cmp::Ordering;

use chrono::{DateTime, Utc};

/// The contract's severity words, most severe first. GitHub's advisory
/// database says "moderate" where the alerts API says "medium"; both mean
/// the same step.
pub fn severity(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "critical" => "critical",
        "high" => "high",
        "medium" | "moderate" => "medium",
        "low" => "low",
        _ => "",
    }
}

/// Position in "most severe first"; an alert without a known severity
/// sorts after every one that has it.
pub fn severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    }
}

/// Most severe first, then newest, then the higher alert number, so the
/// order is the same on every read.
pub fn alert_order(
    a: (&str, Option<DateTime<Utc>>, u64),
    b: (&str, Option<DateTime<Utc>>, u64),
) -> Ordering {
    severity_rank(a.0)
        .cmp(&severity_rank(b.0))
        .then_with(|| b.1.cmp(&a.1))
        .then_with(|| b.2.cmp(&a.2))
}

/// GitHub's ecosystem word in the platform's vocabulary, which is the one
/// `Dependency.ecosystem` uses (dependencies, OSV): crates are `cargo` and
/// workflow actions are `github_actions` everywhere else here, so they are
/// here too.
pub fn ecosystem(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "rust" => "cargo".to_owned(),
        "actions" => "github_actions".to_owned(),
        other => other.to_owned(),
    }
}

/// The package-manager segment Dependabot puts in its branch names for an
/// alert ecosystem, when it is known. `None` means "do not judge by it".
fn branch_ecosystems(ecosystem: &str) -> Option<&'static [&'static str]> {
    Some(match ecosystem {
        "npm" => &["npm_and_yarn", "bun"],
        "pip" => &["pip", "uv"],
        "cargo" | "rust" => &["cargo"],
        "go" => &["go_modules"],
        "maven" => &["maven", "gradle"],
        "nuget" => &["nuget", "dotnet_sdk"],
        "composer" => &["composer"],
        "rubygems" => &["bundler"],
        "actions" | "github_actions" => &["github_actions"],
        "pub" => &["pub"],
        "swift" => &["swift"],
        "erlang" | "hex" => &["hex"],
        "docker" => &["docker", "docker_compose"],
        _ => return None,
    })
}

/// A package name the way Dependabot writes it into a branch: lower-case,
/// `@` dropped (`@babel/core` → `babel/core`), `:` and brackets turned into
/// `-` (`group:artifact` → `group-artifact`), and nothing a git ref cannot
/// hold.
fn branch_name(package: &str) -> String {
    package
        .trim()
        .chars()
        .filter_map(|c| match c.to_ascii_lowercase() {
            '@' => None,
            ':' | '[' | ']' => Some('-'),
            c if c.is_ascii_alphanumeric() => Some(c),
            c @ ('/' | '-' | '_' | '.' | '(' | ')' | '{' | '}') => Some(c),
            _ => None,
        })
        .collect()
}

/// The directory of a manifest as a branch segment: `web/package.json` →
/// `web`, a manifest at the root → empty.
fn manifest_dir(manifest_path: &str) -> String {
    let path = manifest_path.trim().trim_matches('/');
    path.rsplit_once('/')
        .map(|(dir, _)| dir.to_ascii_lowercase().replace(' ', "-"))
        .unwrap_or_default()
}

/// How well a branch names a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum BranchMatch {
    /// The name sits at a segment boundary somewhere in the branch.
    Named,
    /// And everything before it is the directory of the alert's manifest.
    NamedInDirectory,
}

/// Whether a Dependabot branch (`dependabot/<ecosystem>/<path-ish>/<package>-<version>`)
/// updates `package`. The name must start a path segment (or follow the
/// `-and-` Dependabot joins several packages with) and be followed by a
/// version (`-` and a digit), by `-and-`, or by the end of the branch:
/// `react` is not named by `react-dom-18.2.0`, and `core` is named by
/// `babel/core-7.24.0` only as far as a branch can tell — a scoped npm
/// name and a directory look alike, which is why the manifest's directory
/// is preferred when it agrees.
fn branch_match(
    branch: &str,
    ecosystem: &str,
    package: &str,
    manifest_path: &str,
) -> Option<BranchMatch> {
    let branch = branch.trim().to_ascii_lowercase();
    let rest = branch.strip_prefix("dependabot/")?;
    let (branch_ecosystem, rest) = rest.split_once('/')?;
    let ecosystem = ecosystem.trim().to_ascii_lowercase();
    if let Some(expected) = branch_ecosystems(&ecosystem)
        && !expected.contains(&branch_ecosystem)
    {
        return None;
    }
    let name = branch_name(package);
    if name.is_empty() {
        return None;
    }
    let directory = manifest_dir(manifest_path);
    let mut best = None;
    let mut from = 0;
    while let Some(found) = rest.get(from..).and_then(|tail| tail.find(&name)) {
        let start = from + found;
        let (before, after) = (&rest[..start], &rest[start + name.len()..]);
        let starts_segment = before.is_empty() || before.ends_with('/');
        let begins = starts_segment || before.ends_with("-and-");
        let ends = after.is_empty()
            || after.starts_with("-and-")
            || after
                .strip_prefix('-')
                .map(|version| version.strip_prefix('v').unwrap_or(version))
                .is_some_and(|version| version.starts_with(|c: char| c.is_ascii_digit()));
        if begins && ends {
            let in_directory = starts_segment && before.trim_end_matches('/') == directory;
            best = best.max(Some(if in_directory {
                BranchMatch::NamedInDirectory
            } else {
                BranchMatch::Named
            }));
        }
        // Next candidate: one character on (the branch is ASCII by now, but
        // a boundary is cheap to respect).
        from = start + rest[start..].chars().next().map_or(1, char::len_utf8);
    }
    best
}

/// The open Dependabot pull request that updates an alert's package, or 0.
/// `pulls` are `(number, head branch)` of the open pull requests Dependabot
/// authored. A branch in the manifest's own directory beats one that only
/// names the package; among equals the newest pull request wins. Best
/// effort by construction: a grouped update names no package at all.
pub fn fix_pull_request(
    ecosystem: &str,
    package: &str,
    manifest_path: &str,
    pulls: &[(u64, &str)],
) -> u64 {
    pulls
        .iter()
        .filter_map(|(number, branch)| {
            branch_match(branch, ecosystem, package, manifest_path).map(|m| (m, *number))
        })
        .max()
        .map_or(0, |(_, number)| number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn severities_normalise_and_rank() {
        assert_eq!(severity("CRITICAL"), "critical");
        assert_eq!(severity(" moderate "), "medium");
        assert_eq!(severity("medium"), "medium");
        assert_eq!(severity("informational"), "");
        let mut words = ["low", "", "critical", "medium", "high"];
        words.sort_by_key(|w| severity_rank(w));
        assert_eq!(words, ["critical", "high", "medium", "low", ""]);
        assert_eq!(ecosystem("Rust"), "cargo");
        assert_eq!(ecosystem("actions"), "github_actions");
        assert_eq!(ecosystem("npm"), "npm");
    }

    #[test]
    fn alerts_order_by_severity_then_newest() {
        let day = |d| Some(Utc.with_ymd_and_hms(2026, 10, d, 0, 0, 0).unwrap());
        let mut alerts = [
            ("low", day(9), 1),
            ("critical", day(1), 2),
            ("high", day(5), 3),
            ("critical", day(8), 4),
            ("high", day(5), 5),
            ("", day(12), 6),
            ("medium", None, 7),
        ];
        alerts.sort_by(|a, b| alert_order(*a, *b));
        let numbers: Vec<u64> = alerts.iter().map(|a| a.2).collect();
        // Critical newest first; equal high alerts by number; unknown last.
        assert_eq!(numbers, [4, 2, 5, 3, 7, 1, 6]);
    }

    #[test]
    fn branches_name_packages_at_segment_boundaries() {
        let named = |branch, eco, package| branch_match(branch, eco, package, "").is_some();
        assert!(named(
            "dependabot/npm_and_yarn/lodash-4.17.21",
            "npm",
            "lodash"
        ));
        assert!(named("dependabot/cargo/serde-1.0.200", "rust", "serde"));
        assert!(named("dependabot/pip/Django-4.2.11", "pip", "django"));
        assert!(named(
            "dependabot/go_modules/github.com/gin-gonic/gin-1.9.1",
            "go",
            "github.com/gin-gonic/gin"
        ));
        assert!(named(
            "dependabot/maven/org.apache.logging.log4j-log4j-core-2.17.1",
            "maven",
            "org.apache.logging.log4j:log4j-core"
        ));
        assert!(named(
            "dependabot/github_actions/actions/checkout-4",
            "actions",
            "actions/checkout"
        ));
        assert!(named(
            "dependabot/github_actions/actions/checkout-v4.2.0",
            "actions",
            "actions/checkout"
        ));
        // Several packages in one update.
        for package in ["semver", "nodemon"] {
            assert!(named(
                "dependabot/npm_and_yarn/semver-and-nodemon-7.5.4",
                "npm",
                package
            ));
        }

        // A longer name that merely starts the same is another package.
        assert!(!named(
            "dependabot/npm_and_yarn/react-dom-18.2.0",
            "npm",
            "react"
        ));
        assert!(!named(
            "dependabot/npm_and_yarn/lodash-es-4.17.21",
            "npm",
            "lodash"
        ));
        // Not at a boundary.
        assert!(!named(
            "dependabot/npm_and_yarn/preact-10.19.0",
            "npm",
            "react"
        ));
        // Another ecosystem's branch, a grouped update, a human's branch.
        assert!(!named("dependabot/pip/lodash-4.17.21", "npm", "lodash"));
        assert!(!named(
            "dependabot/npm_and_yarn/production-dependencies-1f2e3d4c5b",
            "npm",
            "lodash"
        ));
        assert!(!named("feature/lodash-4.17.21", "npm", "lodash"));
        assert!(!named("dependabot/npm_and_yarn/lodash-4.17.21", "npm", ""));
        // An ecosystem this code has no branch word for is not judged by it.
        assert!(named("dependabot/elm/elm/core-1.0.5", "elm", "elm/core"));
    }

    #[test]
    fn scoped_npm_names_match_without_the_at_sign() {
        let pulls = [
            (11, "dependabot/npm_and_yarn/babel/core-7.24.0"),
            (12, "dependabot/npm_and_yarn/web/types/node-20.11.0"),
            (13, "dependabot/npm_and_yarn/web/babel/traverse-7.23.2"),
        ];
        assert_eq!(
            fix_pull_request("npm", "@babel/core", "package-lock.json", &pulls),
            11
        );
        assert_eq!(
            fix_pull_request("npm", "@types/node", "web/package.json", &pulls),
            12
        );
        assert_eq!(
            fix_pull_request("npm", "@babel/traverse", "web/yarn.lock", &pulls),
            13
        );
        // No pull request for this one.
        assert_eq!(
            fix_pull_request("npm", "@babel/parser", "package.json", &pulls),
            0
        );
        assert_eq!(fix_pull_request("npm", "@babel/core", "", &[]), 0);
    }

    #[test]
    fn the_manifest_directory_and_then_the_newest_pull_request_decide() {
        // `core` in /babel and `@babel/core` at the root share a branch
        // shape; the manifest's directory tells them apart when both exist.
        let pulls = [
            (20, "dependabot/npm_and_yarn/babel/core-7.24.0"),
            (21, "dependabot/npm_and_yarn/core-3.0.0"),
            (22, "dependabot/npm_and_yarn/api/minimist-1.2.8"),
            (23, "dependabot/npm_and_yarn/web/minimist-1.2.8"),
            (24, "dependabot/npm_and_yarn/minimist-1.2.6"),
            (25, "dependabot/npm_and_yarn/minimist-1.2.8"),
        ];
        assert_eq!(fix_pull_request("npm", "core", "package.json", &pulls), 21);
        assert_eq!(
            fix_pull_request("npm", "core", "babel/package.json", &pulls),
            20
        );
        assert_eq!(
            fix_pull_request("npm", "minimist", "web/package-lock.json", &pulls),
            23
        );
        // Two at the root: the newer pull request.
        assert_eq!(
            fix_pull_request("npm", "minimist", "package-lock.json", &pulls),
            25
        );
        // A directory nobody opened a pull request for: any that names it,
        // newest first.
        assert_eq!(
            fix_pull_request("npm", "minimist", "tools/package-lock.json", &pulls),
            25
        );
    }
}
