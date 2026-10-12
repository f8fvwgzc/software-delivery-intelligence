//! Known-vulnerable dependencies (CONTRACT.md §20): the pure half. The
//! coordinates come from the dependencies rule; the answers come from OSV
//! through `crate::osv` (network, cached 24 h, failing soft). Nothing is
//! bundled: without an answer there is no finding.

use std::collections::BTreeMap;

use super::{Finding, Severity, dependencies::AddedDependency};
use crate::db::osv::Vulnerability;

/// `(ecosystem, name, version)`.
pub type Coordinates = (String, String, String);

pub fn coordinates(dep: &AddedDependency) -> Coordinates {
    (
        dep.ecosystem.to_owned(),
        dep.name.clone(),
        dep.version.clone(),
    )
}

fn severity_of(vulns: &[Vulnerability]) -> Severity {
    vulns
        .iter()
        .map(|v| match v.severity.to_ascii_uppercase().as_str() {
            "CRITICAL" => Severity::Critical,
            "HIGH" => Severity::High,
            "MODERATE" | "MEDIUM" => Severity::Medium,
            "LOW" => Severity::Low,
            _ => Severity::Medium,
        })
        .max()
        .unwrap_or(Severity::Medium)
}

/// One finding per added dependency that OSV knows vulnerabilities for.
pub fn findings(
    deps: &[AddedDependency],
    answers: &BTreeMap<Coordinates, Vec<Vulnerability>>,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for dep in deps {
        let Some(vulns) = answers.get(&coordinates(dep)).filter(|v| !v.is_empty()) else {
            continue;
        };
        let listed: Vec<String> = vulns
            .iter()
            .take(5)
            .map(|v| {
                let mut line = format!("`{}`", v.id);
                if !v.summary.trim().is_empty() {
                    line.push_str(&format!(" — {}", v.summary.trim()));
                }
                if !v.fixed.trim().is_empty() {
                    line.push_str(&format!(" (fixed in {})", v.fixed.trim()));
                }
                line
            })
            .collect();
        let fixed = vulns
            .iter()
            .map(|v| v.fixed.trim())
            .filter(|f| !f.is_empty())
            .max()
            .map(str::to_owned);
        let mut finding = Finding::new(
            "dependencies",
            "vulnerable",
            severity_of(vulns),
            &dep.path,
            dep.line,
            format!("Known vulnerability in {} {}", dep.name, dep.version),
            format!(
                "`{}` {} ({}) has {} known advisor{} in OSV:\n- {}\n{}",
                dep.name,
                dep.version,
                dep.ecosystem,
                vulns.len(),
                if vulns.len() == 1 { "y" } else { "ies" },
                listed.join("\n- "),
                match &fixed {
                    Some(v) => format!("Upgrade to {v} or later."),
                    None =>
                        "No fixed version is published yet; consider another package.".to_owned(),
                }
            ),
        );
        if vulns.len() > 5 {
            finding
                .detail
                .push_str(&format!(" ({} more)", vulns.len() - 5));
        }
        out.push(finding);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_follow_the_answers() {
        let deps = vec![
            AddedDependency {
                ecosystem: "npm",
                name: "left-pad".into(),
                version: "1.3.0".into(),
                path: "package.json".into(),
                line: 4,
            },
            AddedDependency {
                ecosystem: "PyPI",
                name: "requests".into(),
                version: "2.31.0".into(),
                path: "requirements.txt".into(),
                line: 1,
            },
        ];
        let mut answers = BTreeMap::new();
        answers.insert(
            coordinates(&deps[0]),
            vec![
                Vulnerability {
                    id: "GHSA-1".into(),
                    summary: "Prototype pollution".into(),
                    severity: "HIGH".into(),
                    fixed: "1.3.1".into(),
                },
                Vulnerability {
                    id: "GHSA-2".into(),
                    severity: "LOW".into(),
                    ..Default::default()
                },
            ],
        );
        answers.insert(coordinates(&deps[1]), vec![]);
        let out = findings(&deps, &answers);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rule, "dependencies.vulnerable");
        assert_eq!(out[0].severity, Severity::High);
        assert_eq!((out[0].path.as_str(), out[0].line), ("package.json", 4));
        assert!(
            out[0]
                .detail
                .contains("`GHSA-1` — Prototype pollution (fixed in 1.3.1)")
        );
        assert!(out[0].detail.contains("Upgrade to 1.3.1 or later."));
        assert!(findings(&deps, &BTreeMap::new()).is_empty());
    }
}
