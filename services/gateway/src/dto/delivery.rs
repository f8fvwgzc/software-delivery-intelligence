//! Continuous delivery (CONTRACT.md §22): registries, environments,
//! releases, deployment runs, the pipeline view, the setup files and
//! workspace secrets.
//!
//! Create and update bodies are flat objects with the fields of the entity
//! they write; lists are bare arrays. Strings the services leave empty are
//! `null` here when they stand for "not set" (a secret name, a URL, a
//! digest that is not known yet).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sdi_proto::{
    execution::v1 as proto, identity::v1 as identity, intelligence::v1 as intelligence,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{intelligence::Artifact, opt_string, opt_timestamp, proto_enum, timestamp};

/// `Environment.position` meaning "not said": an edit keeps the stage where
/// it is, a new environment goes after the last one. The execution service
/// reads the same value (`delivery::model::POSITION_UNSET`).
pub const POSITION_UNSET: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Registries
// ---------------------------------------------------------------------------

/// Where images live. One per workspace is the default; GHCR needs no secret.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    /// Registry id.
    pub id: String,
    /// Name shown in the workspace.
    pub name: String,
    /// `ghcr`, `dockerhub`, `acr`, `ecr`, `gar` or `generic`.
    pub kind: String,
    /// e.g. `ghcr.io`, `myorg.azurecr.io`.
    pub host: String,
    /// Org, user or project path images are pushed under.
    pub namespace: String,
    /// `github_token` (GHCR), `oidc` (cloud registries, reachable from CI only) or `secret`.
    pub auth: String,
    /// Name of the workspace secret holding the user name (never the value); null unless `auth` is `secret`.
    pub username_secret: Option<String>,
    /// Name of the workspace secret holding the password or token; null unless `auth` is `secret`.
    pub password_secret: Option<String>,
    /// True for the registry releases use when their environment names none.
    pub is_default: bool,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<proto::Registry> for Registry {
    fn from(registry: proto::Registry) -> Self {
        Self {
            id: registry.id,
            name: registry.name,
            kind: registry.kind,
            host: registry.host,
            namespace: registry.namespace,
            auth: registry.auth,
            username_secret: opt_string(registry.username_secret),
            password_secret: opt_string(registry.password_secret),
            is_default: registry.is_default,
            created_at: timestamp(registry.created_at.as_ref()),
            updated_at: timestamp(registry.updated_at.as_ref()),
        }
    }
}

/// Body of `POST /registries` and `PUT /registries/{id}` (admin).
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertRegistryRequest {
    /// Name shown in the workspace.
    pub name: String,
    /// `ghcr`, `dockerhub`, `acr`, `ecr`, `gar` or `generic`.
    pub kind: String,
    /// Registry host; `ghcr` and `dockerhub` have a default.
    pub host: Option<String>,
    /// Org, user or project path images are pushed under.
    pub namespace: Option<String>,
    /// `github_token`, `oidc` or `secret`; `ghcr` defaults to `github_token`.
    pub auth: Option<String>,
    /// Name of a workspace secret (for `auth: secret`).
    pub username_secret: Option<String>,
    /// Name of a workspace secret (for `auth: secret`).
    pub password_secret: Option<String>,
    /// Make it the workspace default (the previous default loses the flag).
    pub is_default: Option<bool>,
}

impl UpsertRegistryRequest {
    /// The proto message; `id` is empty to create.
    pub fn into_proto(self, id: String) -> proto::Registry {
        proto::Registry {
            id,
            name: self.name,
            kind: self.kind,
            host: self.host.unwrap_or_default(),
            namespace: self.namespace.unwrap_or_default(),
            auth: self.auth.unwrap_or_default(),
            username_secret: self.username_secret.unwrap_or_default(),
            password_secret: self.password_secret.unwrap_or_default(),
            is_default: self.is_default.unwrap_or(false),
            created_at: None,
            updated_at: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Environments
// ---------------------------------------------------------------------------

/// A place a release runs, and how it gets there.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    /// Environment id.
    pub id: String,
    /// Project it belongs to.
    pub project_id: String,
    /// Name, unique within the project.
    pub name: String,
    /// Promotion order, 0 first.
    pub position: u32,
    /// Deployer: `github_actions`, `gitops_flux`, `gitops_argocd`, `jenkins`, `azure_devops`, `webhook` or `docker_local`.
    pub kind: String,
    /// Deployer settings; keys per kind in CONTRACT.md §22. Secrets are named, never held.
    pub config: BTreeMap<String, String>,
    /// Where the environment is reachable; null when not configured.
    pub url: Option<String>,
    /// Path polled on `url` after a deploy; null when not configured.
    pub health_path: Option<String>,
    /// Registry its images come from; null uses the workspace default.
    pub registry_id: Option<String>,
    /// Production semantics: `deploy.production` policy (approval always required).
    pub protected: bool,
    /// Promote here automatically when the previous stage succeeds (still gated).
    pub auto_promote: bool,
    /// `ui`, or `config` when declared in `.sdi.yml` (read-only here).
    pub source: String,
    /// The release that runs there now; null before the first successful deployment.
    pub current_release_id: Option<String>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<proto::Environment> for Environment {
    fn from(environment: proto::Environment) -> Self {
        Self {
            id: environment.id,
            project_id: environment.project_id,
            name: environment.name,
            position: environment.position,
            kind: environment.kind,
            config: environment.config.into_iter().collect(),
            url: opt_string(environment.url),
            health_path: opt_string(environment.health_path),
            registry_id: opt_string(environment.registry_id),
            protected: environment.protected,
            auto_promote: environment.auto_promote,
            source: environment.source,
            current_release_id: opt_string(environment.current_release_id),
            created_at: timestamp(environment.created_at.as_ref()),
            updated_at: timestamp(environment.updated_at.as_ref()),
        }
    }
}

/// Body of `POST /projects/{id}/environments` and `PUT /environments/{id}` (admin).
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertEnvironmentRequest {
    /// Name, unique within the project.
    pub name: String,
    /// Deployer kind.
    pub kind: String,
    /// Deployer settings; unknown keys are refused by name.
    pub config: Option<BTreeMap<String, String>>,
    /// Where the environment is reachable.
    pub url: Option<String>,
    /// Path polled on `url` after a deploy.
    pub health_path: Option<String>,
    /// Registry its images come from; null uses the workspace default.
    pub registry_id: Option<String>,
    /// Production semantics.
    pub protected: Option<bool>,
    /// Promote here automatically when the previous stage succeeds.
    pub auto_promote: Option<bool>,
    /// Promotion order, 0 first. Omitted: a new environment goes last, an edited one stays where it is.
    pub position: Option<u32>,
}

impl UpsertEnvironmentRequest {
    /// The proto message; `id` is empty to create, `project_id` empty to
    /// edit (an environment never changes project).
    pub fn into_proto(self, id: String, project_id: String) -> proto::Environment {
        proto::Environment {
            id,
            project_id,
            name: self.name,
            position: self.position.unwrap_or(POSITION_UNSET),
            kind: self.kind,
            config: self.config.unwrap_or_default().into_iter().collect(),
            url: self.url.unwrap_or_default(),
            health_path: self.health_path.unwrap_or_default(),
            registry_id: self.registry_id.unwrap_or_default(),
            protected: self.protected.unwrap_or(false),
            auto_promote: self.auto_promote.unwrap_or(false),
            // Only `.sdi.yml` makes config-managed environments.
            source: String::new(),
            current_release_id: String::new(),
            created_at: None,
            updated_at: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Releases
// ---------------------------------------------------------------------------

/// One immutable image at one commit.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    /// Release id.
    pub id: String,
    /// Project it belongs to.
    pub project_id: String,
    /// Tag shown to people, e.g. `v1.4.2` or the short SHA.
    pub version: String,
    /// Commit the image was built from.
    pub commit_sha: String,
    /// `host/namespace/name`, without tag.
    pub image: String,
    /// Image tag.
    pub tag: String,
    /// `sha256:…` once known; null before the registry or the build reported it.
    pub digest: Option<String>,
    /// `pipeline`, `package` or `manual`.
    pub source: String,
    /// The CI run that built it, when a webhook created the release.
    pub pipeline_run_id: Option<String>,
    /// Commit subjects since the previous release, Markdown.
    pub notes: String,
    /// The registry confirmed the manifest exists.
    pub verified: bool,
    /// Who created it (`system` for webhooks).
    pub created_by: Option<String>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// Names of the environments this release currently runs in, in promotion order.
    pub deployed_to: Vec<String>,
    /// The last inspection of the image (CONTRACT.md §22.1); null when it
    /// was never inspected. The full report is only on `GET /releases/{id}`.
    pub inspection: Option<ReleaseInspection>,
}

/// What reverse-engineering the release image found (CONTRACT.md §22.1).
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInspection {
    /// `passed`, `findings` or `incomplete`.
    pub status: String,
    /// Most severe finding: `info`, `low`, `medium`, `high` or `critical`; null without findings.
    pub max_severity: Option<String>,
    /// Findings in the report.
    pub finding_count: u32,
    /// When the report was made.
    pub inspected_at: DateTime<Utc>,
    /// The whole report; only on `GET /releases/{id}`.
    pub report: Option<InspectionReport>,
}

/// One inspection of one image: what was read, what ships, what was found.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectionReport {
    /// Version of this shape.
    pub schema: u32,
    /// `host/namespace/name:tag` as inspected.
    pub image: String,
    /// Digest of the platform manifest that was read.
    pub digest: String,
    /// `linux/amd64`.
    pub platform: String,
    /// Layers the image has.
    pub layers: u32,
    /// Layers that were read.
    pub layers_read: u32,
    /// Files looked at across layers.
    pub files_seen: u64,
    /// Bytes read out of those files.
    pub bytes_read: u64,
    /// The distribution, from `/etc/os-release`.
    #[serde(default)]
    pub os: Option<InspectionOs>,
    /// Application binaries and binaries that carry their own dependency list.
    pub binaries: Vec<InspectedBinary>,
    /// Packages the image ships, wherever they were read.
    pub components: Vec<InspectedComponent>,
    /// Most severe first.
    pub findings: Vec<InspectionFinding>,
    /// What was not read, and why.
    pub limitations: Vec<String>,
    /// External analyzers that ran (the REA CLI).
    #[serde(default)]
    pub tools: Vec<InspectionToolRun>,
}

/// `/etc/os-release` of the image.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectionOs {
    /// `debian`, `alpine`, …
    pub id: String,
    /// `12`, `3.20.3`.
    pub version_id: String,
    /// `Debian GNU/Linux 12 (bookworm)`.
    pub pretty_name: String,
}

/// One ELF executable or shared object.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectedBinary {
    /// Path in the image.
    pub path: String,
    /// `elf`.
    pub format: String,
    /// `x86_64`, `aarch64`, …
    pub machine: String,
    /// 32 or 64.
    pub bits: u8,
    /// `executable`, `pie_executable`, `static_pie_executable`, `shared_object`.
    pub kind: String,
    /// `go`, `rust` or `native`.
    pub language: String,
    /// Bytes.
    pub size: u64,
    /// GNU build-id, hex.
    #[serde(default)]
    pub build_id: String,
    /// Compiler that built it (`go1.22.5`).
    #[serde(default)]
    pub toolchain: String,
    /// Main module or root crate.
    #[serde(default)]
    pub main_module: String,
    /// Selected build settings (`vcs.revision`, `vcs.modified`, `CGO_ENABLED`, …).
    #[serde(default)]
    pub build_settings: BTreeMap<String, String>,
    /// Dependencies embedded in the binary.
    pub dependency_count: u32,
    /// The binary carries its own dependency list (Go build info, cargo-auditable).
    #[serde(default)]
    pub embedded_dependencies: bool,
    /// Written by the team (not the distribution's, not a third-party package's).
    pub application: bool,
    /// Hardening properties with their state and evidence.
    pub hardening: Vec<HardeningProperty>,
}

/// One hardening property, REA's evidence model.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HardeningProperty {
    /// `pie`, `nx`, `relro`, `stack_protector`, `fortify`, `rpath`, `runpath`, `stripped`.
    pub property: String,
    /// `enabled`, `disabled`, `partial`, `unknown` or `not_applicable`.
    pub state: String,
    /// `observed` (read from the bytes) or `derived` (reasoned from them).
    pub confidence: String,
    /// The bytes the state was read from, in words.
    pub basis: String,
}

/// A package the image ships.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectedComponent {
    /// OSV ecosystem: `Go`, `crates.io`, `npm`, `PyPI`, `Debian:12`, `Alpine:v3.20`, …
    pub ecosystem: String,
    /// Package name.
    pub name: String,
    /// Version as shipped.
    pub version: String,
    /// File it was read from.
    pub path: String,
    /// `go-buildinfo`, `go-toolchain`, `cargo-auditable`, `package.json`, `dist-info`, `dpkg`, `apk`.
    pub source: String,
}

/// One finding of an inspection.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectionFinding {
    /// Stable rule id (`binary.exec_stack`, `dependency.vulnerable`): what `inspect_ignore` names.
    pub rule: String,
    /// `info`, `low`, `medium`, `high` or `critical`.
    pub severity: String,
    /// One line.
    pub title: String,
    /// File in the image; empty for image-wide findings.
    #[serde(default)]
    pub path: String,
    /// The evidence.
    pub detail: String,
    /// `observed` or `derived`.
    pub confidence: String,
    /// What would make it go away.
    pub fix: String,
}

/// An external analyzer run and what it said.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectionToolRun {
    /// `rea`.
    pub tool: String,
    /// The command (`inspect-binary-hardening`).
    pub command: String,
    /// Binary it ran on.
    pub path: String,
    /// It exited successfully.
    pub ok: bool,
    /// Bounded excerpt of what it printed.
    pub summary: String,
}

impl From<proto::ReleaseInspection> for ReleaseInspection {
    fn from(inspection: proto::ReleaseInspection) -> Self {
        let report = (!inspection.report_json.is_empty())
            .then(|| serde_json::from_str(&inspection.report_json).ok())
            .flatten();
        Self {
            status: inspection.status,
            max_severity: opt_string(inspection.max_severity),
            finding_count: inspection.finding_count,
            inspected_at: timestamp(inspection.inspected_at.as_ref()),
            report,
        }
    }
}

impl From<proto::Release> for Release {
    fn from(release: proto::Release) -> Self {
        Self {
            id: release.id,
            project_id: release.project_id,
            version: release.version,
            commit_sha: release.commit_sha,
            image: release.image,
            tag: release.tag,
            digest: opt_string(release.digest),
            source: release.source,
            pipeline_run_id: opt_string(release.pipeline_run_id),
            notes: release.notes,
            verified: release.verified,
            created_by: opt_string(release.created_by),
            created_at: timestamp(release.created_at.as_ref()),
            deployed_to: release.deployed_to,
            inspection: release.inspection.map(ReleaseInspection::from),
        }
    }
}

/// Body of `POST /projects/{id}/releases` (member). Every field is
/// optional: an empty body releases the default branch head.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateReleaseRequest {
    /// Version shown to people; defaults to the tag.
    pub version: Option<String>,
    /// Commit to release; defaults to the default branch head.
    pub commit_sha: Option<String>,
    /// `host/namespace/name`; defaults to the default registry and the repository name.
    pub image: Option<String>,
    /// Image tag; defaults to the short commit SHA.
    pub tag: Option<String>,
    /// `sha256:…`, when the build reported it.
    pub digest: Option<String>,
}

/// Query of `GET /projects/{id}/releases`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListReleasesQuery {
    /// Page size, newest first (default 50, max 200).
    pub limit: Option<u32>,
    /// Cursor: releases created strictly before this time.
    pub before: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Deployment runs
// ---------------------------------------------------------------------------

proto_enum! {
    /// Where a deployment run stands. Derived from the execution that
    /// carries it: `failed` with `finishedAt: null` is a run paused on a
    /// failure that still waits for a remediation.
    DeploymentRunStatus => proto::DeploymentStatus {
        Pending, AwaitingApproval, Deploying, Verifying, Succeeded, Failed, Cancelled
    }
}

/// A release going to an environment, backed by a gated execution.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentRun {
    /// Run id.
    pub id: String,
    /// Project it belongs to.
    pub project_id: String,
    /// Environment it deploys to.
    pub environment_id: String,
    /// Environment name when the run started.
    pub environment_name: String,
    /// Release it deploys.
    pub release_id: String,
    /// Release version when the run started.
    pub release_version: String,
    /// `promote` or `rollback`.
    pub kind: String,
    /// Status.
    pub status: DeploymentRunStatus,
    /// The execution that carries the steps, approvals and logs.
    pub execution_id: String,
    /// Workflow run, pull request or build URL at the deployer; null until the deployer reported one.
    pub external_url: Option<String>,
    /// What the environment ran when the run started.
    pub previous_release_id: Option<String>,
    /// Who started it (`system` for automatic promotions).
    pub started_by: Option<String>,
    /// When it started.
    pub created_at: DateTime<Utc>,
    /// When it ended; null while it is open.
    pub finished_at: Option<DateTime<Utc>>,
    /// Seconds from start to end (to now while it is open).
    pub duration_seconds: u32,
}

impl From<proto::DeploymentRun> for DeploymentRun {
    fn from(run: proto::DeploymentRun) -> Self {
        Self {
            id: run.id,
            project_id: run.project_id,
            environment_id: run.environment_id,
            environment_name: run.environment_name,
            release_id: run.release_id,
            release_version: run.release_version,
            kind: run.kind,
            status: run.status.into(),
            execution_id: run.execution_id,
            external_url: opt_string(run.external_url),
            previous_release_id: opt_string(run.previous_release_id),
            started_by: opt_string(run.started_by),
            created_at: timestamp(run.created_at.as_ref()),
            finished_at: opt_timestamp(run.finished_at.as_ref()),
            duration_seconds: run.duration_seconds,
        }
    }
}

/// Body of `POST /environments/{id}/promote` (approver).
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PromoteRequest {
    /// Release to deploy; null promotes the stage's candidate.
    pub release_id: Option<String>,
}

/// Body of `POST /environments/{id}/rollback` (approver).
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RollbackRequest {
    /// Release to go back to; null picks the previous successful one.
    pub release_id: Option<String>,
    /// Why, for the audit trail.
    pub reason: Option<String>,
}

/// Query of `GET /projects/{id}/deployment-runs`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListDeploymentRunsQuery {
    /// Only runs of this environment.
    pub environment_id: Option<String>,
    /// Page size, newest first (default 50, max 200).
    pub limit: Option<u32>,
    /// Cursor: runs started strictly before this time.
    pub before: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// One stage of a project's pipeline.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStage {
    /// The environment.
    pub environment: Environment,
    /// What runs there now.
    pub current: Option<Release>,
    /// The latest deployment to it.
    pub last_run: Option<DeploymentRun>,
    /// Newest release ahead of `current` that already succeeded in the previous stage (any release for the first stage).
    pub candidate: Option<Release>,
    /// `healthy`, `unhealthy` or `unknown` (no URL configured): a live check, cached for 30 seconds.
    pub health: String,
}

/// Environments in order, what runs in each, and what could be promoted next.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryPipeline {
    /// Project.
    pub project_id: String,
    /// Stages in promotion order.
    pub stages: Vec<PipelineStage>,
    /// The workspace's default registry.
    pub registry: Option<Registry>,
    /// 0.0 – 1.0 over deployment runs that finished in the last 30 days.
    pub success_rate: f64,
    /// Deployment runs started in the last 30 days.
    #[serde(rename = "deployments30d")]
    pub deployments_30d: u32,
    /// Runs behind `successRate`: finished in the last 30 days. A paused run
    /// is not finished, so a rate over zero runs means "no data yet".
    #[serde(rename = "finishedRuns30d")]
    pub finished_runs_30d: u32,
    /// Median seconds from a release being created to the last stage succeeding (0 when nothing got there).
    pub lead_time_seconds: u32,
}

impl From<proto::PipelineStage> for PipelineStage {
    fn from(stage: proto::PipelineStage) -> Self {
        Self {
            environment: stage.environment.unwrap_or_default().into(),
            current: stage.current.map(Into::into),
            last_run: stage.last_run.map(Into::into),
            candidate: stage.candidate.map(Into::into),
            health: stage.health,
        }
    }
}

impl From<proto::DeliveryPipeline> for DeliveryPipeline {
    fn from(pipeline: proto::DeliveryPipeline) -> Self {
        Self {
            project_id: pipeline.project_id,
            stages: pipeline.stages.into_iter().map(Into::into).collect(),
            registry: pipeline.registry.map(Into::into),
            success_rate: pipeline.success_rate,
            deployments_30d: pipeline.deployments_30d,
            finished_runs_30d: pipeline.finished_30d,
            lead_time_seconds: pipeline.lead_time_seconds,
        }
    }
}

// ---------------------------------------------------------------------------
// Setup files
// ---------------------------------------------------------------------------

/// The files that wire a project's CI to its registry and its
/// environments' deployers, generated from facts (CONTRACT.md §14, §22).
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryArtifacts {
    /// The files, complete and ready to commit.
    pub artifacts: Vec<Artifact>,
    /// Plain sentences: what each file does and what must be configured.
    pub notes: Vec<String>,
    /// Secret names the generated files reference.
    pub required_secrets: Vec<String>,
}

impl From<intelligence::DeliveryArtifacts> for DeliveryArtifacts {
    fn from(artifacts: intelligence::DeliveryArtifacts) -> Self {
        Self {
            artifacts: artifacts.artifacts.into_iter().map(Into::into).collect(),
            notes: artifacts.notes,
            required_secrets: artifacts.required_secrets,
        }
    }
}

// ---------------------------------------------------------------------------
// Workspace secrets
// ---------------------------------------------------------------------------

/// A workspace secret as the API shows it: the value never leaves the
/// identity service.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretInfo {
    /// Upper snake case, e.g. `JENKINS_TOKEN`.
    pub name: String,
    /// Last 4 characters of the value, for recognition only; null for short values.
    pub hint: Option<String>,
    /// What it is for.
    pub description: Option<String>,
    /// Login of whoever stored it last.
    pub updated_by: Option<String>,
    /// When it was stored last.
    pub updated_at: DateTime<Utc>,
}

impl From<identity::SecretInfo> for SecretInfo {
    fn from(secret: identity::SecretInfo) -> Self {
        Self {
            name: secret.name,
            hint: opt_string(secret.hint),
            description: opt_string(secret.description),
            updated_by: opt_string(secret.updated_by),
            updated_at: timestamp(secret.updated_at.as_ref()),
        }
    }
}

/// Body of `PUT /workspaces/{id}/secrets/{name}` (admin). The value is
/// write-only.
#[derive(Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetSecretRequest {
    /// The secret value; stored encrypted, never returned.
    pub value: String,
    /// What it is for.
    pub description: Option<String>,
}

impl std::fmt::Debug for SetSecretRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The value never reaches a log line.
        f.debug_struct("SetSecretRequest")
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn bodies_are_flat_and_omitted_fields_mean_not_said() {
        let body: UpsertEnvironmentRequest = serde_json::from_value(json!({
            "name": "staging",
            "kind": "github_actions",
            "config": {"workflow": "deploy.yml"},
            "url": "",
            "healthPath": "/health",
            "registryId": null,
            "protected": false,
            "autoPromote": true,
        }))
        .unwrap();
        let environment = body.into_proto("e1".into(), String::new());
        assert_eq!(environment.id, "e1");
        assert_eq!(
            environment.position, POSITION_UNSET,
            "an edit keeps the stage in place"
        );
        assert_eq!(environment.config["workflow"], "deploy.yml");
        assert_eq!(environment.health_path, "/health");
        assert!(environment.registry_id.is_empty());
        assert!(environment.auto_promote);
        assert!(
            environment.source.is_empty(),
            "a body cannot claim to be config-managed"
        );

        let body: UpsertEnvironmentRequest = serde_json::from_value(json!({
            "name": "production", "kind": "webhook", "position": 0, "source": "config",
        }))
        .unwrap();
        let environment = body.into_proto(String::new(), "p1".into());
        assert_eq!(environment.position, 0, "0 is the first stage, not 'unset'");
        assert!(environment.source.is_empty());

        let body: UpsertRegistryRequest = serde_json::from_value(json!({
            "name": "acr", "kind": "acr", "host": "acme.azurecr.io", "namespace": "shop",
            "auth": "secret", "usernameSecret": "ACR_USER", "passwordSecret": "ACR_PASSWORD",
            "isDefault": true,
        }))
        .unwrap();
        let registry = body.into_proto(String::new());
        assert_eq!(registry.username_secret, "ACR_USER");
        assert!(registry.is_default);
        let minimal: UpsertRegistryRequest =
            serde_json::from_value(json!({"name": "ghcr", "kind": "ghcr", "usernameSecret": null}))
                .unwrap();
        assert!(minimal.into_proto("r1".into()).host.is_empty());
    }

    #[test]
    fn responses_use_the_contract_names_and_null_what_is_not_set() {
        let pipeline = DeliveryPipeline::from(proto::DeliveryPipeline {
            project_id: "p1".into(),
            stages: vec![proto::PipelineStage {
                environment: Some(proto::Environment {
                    id: "e1".into(),
                    name: "staging".into(),
                    kind: "webhook".into(),
                    source: "ui".into(),
                    ..Default::default()
                }),
                last_run: Some(proto::DeploymentRun {
                    id: "d1".into(),
                    status: proto::DeploymentStatus::AwaitingApproval as i32,
                    execution_id: "x1".into(),
                    ..Default::default()
                }),
                health: "unknown".into(),
                ..Default::default()
            }],
            registry: None,
            success_rate: 0.75,
            deployments_30d: 8,
            lead_time_seconds: 3600,
            finished_30d: 4,
        });
        let json = serde_json::to_value(&pipeline).unwrap();
        assert_eq!(json["successRate"], 0.75);
        assert_eq!(json["deployments30d"], 8);
        assert_eq!(json["finishedRuns30d"], 4);
        assert_eq!(json["leadTimeSeconds"], 3600);
        assert_eq!(json["registry"], serde_json::Value::Null);
        let stage = &json["stages"][0];
        for key in ["environment", "current", "lastRun", "candidate", "health"] {
            assert!(stage.get(key).is_some(), "{key}");
        }
        assert_eq!(stage["current"], serde_json::Value::Null);
        assert_eq!(stage["lastRun"]["status"], "awaiting_approval");
        assert_eq!(stage["lastRun"]["executionId"], "x1");
        assert_eq!(stage["lastRun"]["externalUrl"], serde_json::Value::Null);
        assert_eq!(stage["lastRun"]["finishedAt"], serde_json::Value::Null);
        assert_eq!(stage["environment"]["registryId"], serde_json::Value::Null);
        assert_eq!(
            stage["environment"]["currentReleaseId"],
            serde_json::Value::Null
        );
        assert_eq!(stage["environment"]["autoPromote"], false);

        let release = serde_json::to_value(Release::from(proto::Release {
            id: "r1".into(),
            deployed_to: vec!["staging".into()],
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(release["digest"], serde_json::Value::Null);
        assert_eq!(release["deployedTo"], json!(["staging"]));
        assert_eq!(release["commitSha"], "");
    }

    #[test]
    fn a_secret_value_is_not_part_of_any_debug_output_or_response() {
        let body: SetSecretRequest =
            serde_json::from_value(json!({"value": "s3cr3t-value-123456", "description": "CI"}))
                .unwrap();
        assert!(!format!("{body:?}").contains("s3cr3t"));
        let info = serde_json::to_value(SecretInfo::from(identity::SecretInfo {
            name: "JENKINS_TOKEN".into(),
            hint: "3456".into(),
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(info["name"], "JENKINS_TOKEN");
        assert_eq!(info["hint"], "3456");
        assert!(info.get("value").is_none());
        assert_eq!(info["description"], serde_json::Value::Null);
    }
}
