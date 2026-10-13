//! The security overview (CONTRACT.md §23.4), the pure half: Dependabot's
//! alerts and OSV's answers in one vocabulary, the dependencies that can be
//! looked up at all, and the counts per severity.

use sdi_proto::{
    intelligence::v1::{SeverityCount, Vulnerability},
    knowledge::v1::{DependabotAlert, Dependency},
};

use crate::{
    db::osv::Vulnerability as OsvVulnerability,
    domain::review::rules::dependencies::{AddedDependency, exact_version},
};

pub const SOURCE_DEPENDABOT: &str = "dependabot";
pub const SOURCE_OSV: &str = "osv";
pub const SOURCE_NONE: &str = "none";

/// Most severe first: the order of `by_severity` and of the list.
pub const SEVERITIES: [&str; 5] = ["critical", "high", "medium", "low", "unknown"];
/// Vulnerabilities one overview lists, at most (Dependabot's own bound).
pub const MAX_VULNERABILITIES: usize = 300;
/// Dependencies one overview asks OSV about, at most.
pub const MAX_LOOKUPS: usize = 500;

/// GitHub's and OSV's severity words in the overview's vocabulary.
pub fn severity_word(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "critical" => "critical",
        "high" => "high",
        "medium" | "moderate" => "medium",
        "low" => "low",
        _ => "unknown",
    }
}

fn rank(severity: &str) -> usize {
    SEVERITIES
        .iter()
        .position(|s| *s == severity)
        .unwrap_or(SEVERITIES.len())
}

/// One open Dependabot alert as the overview shows it.
pub fn from_alert(alert: &DependabotAlert) -> Vulnerability {
    let id = [alert.ghsa_id.trim(), alert.cve_id.trim()]
        .into_iter()
        .find(|id| !id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("dependabot-alert-{}", alert.number));
    Vulnerability {
        source: SOURCE_DEPENDABOT.to_owned(),
        id,
        cve_id: alert.cve_id.trim().to_owned(),
        ecosystem: alert.ecosystem.clone(),
        package: alert.package.clone(),
        // Dependabot's alert does not carry the version the manifest names.
        current_version: String::new(),
        vulnerable_range: alert.vulnerable_range.clone(),
        fixed_version: alert.fixed_version.clone(),
        severity: severity_word(&alert.severity).to_owned(),
        summary: alert.summary.clone(),
        manifest_path: alert.manifest_path.clone(),
        scope: alert.scope.clone(),
        url: alert.html_url.clone(),
        fix_pull_request: alert.fix_pull_request,
        detected_at: alert.created_at,
    }
}

/// The scan's ecosystem names in OSV's. `None` for what OSV is not asked
/// about (container images, GitHub Actions, ecosystems without advisories).
pub fn osv_ecosystem(ecosystem: &str) -> Option<&'static str> {
    Some(match ecosystem.trim().to_ascii_lowercase().as_str() {
        "npm" => "npm",
        "cargo" => "crates.io",
        "pip" | "pypi" => "PyPI",
        "go" | "gomod" => "Go",
        "maven" | "gradle" => "Maven",
        "nuget" => "NuGet",
        "composer" => "Packagist",
        "bundler" | "rubygems" => "RubyGems",
        "mix" | "hex" => "Hex",
        _ => return None,
    })
}

/// A dependency OSV can be asked about, with the manifest entry it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup<'a> {
    pub dependency: &'a Dependency,
    pub query: AddedDependency,
}

/// The dependencies with an exact version in an ecosystem OSV knows, each
/// coordinate once, in the scan's order. A range cannot be looked up and
/// is skipped, and what is a range depends on the package manager: a bare
/// `1` in a `Cargo.toml` is one (see `exact_version`). Asking OSV about a
/// range's lower bound would report what the project very likely does not
/// have. The second value is how many were skipped
/// because the bound was reached.
pub fn lookups(dependencies: &[Dependency]) -> (Vec<Lookup<'_>>, usize) {
    let mut out: Vec<Lookup<'_>> = Vec::new();
    let mut over = 0;
    for dependency in dependencies {
        let Some(ecosystem) = osv_ecosystem(&dependency.ecosystem) else {
            continue;
        };
        let Some(version) = exact_version(ecosystem, &dependency.version) else {
            continue;
        };
        let name = dependency.name.trim();
        if name.is_empty() {
            continue;
        }
        if out.iter().any(|l| {
            l.query.ecosystem == ecosystem && l.query.name == name && l.query.version == version
        }) {
            continue;
        }
        if out.len() >= MAX_LOOKUPS {
            over += 1;
            continue;
        }
        out.push(Lookup {
            dependency,
            query: AddedDependency {
                ecosystem,
                name: name.to_owned(),
                version,
                path: dependency.manifest_path.clone(),
                line: 0,
            },
        });
    }
    (out, over)
}

/// One OSV advisory for one dependency as the overview shows it.
pub fn from_osv(lookup: &Lookup<'_>, advisory: &OsvVulnerability) -> Vulnerability {
    let id = advisory.id.trim();
    Vulnerability {
        source: SOURCE_OSV.to_owned(),
        id: id.to_owned(),
        cve_id: if id.to_ascii_uppercase().starts_with("CVE-") {
            id.to_owned()
        } else {
            String::new()
        },
        ecosystem: lookup.dependency.ecosystem.clone(),
        package: lookup.query.name.clone(),
        current_version: lookup.query.version.clone(),
        // The cached answer keeps the first fixed version, not the range.
        vulnerable_range: String::new(),
        fixed_version: advisory.fixed.trim().to_owned(),
        severity: severity_word(&advisory.severity).to_owned(),
        summary: advisory.summary.trim().to_owned(),
        manifest_path: lookup.dependency.manifest_path.clone(),
        scope: match lookup.dependency.scope.as_str() {
            "runtime" => "runtime",
            "dev" | "development" | "build" => "development",
            _ => "",
        }
        .to_owned(),
        url: format!("https://osv.dev/vulnerability/{id}"),
        fix_pull_request: 0,
        detected_at: None,
    }
}

/// Most severe first, then by package and advisory, cut at the bound.
/// Returns whether anything was cut.
pub fn order_and_bound(vulnerabilities: &mut Vec<Vulnerability>) -> bool {
    vulnerabilities.sort_by(|a, b| {
        rank(&a.severity)
            .cmp(&rank(&b.severity))
            .then_with(|| a.package.cmp(&b.package))
            .then_with(|| a.id.cmp(&b.id))
    });
    let cut = vulnerabilities.len() > MAX_VULNERABILITIES;
    vulnerabilities.truncate(MAX_VULNERABILITIES);
    cut
}

/// Counts per severity, most severe first; severities with none are left out.
pub fn by_severity(vulnerabilities: &[Vulnerability]) -> Vec<SeverityCount> {
    SEVERITIES
        .iter()
        .filter_map(|severity| {
            let count = vulnerabilities
                .iter()
                .filter(|v| v.severity == *severity)
                .count();
            (count > 0).then(|| SeverityCount {
                severity: (*severity).to_owned(),
                count: u32::try_from(count).unwrap_or(u32::MAX),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dependency(ecosystem: &str, name: &str, version: &str) -> Dependency {
        Dependency {
            id: format!("{ecosystem}-{name}"),
            project_id: "proj-1".into(),
            ecosystem: ecosystem.into(),
            name: name.into(),
            version: version.into(),
            manifest_path: "package.json".into(),
            scope: "dev".into(),
        }
    }

    #[test]
    fn alerts_map_field_by_field() {
        let alert = DependabotAlert {
            number: 4,
            ecosystem: "npm".into(),
            package: "left-pad".into(),
            manifest_path: "package.json".into(),
            scope: "runtime".into(),
            severity: "HIGH".into(),
            ghsa_id: "GHSA-aaaa".into(),
            cve_id: "CVE-2024-1".into(),
            summary: "Prototype pollution".into(),
            vulnerable_range: "< 1.3.1".into(),
            fixed_version: "1.3.1".into(),
            html_url: "https://github.com/o/r/security/dependabot/4".into(),
            fix_pull_request: 31,
            ..Default::default()
        };
        let v = from_alert(&alert);
        assert_eq!(
            (v.source.as_str(), v.id.as_str(), v.cve_id.as_str()),
            ("dependabot", "GHSA-aaaa", "CVE-2024-1")
        );
        assert_eq!(
            (v.package.as_str(), v.severity.as_str()),
            ("left-pad", "high")
        );
        assert_eq!(v.vulnerable_range, "< 1.3.1");
        assert_eq!(v.fixed_version, "1.3.1");
        assert_eq!(v.fix_pull_request, 31);
        assert_eq!(v.scope, "runtime");
        assert!(v.current_version.is_empty());
        // Without a GHSA id the CVE names it; without either, the alert does.
        let cve_only = from_alert(&DependabotAlert {
            ghsa_id: String::new(),
            ..alert.clone()
        });
        assert_eq!(cve_only.id, "CVE-2024-1");
        let anonymous = from_alert(&DependabotAlert {
            ghsa_id: String::new(),
            cve_id: String::new(),
            severity: String::new(),
            ..alert
        });
        assert_eq!(anonymous.id, "dependabot-alert-4");
        assert_eq!(anonymous.severity, "unknown");
    }

    #[test]
    fn only_exact_versions_in_known_ecosystems_are_looked_up() {
        let deps = vec![
            dependency("npm", "left-pad", "1.3.0"),
            dependency("npm", "react", "^19.0.0"),
            dependency("cargo", "serde", "=1.0.200"),
            // Ranges in their package manager's own terms: never looked up.
            dependency("cargo", "regex", "1"),
            dependency("cargo", "tokio", "1.38.0"),
            dependency("npm", "vite", "8"),
            dependency("pip", "fastapi", "0.110.0"),
            dependency("pip", "numpy", ">=1.26"),
            dependency("pip", "requests", "==2.32.0"),
            dependency("go", "github.com/x/y", "v1.2.3"),
            dependency("docker", "node", "22"),
            dependency("github_actions", "actions/checkout", "4"),
            dependency("npm", "left-pad", "1.3.0"),
            dependency("npm", "", "1.0.0"),
        ];
        let (found, over) = lookups(&deps);
        assert_eq!(over, 0);
        let coords: Vec<(&str, &str, &str)> = found
            .iter()
            .map(|l| {
                (
                    l.query.ecosystem,
                    l.query.name.as_str(),
                    l.query.version.as_str(),
                )
            })
            .collect();
        assert_eq!(
            coords,
            vec![
                ("npm", "left-pad", "1.3.0"),
                ("crates.io", "serde", "1.0.200"),
                ("PyPI", "requests", "2.32.0"),
                ("Go", "github.com/x/y", "1.2.3"),
            ]
        );
        let many: Vec<Dependency> = (0..MAX_LOOKUPS + 7)
            .map(|i| dependency("npm", &format!("pkg-{i}"), "1.0.0"))
            .collect();
        let (found, over) = lookups(&many);
        assert_eq!((found.len(), over), (MAX_LOOKUPS, 7));
    }

    #[test]
    fn osv_answers_are_ordered_counted_and_bounded() {
        let deps = [dependency("npm", "left-pad", "1.3.0")];
        let (found, _) = lookups(&deps);
        let advisory = |id: &str, severity: &str| OsvVulnerability {
            id: id.into(),
            summary: " Prototype pollution ".into(),
            severity: severity.into(),
            fixed: "1.3.1".into(),
        };
        let v = from_osv(&found[0], &advisory("GHSA-left", "MODERATE"));
        assert_eq!(
            (v.source.as_str(), v.severity.as_str(), v.scope.as_str()),
            ("osv", "medium", "development")
        );
        assert_eq!(v.current_version, "1.3.0");
        assert_eq!(v.fixed_version, "1.3.1");
        assert_eq!(v.summary, "Prototype pollution");
        assert_eq!(v.url, "https://osv.dev/vulnerability/GHSA-left");
        assert!(v.cve_id.is_empty());
        assert_eq!(
            from_osv(&found[0], &advisory("CVE-2024-9", "")).cve_id,
            "CVE-2024-9"
        );

        let mut list = vec![
            from_osv(&found[0], &advisory("b", "LOW")),
            from_osv(&found[0], &advisory("a", "")),
            from_osv(&found[0], &advisory("c", "CRITICAL")),
            from_osv(&found[0], &advisory("d", "HIGH")),
            from_osv(&found[0], &advisory("e", "CRITICAL")),
        ];
        assert!(!order_and_bound(&mut list));
        let ids: Vec<&str> = list.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["c", "e", "d", "b", "a"]);
        let counts: Vec<(String, u32)> = by_severity(&list)
            .into_iter()
            .map(|c| (c.severity, c.count))
            .collect();
        assert_eq!(
            counts,
            vec![
                ("critical".to_owned(), 2),
                ("high".to_owned(), 1),
                ("low".to_owned(), 1),
                ("unknown".to_owned(), 1)
            ]
        );
        let mut huge: Vec<Vulnerability> = (0..MAX_VULNERABILITIES + 5)
            .map(|i| from_osv(&found[0], &advisory(&format!("id-{i:04}"), "LOW")))
            .collect();
        assert!(order_and_bound(&mut huge));
        assert_eq!(huge.len(), MAX_VULNERABILITIES);
        assert!(by_severity(&[]).is_empty());
        assert_eq!(osv_ecosystem("Gradle"), Some("Maven"));
        assert_eq!(osv_ecosystem("cpan"), None);
    }
}
