//! `artifact.inspect` (CONTRACT.md §22.1): reverse-engineers the release
//! image between "the registry has it" and "deploy it".
//!
//! The image is read the way a container runtime would pull it, through
//! the same Registry v2 API, credentials and token dance as
//! `registry.verify_image`: the manifest (a multi-platform index resolves
//! to `linux/amd64`, else the first Linux image), the image config, then
//! every layer blob, lowest first. Each blob is streamed into a temporary
//! file, checked against its digest, decompressed and handed to
//! `sdi_inspect`, which keeps evidence only for files the final filesystem
//! holds. Nothing is run and nothing is written anywhere but the temporary
//! files, which go away with the step.
//!
//! What the image ships is then asked about at OSV (`OSV_API_URL`, the
//! same endpoint the review uses), and, when `SDI_REA_BIN` names the REA
//! CLI, its own binary analyzers run over the application binaries for a
//! second opinion. The full report travels to the release as the progress
//! output `inspection_report` (the delivery projection moves it there);
//! the step's outputs keep the summary.
//!
//! The environment decides what the findings do (`params.mode`): `report`
//! never stops the run (an inspection that could not finish says so and the
//! run goes on); `gate` fails the step with ARTIFACT_FINDINGS when findings
//! at or above `fail_on` remain after `ignore`, and fails it with the
//! registry's own code when the image could not be read. A person can then
//! accept and deploy anyway (`accept_findings`, recorded in the run).

use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use flate2::read::GzDecoder;
use reqwest::{Method, StatusCode, header};
use sdi_inspect::{
    Confidence, Finding, Gate, ImageScan, Limits, Report, Severity, ToolRun, osv, rules,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::{
    ACCEPT, Credentials, Probe, RegistryAdapter, SKIPPED_OIDC, api_host, parse_challenge,
    repository_path,
};
use crate::{
    adapters::{RunContext, StepInput, StepOutcome, net},
    domain::diagnosis,
};

/// The progress output that carries the whole report to the release.
pub const REPORT_OUTPUT: &str = "inspection_report";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// One blob may take this long to download.
const BLOB_TIMEOUT: Duration = Duration::from_secs(600);
/// Redirect hops a blob may take to its storage (registries answer 307 to a CDN).
const MAX_REDIRECTS: usize = 4;
/// OSV: coordinates asked per inspection, advisories whose details are read,
/// and the time all of it may take.
const MAX_OSV_QUERIES: usize = 1000;
const MAX_OSV_DETAILS: usize = 60;
const OSV_BUDGET: Duration = Duration::from_secs(30);
/// The REA CLI runs over at most this many binaries, each command this long.
const MAX_REA_BINARIES: usize = 5;
const REA_TIMEOUT: Duration = Duration::from_secs(90);
/// What the report keeps.
const MAX_FINDINGS: usize = 500;
const MAX_COMPONENTS: usize = 2000;

/// How the execution service inspects (CONTRACT.md §3).
#[derive(Debug, Clone)]
pub struct InspectSettings {
    /// OSV base (`OSV_API_URL`; `off` → `None`).
    pub osv_url: Option<String>,
    /// The REA CLI (`SDI_REA_BIN`), for a second opinion on binaries.
    pub rea_bin: Option<PathBuf>,
    /// Compressed layer bytes one inspection may download
    /// (`SDI_INSPECT_MAX_MB`, default 2048).
    pub max_download_bytes: u64,
    /// The whole step's limit (`params.timeout_seconds` overrides it).
    pub timeout: Duration,
    /// Where layer blobs are spooled while they are read, one at a time
    /// (`<SDI_RUN_DIR>/inspect`: the run volume, not the container's
    /// in-memory /tmp). `None` uses the system temporary directory.
    pub work_dir: Option<PathBuf>,
}

/// OSV's public endpoint, `OSV_API_URL`'s default.
pub const DEFAULT_OSV_URL: &str = "https://api.osv.dev";

/// Nothing external: no OSV, no REA. `from_env` is the service's real
/// configuration.
impl Default for InspectSettings {
    fn default() -> Self {
        Self {
            osv_url: None,
            rea_bin: None,
            max_download_bytes: 2048 * 1024 * 1024,
            timeout: Duration::from_secs(15 * 60),
            work_dir: None,
        }
    }
}

impl InspectSettings {
    pub fn from_env() -> Self {
        // Unset or empty (compose passes `${OSV_API_URL:-}`) is the public
        // endpoint; `off` turns the lookup off.
        let osv = std::env::var("OSV_API_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_OSV_URL.to_owned());
        let osv = osv.trim().trim_end_matches('/');
        let max_mb = std::env::var("SDI_INSPECT_MAX_MB")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|mb| *mb > 0)
            .unwrap_or(2048);
        Self {
            osv_url: (!osv.is_empty() && !osv.eq_ignore_ascii_case("off")).then(|| osv.to_owned()),
            rea_bin: std::env::var("SDI_REA_BIN")
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            max_download_bytes: max_mb * 1024 * 1024,
            work_dir: Some(
                PathBuf::from(std::env::var("SDI_RUN_DIR").unwrap_or_else(|_| ".run".to_owned()))
                    .join("inspect"),
            ),
            ..Self::default()
        }
    }

    /// A temporary file that is gone once dropped, in the work directory
    /// when it can be used.
    fn spool(&self) -> std::io::Result<std::fs::File> {
        if let Some(dir) = &self.work_dir
            && std::fs::create_dir_all(dir).is_ok()
        {
            return tempfile::tempfile_in(dir);
        }
        tempfile::tempfile()
    }

    fn scratch_dir(&self) -> std::io::Result<tempfile::TempDir> {
        if let Some(dir) = &self.work_dir
            && std::fs::create_dir_all(dir).is_ok()
        {
            return tempfile::tempdir_in(dir);
        }
        tempfile::tempdir()
    }
}

/// Why the image could not be read, as a step failure.
struct Unreadable {
    code: &'static str,
    detail: String,
}

impl From<Probe> for Unreadable {
    fn from(probe: Probe) -> Self {
        match probe {
            Probe::Found { digest } => Unreadable {
                code: diagnosis::UNKNOWN,
                detail: format!("unexpected answer for {digest}"),
            },
            Probe::Missing { detail } => Unreadable {
                code: diagnosis::IMAGE_NOT_FOUND,
                detail,
            },
            Probe::Denied { detail } => Unreadable {
                code: diagnosis::PERMISSION_DENIED,
                detail,
            },
            Probe::Throttled { detail } => Unreadable {
                code: diagnosis::QUOTA_EXCEEDED,
                detail,
            },
            Probe::Failed { detail } if detail.contains("timed out") => Unreadable {
                code: diagnosis::TIMEOUT,
                detail,
            },
            Probe::Failed { detail } => Unreadable {
                code: diagnosis::UNKNOWN,
                detail,
            },
        }
    }
}

fn unreadable(code: &'static str, detail: impl Into<String>) -> Unreadable {
    Unreadable {
        code,
        detail: detail.into(),
    }
}

/// What a step needs to talk to the registry for one image.
struct Session<'a> {
    adapter: &'a RegistryAdapter,
    ctx: &'a RunContext,
    base: String,
    repository: String,
    credentials: Option<Credentials>,
    authorization: Option<String>,
}

impl Session<'_> {
    /// `GET` with the registry's auth: anonymous first, then whatever its
    /// challenge asks for (once). Redirects are returned to the caller.
    async fn get(
        &mut self,
        url: &str,
        accept: Option<&str>,
        timeout: Duration,
    ) -> Result<reqwest::Response, Probe> {
        let path = url.strip_prefix(&self.base).unwrap_or(url).to_owned();
        for attempt in 0..2 {
            let mut request = self
                .adapter
                .net
                .request(Method::GET, url, timeout)
                .await
                .map_err(|detail| Probe::Failed {
                    detail: format!("GET {path} refused: {detail}"),
                })?;
            if let Some(accept) = accept {
                request = request.header(header::ACCEPT, accept);
            }
            if let Some(authorization) = &self.authorization {
                request = request.header(header::AUTHORIZATION, authorization);
            }
            let response = request.send().await.map_err(|e| Probe::Failed {
                detail: format!("GET {path} failed: {}", net::transport_error(e)),
            })?;
            let status = response.status();
            self.ctx.logs.line(format!("GET {path} -> {status}"));
            match status {
                s if s.is_success() || s.is_redirection() => return Ok(response),
                StatusCode::UNAUTHORIZED if attempt == 0 => {
                    let challenge = response
                        .headers()
                        .get(header::WWW_AUTHENTICATE)
                        .and_then(|v| v.to_str().ok())
                        .and_then(parse_challenge);
                    match challenge {
                        Some(c) if c.scheme == "bearer" => {
                            let token = self
                                .adapter
                                .bearer_token(
                                    &c,
                                    &self.base,
                                    &self.repository,
                                    self.credentials.as_ref(),
                                    self.ctx,
                                )
                                .await?;
                            self.authorization = Some(format!("Bearer {token}"));
                        }
                        Some(c) if c.scheme == "basic" => match &self.credentials {
                            Some(credentials) => {
                                self.authorization =
                                    Some(format!("Basic {}", super::basic_pair(credentials)));
                            }
                            None => {
                                return Err(Probe::Denied {
                                    detail: format!(
                                        "GET {path} -> {status}: the registry asks for credentials and none are configured"
                                    ),
                                });
                            }
                        },
                        _ => {
                            return Err(Probe::Denied {
                                detail: format!(
                                    "GET {path} -> {status} without a usable WWW-Authenticate challenge"
                                ),
                            });
                        }
                    }
                }
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    return Err(Probe::Denied {
                        detail: format!("GET {path} -> {status}"),
                    });
                }
                StatusCode::NOT_FOUND => {
                    return Err(Probe::Missing {
                        detail: format!("GET {path} -> {status}"),
                    });
                }
                StatusCode::TOO_MANY_REQUESTS => {
                    return Err(Probe::Throttled {
                        detail: format!("GET {path} -> {status}"),
                    });
                }
                other => {
                    return Err(Probe::Failed {
                        detail: format!("GET {path} -> {other}"),
                    });
                }
            }
        }
        Err(Probe::Denied {
            detail: format!("GET {path} kept answering 401"),
        })
    }

    async fn json(&mut self, url: &str, accept: &str) -> Result<Value, Unreadable> {
        let response = self.get(url, Some(accept), REQUEST_TIMEOUT).await?;
        if response.status().is_redirection() {
            return Err(unreadable(
                diagnosis::UNKNOWN,
                format!("{url} redirected a manifest request"),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| unreadable(diagnosis::UNKNOWN, net::transport_error(e)))?;
        if bytes.len() > net::MAX_BODY_BYTES {
            return Err(unreadable(
                diagnosis::UNKNOWN,
                "the manifest is larger than 4 MiB",
            ));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            unreadable(
                diagnosis::UNKNOWN,
                format!("the registry answered JSON that does not parse: {e}"),
            )
        })
    }

    /// Streams the blob `digest` into a temporary file, following the
    /// registry's redirect to its storage (without the registry's
    /// credentials once the host changes), and checks its digest.
    async fn blob(
        &mut self,
        digest: &str,
        budget: u64,
    ) -> Result<(std::fs::File, u64), Unreadable> {
        let mut url = format!("{}/v2/{}/blobs/{digest}", self.base, self.repository);
        let mut response = self.get(&url, None, BLOB_TIMEOUT).await?;
        let registry = reqwest::Url::parse(&self.base).ok();
        for _ in 0..MAX_REDIRECTS {
            if !response.status().is_redirection() {
                break;
            }
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    unreadable(diagnosis::UNKNOWN, "a blob redirect without a Location")
                })?;
            let next = reqwest::Url::parse(&url)
                .and_then(|current| current.join(location))
                .map_err(|e| unreadable(diagnosis::UNKNOWN, format!("blob redirect: {e}")))?;
            let same_host = registry.as_ref().is_some_and(|r| {
                r.host_str() == next.host_str()
                    && r.port_or_known_default() == next.port_or_known_default()
            });
            url = next.to_string();
            let mut request = self
                .adapter
                .net
                .request(Method::GET, &url, BLOB_TIMEOUT)
                .await
                .map_err(|d| {
                    unreadable(diagnosis::UNKNOWN, format!("blob storage refused: {d}"))
                })?;
            // The registry's token (possibly the workspace's GitHub token's
            // offspring) never goes to another host.
            if same_host && let Some(authorization) = &self.authorization {
                request = request.header(header::AUTHORIZATION, authorization);
            }
            response = request.send().await.map_err(|e| {
                unreadable(
                    diagnosis::UNKNOWN,
                    format!("blob download failed: {}", net::transport_error(e)),
                )
            })?;
            self.ctx.logs.line(format!(
                "GET {} -> {}",
                reqwest::Url::parse(&url)
                    .map(|u| net::redact_url(&u))
                    .unwrap_or_default(),
                response.status()
            ));
        }
        if !response.status().is_success() {
            return Err(unreadable(
                diagnosis::UNKNOWN,
                format!("blob {digest} answered {}", response.status()),
            ));
        }
        let file = self
            .adapter
            .inspect_settings
            .spool()
            .map_err(|e| unreadable(diagnosis::UNKNOWN, format!("temporary file: {e}")))?;
        let mut writer = tokio::fs::File::from_std(
            file.try_clone()
                .map_err(|e| unreadable(diagnosis::UNKNOWN, e.to_string()))?,
        );
        let mut hasher = Sha256::new();
        let mut written: u64 = 0;
        while let Some(chunk) = response.chunk().await.map_err(|e| {
            unreadable(
                diagnosis::UNKNOWN,
                format!("blob download failed: {}", net::transport_error(e)),
            )
        })? {
            written += chunk.len() as u64;
            if written > budget {
                return Err(unreadable(
                    diagnosis::UNKNOWN,
                    format!("blob {digest} is larger than the bytes left to read"),
                ));
            }
            hasher.update(&chunk);
            writer
                .write_all(&chunk)
                .await
                .map_err(|e| unreadable(diagnosis::UNKNOWN, format!("temporary file: {e}")))?;
        }
        writer
            .flush()
            .await
            .map_err(|e| unreadable(diagnosis::UNKNOWN, e.to_string()))?;
        let actual = format!("sha256:{}", hex::encode(hasher.finalize()));
        if digest.starts_with("sha256:") && actual != digest {
            return Err(unreadable(
                diagnosis::UNKNOWN,
                format!(
                    "blob {digest} downloaded as {actual}: the registry served other bytes than the manifest names"
                ),
            ));
        }
        let mut file = file;
        file.seek(SeekFrom::Start(0))
            .map_err(|e| unreadable(diagnosis::UNKNOWN, e.to_string()))?;
        Ok((file, written))
    }
}

/// The manifest of one platform from an index: `linux/amd64`, else
/// `linux/arm64`, else the first Linux image that is not an attestation.
pub fn pick_platform(index: &Value) -> Option<(String, String)> {
    let manifests = index["manifests"].as_array()?;
    let platform = |m: &Value| {
        (
            m["platform"]["os"].as_str().unwrap_or_default().to_owned(),
            m["platform"]["architecture"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        )
    };
    let usable: Vec<&Value> = manifests
        .iter()
        .filter(|m| {
            m["annotations"]["vnd.docker.reference.type"].as_str() != Some("attestation-manifest")
        })
        .filter(|m| platform(m).0 == "linux")
        .collect();
    let chosen = usable
        .iter()
        .find(|m| platform(m).1 == "amd64")
        .or_else(|| usable.iter().find(|m| platform(m).1 == "arm64"))
        .or_else(|| usable.first())?;
    let (os, arch) = platform(chosen);
    Some((
        chosen["digest"].as_str()?.to_owned(),
        format!("{os}/{arch}"),
    ))
}

/// Findings the image config shows: who the process runs as and
/// credentials baked into its environment (names only).
pub fn config_findings(config: &Value) -> Vec<Finding> {
    let mut out = Vec::new();
    let user = config["config"]["User"].as_str().unwrap_or_default().trim();
    let root = user.is_empty()
        || user == "root"
        || user == "0"
        || user.starts_with("0:")
        || user.starts_with("root:");
    if root {
        out.push(Finding {
            rule: "image.runs_as_root".into(),
            severity: Severity::Medium,
            title: "The container runs as root".into(),
            path: String::new(),
            detail: if user.is_empty() {
                "The image config sets no User, so the process starts as root (whatever the base image or the Dockerfile intended).".into()
            } else {
                format!("The image config sets User to {user}.")
            },
            confidence: Confidence::Observed,
            fix: "Add a USER with a non-zero uid to the runtime stage (and make the files it writes owned by it).".into(),
        });
    }
    let secrets: Vec<String> = config["config"]["Env"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|pair| pair.split_once('='))
        .filter(|(name, value)| {
            !value.trim().is_empty() && sdi_inspect::packages::secret_like(name)
        })
        .map(|(name, _)| name.to_owned())
        .collect();
    if !secrets.is_empty() {
        out.push(Finding {
            rule: "image.secret_in_env".into(),
            severity: Severity::High,
            title: "Credential set in the image's environment".into(),
            path: String::new(),
            detail: format!(
                "The image config sets {} with a value. Anyone who can pull the image reads it with docker inspect; values were not copied into this report.",
                secrets.join(", ")
            ),
            confidence: Confidence::Observed,
            fix: "Remove the ENV line, rotate the value, and inject it at deploy time from a secret store.".into(),
        });
    }
    out
}

/// What the step says, for the summary outputs and the gate.
struct Verdict {
    status: &'static str,
    blocking: Vec<Finding>,
}

fn param<'a>(step: &'a StepInput, key: &str) -> &'a str {
    step.params.get(key).map(|v| v.trim()).unwrap_or_default()
}

impl RegistryAdapter {
    pub(crate) async fn inspect(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        let mode = match param(step, "mode") {
            "" => sdi_inspect::MODE_REPORT,
            mode => mode,
        };
        let gate = Gate::parse(param(step, "fail_on"), param(step, "ignore"));
        let accepted = param(step, "accept_findings") == "true";
        let timeout = param(step, "timeout_seconds")
            .parse::<u64>()
            .ok()
            .filter(|s| *s > 0)
            .map_or(self.inspect_settings.timeout, Duration::from_secs);
        let image = param(step, "image");
        let named = format!("{image}:{}", param(step, "tag"));

        let read = tokio::time::timeout(timeout, self.read_image(step, ctx)).await;
        let report = match read {
            Ok(Ok(report)) => report,
            Ok(Err(failure)) => return self.unread(mode, accepted, &named, failure, ctx),
            Err(_) => {
                return self.unread(
                    mode,
                    accepted,
                    &named,
                    unreadable(
                        diagnosis::TIMEOUT,
                        format!(
                            "the inspection did not finish within {}s",
                            timeout.as_secs()
                        ),
                    ),
                    ctx,
                );
            }
        };
        let blocking: Vec<Finding> = if mode == sdi_inspect::MODE_GATE {
            gate.blocking(&report).into_iter().cloned().collect()
        } else {
            Vec::new()
        };
        let verdict = Verdict {
            status: match (blocking.is_empty(), accepted, report.findings.is_empty()) {
                (false, true, _) => "accepted",
                (false, false, _) => "blocked",
                (true, _, true)
                    if report
                        .limitations
                        .iter()
                        .any(|l| l.starts_with("not read:")) =>
                {
                    "incomplete"
                }
                (true, _, true) => "passed",
                (true, _, false) => "findings",
            },
            blocking,
        };
        if let Ok(json) = serde_json::to_string(&report) {
            ctx.progress.emit(REPORT_OUTPUT, json);
        }
        let counts = report
            .counts()
            .iter()
            .map(|(s, n)| format!("{n} {}", s.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        let summary = format!(
            "inspected {named} ({}, {} layers, {} binaries, {} components): {}",
            if report.platform.is_empty() {
                "single platform"
            } else {
                &report.platform
            },
            report.layers,
            report.binaries.len(),
            report.components.len(),
            if counts.is_empty() {
                "no findings".to_owned()
            } else {
                counts
            },
        );
        ctx.logs.line(summary.clone());
        let outputs = BTreeMap::from([
            ("inspection_status".to_owned(), verdict.status.to_owned()),
            (
                "inspection_findings".to_owned(),
                report.findings.len().to_string(),
            ),
            (
                "inspection_max_severity".to_owned(),
                report
                    .max_severity()
                    .map(Severity::as_str)
                    .unwrap_or("none")
                    .to_owned(),
            ),
            (
                "inspection_blocking".to_owned(),
                verdict.blocking.len().to_string(),
            ),
            ("inspection_digest".to_owned(), report.digest.clone()),
        ]);
        if verdict.status == "blocked" {
            let lines: Vec<String> = verdict
                .blocking
                .iter()
                .take(10)
                .map(|f| {
                    format!(
                        "[{}] {}{}: {}",
                        f.severity.as_str(),
                        f.title,
                        if f.path.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", f.path)
                        },
                        f.fix
                    )
                })
                .collect();
            return StepOutcome::failed(
                diagnosis::ARTIFACT_FINDINGS,
                format!(
                    "{} finding{} at or above {} in {named} stop{} this deployment.\n{}",
                    verdict.blocking.len(),
                    if verdict.blocking.len() == 1 { "" } else { "s" },
                    gate.fail_on.as_str(),
                    if verdict.blocking.len() == 1 { "s" } else { "" },
                    lines.join("\n")
                ),
            );
        }
        let output = if verdict.status == "accepted" {
            format!(
                "{summary}; {} blocking finding{} accepted by a person",
                verdict.blocking.len(),
                if verdict.blocking.len() == 1 { "" } else { "s" }
            )
        } else {
            summary
        };
        StepOutcome::Succeeded { output, outputs }
    }

    /// The image could not be read: `report` lets the run go on, `gate`
    /// stops it with the registry's own code, unless a person accepted.
    fn unread(
        &self,
        mode: &str,
        accepted: bool,
        named: &str,
        failure: Unreadable,
        ctx: &RunContext,
    ) -> StepOutcome {
        let text = format!("could not inspect {named}: {}", failure.detail);
        ctx.logs.line(text.clone());
        if mode == sdi_inspect::MODE_GATE && !accepted {
            return StepOutcome::failed(failure.code, text);
        }
        StepOutcome::Succeeded {
            output: format!(
                "{text}; {}",
                if accepted {
                    "deploying without an inspection, as accepted"
                } else {
                    "the run goes on (report mode)"
                }
            ),
            outputs: BTreeMap::from([
                ("inspection_status".to_owned(), "skipped".to_owned()),
                ("inspection_findings".to_owned(), "0".to_owned()),
                ("inspection_max_severity".to_owned(), "none".to_owned()),
                ("inspection_blocking".to_owned(), "0".to_owned()),
            ]),
        }
    }

    async fn read_image(&self, step: &StepInput, ctx: &RunContext) -> Result<Report, Unreadable> {
        if param(step, "registry_auth") == "oidc" {
            return Err(unreadable(diagnosis::UNKNOWN, SKIPPED_OIDC));
        }
        let image = param(step, "image");
        if image.is_empty() {
            return Err(unreadable(
                diagnosis::UNKNOWN,
                "params.image is required for artifact.inspect",
            ));
        }
        let host = match param(step, "registry_host") {
            "" => image.split('/').next().unwrap_or_default(),
            host => host,
        };
        // The digest the registry step just confirmed is the image to read,
        // so a tag moved in between cannot swap what gets inspected.
        let digest = ctx
            .outputs
            .get("digest")
            .map(|d| d.trim())
            .filter(|d| d.starts_with("sha256:"))
            .unwrap_or_else(|| param(step, "digest"));
        let reference = if digest.is_empty() {
            param(step, "tag")
        } else {
            digest
        };
        if reference.is_empty() {
            return Err(unreadable(
                diagnosis::UNKNOWN,
                "the release has neither a digest nor a tag to read",
            ));
        }
        let credentials =
            self.credentials(step, host, ctx)
                .await
                .map_err(|outcome| match outcome {
                    StepOutcome::Failed { raw_output, .. } => {
                        unreadable(diagnosis::UNKNOWN, raw_output)
                    }
                    StepOutcome::Succeeded { output, .. } => unreadable(diagnosis::UNKNOWN, output),
                })?;
        let mut session = Session {
            adapter: self,
            ctx,
            base: self.base_url(host),
            repository: repository_path(image, host),
            credentials,
            authorization: None,
        };
        ctx.logs.line(format!(
            "inspecting {image}@{reference} on {}",
            api_host(host)
        ));
        let manifests = format!("{}/v2/{}/manifests", session.base, session.repository);
        let manifest_url = |reference: &str| format!("{manifests}/{reference}");
        let mut manifest = session.json(&manifest_url(reference), ACCEPT).await?;
        let mut platform = String::new();
        let mut manifest_digest = digest.to_owned();
        if manifest["manifests"].is_array() {
            let (chosen, label) = pick_platform(&manifest).ok_or_else(|| {
                unreadable(
                    diagnosis::IMAGE_NOT_FOUND,
                    "the image index holds no Linux image",
                )
            })?;
            platform = label;
            manifest = session.json(&manifest_url(&chosen), ACCEPT).await?;
            manifest_digest = chosen;
        }
        let layers: Vec<(String, String, u64)> = manifest["layers"]
            .as_array()
            .ok_or_else(|| {
                unreadable(
                    diagnosis::UNKNOWN,
                    "the manifest lists no layers (a schema 1 manifest is not supported)",
                )
            })?
            .iter()
            .map(|l| {
                (
                    l["digest"].as_str().unwrap_or_default().to_owned(),
                    l["mediaType"].as_str().unwrap_or_default().to_owned(),
                    l["size"].as_u64().unwrap_or(0),
                )
            })
            .collect();

        let mut config_findings_found = Vec::new();
        if let Some(config_digest) = manifest["config"]["digest"].as_str() {
            match session
                .blob(config_digest, net::MAX_BODY_BYTES as u64)
                .await
            {
                Ok((mut file, _)) => {
                    let mut text = String::new();
                    if file.read_to_string(&mut text).is_ok()
                        && let Ok(config) = serde_json::from_str::<Value>(&text)
                    {
                        if platform.is_empty() {
                            platform = format!(
                                "{}/{}",
                                config["os"].as_str().unwrap_or("linux"),
                                config["architecture"].as_str().unwrap_or("unknown")
                            );
                        }
                        config_findings_found = config_findings(&config);
                    }
                }
                Err(failure) => ctx
                    .logs
                    .line(format!("image config not read: {}", failure.detail)),
            }
        }

        // REA's second opinion needs the binaries on disk.
        let rea_dir = match &self.inspect_settings.rea_bin {
            Some(_) => self.inspect_settings.scratch_dir().ok(),
            None => None,
        };
        let kept: Arc<Mutex<Vec<(String, PathBuf, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let mut scan = ImageScan::new(Limits::default());
        if let Some(dir) = &rea_dir {
            let dir = dir.path().to_path_buf();
            let kept = Arc::clone(&kept);
            scan = scan.on_binary(Box::new(move |path: &str, bytes: &[u8]| {
                let mut kept = kept.lock().unwrap_or_else(|p| p.into_inner());
                if kept.len() >= MAX_REA_BINARIES || !sdi_inspect::scan::is_application_path(path) {
                    return;
                }
                let file = dir.join(format!(
                    "{}-{}",
                    kept.len(),
                    path.rsplit('/').next().unwrap_or("binary")
                ));
                if std::fs::write(&file, bytes).is_ok() {
                    kept.push((path.to_owned(), file, String::new()));
                }
            }));
        }

        let mut budget = self.inspect_settings.max_download_bytes;
        let mut read = 0u32;
        for (index, (digest, media_type, size)) in layers.iter().enumerate() {
            if *size > budget {
                scan.limitation(format!(
                    "not read: layers {} to {} ({} bytes left of the {}-byte download budget; SDI_INSPECT_MAX_MB raises it)",
                    index + 1,
                    layers.len(),
                    budget,
                    self.inspect_settings.max_download_bytes
                ));
                break;
            }
            if media_type.contains("zstd") {
                scan.limitation(format!("not read: layer {} is zstd-compressed", index + 1));
                // Its files are unknown: later whiteouts still apply.
                scan.layer(std::io::empty()).ok();
                continue;
            }
            let (file, downloaded) = session.blob(digest, budget).await?;
            budget -= downloaded.min(budget);
            let task = tokio::task::spawn_blocking(move || {
                let mut scan = scan;
                let mut file = file;
                let mut magic = [0u8; 4];
                let n = file.read(&mut magic).unwrap_or(0);
                let _ = file.seek(SeekFrom::Start(0));
                let outcome = if n >= 2 && magic[..2] == [0x1f, 0x8b] {
                    scan.layer(GzDecoder::new(std::io::BufReader::new(file)))
                } else if n == 4 && magic == [0x28, 0xb5, 0x2f, 0xfd] {
                    scan.limitation(format!("not read: layer {} is zstd-compressed", index + 1));
                    scan.layer(std::io::empty())
                } else {
                    scan.layer(std::io::BufReader::new(file))
                };
                (scan, outcome)
            });
            let (returned, outcome) = task.await.map_err(|e| {
                unreadable(diagnosis::UNKNOWN, format!("the layer scan stopped: {e}"))
            })?;
            scan = returned;
            if let Err(error) = outcome {
                scan.limitation(format!("not read: layer {} ({error})", index + 1));
            } else {
                read += 1;
            }
        }

        let mut report = scan.finish();
        report.image = format!("{image}:{}", param(step, "tag"));
        report.digest = manifest_digest;
        report.platform = platform;
        report.layers = layers.len() as u32;
        report.layers_read = read;
        report.findings.extend(config_findings_found);
        let today = chrono::Utc::now().date_naive();
        rules::apply(
            &mut report,
            &rules::Context {
                today: (
                    chrono::Datelike::year(&today),
                    chrono::Datelike::month(&today),
                    chrono::Datelike::day(&today),
                ),
                repo_full_name: param(step, "repo"),
                commit_sha: param(step, "commit_sha"),
            },
        );
        self.vulnerabilities(&mut report, ctx).await;
        if let (Some(bin), Some(_dir)) = (&self.inspect_settings.rea_bin, &rea_dir) {
            let kept = std::mem::take(&mut *kept.lock().unwrap_or_else(|p| p.into_inner()));
            report.tools = rea_second_opinion(bin, &report, kept).await;
        }
        rules::finalize(&mut report, MAX_FINDINGS, MAX_COMPONENTS);
        Ok(report)
    }

    /// OSV for everything the image ships. Fails soft: no network, a slow
    /// or broken endpoint means a limitation, never a failed step.
    async fn vulnerabilities(&self, report: &mut Report, ctx: &RunContext) {
        let Some(base) = &self.inspect_settings.osv_url else {
            report
                .limitations
                .push("not looked up: OSV_API_URL is off".into());
            return;
        };
        let queries = osv::queries(report, MAX_OSV_QUERIES);
        if queries.is_empty() {
            return;
        }
        match tokio::time::timeout(OSV_BUDGET, self.ask_osv(base, &queries)).await {
            Ok(Ok(answers)) => {
                ctx.logs.line(format!(
                    "OSV: {} of {} components have known advisories",
                    answers.len(),
                    queries.len()
                ));
                osv::add_findings(report, &answers);
            }
            Ok(Err(error)) => report
                .limitations
                .push(format!("vulnerabilities not looked up: {error}")),
            Err(_) => report.limitations.push(format!(
                "vulnerabilities not looked up: OSV did not answer within {}s",
                OSV_BUDGET.as_secs()
            )),
        }
    }

    async fn ask_osv(
        &self,
        base: &str,
        queries: &[osv::Coordinates],
    ) -> Result<BTreeMap<osv::Coordinates, Vec<osv::Advisory>>, String> {
        let body = json!({
            "queries": queries.iter().map(|(ecosystem, name, version)| json!({
                "package": {"name": name, "ecosystem": ecosystem},
                "version": version,
            })).collect::<Vec<_>>()
        });
        let response = self
            .net
            .request(
                Method::POST,
                &format!("{base}/v1/querybatch"),
                REQUEST_TIMEOUT,
            )
            .await?
            .json(&body)
            .send()
            .await
            .map_err(net::transport_error)?;
        if !response.status().is_success() {
            return Err(format!("OSV answered {}", response.status()));
        }
        let value: Value = response
            .json()
            .await
            .map_err(|e| format!("OSV answer: {e}"))?;
        let results = value["results"].as_array().cloned().unwrap_or_default();
        if results.len() != queries.len() {
            return Err(format!(
                "OSV answered {} results for {} queries",
                results.len(),
                queries.len()
            ));
        }
        let ids: Vec<Vec<String>> = results
            .iter()
            .map(|r| {
                r["vulns"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v["id"].as_str().map(str::to_owned))
                    .collect()
            })
            .collect();
        let mut distinct: Vec<String> = Vec::new();
        for id in ids.iter().flatten() {
            if !distinct.contains(id) && distinct.len() < MAX_OSV_DETAILS {
                distinct.push(id.clone());
            }
        }
        let fetched = futures::future::join_all(distinct.iter().map(|id| async move {
            let url = format!("{base}/v1/vulns/{id}");
            let response = self
                .net
                .request(Method::GET, &url, REQUEST_TIMEOUT)
                .await
                .ok()?
                .send()
                .await
                .ok()?;
            if !response.status().is_success() {
                return None;
            }
            Some((id.clone(), response.json::<Value>().await.ok()?))
        }))
        .await;
        let details: BTreeMap<String, Value> = fetched.into_iter().flatten().collect();
        let mut answers = BTreeMap::new();
        for (coords, ids) in queries.iter().zip(ids) {
            if ids.is_empty() {
                continue;
            }
            let advisories = ids
                .iter()
                .map(|id| match details.get(id) {
                    Some(value) => osv::advisory(id, value, &coords.1),
                    None => osv::Advisory {
                        id: id.clone(),
                        ..osv::Advisory::default()
                    },
                })
                .collect();
            answers.insert(coords.clone(), advisories);
        }
        Ok(answers)
    }
}

/// Runs REA's own binary analyzers (`inspect-binary-hardening`, and
/// `inspect-go-binary` / `inspect-rust-binary` by language) over the kept
/// application binaries, each with a time limit. What it says is evidence
/// next to ours, bounded; it never fails the step.
async fn rea_second_opinion(
    bin: &std::path::Path,
    report: &Report,
    kept: Vec<(String, PathBuf, String)>,
) -> Vec<ToolRun> {
    let mut runs = Vec::new();
    for (path, file, _) in kept {
        let language = report
            .binaries
            .iter()
            .find(|b| b.path == path)
            .map_or("native", |b| b.language.as_str());
        let mut commands = vec!["inspect-binary-hardening"];
        match language {
            "go" => commands.push("inspect-go-binary"),
            "rust" => commands.push("inspect-rust-binary"),
            _ => {}
        }
        for command in commands {
            let mut child = tokio::process::Command::new(bin);
            child
                .arg(command)
                .arg(&file)
                .arg("--json")
                .kill_on_drop(true);
            let run = tokio::time::timeout(REA_TIMEOUT, child.output()).await;
            let (ok, summary) = match run {
                Ok(Ok(output)) => {
                    let text = if output.status.success() {
                        String::from_utf8_lossy(&output.stdout).into_owned()
                    } else {
                        String::from_utf8_lossy(&output.stderr).into_owned()
                    };
                    (output.status.success(), net::excerpt(text.trim(), 600))
                }
                Ok(Err(error)) => (false, format!("could not start {}: {error}", bin.display())),
                Err(_) => (
                    false,
                    format!("did not finish within {}s", REA_TIMEOUT.as_secs()),
                ),
            };
            runs.push(ToolRun {
                tool: "rea".into(),
                command: command.into(),
                path: path.clone(),
                ok,
                summary,
            });
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, io::Write, sync::Arc};

    use axum::{
        Json, Router,
        body::Bytes,
        extract::{Path, State},
        http::StatusCode as HttpStatus,
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    use flate2::{Compression, write::GzEncoder};

    use super::*;
    use crate::adapters::{
        Adapter, LogSink, OutputSink,
        net::{Net, NetPolicy},
    };

    #[test]
    fn an_index_resolves_to_linux_amd64_and_skips_attestations() {
        let index = json!({"manifests": [
            {"digest": "sha256:att", "platform": {"os": "unknown", "architecture": "unknown"},
             "annotations": {"vnd.docker.reference.type": "attestation-manifest"}},
            {"digest": "sha256:arm", "platform": {"os": "linux", "architecture": "arm64"}},
            {"digest": "sha256:amd", "platform": {"os": "linux", "architecture": "amd64"}}
        ]});
        assert_eq!(
            pick_platform(&index),
            Some(("sha256:amd".into(), "linux/amd64".into()))
        );
        let arm_only = json!({"manifests": [{"digest": "sha256:arm", "platform": {"os": "linux", "architecture": "arm64"}}]});
        assert_eq!(pick_platform(&arm_only).unwrap().1, "linux/arm64");
        assert_eq!(
            pick_platform(&json!({"manifests": [{"digest": "x", "platform": {"os": "windows"}}]})),
            None
        );
    }

    #[test]
    fn the_image_config_tells_who_runs_and_what_is_baked_in() {
        let root = config_findings(
            &json!({"config": {"Env": ["PATH=/usr/bin", "API_TOKEN=abc123", "EMPTY_SECRET="]}}),
        );
        let rules: Vec<&str> = root.iter().map(|f| f.rule.as_str()).collect();
        assert_eq!(rules, ["image.runs_as_root", "image.secret_in_env"]);
        assert!(root[1].detail.contains("API_TOKEN"));
        assert!(
            !root[1].detail.contains("abc123"),
            "values never reach the report"
        );
        assert!(!root[1].detail.contains("EMPTY_SECRET"));
        assert!(config_findings(&json!({"config": {"User": "10001"}})).is_empty());
        assert_eq!(
            config_findings(&json!({"config": {"User": "0:0"}})).len(),
            1
        );
    }

    fn sha(bytes: &[u8]) -> String {
        format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
    }

    /// One gzip layer of `(path, mode, content)` files.
    fn layer(files: &[(&str, u32, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, mode, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            builder.append_data(&mut header, path, *content).unwrap();
        }
        let tar = builder.into_inner().unwrap();
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        gz.write_all(&tar).unwrap();
        gz.finish().unwrap()
    }

    /// A registry holding one multi-platform image whose layer blob is
    /// redirected to storage, and an OSV that knows one advisory.
    struct Image {
        blobs: HashMap<String, Vec<u8>>,
        index: String,
        manifest: String,
        manifest_digest: String,
        calls: std::sync::Mutex<Vec<String>>,
    }

    fn image(tamper: bool) -> Image {
        let layer = layer(&[
            (
                "etc/os-release",
                0o644,
                b"PRETTY_NAME=\"Debian GNU/Linux 10 (buster)\"\nID=debian\nVERSION_ID=\"10\"\n",
            ),
            (
                "var/lib/dpkg/status",
                0o644,
                b"Package: openssl\nStatus: install ok installed\nVersion: 1.1.1n-0+deb10u3\n",
            ),
            (
                "root/.ssh/id_rsa",
                0o600,
                b"-----BEGIN RSA PRIVATE KEY-----\nMIIB\n-----END RSA PRIVATE KEY-----\n",
            ),
            ("app/.env", 0o644, b"DATABASE_PASSWORD=hunter2\n"),
        ]);
        let config = json!({"os": "linux", "architecture": "amd64", "config": {"User": "", "Env": ["PATH=/bin"]}}).to_string();
        let layer_digest = sha(&layer);
        let config_digest = sha(config.as_bytes());
        let manifest = json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {"mediaType": "application/vnd.oci.image.config.v1+json", "digest": config_digest, "size": config.len()},
            "layers": [{"mediaType": "application/vnd.oci.image.layer.v1.tar+gzip", "digest": layer_digest, "size": layer.len()}]
        })
        .to_string();
        let manifest_digest = sha(manifest.as_bytes());
        let index = json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [
                {"digest": "sha256:attestation", "platform": {"os": "unknown", "architecture": "unknown"},
                 "annotations": {"vnd.docker.reference.type": "attestation-manifest"}},
                {"digest": manifest_digest, "platform": {"os": "linux", "architecture": "amd64"}}
            ]
        })
        .to_string();
        let mut served_layer = layer.clone();
        if tamper {
            served_layer[20] ^= 0xff;
        }
        Image {
            blobs: HashMap::from([
                (layer_digest, served_layer),
                (config_digest, config.into_bytes()),
            ]),
            index,
            manifest,
            manifest_digest,
            calls: Default::default(),
        }
    }

    async fn manifests(State(image): State<Arc<Image>>, Path(reference): Path<String>) -> Response {
        image
            .calls
            .lock()
            .unwrap()
            .push(format!("manifest {reference}"));
        if reference == "a1b2c3d" {
            (HttpStatus::OK, image.index.clone()).into_response()
        } else if reference == image.manifest_digest {
            (HttpStatus::OK, image.manifest.clone()).into_response()
        } else {
            HttpStatus::NOT_FOUND.into_response()
        }
    }

    async fn blobs(State(image): State<Arc<Image>>, Path(digest): Path<String>) -> Response {
        image.calls.lock().unwrap().push(format!("blob {digest}"));
        match image.blobs.get(&digest) {
            // Layers live in storage, like every real registry's.
            Some(bytes) if bytes.len() > 300 => (
                HttpStatus::TEMPORARY_REDIRECT,
                [("location", format!("/storage/{digest}?sig=x"))],
            )
                .into_response(),
            Some(bytes) => (HttpStatus::OK, bytes.clone()).into_response(),
            None => HttpStatus::NOT_FOUND.into_response(),
        }
    }

    async fn storage(State(image): State<Arc<Image>>, Path(digest): Path<String>) -> Response {
        image.calls.lock().unwrap().push("storage".into());
        (HttpStatus::OK, image.blobs[&digest].clone()).into_response()
    }

    async fn querybatch(body: Bytes) -> Json<Value> {
        let request: Value = serde_json::from_slice(&body).unwrap();
        let results: Vec<Value> = request["queries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|q| {
                if q["package"]["name"] == "openssl" && q["package"]["ecosystem"] == "Debian:10" {
                    json!({"vulns": [{"id": "DEBIAN-CVE-2023-0286"}]})
                } else {
                    json!({})
                }
            })
            .collect();
        Json(json!({"results": results}))
    }

    async fn vuln(Path(id): Path<String>) -> Json<Value> {
        Json(json!({
            "id": id,
            "summary": "X.400 address type confusion",
            "affected": [{"package": {"name": "openssl", "ecosystem": "Debian:10"},
                          "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "1.1.1n-0+deb10u4"}]}]}]
        }))
    }

    async fn serve(image: Image) -> (Arc<Image>, String) {
        let image = Arc::new(image);
        let app = Router::new()
            .route("/v2/acme/api/manifests/{reference}", get(manifests))
            .route("/v2/acme/api/blobs/{digest}", get(blobs))
            .route("/storage/{digest}", get(storage))
            .route("/osv/v1/querybatch", post(querybatch))
            .route("/osv/v1/vulns/{id}", get(vuln))
            .with_state(Arc::clone(&image));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (image, addr)
    }

    fn adapter(addr: &str) -> RegistryAdapter {
        RegistryAdapter::new(Net::new(NetPolicy::ALLOW_PRIVATE), None).with_inspection(
            InspectSettings {
                osv_url: Some(format!("http://{addr}/osv")),
                ..InspectSettings::default()
            },
        )
    }

    fn step(addr: &str, mode: &str, extra: &[(&str, &str)]) -> StepInput {
        let mut params = BTreeMap::from([
            ("mode".to_owned(), mode.to_owned()),
            ("fail_on".to_owned(), "high".to_owned()),
            ("registry_kind".to_owned(), "generic".to_owned()),
            ("registry_host".to_owned(), addr.to_owned()),
            ("registry_auth".to_owned(), "github_token".to_owned()),
            ("image".to_owned(), format!("{addr}/acme/api")),
            ("tag".to_owned(), "a1b2c3d".to_owned()),
            ("repo".to_owned(), "acme/api".to_owned()),
            ("commit_sha".to_owned(), "a1b2c3d4e5f6".to_owned()),
        ]);
        for (k, v) in extra {
            params.insert((*k).to_owned(), (*v).to_owned());
        }
        StepInput {
            key: "inspect_image".into(),
            title: "Inspect the shipped image".into(),
            action_type: "artifact.inspect".into(),
            adapter: "registry".into(),
            params,
            attempt: 1,
        }
    }

    fn context() -> (RunContext, tokio::sync::mpsc::Receiver<(String, String)>) {
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(512);
        let (progress_tx, progress_rx) = tokio::sync::mpsc::channel(16);
        let ctx = RunContext {
            logs: LogSink::new(log_tx),
            progress: OutputSink::new(progress_tx),
            ..Default::default()
        };
        (ctx, progress_rx)
    }

    fn report_of(mut progress: tokio::sync::mpsc::Receiver<(String, String)>) -> Report {
        let (key, json) = progress
            .try_recv()
            .expect("the report is emitted as progress");
        assert_eq!(key, REPORT_OUTPUT);
        serde_json::from_str(&json).unwrap()
    }

    #[tokio::test]
    async fn report_mode_reads_the_released_image_and_lets_the_run_go_on() {
        let (image, addr) = serve(image(false)).await;
        let (ctx, progress) = context();
        let outcome = adapter(&addr).run(&step(&addr, "report", &[]), &ctx).await;
        let StepOutcome::Succeeded { output, outputs } = outcome else {
            panic!("report mode never stops the run: {outcome:?}");
        };
        assert_eq!(outputs["inspection_status"], "findings");
        assert_eq!(outputs["inspection_max_severity"], "critical");
        assert_eq!(outputs["inspection_blocking"], "0");
        assert!(output.contains("linux/amd64"), "{output}");
        let report = report_of(progress);
        assert_eq!(report.platform, "linux/amd64");
        assert_eq!(
            report.digest, image.manifest_digest,
            "the platform manifest that was read"
        );
        assert_eq!((report.layers, report.layers_read), (1, 1));
        let rules: Vec<&str> = report.findings.iter().map(|f| f.rule.as_str()).collect();
        for rule in [
            "image.private_key",
            "image.os_eol",
            "image.env_file",
            "image.runs_as_root",
            "dependency.vulnerable",
        ] {
            assert!(rules.contains(&rule), "{rule} in {rules:?}");
        }
        assert_eq!(rules[0], "image.private_key", "most severe first");
        let openssl = report
            .findings
            .iter()
            .find(|f| f.rule == "dependency.vulnerable")
            .unwrap();
        assert!(
            openssl.detail.contains("DEBIAN-CVE-2023-0286"),
            "{}",
            openssl.detail
        );
        assert!(openssl.fix.contains("1.1.1n-0+deb10u4"), "{}", openssl.fix);
        // Secrets are named, never copied.
        let json = serde_json::to_string(&report).unwrap();
        assert!(
            !json.contains("hunter2") && !json.contains("MIIB"),
            "{json}"
        );
        // The attestation entry was never fetched; the layer came from storage.
        let calls = image.calls.lock().unwrap().clone();
        assert!(
            !calls.iter().any(|c| c.contains("attestation")),
            "{calls:?}"
        );
        assert!(calls.contains(&"storage".to_owned()), "{calls:?}");
    }

    #[tokio::test]
    async fn gate_mode_stops_the_run_until_a_person_accepts() {
        let (_, addr) = serve(image(false)).await;
        let (ctx, progress) = context();
        let outcome = adapter(&addr).run(&step(&addr, "gate", &[]), &ctx).await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("the gate must stop the run: {outcome:?}");
        };
        assert_eq!(code, diagnosis::ARTIFACT_FINDINGS);
        assert!(raw_output.contains("at or above high"), "{raw_output}");
        assert!(
            raw_output.contains("[critical] RSA private key in the image"),
            "{raw_output}"
        );
        // The report still reaches the release.
        assert!(!report_of(progress).findings.is_empty());

        // Ignoring every family that fired lets it through.
        let (ctx, _) = context();
        let relaxed = step(
            &addr,
            "gate",
            &[("ignore", "image.*,dependency.vulnerable")],
        );
        assert!(matches!(
            adapter(&addr).run(&relaxed, &ctx).await,
            StepOutcome::Succeeded { .. }
        ));

        // "Deploy anyway": the same findings, accepted and recorded.
        let (ctx, _) = context();
        let accepted = step(&addr, "gate", &[("accept_findings", "true")]);
        let StepOutcome::Succeeded { output, outputs } = adapter(&addr).run(&accepted, &ctx).await
        else {
            panic!("an accepted gate lets the run continue");
        };
        assert_eq!(outputs["inspection_status"], "accepted");
        assert!(output.contains("accepted by a person"), "{output}");
    }

    #[tokio::test]
    async fn an_image_that_cannot_be_trusted_or_read_follows_the_mode() {
        // The registry served other bytes than the manifest names.
        let (_, addr) = serve(image(true)).await;
        let (ctx, _) = context();
        let StepOutcome::Failed {
            code, raw_output, ..
        } = adapter(&addr).run(&step(&addr, "gate", &[]), &ctx).await
        else {
            panic!("a tampered blob fails the gate");
        };
        assert_eq!(code, diagnosis::UNKNOWN);
        assert!(raw_output.contains("served other bytes"), "{raw_output}");
        // Report mode notes it and goes on.
        let (ctx, _) = context();
        let StepOutcome::Succeeded { outputs, output } =
            adapter(&addr).run(&step(&addr, "report", &[]), &ctx).await
        else {
            panic!("report mode goes on");
        };
        assert_eq!(outputs["inspection_status"], "skipped");
        assert!(output.contains("report mode"), "{output}");
        // A tag that is not there.
        let (ctx, _) = context();
        let missing = step(&addr, "gate", &[("tag", "0000000")]);
        let StepOutcome::Failed { code, .. } = adapter(&addr).run(&missing, &ctx).await else {
            panic!("a missing image fails the gate");
        };
        assert_eq!(code, diagnosis::IMAGE_NOT_FOUND);
        // An oidc registry cannot be read from here at all.
        let (ctx, _) = context();
        let oidc = step(&addr, "report", &[("registry_auth", "oidc")]);
        let StepOutcome::Succeeded { outputs, .. } = adapter(&addr).run(&oidc, &ctx).await else {
            panic!("report mode goes on");
        };
        assert_eq!(outputs["inspection_status"], "skipped");
    }
}
