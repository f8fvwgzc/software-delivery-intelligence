//! Input and output types of the analyzer.
//!
//! These are plain data: the knowledge service fills a [`RepoSnapshot`] from
//! the provider API, calls [`crate::analyze_at`], and persists the [`Analysis`].
//! Field names deliberately mirror `sdi.knowledge.v1` so the mapping is 1:1.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// Everything known about a repository at one commit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoSnapshot {
    /// `owner/name`.
    pub full_name: String,
    pub description: String,
    pub default_branch: String,
    pub commit_sha: String,
    pub is_private: bool,
    /// Primary language as reported by the provider. May be empty.
    pub primary_language: String,
    /// Language → bytes of code, as reported by the provider.
    pub languages: BTreeMap<String, u64>,
    pub topics: Vec<String>,
    /// Every path in the repository, files and directories.
    pub tree: Vec<TreeEntry>,
    /// Contents of the files chosen by [`crate::wanted_files`], keyed by path.
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
}

// ---------------------------------------------------------------------------
// Facts
// ---------------------------------------------------------------------------

/// Detected facts are read straight from the repository. Inferred facts are
/// conclusions drawn from them. The two are never mixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FactKind {
    Detected,
    Inferred,
    /// Declared by the repository's `.sdi.yml`. Trusted like detected, shown apart.
    Configured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FactCategory {
    Repository,
    Language,
    Framework,
    Datastore,
    Container,
    CiCd,
    Iac,
    Cloud,
    Testing,
    Dependency,
    Security,
    Documentation,
    Deployment,
    Migration,
    /// Runtime versions and their end-of-life status.
    Runtime,
    /// Values declared in `.sdi.yml`.
    Configuration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactDraft {
    pub kind: FactKind,
    pub category: FactCategory,
    /// Stable machine key, unique per analysis, e.g. `framework:react`.
    pub key: String,
    pub label: String,
    pub value: String,
    /// `0.0..=1.0`. Detected facts are `1.0`.
    pub confidence: f64,
    /// Empty when the fact does not come from one file.
    pub source_path: String,
    /// Short excerpt that proves the fact. Never a secret.
    pub evidence: String,
}

// ---------------------------------------------------------------------------
// Knowledge graph
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    Repository,
    Service,
    Library,
    Datastore,
    Infrastructure,
    Pipeline,
    Environment,
    Deployment,
    ContainerImage,
    IacModule,
    TestSuite,
    Document,
    ExternalApi,
    Owner,
    Migration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeDraft {
    pub kind: NodeKind,
    /// Stable within a project, unique per analysis, e.g. `service:api`.
    pub key: String,
    pub label: String,
    pub description: String,
    pub source_path: String,
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationCategory {
    Ownership,
    Structure,
    Source,
    Execution,
    Infrastructure,
    Validation,
    Governance,
    Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeDraft {
    /// [`NodeDraft::key`] of the source node. Must exist in the same analysis.
    pub source_key: String,
    /// [`NodeDraft::key`] of the target node. Must exist in the same analysis.
    pub target_key: String,
    /// Upper snake case verb, e.g. `DEPENDS_ON`, `DEPLOYED_TO`, `TESTED_BY`.
    pub relation: String,
    pub category: RelationCategory,
    pub attributes: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Dependencies and pipelines
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyDraft {
    /// `npm` | `cargo` | `pip` | `go` | `maven` | `docker` | `github_actions` | ...
    pub ecosystem: String,
    pub name: String,
    pub version: String,
    pub manifest_path: String,
    /// `runtime` | `dev` | `build`
    pub scope: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependabotDraft {
    pub configured: bool,
    pub config_path: String,
    pub ecosystems: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineJobDraft {
    pub key: String,
    pub name: String,
    pub runs_on: String,
    pub needs: Vec<String>,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineDraft {
    /// `github_actions` | `gitlab_ci` | ...
    pub provider: String,
    pub name: String,
    pub path: String,
    pub triggers: Vec<String>,
    pub jobs: Vec<PipelineJobDraft>,
}

// ---------------------------------------------------------------------------
// Questions and understanding
// ---------------------------------------------------------------------------

/// Something the repository cannot tell us. Only asked when it truly cannot
/// be detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionDraft {
    /// Stable machine key, e.g. `deployment_target`.
    pub key: String,
    pub prompt: String,
    /// Why it could not be detected.
    pub reason: String,
    pub options: Vec<String>,
    pub allow_free_text: bool,
    /// Pre-filled when `.sdi.yml` answers the question; empty otherwise.
    pub answer: String,
    /// `"config"` when pre-filled; empty otherwise.
    pub answered_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionDraft {
    pub key: String,
    pub title: String,
    pub summary: String,
    pub items: Vec<String>,
}

/// TL;DR first, deep dive underneath.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnderstandingDraft {
    pub tldr: String,
    pub architecture: String,
    pub stack: Vec<String>,
    pub sections: Vec<SectionDraft>,
    /// Ordered hops a request takes, e.g. `["Browser", "React", "API", "PostgreSQL"]`.
    pub request_flow: Vec<String>,
}

// ---------------------------------------------------------------------------
// Semantic memory and learned patterns
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChunkKind {
    Code,
    Iac,
    Document,
    CiCd,
    Architecture,
}

/// A piece of text worth embedding. Chunks never contain secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkDraft {
    pub kind: ChunkKind,
    pub path: String,
    pub title: String,
    pub content: String,
}

/// A way of working this repository already follows, worth remembering so
/// other projects can reuse it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearnedPatternDraft {
    /// Stable key built from what defines the pattern, e.g. `deployment:docker+github_actions`.
    pub key: String,
    pub name: String,
    pub summary: String,
    /// `deployment` | `ci_cd` | `testing` | `infrastructure` | `security` | `documentation` | `dependency`
    pub category: String,
    pub technologies: Vec<String>,
    pub cloud: String,
    pub runtime: String,
    pub iac: String,
    pub delivery: String,
    /// Capability keys, see [`Analysis::capabilities`].
    pub requires: Vec<String>,
}

// ---------------------------------------------------------------------------
// Delivery maturity and repository configuration (CONTRACT.md §12, §13)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaturityAxisDraft {
    /// `containerization` | `ci` | `tests` | `iac` | `dependencies` | `docs` | `secrets` | `runtime`
    pub key: String,
    pub label: String,
    /// 0–100
    pub score: u32,
    pub summary: String,
    /// Plain sentences naming the evidence.
    pub findings: Vec<String>,
    /// Pattern ids that would raise this axis, best first.
    pub recommended_patterns: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaturityDraft {
    /// Weighted mean of the axes, 0–100.
    pub overall: u32,
    /// `legacy` (< 35) | `transitional` (35–69) | `modern` (≥ 70)
    pub stage: String,
    pub axes: Vec<MaturityAxisDraft>,
}

/// What `.sdi.yml` declared. `present=false` means no file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectConfigDraft {
    pub present: bool,
    pub path: String,
    pub deploy_target: String,
    pub environments: Vec<String>,
    pub region: String,
    pub budget_monthly_usd: f64,
    pub test_command: String,
    pub health_path: String,
    pub port: u32,
    pub scan_exclude: Vec<String>,
    /// The `review:` section (CONTRACT.md §12), applied by the code review.
    #[serde(default)]
    pub review: ReviewConfigDraft,
    /// The `delivery:` section (CONTRACT.md §22) as canonical JSON: a
    /// [`DeliveryConfigDraft`] with every key present, keys in declaration
    /// order and `config` maps sorted. Empty when the section is absent.
    #[serde(default)]
    pub delivery_json: String,
    /// Parse problems, in plain words. The config is ignored when non-empty.
    pub errors: Vec<String>,
}

/// Registry kinds a `delivery.registry` may name (CONTRACT.md §22).
pub const REGISTRY_KINDS: &[&str] = &["ghcr", "dockerhub", "acr", "ecr", "gar", "generic"];

/// Deployer kinds a `delivery.environments[].kind` may name (CONTRACT.md §22).
pub const DEPLOYER_KINDS: &[&str] = &[
    "github_actions",
    "gitops_flux",
    "gitops_argocd",
    "jenkins",
    "azure_devops",
    "webhook",
    "docker_local",
    // SDI's own GitOps engine (CONTRACT.md §24): an application on a registered cluster.
    "sdi_gitops",
];

/// `.sdi.yml` `delivery:` — where images go and which environments a
/// release moves through. This is the shape of
/// [`ProjectConfigDraft::delivery_json`]; the knowledge service turns the
/// environments into `Execution.SyncEnvironments` and the generators read
/// the registry and the build workflow.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeliveryConfigDraft {
    /// `kind` is empty when the file declared no registry (the workspace
    /// default applies).
    pub registry: DeliveryRegistryDraft,
    /// File name of the workflow whose success creates a release, e.g. `ci.yml`.
    pub build_workflow: String,
    /// In promotion order: the first is stage 0.
    pub environments: Vec<DeliveryEnvironmentDraft>,
}

impl DeliveryConfigDraft {
    /// Nothing declared: no registry, no build workflow, no environments.
    pub fn is_empty(&self) -> bool {
        self.registry.kind.is_empty()
            && self.build_workflow.is_empty()
            && self.environments.is_empty()
    }

    /// The canonical JSON stored in [`ProjectConfigDraft::delivery_json`].
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Reads [`ProjectConfigDraft::delivery_json`] back; `None` when it is
    /// empty or not the canonical shape.
    pub fn from_json(json: &str) -> Option<Self> {
        if json.trim().is_empty() {
            return None;
        }
        serde_json::from_str(json).ok()
    }
}

/// `delivery.registry`: where the project's images live.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeliveryRegistryDraft {
    /// One of [`REGISTRY_KINDS`]; empty when not declared.
    pub kind: String,
    /// Registry host without scheme, e.g. `ghcr.io`, `myorg.azurecr.io`.
    pub host: String,
    /// Org, user or project path images are pushed under; may be empty.
    pub namespace: String,
}

/// One entry of `delivery.environments`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeliveryEnvironmentDraft {
    pub name: String,
    /// One of [`DEPLOYER_KINDS`].
    pub kind: String,
    pub url: String,
    pub health_path: String,
    pub protected: bool,
    pub auto_promote: bool,
    /// Name of the workspace registry this environment pulls from; empty
    /// uses `delivery.registry`, then the workspace default.
    pub registry: String,
    /// Deployer settings, keys per kind (CONTRACT.md §22). Values are
    /// strings; secrets are referenced by name, never by value.
    pub config: BTreeMap<String, String>,
}

/// `.sdi.yml` `review:` — how the repository tunes its pull request reviews.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewConfigDraft {
    /// Globs of paths no rule looks at (generated code, vendored trees).
    pub ignore_paths: Vec<String>,
    /// Repository-defined regex rules over added lines.
    pub rules: Vec<CustomRuleDraft>,
    /// Severity overrides by rule id (`secrets.aws_key: critical`).
    pub severity: BTreeMap<String, String>,
    /// Globs of source paths whose change must come with a test change,
    /// whether or not a test framework was detected.
    pub require_tests_for: Vec<String>,
}

/// One custom review rule. `pattern` is a regex (validated at parse time)
/// matched against every added line of the files `paths` select (all files
/// when empty); the finding carries `message` with the severity given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomRuleDraft {
    pub id: String,
    pub pattern: String,
    /// info | low | medium | high | critical
    pub severity: String,
    pub message: String,
    pub paths: Vec<String>,
}

/// The severities a `.sdi.yml` review rule or override may name.
pub const REVIEW_SEVERITIES: &[&str] = &["info", "low", "medium", "high", "critical"];

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    pub facts: Vec<FactDraft>,
    pub nodes: Vec<NodeDraft>,
    pub edges: Vec<EdgeDraft>,
    pub dependencies: Vec<DependencyDraft>,
    pub dependabot: DependabotDraft,
    pub pipelines: Vec<PipelineDraft>,
    pub questions: Vec<QuestionDraft>,
    pub understanding: UnderstandingDraft,
    pub chunks: Vec<ChunkDraft>,
    pub learned_patterns: Vec<LearnedPatternDraft>,
    /// Display names, most significant first, e.g. `["React", "Rust", "PostgreSQL"]`.
    pub technologies: Vec<String>,
    /// Capability keys patterns are matched against. See `CONTRACT.md` for the vocabulary.
    pub capabilities: Vec<String>,
    /// One paragraph describing the project; the project embedding is built from it.
    pub profile_text: String,
    pub maturity: MaturityDraft,
    pub config: ProjectConfigDraft,
}
