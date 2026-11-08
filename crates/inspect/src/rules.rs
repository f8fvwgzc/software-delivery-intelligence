//! Findings from evidence: what the scan read, judged against the release
//! it belongs to (its repository and commit) and the day of the run. The
//! file-level findings (keys, env files, source maps) are the scan's; these
//! are the ones that need context.

use sdi_analyzer::eol::{self, Day};

use crate::{
    hardening::{self},
    report::{BinaryReport, Confidence, Finding, Report, Severity, State},
};

pub struct Context<'a> {
    /// The day of the run (the crate has no clock).
    pub today: Day,
    /// `owner/name` of the release's repository.
    pub repo_full_name: &'a str,
    /// The commit the release says the image was built from.
    pub commit_sha: &'a str,
}

/// Whether a Go main module is the release's own repository
/// (`github.com/acme/api`, `github.com/acme/api/v2`, a GHES host).
pub fn is_repository_module(module: &str, repo_full_name: &str) -> bool {
    let module = module.trim().to_ascii_lowercase();
    let repo = repo_full_name.trim().trim_matches('/').to_ascii_lowercase();
    if module.is_empty() || !repo.contains('/') {
        return false;
    }
    let suffix = format!("/{repo}");
    module.ends_with(&suffix) || module.contains(&format!("{suffix}/"))
}

fn same_commit(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim().to_ascii_lowercase(), b.trim().to_ascii_lowercase());
    let n = a.len().min(b.len());
    n >= 7 && a[..n] == b[..n]
}

fn setting<'b>(binary: &'b BinaryReport, key: &str) -> Option<&'b str> {
    binary.build_settings.get(key).map(String::as_str)
}

fn finding(
    rule: &str,
    severity: Severity,
    title: impl Into<String>,
    path: &str,
    detail: impl Into<String>,
    confidence: Confidence,
    fix: impl Into<String>,
) -> Finding {
    Finding {
        rule: rule.to_owned(),
        severity,
        title: title.into(),
        path: path.to_owned(),
        detail: detail.into(),
        confidence,
        fix: fix.into(),
    }
}

pub fn apply(report: &mut Report, ctx: &Context<'_>) {
    // A Go binary built from this repository is the application wherever
    // the Dockerfile copied it.
    for binary in &mut report.binaries {
        if !binary.application && is_repository_module(&binary.main_module, ctx.repo_full_name) {
            binary.application = true;
        }
    }
    let mut found = Vec::new();
    for binary in &report.binaries {
        if binary.application {
            hardening_findings(binary, &mut found);
        }
        toolchain_findings(binary, ctx, &mut found);
    }
    if let Some(os) = &report.os {
        let (runtime, version) = match os.id.as_str() {
            "debian" => (eol::DEBIAN, eol::debian_version(&os.version_id)),
            "ubuntu" => (eol::UBUNTU, eol::ubuntu_version(&os.version_id)),
            "alpine" => (eol::ALPINE, eol::alpine_version(&os.version_id)),
            _ => ("", None),
        };
        if let Some(version) = version
            && let Some(day) = eol::is_eol(runtime, &version, ctx.today)
        {
            let name = if os.pretty_name.is_empty() {
                format!("{} {}", os.id, os.version_id)
            } else {
                os.pretty_name.clone()
            };
            found.push(finding(
                "image.os_eol",
                Severity::High,
                format!("{name} is past its end of life"),
                "etc/os-release",
                format!(
                    "The image runs on {name}, whose security support ended on {}. Packages in it no longer receive fixes, whatever the Dockerfile's FROM line reads today.",
                    eol::format_day(day)
                ),
                Confidence::Observed,
                "Move the runtime stage to a supported base image and rebuild.",
            ));
        }
    }
    report.findings.extend(found);
}

fn hardening_findings(binary: &BinaryReport, out: &mut Vec<Finding>) {
    let rows = &binary.hardening;
    let state = |p: &str| hardening::state_of(rows, p);
    let path = binary.path.as_str();
    if let Some(nx) = state("nx")
        && nx.state == State::Disabled
    {
        out.push(finding(
            "binary.exec_stack",
            Severity::High,
            "Executable stack",
            path,
            format!("{}: {}.", binary.path, nx.basis),
            nx.confidence,
            "Link with -z noexecstack and find the object that asks for an executable stack (often hand-written assembly without a .note.GNU-stack section).",
        ));
    }
    if let Some(pie) = state("pie")
        && pie.state == State::Disabled
    {
        let go = binary.language == "go";
        out.push(finding(
            "binary.no_pie",
            if go { Severity::Low } else { Severity::Medium },
            "Not a position-independent executable",
            path,
            format!("{}: {}; ASLR cannot move its code.", binary.path, pie.basis),
            pie.confidence,
            if go {
                "Build with -buildmode=pie (go build -buildmode=pie)."
            } else {
                "Compile with -fPIE and link with -pie (most distributions' compilers default to it; check the build image)."
            },
        ));
    }
    if let Some(relro) = state("relro") {
        match relro.state {
            State::Disabled if binary.language != "go" => out.push(finding(
                "binary.no_relro",
                Severity::Low,
                "No RELRO",
                path,
                format!(
                    "{}: {}; relocations stay writable after start.",
                    binary.path, relro.basis
                ),
                relro.confidence,
                "Link with -Wl,-z,relro,-z,now.",
            )),
            State::Partial => out.push(finding(
                "binary.partial_relro",
                Severity::Info,
                "Partial RELRO",
                path,
                format!("{}: {}.", binary.path, relro.basis),
                relro.confidence,
                "Link with -Wl,-z,now as well for full RELRO.",
            )),
            _ => {}
        }
    }
    if let Some(canary) = state("stack_protector")
        && canary.state == State::Disabled
    {
        out.push(finding(
            "binary.no_stack_protector",
            Severity::Low,
            "No stack protector",
            path,
            format!("{}: {}.", binary.path, canary.basis),
            canary.confidence,
            "Compile with -fstack-protector-strong.",
        ));
    }
    let risky = hardening::risky_search_paths(rows);
    if !risky.is_empty() {
        out.push(finding(
            "binary.unsafe_rpath",
            Severity::Medium,
            "Library search path an attacker could plant into",
            path,
            format!(
                "{} searches {} for shared libraries: a relative or shared temporary directory lets whoever writes there replace a library.",
                binary.path,
                risky.iter().map(|p| if p.is_empty() { "\"\" (the working directory)".to_owned() } else { p.clone() }).collect::<Vec<_>>().join(", ")
            ),
            Confidence::Observed,
            "Use $ORIGIN-relative or absolute system paths in -Wl,-rpath, or drop RPATH and install the libraries where the loader looks.",
        ));
    }
    if let Some(stripped) = state("stripped")
        && stripped.state == State::Disabled
    {
        out.push(finding(
            "binary.debug_info",
            Severity::Info,
            "Debug information shipped",
            path,
            format!(
                "{}: {}. It makes the image larger and the binary easier to reverse-engineer.",
                binary.path, stripped.basis
            ),
            Confidence::Observed,
            "Strip the runtime copy (strip, -ldflags=\"-s -w\", or profile.release strip = true) and keep debug files elsewhere.",
        ));
    }
    if binary.language == "rust" && !binary.embedded_dependencies {
        out.push(finding(
            "rust.no_dependency_list",
            Severity::Info,
            "Rust binary does not say what it links",
            path,
            format!(
                "{} has no cargo-auditable dependency list, so the crate versions it was built from cannot be checked after the build.",
                binary.path
            ),
            Confidence::Observed,
            "Build with cargo auditable build --release so every shipped binary carries its crate list.",
        ));
    }
}

fn toolchain_findings(binary: &BinaryReport, ctx: &Context<'_>, out: &mut Vec<Finding>) {
    if binary.language != "go" || binary.toolchain.is_empty() {
        return;
    }
    let path = binary.path.as_str();
    if let Some(minor) = eol::go_minor(&binary.toolchain)
        && let Some(day) = eol::is_eol(eol::GO, &minor, ctx.today)
    {
        out.push(finding(
            "go.toolchain_eol",
            Severity::Medium,
            format!("Built with Go {minor}, past its end of life"),
            path,
            format!(
                "{} was compiled by {}; Go {minor} stopped receiving security fixes on {}. The standard library inside the binary is that version's, whatever go.mod says.",
                binary.path,
                binary.toolchain,
                eol::format_day(day)
            ),
            Confidence::Observed,
            "Rebuild with a supported Go release (bump the builder image and the go directive).",
        ));
    }
    if !binary.application {
        return;
    }
    if setting(binary, "vcs.modified") == Some("true") {
        out.push(finding(
            "go.dirty_build",
            Severity::Medium,
            "Built from uncommitted changes",
            path,
            format!(
                "{} records vcs.modified=true: the working tree had changes no commit holds, so the image is not reproducible from the repository.",
                binary.path
            ),
            Confidence::Observed,
            "Build in CI from a clean checkout of the release commit.",
        ));
    }
    if let Some(revision) = setting(binary, "vcs.revision")
        && !ctx.commit_sha.trim().is_empty()
        && is_repository_module(&binary.main_module, ctx.repo_full_name)
        && !same_commit(revision, ctx.commit_sha)
    {
        out.push(finding(
            "provenance.commit_mismatch",
            Severity::High,
            "Binary was built from a different commit than the release",
            path,
            format!(
                "{} ({}) was built from commit {}, but the release says {}. The image does not contain the code that was reviewed for this release.",
                binary.path,
                binary.main_module,
                &revision[..revision.len().min(12)],
                &ctx.commit_sha[..ctx.commit_sha.len().min(12)]
            ),
            Confidence::Observed,
            "Rebuild the image from the release commit (and check for a stale build cache or a moved tag).",
        ));
    }
}

/// Orders findings most severe first, drops exact duplicates and caps what
/// is stored.
pub fn finalize(report: &mut Report, max_findings: usize, max_components: usize) {
    report.findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| a.path.cmp(&b.path))
    });
    report
        .findings
        .dedup_by(|a, b| a.rule == b.rule && a.path == b.path && a.title == b.title);
    if report.findings.len() > max_findings {
        let dropped = report.findings.len() - max_findings;
        report.findings.truncate(max_findings);
        report
            .limitations
            .push(format!("{dropped} less severe findings were not stored"));
    }
    if report.components.len() > max_components {
        let dropped = report.components.len() - max_components;
        report.components.truncate(max_components);
        report.limitations.push(format!(
            "{dropped} more components were found than are stored"
        ));
    }
}

/// What makes a run stop: the environment's threshold and the rules it
/// chose to accept (`inspect_fail_on`, `inspect_ignore`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    pub fail_on: Severity,
    /// Rule ids, or families with a trailing `.*` (`binary.*`).
    pub ignore: Vec<String>,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            fail_on: Severity::High,
            ignore: Vec::new(),
        }
    }
}

impl Gate {
    pub fn parse(fail_on: &str, ignore: &str) -> Gate {
        Gate {
            fail_on: Severity::parse(fail_on).unwrap_or(Severity::High),
            ignore: ignore
                .split([',', ' ', '\n'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        }
    }

    pub fn ignores(&self, rule: &str) -> bool {
        self.ignore
            .iter()
            .any(|pattern| match pattern.strip_suffix(".*") {
                Some(family) => rule
                    .strip_prefix(family)
                    .is_some_and(|rest| rest.starts_with('.')),
                None => pattern == rule,
            })
    }

    /// Findings at or above the threshold that the environment did not
    /// accept.
    pub fn blocking<'r>(&self, report: &'r Report) -> Vec<&'r Finding> {
        report
            .findings
            .iter()
            .filter(|f| f.severity >= self.fail_on && !self.ignores(&f.rule))
            .collect()
    }
}

/// A rule id as `.sdi.yml` may name it: `family.name` or `family.*`.
pub fn valid_rule_pattern(pattern: &str) -> bool {
    let body = pattern.strip_suffix(".*").unwrap_or(pattern);
    !body.is_empty()
        && body.len() <= 64
        && body
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_'))
        && !body.starts_with('.')
        && !body.ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{HardeningRow, OsRelease};

    fn row(property: &str, state: State, basis: &str) -> HardeningRow {
        HardeningRow {
            property: property.into(),
            state,
            confidence: Confidence::Observed,
            basis: basis.into(),
        }
    }

    fn go_binary(path: &str, module: &str, settings: &[(&str, &str)]) -> BinaryReport {
        BinaryReport {
            path: path.into(),
            format: "elf".into(),
            machine: "x86_64".into(),
            bits: 64,
            kind: "executable".into(),
            language: "go".into(),
            size: 1,
            build_id: String::new(),
            toolchain: "go1.20.5".into(),
            main_module: module.into(),
            build_settings: settings
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
            dependency_count: 0,
            embedded_dependencies: true,
            application: false,
            hardening: vec![
                row("pie", State::Disabled, "e_type ET_EXEC"),
                row("nx", State::Enabled, "PT_GNU_STACK without PF_X"),
                row("relro", State::Disabled, "no PT_GNU_RELRO segment"),
            ],
        }
    }

    const CTX: Context<'static> = Context {
        today: (2026, 11, 8),
        repo_full_name: "acme/api",
        commit_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    };

    fn rules(report: &Report) -> Vec<&str> {
        report.findings.iter().map(|f| f.rule.as_str()).collect()
    }

    #[test]
    fn the_repository_s_own_go_binary_is_judged_wherever_it_lives() {
        let mut report = Report {
            binaries: vec![
                go_binary(
                    "usr/bin/api",
                    "github.com/acme/api",
                    &[
                        ("vcs.revision", "bbbbbbbbbbbbbbbb"),
                        ("vcs.modified", "true"),
                    ],
                ),
                go_binary(
                    "usr/bin/kubectl",
                    "k8s.io/kubectl",
                    &[("vcs.revision", "cccccccc")],
                ),
            ],
            ..Report::default()
        };
        apply(&mut report, &CTX);
        assert!(report.binaries[0].application);
        assert!(!report.binaries[1].application);
        let found = rules(&report);
        assert!(found.contains(&"provenance.commit_mismatch"));
        assert!(found.contains(&"go.dirty_build"));
        assert!(found.contains(&"binary.no_pie"));
        assert!(
            !found.contains(&"binary.no_relro"),
            "Go links without RELRO by design"
        );
        // Both toolchains are past their end of life (Go 1.20: 2024-02-06).
        assert_eq!(
            found.iter().filter(|r| **r == "go.toolchain_eol").count(),
            2
        );
        // kubectl's own commit is not this release's business.
        assert_eq!(
            found
                .iter()
                .filter(|r| **r == "provenance.commit_mismatch")
                .count(),
            1
        );
        let pie = report
            .findings
            .iter()
            .find(|f| f.rule == "binary.no_pie")
            .unwrap();
        assert_eq!(pie.severity, Severity::Low);
    }

    #[test]
    fn a_matching_commit_is_not_a_finding() {
        let mut report = Report {
            binaries: vec![go_binary(
                "app/api",
                "github.com/acme/api",
                &[("vcs.revision", "aaaaaaaaaaaa")],
            )],
            ..Report::default()
        };
        report.binaries[0].application = true;
        apply(&mut report, &CTX);
        assert!(!rules(&report).contains(&"provenance.commit_mismatch"));
    }

    #[test]
    fn an_end_of_life_distribution_is_high() {
        let mut report = Report {
            os: Some(OsRelease {
                id: "debian".into(),
                version_id: "10".into(),
                pretty_name: "Debian GNU/Linux 10 (buster)".into(),
            }),
            ..Report::default()
        };
        apply(&mut report, &CTX);
        let eol = &report.findings[0];
        assert_eq!(eol.rule, "image.os_eol");
        assert_eq!(eol.severity, Severity::High);
        assert!(eol.detail.contains("2024-06-30"));
    }

    #[test]
    fn the_gate_blocks_at_its_threshold_minus_what_was_accepted() {
        let mut report = Report::default();
        for (rule, severity) in [
            ("image.private_key", Severity::Critical),
            ("binary.exec_stack", Severity::High),
            ("binary.no_pie", Severity::Medium),
        ] {
            report.findings.push(finding(
                rule,
                severity,
                "t",
                "p",
                "d",
                Confidence::Observed,
                "f",
            ));
        }
        let gate = Gate::parse("high", "");
        assert_eq!(gate.blocking(&report).len(), 2);
        let gate = Gate::parse("medium", "binary.*");
        let blocking: Vec<&str> = gate
            .blocking(&report)
            .iter()
            .map(|f| f.rule.as_str())
            .collect();
        assert_eq!(blocking, ["image.private_key"]);
        assert!(
            Gate::parse("critical", "image.private_key")
                .blocking(&report)
                .is_empty()
        );
        assert_eq!(Gate::parse("nonsense", "").fail_on, Severity::High);
        assert!(!gate.ignores("binaryx.y"));
        assert!(valid_rule_pattern("binary.*"));
        assert!(valid_rule_pattern("dependency.vulnerable"));
        assert!(!valid_rule_pattern("Binary.X"));
        assert!(!valid_rule_pattern(".*"));
    }

    #[test]
    fn finalize_orders_and_caps() {
        let mut report = Report::default();
        for (rule, severity) in [
            ("a.low", Severity::Low),
            ("b.crit", Severity::Critical),
            ("a.low", Severity::Low),
        ] {
            report.findings.push(finding(
                rule,
                severity,
                "t",
                "p",
                "d",
                Confidence::Observed,
                "f",
            ));
        }
        finalize(&mut report, 1, 10);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule, "b.crit");
        assert_eq!(
            report.limitations,
            ["1 less severe findings were not stored"]
        );
    }

    #[test]
    fn repository_modules() {
        assert!(is_repository_module("github.com/acme/api", "acme/api"));
        assert!(is_repository_module("github.com/Acme/API/v2", "acme/api"));
        assert!(is_repository_module(
            "ghe.corp.example/acme/api/cmd",
            "acme/api"
        ));
        assert!(!is_repository_module(
            "github.com/acme/api-client",
            "acme/api"
        ));
        assert!(!is_repository_module("", "acme/api"));
    }
}
