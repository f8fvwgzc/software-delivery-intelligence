//! Walks an image's layers the way a container runtime stacks them, and
//! keeps evidence only for files the final filesystem still holds: a later
//! layer that overwrites a path, deletes it (`.wh.<name>`) or hides a
//! directory (`.wh..wh..opq`) drops what an earlier layer said about it
//! (OCI image spec, "Representing Changes").
//!
//! Each layer is a tar stream the caller has already decompressed. Files
//! are read once, in order, under byte budgets; what is too large or past
//! a budget becomes a limitation, never a silent gap (REA's rule: absence
//! is not a verdict).

use std::{collections::BTreeMap, io::Read};

use crate::{
    auditable,
    elf::{self, Elf},
    golang, hardening,
    hardening::Language,
    packages,
    report::{BinaryReport, Component, Confidence, Finding, OsRelease, Report, SCHEMA, Severity},
};

#[derive(Debug, Clone)]
pub struct Limits {
    /// Largest single binary read into memory.
    pub max_binary_bytes: u64,
    /// Largest package database or manifest read.
    pub max_text_bytes: u64,
    /// Application binaries analyzed (and kept in the report).
    pub max_binaries: usize,
    /// Files looked at across all layers.
    pub max_entries: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_binary_bytes: 128 * 1024 * 1024,
            max_text_bytes: 8 * 1024 * 1024,
            max_binaries: 300,
            max_entries: 2_000_000,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("layer is not a readable tar stream: {0}")]
    Tar(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
enum Evidence {
    Binary {
        report: Box<BinaryReport>,
        components: Vec<Component>,
    },
    Component(Component),
    /// Parsed before the distribution is known; the ecosystem is set at the end.
    PackageDb(Vec<Component>),
    OsRelease(OsRelease),
    SourceMap {
        size: u64,
    },
    PrivateKey {
        kind: &'static str,
    },
    EnvFile {
        names: Vec<String>,
    },
}

#[derive(Debug, Clone)]
struct Entry {
    layer: u32,
    evidence: Evidence,
}

/// Called with every binary that is kept, so a caller can hand it to an
/// external analyzer (the REA CLI) without the scan touching the disk.
pub type BinaryHook = Box<dyn FnMut(&str, &[u8]) + Send>;

pub struct ImageScan {
    limits: Limits,
    entries: BTreeMap<String, Entry>,
    layer: u32,
    files_seen: u64,
    bytes_read: u64,
    binaries_kept: usize,
    system_binaries: u64,
    limitations: Vec<String>,
    hook: Option<BinaryHook>,
    stopped: bool,
}

/// What a path is, judged from its name and mode before reading it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    OsRelease,
    Dpkg,
    Apk,
    Npm,
    Python,
    SourceMap,
    KeyCandidate,
    EnvFile,
    MaybeBinary,
    Skip,
}

/// Directories the distribution owns. A binary here is the base image's
/// unless its bytes say otherwise.
fn os_owned(path: &str) -> bool {
    const DIRS: [&str; 14] = [
        "bin/",
        "sbin/",
        "lib/",
        "lib32/",
        "lib64/",
        "libx32/",
        "usr/bin/",
        "usr/sbin/",
        "usr/lib/",
        "usr/lib32/",
        "usr/lib64/",
        "usr/libexec/",
        "usr/share/",
        "etc/",
    ];
    DIRS.iter().any(|d| path.starts_with(d))
}

/// Code someone else published that the application installed.
fn third_party(path: &str) -> bool {
    let p = format!("/{path}");
    [
        "/node_modules/",
        "/site-packages/",
        "/dist-packages/",
        "/.cargo/",
        "/go/pkg/mod/",
        "/gems/",
        "/vendor/bundle/",
    ]
    .iter()
    .any(|d| p.contains(d))
}

/// Where test fixtures and documentation keys live: a PEM there is
/// someone's example, not a deployed credential.
fn fixture_tree(path: &str) -> bool {
    let p = format!("/{path}/");
    third_party(path)
        || path.starts_with("usr/share/")
        || path.starts_with("usr/lib/")
        || path.starts_with("usr/local/lib/")
        || path == "etc/ssl/private/ssl-cert-snakeoil.key"
        || [
            "/test/",
            "/tests/",
            "/testdata/",
            "/fixtures/",
            "/examples/",
            "/example/",
            "/docs/",
        ]
        .iter()
        .any(|d| p.contains(d))
}

pub fn is_application_path(path: &str) -> bool {
    !os_owned(path) && !third_party(path)
}

fn classify(path: &str, mode: u32, size: u64) -> Class {
    let base = path.rsplit('/').next().unwrap_or(path);
    let parent = |n: usize| path.rsplit('/').nth(n).unwrap_or_default();
    match path {
        "etc/os-release" | "usr/lib/os-release" => return Class::OsRelease,
        "var/lib/dpkg/status" => return Class::Dpkg,
        "lib/apk/db/installed" => return Class::Apk,
        _ => {}
    }
    if path.starts_with("var/lib/dpkg/status.d/") && !base.contains('.') {
        return Class::Dpkg;
    }
    if base == "package.json"
        && (parent(2) == "node_modules"
            || (parent(2).starts_with('@') && parent(3) == "node_modules"))
    {
        return Class::Npm;
    }
    if base == "METADATA"
        && parent(1).ends_with(".dist-info")
        && (path.contains("site-packages/") || path.contains("dist-packages/"))
    {
        return Class::Python;
    }
    if [".js.map", ".mjs.map", ".cjs.map", ".css.map"]
        .iter()
        .any(|ext| base.ends_with(ext))
        && !third_party(path)
    {
        return Class::SourceMap;
    }
    let key_name = matches!(base, "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519")
        || base.ends_with(".pem")
        || base.ends_with(".key");
    if key_name && !fixture_tree(path) && size <= 64 * 1024 {
        return Class::KeyCandidate;
    }
    let env = base == ".env"
        || base.strip_prefix(".env.").is_some_and(|suffix| {
            !matches!(
                suffix,
                "example" | "sample" | "template" | "dist" | "defaults" | "schema"
            )
        });
    if env && !fixture_tree(path) {
        return Class::EnvFile;
    }
    let executable = mode & 0o111 != 0;
    let shared = base.ends_with(".so") || base.contains(".so.") || base.ends_with(".node");
    if size >= 64 && (executable || (shared && !os_owned(path))) {
        return Class::MaybeBinary;
    }
    Class::Skip
}

/// `./usr/bin/x` → `usr/bin/x`; anything escaping the root is refused.
fn normalize(raw: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in raw.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            other => parts.push(other),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

impl ImageScan {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            entries: BTreeMap::new(),
            layer: 0,
            files_seen: 0,
            bytes_read: 0,
            binaries_kept: 0,
            system_binaries: 0,
            limitations: Vec::new(),
            hook: None,
            stopped: false,
        }
    }

    pub fn on_binary(mut self, hook: BinaryHook) -> Self {
        self.hook = Some(hook);
        self
    }

    /// Notes something the caller could not read (a zstd layer, a layer
    /// over the byte budget).
    pub fn limitation(&mut self, text: impl Into<String>) {
        self.limitations.push(text.into());
    }

    /// The next layer, lowest first, as an uncompressed tar stream.
    pub fn layer<R: Read>(&mut self, reader: R) -> Result<(), ScanError> {
        self.layer += 1;
        if self.stopped {
            return Ok(());
        }
        let layer = self.layer;
        let mut archive = tar::Archive::new(reader);
        for entry in archive.entries()? {
            let mut entry = entry?;
            self.files_seen += 1;
            if self.files_seen > self.limits.max_entries {
                self.limitations.push(format!(
                    "stopped after {} files: the image holds more than the inspection reads",
                    self.limits.max_entries
                ));
                self.stopped = true;
                break;
            }
            let Some(path) = entry
                .path()
                .ok()
                .and_then(|p| p.to_str().map(str::to_owned))
                .and_then(|p| normalize(&p))
            else {
                continue;
            };
            let (dir, base) = match path.rsplit_once('/') {
                Some((dir, base)) => (dir.to_owned(), base.to_owned()),
                None => (String::new(), path.clone()),
            };
            if base == ".wh..wh..opq" {
                // Hides what lower layers had in `dir`, not this layer's files.
                let prefix = format!("{dir}/");
                self.entries
                    .retain(|p, e| e.layer == layer || !(dir.is_empty() || p.starts_with(&prefix)));
                continue;
            }
            if let Some(hidden) = base.strip_prefix(".wh.") {
                let target = if dir.is_empty() {
                    hidden.to_owned()
                } else {
                    format!("{dir}/{hidden}")
                };
                let prefix = format!("{target}/");
                self.entries
                    .retain(|p, _| p != &target && !p.starts_with(&prefix));
                continue;
            }
            let kind = entry.header().entry_type();
            if !kind.is_file() {
                if !kind.is_dir() {
                    // A link or device now stands where a file was.
                    self.entries.remove(&path);
                }
                continue;
            }
            self.entries.remove(&path);
            let size = entry.size();
            let mode = entry.header().mode().unwrap_or(0);
            let class = classify(&path, mode, size);
            if class == Class::Skip {
                continue;
            }
            if let Some(evidence) = self.read(&mut entry, &path, class, size)? {
                self.entries.insert(path, Entry { layer, evidence });
            }
        }
        Ok(())
    }

    fn take<R: Read>(&mut self, entry: &mut R, limit: u64) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        entry.take(limit).read_to_end(&mut buf)?;
        self.bytes_read += buf.len() as u64;
        Ok(buf)
    }

    fn text<R: Read>(
        &mut self,
        entry: &mut R,
        path: &str,
        size: u64,
        limit: u64,
    ) -> std::io::Result<Option<String>> {
        if size > limit {
            self.limitations.push(format!(
                "{path} is {size} bytes, over the {limit}-byte limit for text files: not read"
            ));
            return Ok(None);
        }
        let bytes = self.take(entry, limit)?;
        Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
    }

    fn read<R: Read>(
        &mut self,
        entry: &mut R,
        path: &str,
        class: Class,
        size: u64,
    ) -> Result<Option<Evidence>, ScanError> {
        let max_text = self.limits.max_text_bytes;
        Ok(match class {
            Class::OsRelease => self
                .text(entry, path, size, 64 * 1024)?
                .map(|t| Evidence::OsRelease(packages::os_release(&t))),
            Class::Dpkg => self
                .text(entry, path, size, max_text)?
                .map(|t| Evidence::PackageDb(packages::dpkg(&t, "", path))),
            Class::Apk => self
                .text(entry, path, size, max_text)?
                .map(|t| Evidence::PackageDb(packages::apk(&t, "", path))),
            Class::Npm => self
                .text(entry, path, size, 1024 * 1024)?
                .and_then(|t| packages::npm(&t, path))
                .map(Evidence::Component),
            Class::Python => self
                .text(entry, path, size, 1024 * 1024)?
                .and_then(|t| packages::python(&t, path))
                .map(Evidence::Component),
            Class::SourceMap => Some(Evidence::SourceMap { size }),
            Class::KeyCandidate => self
                .text(entry, path, size, 64 * 1024)?
                .and_then(|t| packages::private_key_kind(&t))
                .map(|kind| Evidence::PrivateKey { kind }),
            Class::EnvFile => self
                .text(entry, path, size, 64 * 1024)?
                .map(|t| Evidence::EnvFile {
                    names: packages::env_names(&t),
                }),
            Class::MaybeBinary => self.binary(entry, path, size)?,
            Class::Skip => None,
        })
    }

    fn binary<R: Read>(
        &mut self,
        entry: &mut R,
        path: &str,
        size: u64,
    ) -> Result<Option<Evidence>, ScanError> {
        let mut magic = [0u8; 4];
        let mut got = 0;
        while got < 4 {
            let n = entry.read(&mut magic[got..])?;
            if n == 0 {
                return Ok(None);
            }
            got += n;
        }
        if !elf::is_elf(&magic) {
            return Ok(None);
        }
        let application = is_application_path(path);
        if size > self.limits.max_binary_bytes {
            if application {
                self.limitations.push(format!(
                    "{path} is {size} bytes, over the {}-byte limit for one binary: not analyzed",
                    self.limits.max_binary_bytes
                ));
            }
            return Ok(None);
        }
        let mut bytes = magic.to_vec();
        bytes.extend(self.take(entry, self.limits.max_binary_bytes)?);
        self.bytes_read += 4;
        let Some((report, components)) =
            analyze_binary(path, &bytes, application, &mut self.limitations)
        else {
            return Ok(None);
        };
        // Only application binaries and binaries that carry their own
        // dependency list stay in the report; the distribution's C tools
        // are counted, not listed.
        let keep = report.application || report.embedded_dependencies;
        if !keep {
            self.system_binaries += 1;
            return Ok(None);
        }
        if self.binaries_kept >= self.limits.max_binaries {
            if self.binaries_kept == self.limits.max_binaries {
                self.limitations.push(format!(
                    "more than {} binaries: the rest were not analyzed",
                    self.limits.max_binaries
                ));
            }
            self.binaries_kept += 1;
            return Ok(None);
        }
        self.binaries_kept += 1;
        if let Some(hook) = self.hook.as_mut() {
            hook(path, &bytes);
        }
        Ok(Some(Evidence::Binary {
            report: Box::new(report),
            components,
        }))
    }

    /// Everything the surviving files say. The caller adds the image's
    /// identity and runs the rules over it.
    pub fn finish(self) -> Report {
        let mut report = Report {
            schema: SCHEMA,
            layers: self.layer,
            layers_read: self.layer,
            files_seen: self.files_seen,
            bytes_read: self.bytes_read,
            limitations: self.limitations,
            ..Report::default()
        };
        // `/etc/os-release` wins over `/usr/lib/os-release`.
        let os = self
            .entries
            .get("etc/os-release")
            .or_else(|| self.entries.get("usr/lib/os-release"))
            .and_then(|e| match &e.evidence {
                Evidence::OsRelease(os) => Some(os.clone()),
                _ => None,
            });
        let os_ecosystem = os.as_ref().and_then(packages::os_ecosystem);
        let os_label = os
            .as_ref()
            .map(|os| match os.id.as_str() {
                "ubuntu" => "Ubuntu".to_owned(),
                "" => "OS".to_owned(),
                other => other.to_owned(),
            })
            .unwrap_or_else(|| "OS".to_owned());
        let mut db_without_ecosystem = false;
        for (path, entry) in self.entries {
            match entry.evidence {
                Evidence::Binary { report: binary, components } => {
                    report.components.extend(components);
                    report.binaries.push(*binary);
                }
                Evidence::Component(component) => report.components.push(component),
                Evidence::PackageDb(list) => {
                    if os_ecosystem.is_none() {
                        db_without_ecosystem = true;
                    }
                    report.components.extend(list.into_iter().map(|mut c| {
                        c.ecosystem = os_ecosystem.clone().unwrap_or_else(|| os_label.clone());
                        c
                    }));
                }
                Evidence::OsRelease(_) => {}
                Evidence::SourceMap { size } => report.findings.push(Finding {
                    rule: "image.source_map".into(),
                    severity: Severity::Low,
                    title: "Source map shipped in the image".into(),
                    detail: format!(
                        "{path} ({size} bytes) maps the minified bundle back to the original source files: anyone who can read the image or the served assets reads the code."
                    ),
                    path,
                    confidence: Confidence::Observed,
                    fix: "Build without source maps for production (or upload them to the error tracker only) and keep *.map out of the runtime stage.".into(),
                }),
                Evidence::PrivateKey { kind } => report.findings.push(Finding {
                    rule: "image.private_key".into(),
                    severity: Severity::Critical,
                    title: format!("{kind} private key in the image"),
                    detail: format!(
                        "{path} holds a {kind} private key. Every layer is readable by whoever can pull the image, so the key is as public as the registry."
                    ),
                    path,
                    confidence: Confidence::Observed,
                    fix: "Remove the key from the build context (.dockerignore), rotate it, and mount it at run time from a secret store.".into(),
                }),
                Evidence::EnvFile { names } => {
                    let secrets: Vec<&String> =
                        names.iter().filter(|n| packages::secret_like(n)).collect();
                    report.findings.push(Finding {
                        rule: "image.env_file".into(),
                        severity: if secrets.is_empty() { Severity::Medium } else { Severity::High },
                        title: "Environment file baked into the image".into(),
                        detail: if names.is_empty() {
                            format!("{path} ships in the image.")
                        } else {
                            format!(
                                "{path} sets {}{}. Values were not read; the names alone say configuration that varies per environment is fixed at build time.",
                                names.join(", "),
                                if secrets.is_empty() {
                                    String::new()
                                } else {
                                    format!(
                                        " ({} look like credentials)",
                                        secrets.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                                    )
                                }
                            )
                        },
                        path,
                        confidence: Confidence::Observed,
                        fix: "Add .env* to .dockerignore and pass configuration as environment variables or mounted secrets at deploy time.".into(),
                    });
                }
            }
        }
        if db_without_ecosystem && os.is_some() {
            report.limitations.push(format!(
                "{os_label} packages are listed but not looked up: OSV has no ecosystem for this distribution here"
            ));
        }
        if self.system_binaries > 0 {
            report.limitations.push(format!(
                "{} distribution binaries without embedded dependency data were counted, not listed; their versions come from the package database",
                self.system_binaries
            ));
        }
        report.os = os;
        report.components.sort();
        report.components.dedup();
        report
    }
}

/// One ELF file: hardening rows, language, toolchain and the dependencies
/// it embeds. `None` when it does not parse (noted as a limitation for an
/// application binary).
pub fn analyze_binary(
    path: &str,
    bytes: &[u8],
    application: bool,
    limitations: &mut Vec<String>,
) -> Option<(BinaryReport, Vec<Component>)> {
    let elf = match Elf::parse(bytes) {
        Ok(elf) => elf,
        Err(error) => {
            if application {
                limitations.push(format!("{path}: {error}"));
            }
            return None;
        }
    };
    let symbols = elf.symbol_names();
    let language = hardening::language(&elf, &symbols);
    let flags_1 = elf
        .dynamic()
        .ok()
        .flatten()
        .and_then(|d| d.value(elf::DT_FLAGS_1))
        .unwrap_or(0);
    let mut report = BinaryReport {
        path: path.to_owned(),
        format: "elf".into(),
        machine: elf.machine_name().into(),
        bits: elf.bits,
        kind: hardening::kind(&elf, flags_1).into(),
        language: language.as_str().into(),
        size: bytes.len() as u64,
        build_id: hardening::build_id(&elf),
        toolchain: String::new(),
        main_module: String::new(),
        build_settings: Default::default(),
        dependency_count: 0,
        embedded_dependencies: false,
        application,
        hardening: hardening::rows(&elf, &symbols, language),
    };
    let mut components = Vec::new();
    match language {
        Language::Go => match golang::read(&elf) {
            Ok(Some(info)) => {
                report.embedded_dependencies = true;
                report.toolchain = info.go_version.clone();
                report.main_module = info
                    .main
                    .as_ref()
                    .map(|m| m.path.clone())
                    .filter(|p| !p.is_empty())
                    .unwrap_or_else(|| info.path.clone());
                const SETTINGS: [&str; 8] = [
                    "vcs.revision",
                    "vcs.modified",
                    "vcs.time",
                    "CGO_ENABLED",
                    "GOOS",
                    "GOARCH",
                    "-trimpath",
                    "-buildmode",
                ];
                report.build_settings = info
                    .settings
                    .iter()
                    .filter(|(k, _)| SETTINGS.contains(&k.as_str()))
                    .cloned()
                    .collect();
                if let Some(version) = golang::release_version(&info.go_version) {
                    components.push(Component {
                        ecosystem: "Go".into(),
                        name: "stdlib".into(),
                        version,
                        path: path.to_owned(),
                        source: "go-toolchain".into(),
                    });
                }
                for dep in &info.deps {
                    // A replaced module is linked from its replacement.
                    let (name, version) = match &dep.replacement {
                        Some((path, version)) if !version.is_empty() => (path, version),
                        Some(_) => continue, // a local directory: no published version
                        None => (&dep.path, &dep.version),
                    };
                    if let Some(version) = golang::osv_version(version) {
                        components.push(Component {
                            ecosystem: "Go".into(),
                            name: name.clone(),
                            version,
                            path: path.to_owned(),
                            source: "go-buildinfo".into(),
                        });
                    }
                }
                report.dependency_count = info.deps.len() as u32;
                if info.unparsed > 0 {
                    limitations.push(format!(
                        "{path}: {} build-info lines did not parse; its module list may be incomplete",
                        info.unparsed
                    ));
                }
            }
            Ok(None) => {
                if application {
                    limitations.push(format!(
                        "{path}: Go binary without build information (stripped of .go.buildinfo); its module versions cannot be read"
                    ));
                }
            }
            Err(error) => limitations.push(format!("{path}: {error}")),
        },
        Language::Rust => {
            match auditable::read(&elf) {
                Ok(Some(packages)) => {
                    report.embedded_dependencies = true;
                    report.main_module = packages
                        .iter()
                        .find(|p| p.root)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    report.dependency_count = packages.iter().filter(|p| !p.root).count() as u32;
                    components.extend(packages.iter().filter(|p| p.source == "crates.io").map(
                        |p| Component {
                            ecosystem: "crates.io".into(),
                            name: p.name.clone(),
                            version: p.version.clone(),
                            path: path.to_owned(),
                            source: "cargo-auditable".into(),
                        },
                    ));
                }
                Ok(None) => {}
                Err(error) => limitations.push(format!("{path}: {error}")),
            }
        }
        Language::Native => {}
    }
    Some((report, components))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ElfBuilder, go_buildinfo_inline, tar_layer};

    fn scan(layers: &[Vec<(&str, u32, Vec<u8>)>]) -> Report {
        let mut scan = ImageScan::new(Limits::default());
        for layer in layers {
            scan.layer(&tar_layer(layer)[..]).unwrap();
        }
        scan.finish()
    }

    #[test]
    fn keeps_what_the_final_filesystem_holds() {
        let app = ElfBuilder::new().stack(true).symbols(&["main"]).build();
        let report = scan(&[
            vec![
                (
                    "etc/os-release",
                    0o644,
                    b"ID=debian\nVERSION_ID=\"12\"\n".to_vec(),
                ),
                (
                    "var/lib/dpkg/status",
                    0o644,
                    b"Package: openssl\nStatus: install ok installed\nVersion: 3.0.11-1\n".to_vec(),
                ),
                ("app/server", 0o755, app.clone()),
                ("app/old", 0o755, app.clone()),
                (
                    "srv/www/static/main.js.map",
                    0o644,
                    b"{\"version\":3}".to_vec(),
                ),
            ],
            vec![
                ("app/.wh.old", 0o644, Vec::new()),
                ("srv/www/.wh..wh..opq", 0o644, Vec::new()),
                ("app/.env", 0o644, b"API_TOKEN=x\nPORT=1\n".to_vec()),
                (
                    "root/.ssh/id_rsa",
                    0o600,
                    b"-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n".to_vec(),
                ),
                (
                    "app/node_modules/x/test/key.pem",
                    0o644,
                    b"-----BEGIN PRIVATE KEY-----".to_vec(),
                ),
            ],
        ]);
        assert_eq!(report.layers, 2);
        assert_eq!(report.os.as_ref().unwrap().id, "debian");
        let paths: Vec<&str> = report.binaries.iter().map(|b| b.path.as_str()).collect();
        assert_eq!(paths, ["app/server"], "the deleted binary is gone");
        assert!(report.binaries[0].application);
        let openssl = report
            .components
            .iter()
            .find(|c| c.name == "openssl")
            .unwrap();
        assert_eq!(openssl.ecosystem, "Debian:12");
        let rules: Vec<&str> = report.findings.iter().map(|f| f.rule.as_str()).collect();
        assert!(rules.contains(&"image.env_file"));
        assert!(rules.contains(&"image.private_key"));
        assert!(
            !rules.contains(&"image.source_map"),
            "hidden by the opaque whiteout"
        );
        assert_eq!(
            rules.iter().filter(|r| **r == "image.private_key").count(),
            1,
            "fixtures are not keys"
        );
        let env = report
            .findings
            .iter()
            .find(|f| f.rule == "image.env_file")
            .unwrap();
        assert_eq!(env.severity, Severity::High);
        assert!(
            !env.detail.contains("=x"),
            "values are never read into the report"
        );
    }

    #[test]
    fn a_later_layer_overwrites_and_go_dependencies_come_out_as_components() {
        let modules = "path\tgithub.com/acme/api\nmod\tgithub.com/acme/api\t(devel)\t\ndep\tgolang.org/x/net\tv0.17.0\th1:x=\n";
        let go = ElfBuilder::new()
            .section(".go.buildinfo", go_buildinfo_inline("go1.21.3", modules))
            .build();
        let report = scan(&[
            vec![("usr/local/bin/api", 0o755, ElfBuilder::new().build())],
            vec![("usr/local/bin/api", 0o755, go)],
        ]);
        assert_eq!(report.binaries.len(), 1);
        let api = &report.binaries[0];
        assert_eq!(api.language, "go");
        assert_eq!(api.toolchain, "go1.21.3");
        assert_eq!(api.main_module, "github.com/acme/api");
        let names: Vec<(&str, &str)> = report
            .components
            .iter()
            .map(|c| (c.name.as_str(), c.version.as_str()))
            .collect();
        assert!(names.contains(&("stdlib", "1.21.3")));
        assert!(names.contains(&("golang.org/x/net", "0.17.0")));
    }

    #[test]
    fn distribution_c_binaries_are_counted_not_listed() {
        let report = scan(&[vec![
            ("usr/bin/ls", 0o755, ElfBuilder::new().build()),
            ("usr/bin/notelf", 0o755, vec![b'#'; 100]),
        ]]);
        assert!(report.binaries.is_empty());
        assert!(
            report
                .limitations
                .iter()
                .any(|l| l.starts_with("1 distribution binaries"))
        );
    }

    #[test]
    fn paths_never_escape_the_root() {
        assert_eq!(normalize("./usr//bin/x").as_deref(), Some("usr/bin/x"));
        assert_eq!(normalize("../etc/passwd"), None);
        assert_eq!(normalize("/"), None);
        assert!(is_application_path("app/server"));
        assert!(is_application_path("usr/local/bin/api"));
        assert!(!is_application_path("usr/bin/ls"));
        assert!(!is_application_path("app/node_modules/x/build/addon.node"));
    }
}
