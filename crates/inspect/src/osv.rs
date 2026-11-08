//! Known vulnerabilities of what the image ships, from OSV. The crate does
//! not talk to the network: it says what to ask (`queries`), reads OSV's
//! advisory JSON (`advisory`), and turns the answers into findings
//! (`add_findings`). The caller does the HTTP with its own guard and
//! budget, the way the review's OSV check does (CONTRACT.md §20).
//!
//! The point of asking about the *image* rather than the manifests: a Go
//! or Rust binary carries the versions it was linked with, a `node_modules`
//! carries what `npm ci` installed, and the base image carries its own
//! packages. A repository can say `golang.org/x/net v0.23.0` while the
//! binary inside the release still links v0.17.0.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::report::{Component, Confidence, Finding, Report, Severity};

/// `(ecosystem, name, version)`, exactly as OSV's `querybatch` takes it.
pub type Coordinates = (String, String, String);

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Advisory {
    pub id: String,
    pub summary: String,
    /// GHSA word or derived from CVSS; empty when OSV gave neither.
    pub severity: String,
    /// First fixed version of a range that affects the package.
    pub fixed: String,
}

/// Ecosystems OSV indexes the image's components under.
pub fn looked_up(ecosystem: &str) -> bool {
    matches!(ecosystem, "Go" | "crates.io" | "npm" | "PyPI")
        || ecosystem.starts_with("Debian:")
        || ecosystem.starts_with("Alpine:")
}

fn distribution(ecosystem: &str) -> bool {
    ecosystem.starts_with("Debian:") || ecosystem.starts_with("Alpine:")
}

/// Distinct coordinates to ask about, application ecosystems first (they
/// are the ones a team fixes itself), at most `max`.
pub fn queries(report: &Report, max: usize) -> Vec<Coordinates> {
    let mut seen: Vec<Coordinates> = Vec::new();
    let mut ordered: Vec<&Component> = report
        .components
        .iter()
        .filter(|c| looked_up(&c.ecosystem))
        .collect();
    ordered.sort_by_key(|c| distribution(&c.ecosystem));
    for c in ordered {
        let coords = (c.ecosystem.clone(), c.name.clone(), c.version.clone());
        if !seen.contains(&coords) {
            seen.push(coords);
            if seen.len() == max {
                break;
            }
        }
    }
    seen
}

/// The parts of an OSV advisory a finding cites (the review's reading of
/// the same JSON, CONTRACT.md §20).
pub fn advisory(id: &str, value: &Value, package: &str) -> Advisory {
    let summary = value["summary"]
        .as_str()
        .or_else(|| value["details"].as_str())
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(160)
        .collect::<String>();
    let severity = value["database_specific"]["severity"]
        .as_str()
        .map(str::to_ascii_uppercase)
        .or_else(|| {
            value["severity"]
                .as_array()?
                .iter()
                .find_map(|s| s["score"].as_str())
                .and_then(cvss_word)
        })
        .unwrap_or_default();
    let fixed = value["affected"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| {
            a["package"]["name"]
                .as_str()
                .is_none_or(|n| n.eq_ignore_ascii_case(package))
        })
        .flat_map(|a| a["ranges"].as_array().cloned().unwrap_or_default())
        .flat_map(|r| r["events"].as_array().cloned().unwrap_or_default())
        .find_map(|e| e["fixed"].as_str().map(str::to_owned))
        .unwrap_or_default();
    Advisory {
        id: id.to_owned(),
        summary,
        severity,
        fixed,
    }
}

/// A CVSS vector to the usual word, by its base metrics.
fn cvss_word(vector: &str) -> Option<String> {
    let v = vector.to_ascii_uppercase();
    if !v.starts_with("CVSS:") {
        return None;
    }
    let network = v.contains("/AV:N");
    let no_privileges = v.contains("/PR:N");
    let high_impact = v.contains("/C:H") || v.contains("/I:H") || v.contains("/A:H");
    Some(
        match (network, no_privileges, high_impact) {
            (true, true, true) => "CRITICAL",
            (true, _, true) | (_, true, true) => "HIGH",
            (_, _, true) | (true, _, _) => "MODERATE",
            _ => "LOW",
        }
        .to_owned(),
    )
}

/// One `dependency.vulnerable` finding per vulnerable component. A
/// distribution package is reported only for advisories that have a fix:
/// "an upgrade exists and the image did not take it" is what a team can
/// act on; the rest is counted in the limitations.
pub fn add_findings(report: &mut Report, answers: &BTreeMap<Coordinates, Vec<Advisory>>) {
    let mut unfixed_distribution = 0usize;
    let mut found = Vec::new();
    for ((ecosystem, name, version), advisories) in answers {
        let relevant: Vec<&Advisory> = advisories
            .iter()
            .filter(|a| !distribution(ecosystem) || !a.fixed.is_empty())
            .collect();
        unfixed_distribution += advisories.len() - relevant.len();
        if relevant.is_empty() {
            continue;
        }
        let paths: Vec<&Component> = report
            .components
            .iter()
            .filter(|c| &c.ecosystem == ecosystem && &c.name == name && &c.version == version)
            .collect();
        let first = paths.first().map(|c| c.path.clone()).unwrap_or_default();
        let source = paths.first().map(|c| c.source.as_str()).unwrap_or_default();
        let severity = relevant
            .iter()
            .map(|a| Severity::from_advisory(&a.severity))
            .max()
            .unwrap_or(Severity::Medium);
        let fixed = highest_fixed(&relevant);
        let listed: Vec<String> = relevant
            .iter()
            .take(5)
            .map(|a| {
                let mut s = a.id.clone();
                if !a.summary.is_empty() {
                    s.push_str(&format!(": {}", a.summary));
                }
                if !a.fixed.is_empty() {
                    s.push_str(&format!(" (fixed in {})", a.fixed));
                }
                s
            })
            .collect();
        let more = relevant.len().saturating_sub(listed.len());
        let embedded = matches!(source, "go-buildinfo" | "go-toolchain" | "cargo-auditable");
        let where_ = if paths.len() > 1 {
            format!("{first} and {} other places", paths.len() - 1)
        } else {
            first.clone()
        };
        let display = if source == "go-toolchain" {
            format!("Go standard library {version}")
        } else {
            format!("{name} {version}")
        };
        let detail = format!(
            "{display} ships in {where_}{}. {}{}",
            if embedded {
                " (read from the compiled binary itself: this is what runs, whatever the repository's manifests say now)"
            } else {
                ""
            },
            listed.join("; "),
            if more > 0 {
                format!("; and {more} more")
            } else {
                String::new()
            }
        );
        let fix = match (source, fixed.as_str()) {
            ("go-toolchain", "") => "Rebuild with a patched Go release.".to_owned(),
            ("go-toolchain", v) => {
                format!("Rebuild with Go {v} or later (bump the builder image).")
            }
            ("dpkg" | "apk", "") => "Rebuild on an updated base image.".to_owned(),
            ("dpkg" | "apk", v) => format!(
                "Rebuild on a base image that carries {name} {v} or later (a newer tag, or upgrade packages in the Dockerfile)."
            ),
            (_, "") => format!("Upgrade {name} and rebuild the image."),
            (_, v) => format!("Upgrade {name} to {v} or later and rebuild the image."),
        };
        found.push(Finding {
            rule: "dependency.vulnerable".into(),
            severity,
            title: format!(
                "{display}: {} known vulnerabilit{}",
                relevant.len(),
                if relevant.len() == 1 { "y" } else { "ies" }
            ),
            path: first,
            detail,
            confidence: Confidence::Observed,
            fix,
        });
    }
    if unfixed_distribution > 0 {
        report.limitations.push(format!(
            "{unfixed_distribution} advisories against distribution packages have no fixed version yet and are not reported as findings"
        ));
    }
    report.findings.extend(found);
}

/// The highest fixed version among the advisories (the one that clears
/// them all, when they are on one line), by a numeric-aware comparison.
fn highest_fixed(advisories: &[&Advisory]) -> String {
    advisories
        .iter()
        .map(|a| a.fixed.as_str())
        .filter(|f| !f.is_empty())
        .max_by(|a, b| version_key(a).cmp(&version_key(b)))
        .unwrap_or_default()
        .to_owned()
}

fn version_key(v: &str) -> Vec<(u64, String)> {
    v.split(['.', '-', '+', '~'])
        .map(|part| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            (digits.parse().unwrap_or(0), part[digits.len()..].to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn component(
        ecosystem: &str,
        name: &str,
        version: &str,
        path: &str,
        source: &str,
    ) -> Component {
        Component {
            ecosystem: ecosystem.into(),
            name: name.into(),
            version: version.into(),
            path: path.into(),
            source: source.into(),
        }
    }

    #[test]
    fn asks_about_application_ecosystems_first() {
        let report = Report {
            components: vec![
                component(
                    "Debian:12",
                    "openssl",
                    "3.0.11",
                    "var/lib/dpkg/status",
                    "dpkg",
                ),
                component(
                    "Go",
                    "golang.org/x/net",
                    "0.17.0",
                    "app/api",
                    "go-buildinfo",
                ),
                component(
                    "Go",
                    "golang.org/x/net",
                    "0.17.0",
                    "app/worker",
                    "go-buildinfo",
                ),
                component("Ubuntu", "bash", "5.1", "var/lib/dpkg/status", "dpkg"),
            ],
            ..Report::default()
        };
        let queries = queries(&report, 10);
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0].0, "Go");
        assert_eq!(queries[1].0, "Debian:12");
        assert_eq!(super::queries(&report, 1).len(), 1);
    }

    #[test]
    fn reads_an_advisory() {
        let value = json!({
            "summary": "HTTP/2 rapid reset\nmore",
            "database_specific": {"severity": "HIGH"},
            "affected": [
                {"package": {"name": "other"}, "ranges": [{"events": [{"fixed": "9.9.9"}]}]},
                {"package": {"name": "golang.org/x/net"}, "ranges": [{"events": [{"introduced": "0"}, {"fixed": "0.17.0"}]}]}
            ]
        });
        let a = advisory("GHSA-qppj-fm5r-hxr3", &value, "golang.org/x/net");
        assert_eq!(a.summary, "HTTP/2 rapid reset");
        assert_eq!(a.severity, "HIGH");
        assert_eq!(a.fixed, "0.17.0");
        let cvss = json!({"severity": [{"type": "CVSS_V3", "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"}]});
        assert_eq!(advisory("X", &cvss, "p").severity, "CRITICAL");
    }

    #[test]
    fn findings_say_the_version_came_from_the_binary_and_how_to_fix_it() {
        let mut report = Report {
            components: vec![
                component(
                    "Go",
                    "golang.org/x/net",
                    "0.17.0",
                    "app/api",
                    "go-buildinfo",
                ),
                component("Go", "stdlib", "1.21.3", "app/api", "go-toolchain"),
                component(
                    "Debian:12",
                    "zlib1g",
                    "1:1.2.13",
                    "var/lib/dpkg/status",
                    "dpkg",
                ),
            ],
            ..Report::default()
        };
        let mut answers = BTreeMap::new();
        let adv = |id: &str, severity: &str, fixed: &str| Advisory {
            id: id.into(),
            summary: "s".into(),
            severity: severity.into(),
            fixed: fixed.into(),
        };
        answers.insert(
            ("Go".into(), "golang.org/x/net".into(), "0.17.0".into()),
            vec![adv("GO-1", "HIGH", "0.23.0"), adv("GO-2", "", "0.19.0")],
        );
        answers.insert(
            ("Go".into(), "stdlib".into(), "1.21.3".into()),
            vec![adv("GO-3", "CRITICAL", "1.21.9")],
        );
        answers.insert(
            ("Debian:12".into(), "zlib1g".into(), "1:1.2.13".into()),
            vec![adv("DEBIAN-1", "", "")],
        );
        add_findings(&mut report, &answers);
        assert_eq!(
            report.findings.len(),
            2,
            "the unfixed distribution advisory is counted, not reported"
        );
        let net = report
            .findings
            .iter()
            .find(|f| f.title.starts_with("golang.org/x/net"))
            .unwrap();
        assert_eq!(net.severity, Severity::High);
        assert!(net.detail.contains("read from the compiled binary"));
        assert_eq!(
            net.fix,
            "Upgrade golang.org/x/net to 0.23.0 or later and rebuild the image."
        );
        let stdlib = report
            .findings
            .iter()
            .find(|f| f.title.starts_with("Go standard library"))
            .unwrap();
        assert_eq!(stdlib.severity, Severity::Critical);
        assert!(stdlib.fix.contains("Go 1.21.9"));
        assert_eq!(report.limitations.len(), 1);
    }

    #[test]
    fn version_ordering_is_numeric() {
        let a = Advisory {
            fixed: "0.9.0".into(),
            ..Advisory::default()
        };
        let b = Advisory {
            fixed: "0.10.0".into(),
            ..Advisory::default()
        };
        assert_eq!(highest_fixed(&[&a, &b]), "0.10.0");
    }
}
