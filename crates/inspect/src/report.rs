//! What an inspection says about one image. Stored as JSON on the release
//! (CONTRACT.md §22.1) and served through the gateway, so every field name
//! is part of the contract: `camelCase`, enums as lowercase words.
//!
//! The evidence model is REA's (`morluto/rea`, MIT): every property carries
//! a `state`, a `confidence` that says whether it was read from the bytes
//! (`observed`) or reasoned from what was read (`derived`), and a `basis`
//! naming the bytes. Absence is not a verdict: what could not be read is a
//! limitation, never a clean result.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Version of this JSON shape. Bump it when a field changes meaning.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub const ALL: [Severity; 5] = [
        Severity::Info,
        Severity::Low,
        Severity::Medium,
        Severity::High,
        Severity::Critical,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }

    /// `info | low | medium | high | critical`, case-insensitive.
    pub fn parse(raw: &str) -> Option<Severity> {
        let raw = raw.trim();
        Severity::ALL
            .into_iter()
            .find(|s| s.as_str().eq_ignore_ascii_case(raw))
    }

    /// An OSV / GHSA severity word (`CRITICAL`, `HIGH`, `MODERATE`, `LOW`).
    /// An advisory without one is `medium`: unknown is not harmless.
    pub fn from_advisory(word: &str) -> Severity {
        match word.trim().to_ascii_uppercase().as_str() {
            "CRITICAL" => Severity::Critical,
            "HIGH" => Severity::High,
            "LOW" => Severity::Low,
            _ => Severity::Medium,
        }
    }
}

/// How a property is known (REA's `confidence`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Read directly from the bytes (a flag, a section, a symbol).
    Observed,
    /// Reasoned from what was read (a platform default, a missing symbol).
    Derived,
}

/// REA's hardening states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Enabled,
    Disabled,
    Partial,
    Unknown,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardeningRow {
    /// `pie`, `nx`, `relro`, `stack_protector`, `fortify`, `rpath`,
    /// `runpath`, `stripped`.
    pub property: String,
    pub state: State,
    pub confidence: Confidence,
    /// The bytes the state was read from, in words.
    pub basis: String,
}

/// One executable or shared object found in the image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryReport {
    pub path: String,
    /// `elf`.
    pub format: String,
    /// `x86_64`, `aarch64`, …
    pub machine: String,
    pub bits: u8,
    /// `executable`, `pie_executable`, `static_pie_executable`,
    /// `shared_object`.
    pub kind: String,
    /// `go`, `rust`, `native`.
    pub language: String,
    pub size: u64,
    /// GNU build-id note, hex.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub build_id: String,
    /// Toolchain that compiled it (`go1.22.5`), when it says.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub toolchain: String,
    /// Main module or root crate (`github.com/acme/api`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub main_module: String,
    /// Selected build settings (`vcs.revision`, `vcs.modified`,
    /// `CGO_ENABLED`, `-trimpath`, `GOARCH`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub build_settings: BTreeMap<String, String>,
    /// Dependencies embedded in the binary itself.
    pub dependency_count: u32,
    /// The binary carries its own dependency list (Go build info,
    /// cargo-auditable). A Rust binary without one hides what it links.
    #[serde(default)]
    pub embedded_dependencies: bool,
    /// Written by the team (outside the distribution's directories and
    /// outside node_modules / site-packages): only these get hardening
    /// findings.
    pub application: bool,
    pub hardening: Vec<HardeningRow>,
}

/// A package the image ships, wherever it was found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    /// OSV ecosystem: `Go`, `crates.io`, `npm`, `PyPI`, `Debian:12`,
    /// `Alpine:v3.20`, `Ubuntu` (not looked up).
    pub ecosystem: String,
    pub name: String,
    pub version: String,
    /// File the version was read from (a binary, a package.json, the dpkg
    /// database).
    pub path: String,
    /// `go-buildinfo`, `go-toolchain`, `cargo-auditable`, `package.json`,
    /// `dist-info`, `dpkg`, `apk`.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsRelease {
    /// `debian`, `ubuntu`, `alpine`, …
    pub id: String,
    pub version_id: String,
    pub pretty_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Stable id, `<family>.<what>` (`binary.exec_stack`,
    /// `dependency.vulnerable`): what `.sdi.yml` `inspect_ignore` names.
    pub rule: String,
    pub severity: Severity,
    pub title: String,
    /// File in the image; empty for image-wide findings.
    #[serde(default)]
    pub path: String,
    /// The evidence, in words.
    pub detail: String,
    pub confidence: Confidence,
    /// What would make the finding go away.
    pub fix: String,
}

/// An external analyzer run (the REA CLI, `SDI_REA_BIN`) and what it said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRun {
    pub tool: String,
    pub command: String,
    pub path: String,
    pub ok: bool,
    /// Bounded excerpt of its JSON answer or of its error.
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub schema: u32,
    /// `host/namespace/name:tag` as inspected.
    pub image: String,
    pub digest: String,
    /// `linux/amd64`.
    pub platform: String,
    pub layers: u32,
    pub layers_read: u32,
    pub files_seen: u64,
    pub bytes_read: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsRelease>,
    pub binaries: Vec<BinaryReport>,
    pub components: Vec<Component>,
    pub findings: Vec<Finding>,
    /// What was not read, and why. Never empty when something was skipped.
    pub limitations: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolRun>,
}

impl Report {
    /// The most severe finding, if any.
    pub fn max_severity(&self) -> Option<Severity> {
        self.findings.iter().map(|f| f.severity).max()
    }

    /// Findings at each severity, most severe first: `[(critical, n), …]`.
    pub fn counts(&self) -> Vec<(Severity, usize)> {
        Severity::ALL
            .into_iter()
            .rev()
            .map(|s| (s, self.findings.iter().filter(|f| f.severity == s).count()))
            .filter(|(_, n)| *n > 0)
            .collect()
    }
}
