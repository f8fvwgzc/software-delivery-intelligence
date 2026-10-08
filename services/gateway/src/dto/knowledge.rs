//! Knowledge area: projects, scans, facts, questions, understanding, graph,
//! dependencies, delivery observation, patterns and semantic search.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sdi_proto::knowledge::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{
    common::{KnowledgeScope, PlanStep},
    opt_string, opt_timestamp, proto_enum, timestamp,
};

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

proto_enum! {
    /// Lifecycle of a connected project.
    ProjectStatus => proto::ProjectStatus { Pending, Scanning, Ready, Failed }
}

proto_enum! {
    /// Observe is read-only analysis (the default); execute allows policy-gated actions.
    ProjectMode => proto::ProjectMode { Observe, Execute }
}

/// A connected repository.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Project id.
    pub id: String,
    /// Display name (the repository name).
    pub name: String,
    /// `owner/name` on GitHub.
    pub repo_full_name: String,
    /// Repository page URL.
    pub repo_url: String,
    /// Default branch that is scanned.
    pub default_branch: String,
    /// True for private repositories.
    pub is_private: bool,
    /// Repository description.
    pub description: String,
    /// Primary language reported by GitHub.
    pub primary_language: String,
    /// Scan lifecycle status.
    pub status: ProjectStatus,
    /// Observe or execute mode.
    pub mode: ProjectMode,
    /// Id of the latest scan, if any.
    pub last_scan_id: Option<String>,
    /// When the latest scan finished.
    pub last_scanned_at: Option<DateTime<Utc>>,
    /// When the project was connected.
    pub created_at: DateTime<Utc>,
    /// One-line understanding. Empty until the first scan succeeds.
    pub tldr: String,
    /// Top technologies, for list display.
    pub stack: Vec<String>,
    /// Number of unanswered questions.
    pub open_questions: u32,
}

impl From<proto::Project> for Project {
    fn from(project: proto::Project) -> Self {
        Self {
            id: project.id,
            name: project.name,
            repo_full_name: project.repo_full_name,
            repo_url: project.repo_url,
            default_branch: project.default_branch,
            is_private: project.is_private,
            description: project.description,
            primary_language: project.primary_language,
            status: project.status.into(),
            mode: project.mode.into(),
            last_scan_id: opt_string(project.last_scan_id),
            last_scanned_at: opt_timestamp(project.last_scanned_at.as_ref()),
            created_at: timestamp(project.created_at.as_ref()),
            tldr: project.tldr,
            stack: project.stack,
            open_questions: project.open_questions,
        }
    }
}

/// Body of `POST /projects`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectProjectRequest {
    /// `owner/name` of the repository to connect.
    pub repo_full_name: String,
}

/// Body of `PATCH /projects/{id}/mode`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetProjectModeRequest {
    /// Mode to switch to.
    pub mode: ProjectMode,
}

// ---------------------------------------------------------------------------
// Scans
// ---------------------------------------------------------------------------

proto_enum! {
    /// Lifecycle of a scan.
    ScanStatus => proto::ScanStatus { Queued, Running, Succeeded, Failed }
}

proto_enum! {
    /// Lifecycle of one scan stage.
    StageStatus => proto::StageStatus { Pending, Running, Done, Failed, Skipped }
}

/// One stage of a scan, for progress display.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScanStage {
    /// Stable key, e.g. `fetch_tree`.
    pub key: String,
    /// Human label.
    pub label: String,
    /// Stage status.
    pub status: StageStatus,
    /// Progress detail or error text.
    pub detail: String,
}

impl From<proto::ScanStage> for ScanStage {
    fn from(stage: proto::ScanStage) -> Self {
        Self {
            key: stage.key,
            label: stage.label,
            status: stage.status.into(),
            detail: stage.detail,
        }
    }
}

/// One analysis run over a project's default branch.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    /// Scan id.
    pub id: String,
    /// Project the scan belongs to.
    pub project_id: String,
    /// Scan status.
    pub status: ScanStatus,
    /// Commit that was analysed.
    pub commit_sha: String,
    /// When the scan started.
    pub started_at: Option<DateTime<Utc>>,
    /// When the scan finished.
    pub finished_at: Option<DateTime<Utc>>,
    /// Files in the tree.
    pub files_total: u32,
    /// Files whose content was analysed.
    pub files_analyzed: u32,
    /// Error text for failed scans.
    pub error: Option<String>,
    /// Stages in order.
    pub stages: Vec<ScanStage>,
    /// Facts produced.
    pub facts_found: u32,
    /// Graph nodes produced.
    pub nodes_found: u32,
    /// Graph edges produced.
    pub edges_found: u32,
}

impl From<proto::Scan> for Scan {
    fn from(scan: proto::Scan) -> Self {
        Self {
            id: scan.id,
            project_id: scan.project_id,
            status: scan.status.into(),
            commit_sha: scan.commit_sha,
            started_at: opt_timestamp(scan.started_at.as_ref()),
            finished_at: opt_timestamp(scan.finished_at.as_ref()),
            files_total: scan.files_total,
            files_analyzed: scan.files_analyzed,
            error: opt_string(scan.error),
            stages: scan.stages.into_iter().map(Into::into).collect(),
            facts_found: scan.facts_found,
            nodes_found: scan.nodes_found,
            edges_found: scan.edges_found,
        }
    }
}

/// Query of `GET /projects/{id}/scans`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListScansQuery {
    /// Maximum number of scans, newest first. 0 uses the service default.
    pub limit: Option<u32>,
}

// ---------------------------------------------------------------------------
// Facts and questions
// ---------------------------------------------------------------------------

proto_enum! {
    /// Detected facts are read from the repository; inferred facts are conclusions drawn from them.
    FactKind => proto::FactKind { Detected, Inferred }
}

proto_enum! {
    /// Area a fact belongs to.
    FactCategory => proto::FactCategory {
        Repository, Language, Framework, Datastore, Container, CiCd, Iac, Cloud,
        Testing, Dependency, Security, Documentation, Deployment, Migration
    }
}

/// Something known about a project, with its evidence.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    /// Fact id.
    pub id: String,
    /// Project the fact belongs to.
    pub project_id: String,
    /// Detected or inferred.
    pub kind: FactKind,
    /// Category.
    pub category: FactCategory,
    /// Stable machine key, e.g. `framework:react`.
    pub key: String,
    /// Human label.
    pub label: String,
    /// Value, e.g. a version or a path.
    pub value: String,
    /// Confidence 0.0 – 1.0; detected facts are 1.0.
    pub confidence: f64,
    /// Repository path the fact came from.
    pub source_path: String,
    /// Short excerpt that proves the fact.
    pub evidence: String,
    /// When the fact was recorded.
    pub detected_at: DateTime<Utc>,
}

impl From<proto::Fact> for Fact {
    fn from(fact: proto::Fact) -> Self {
        Self {
            id: fact.id,
            project_id: fact.project_id,
            kind: fact.kind.into(),
            category: fact.category.into(),
            key: fact.key,
            label: fact.label,
            value: fact.value,
            confidence: fact.confidence,
            source_path: fact.source_path,
            evidence: fact.evidence,
            detected_at: timestamp(fact.detected_at.as_ref()),
        }
    }
}

/// Query of `GET /projects/{id}/facts`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListFactsQuery {
    /// Only facts of this kind.
    pub kind: Option<FactKind>,
    /// Only facts of this category.
    pub category: Option<FactCategory>,
}

/// Something the scan could not know. Asked once, remembered afterwards.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    /// Question id.
    pub id: String,
    /// Project the question belongs to.
    pub project_id: String,
    /// Stable machine key, e.g. `deployment_target`.
    pub key: String,
    /// The question shown to the user.
    pub prompt: String,
    /// Why the platform could not detect it on its own.
    pub reason: String,
    /// Suggested answers.
    pub options: Vec<String>,
    /// True when a free-text answer is accepted.
    pub allow_free_text: bool,
    /// The answer, once given.
    pub answer: Option<String>,
    /// When it was answered.
    pub answered_at: Option<DateTime<Utc>>,
}

impl From<proto::Question> for Question {
    fn from(question: proto::Question) -> Self {
        Self {
            id: question.id,
            project_id: question.project_id,
            key: question.key,
            prompt: question.prompt,
            reason: question.reason,
            options: question.options,
            allow_free_text: question.allow_free_text,
            answer: opt_string(question.answer),
            answered_at: opt_timestamp(question.answered_at.as_ref()),
        }
    }
}

/// Query of `GET /projects/{id}/questions`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListQuestionsQuery {
    /// Only unanswered questions.
    pub only_open: Option<bool>,
}

/// Body of `POST /questions/{id}/answer`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnswerQuestionRequest {
    /// One of the options, or free text when allowed.
    pub answer: String,
}

// ---------------------------------------------------------------------------
// Understanding
// ---------------------------------------------------------------------------

/// One section of the deep-dive understanding.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnderstandingSection {
    /// Stable key, e.g. `datastores`.
    pub key: String,
    /// Section title.
    pub title: String,
    /// Short summary.
    pub summary: String,
    /// Bullet items.
    pub items: Vec<String>,
}

impl From<proto::UnderstandingSection> for UnderstandingSection {
    fn from(section: proto::UnderstandingSection) -> Self {
        Self {
            key: section.key,
            title: section.title,
            summary: section.summary,
            items: section.items,
        }
    }
}

/// TL;DR first, deep dive underneath.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Understanding {
    /// Project id.
    pub project_id: String,
    /// One-line summary.
    pub tldr: String,
    /// Architecture description.
    pub architecture: String,
    /// Technology stack display names.
    pub stack: Vec<String>,
    /// Sections in display order.
    pub sections: Vec<UnderstandingSection>,
    /// Ordered hops a request takes through the system.
    pub request_flow: Vec<String>,
    /// Commit the understanding was generated from.
    pub commit_sha: String,
    /// When it was generated.
    pub generated_at: DateTime<Utc>,
}

impl From<proto::Understanding> for Understanding {
    fn from(understanding: proto::Understanding) -> Self {
        Self {
            project_id: understanding.project_id,
            tldr: understanding.tldr,
            architecture: understanding.architecture,
            stack: understanding.stack,
            sections: understanding.sections.into_iter().map(Into::into).collect(),
            request_flow: understanding.request_flow,
            commit_sha: understanding.commit_sha,
            generated_at: timestamp(understanding.generated_at.as_ref()),
        }
    }
}

// ---------------------------------------------------------------------------
// Graph
// ---------------------------------------------------------------------------

proto_enum! {
    /// What a graph node represents.
    NodeKind => proto::NodeKind {
        Repository, Service, Library, Datastore, Infrastructure, Pipeline, Environment,
        Deployment, ContainerImage, IacModule, TestSuite, Document, ExternalApi, Owner, Migration
    }
}

proto_enum! {
    /// Family of a graph relation.
    RelationCategory => proto::RelationCategory {
        Ownership, Structure, Source, Execution, Infrastructure, Validation, Governance, Outcome
    }
}

/// A node of the project knowledge graph.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    /// Node id.
    pub id: String,
    /// Project the node belongs to.
    pub project_id: String,
    /// Node kind.
    pub kind: NodeKind,
    /// Stable within a project, e.g. `service:api`.
    pub key: String,
    /// Human label.
    pub label: String,
    /// Description.
    pub description: String,
    /// Repository path the node came from.
    pub source_path: String,
    /// Extra attributes.
    pub attributes: BTreeMap<String, String>,
}

impl From<proto::GraphNode> for GraphNode {
    fn from(node: proto::GraphNode) -> Self {
        Self {
            id: node.id,
            project_id: node.project_id,
            kind: node.kind.into(),
            key: node.key,
            label: node.label,
            description: node.description,
            source_path: node.source_path,
            attributes: node.attributes.into_iter().collect(),
        }
    }
}

/// A temporal edge: `validTo` unset means "still true".
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    /// Edge id.
    pub id: String,
    /// Project the edge belongs to.
    pub project_id: String,
    /// Source node id.
    pub source_id: String,
    /// Target node id.
    pub target_id: String,
    /// Upper snake case verb, e.g. `DEPENDS_ON`.
    pub relation: String,
    /// Relation family.
    pub category: RelationCategory,
    /// Since when the relation holds.
    pub valid_from: Option<DateTime<Utc>>,
    /// When it stopped holding; null while current.
    pub valid_to: Option<DateTime<Utc>>,
    /// Extra attributes.
    pub attributes: BTreeMap<String, String>,
}

impl From<proto::GraphEdge> for GraphEdge {
    fn from(edge: proto::GraphEdge) -> Self {
        Self {
            id: edge.id,
            project_id: edge.project_id,
            source_id: edge.source_id,
            target_id: edge.target_id,
            relation: edge.relation,
            category: edge.category.into(),
            valid_from: opt_timestamp(edge.valid_from.as_ref()),
            valid_to: opt_timestamp(edge.valid_to.as_ref()),
            attributes: edge.attributes.into_iter().collect(),
        }
    }
}

/// Nodes and edges of one project.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    /// All nodes.
    pub nodes: Vec<GraphNode>,
    /// All edges.
    pub edges: Vec<GraphEdge>,
}

impl From<proto::Graph> for Graph {
    fn from(graph: proto::Graph) -> Self {
        Self {
            nodes: graph.nodes.into_iter().map(Into::into).collect(),
            edges: graph.edges.into_iter().map(Into::into).collect(),
        }
    }
}

/// Query of `GET /projects/{id}/graph`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct GetGraphQuery {
    /// Also return edges that are no longer valid.
    pub include_history: Option<bool>,
}

/// Body of `POST /projects/{id}/impact`. One of `nodeId` or `path` is required.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeImpactRequest {
    /// Graph node to start from.
    pub node_id: Option<String>,
    /// Repository path to start from.
    pub path: Option<String>,
    /// Maximum hops to walk. 0 uses the service default.
    pub max_depth: Option<u32>,
}

/// A node reached from the origin.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImpactedNode {
    /// The node.
    pub node: Option<GraphNode>,
    /// Hops from the origin.
    pub distance: u32,
    /// Relations walked from the origin to reach this node.
    pub via: Vec<String>,
}

impl From<proto::ImpactedNode> for ImpactedNode {
    fn from(node: proto::ImpactedNode) -> Self {
        Self {
            node: node.node.map(Into::into),
            distance: node.distance,
            via: node.via,
        }
    }
}

/// What a change to one node would touch.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImpactAnalysis {
    /// Node the analysis started from.
    pub origin: Option<GraphNode>,
    /// Reached nodes, nearest first.
    pub impacted: Vec<ImpactedNode>,
    /// Plain-words summary.
    pub summary: String,
}

impl From<proto::ImpactAnalysis> for ImpactAnalysis {
    fn from(analysis: proto::ImpactAnalysis) -> Self {
        Self {
            origin: analysis.origin.map(Into::into),
            impacted: analysis.impacted.into_iter().map(Into::into).collect(),
            summary: analysis.summary,
        }
    }
}

// ---------------------------------------------------------------------------
// Dependencies
// ---------------------------------------------------------------------------

/// One declared dependency.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    /// Dependency id.
    pub id: String,
    /// Project it belongs to.
    pub project_id: String,
    /// Ecosystem: `npm`, `cargo`, `pip`, `go`, `maven`, `docker`, `github_actions`, ...
    pub ecosystem: String,
    /// Package name.
    pub name: String,
    /// Declared version or requirement.
    pub version: String,
    /// Manifest it was read from.
    pub manifest_path: String,
    /// `runtime`, `dev` or `build`.
    pub scope: String,
}

impl From<proto::Dependency> for Dependency {
    fn from(dependency: proto::Dependency) -> Self {
        Self {
            id: dependency.id,
            project_id: dependency.project_id,
            ecosystem: dependency.ecosystem,
            name: dependency.name,
            version: dependency.version,
            manifest_path: dependency.manifest_path,
            scope: dependency.scope,
        }
    }
}

/// Dependency count per ecosystem.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EcosystemCount {
    /// Ecosystem name.
    pub ecosystem: String,
    /// Number of dependencies.
    pub count: u32,
}

impl From<proto::EcosystemCount> for EcosystemCount {
    fn from(count: proto::EcosystemCount) -> Self {
        Self {
            ecosystem: count.ecosystem,
            count: count.count,
        }
    }
}

/// Whether Dependabot is configured.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DependabotStatus {
    /// True when a Dependabot config exists.
    pub configured: bool,
    /// Path of the config file.
    pub config_path: String,
    /// Ecosystems it updates.
    pub ecosystems: Vec<String>,
    /// Open Dependabot pull requests.
    pub open_pull_requests: u32,
}

impl From<proto::DependabotStatus> for DependabotStatus {
    fn from(status: proto::DependabotStatus) -> Self {
        Self {
            configured: status.configured,
            config_path: status.config_path,
            ecosystems: status.ecosystems,
            open_pull_requests: status.open_pull_requests,
        }
    }
}

/// Response of `GET /projects/{id}/dependencies`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DependenciesResponse {
    /// Every dependency.
    pub dependencies: Vec<Dependency>,
    /// Counts per ecosystem.
    pub by_ecosystem: Vec<EcosystemCount>,
    /// Dependabot status.
    pub dependabot: Option<DependabotStatus>,
}

impl From<proto::ListDependenciesResponse> for DependenciesResponse {
    fn from(response: proto::ListDependenciesResponse) -> Self {
        Self {
            dependencies: response.dependencies.into_iter().map(Into::into).collect(),
            by_ecosystem: response.by_ecosystem.into_iter().map(Into::into).collect(),
            dependabot: response.dependabot.map(Into::into),
        }
    }
}

// ---------------------------------------------------------------------------
// Pipelines and deployments
// ---------------------------------------------------------------------------

/// One job of a pipeline definition.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineJob {
    /// Job key.
    pub key: String,
    /// Job name.
    pub name: String,
    /// Runner label.
    pub runs_on: String,
    /// Jobs it needs first.
    pub needs: Vec<String>,
    /// Step names.
    pub steps: Vec<String>,
}

impl From<proto::PipelineJob> for PipelineJob {
    fn from(job: proto::PipelineJob) -> Self {
        Self {
            key: job.key,
            name: job.name,
            runs_on: job.runs_on,
            needs: job.needs,
            steps: job.steps,
        }
    }
}

/// A pipeline definition found in the repository.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Pipeline {
    /// Pipeline id.
    pub id: String,
    /// Project it belongs to.
    pub project_id: String,
    /// `github_actions`, `gitlab_ci`, ...
    pub provider: String,
    /// Workflow name.
    pub name: String,
    /// Definition path.
    pub path: String,
    /// Trigger events.
    pub triggers: Vec<String>,
    /// Jobs in definition order.
    pub jobs: Vec<PipelineJob>,
}

impl From<proto::Pipeline> for Pipeline {
    fn from(pipeline: proto::Pipeline) -> Self {
        Self {
            id: pipeline.id,
            project_id: pipeline.project_id,
            provider: pipeline.provider,
            name: pipeline.name,
            path: pipeline.path,
            triggers: pipeline.triggers,
            jobs: pipeline.jobs.into_iter().map(Into::into).collect(),
        }
    }
}

proto_enum! {
    /// Outcome of a pipeline run.
    RunStatus => proto::RunStatus { Queued, InProgress, Succeeded, Failed, Cancelled, Skipped }
}

/// Root cause of a failed run.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunFailure {
    /// Failed job.
    pub job: String,
    /// Failed step.
    pub step: String,
    /// What went wrong.
    pub summary: String,
    /// What to try.
    pub suggested_fix: String,
}

impl From<proto::RunFailure> for RunFailure {
    fn from(failure: proto::RunFailure) -> Self {
        Self {
            job: failure.job,
            step: failure.step,
            summary: failure.summary,
            suggested_fix: failure.suggested_fix,
        }
    }
}

/// One pipeline run observed at the provider.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineRun {
    /// Run id.
    pub id: String,
    /// Project it belongs to.
    pub project_id: String,
    /// Workflow name.
    pub pipeline_name: String,
    /// Workflow definition path.
    pub pipeline_path: String,
    /// Provider run number.
    pub run_number: u64,
    /// Outcome.
    pub status: RunStatus,
    /// Branch.
    pub branch: String,
    /// Commit.
    pub commit_sha: String,
    /// Commit message.
    pub commit_message: String,
    /// Who triggered it.
    pub actor: String,
    /// Trigger event.
    pub event: String,
    /// When it started.
    pub started_at: Option<DateTime<Utc>>,
    /// Duration in seconds.
    pub duration_seconds: u32,
    /// Provider page URL.
    pub html_url: String,
    /// Present only for failed runs.
    pub failure: Option<RunFailure>,
}

impl From<proto::PipelineRun> for PipelineRun {
    fn from(run: proto::PipelineRun) -> Self {
        Self {
            id: run.id,
            project_id: run.project_id,
            pipeline_name: run.pipeline_name,
            pipeline_path: run.pipeline_path,
            run_number: run.run_number,
            status: run.status.into(),
            branch: run.branch,
            commit_sha: run.commit_sha,
            commit_message: run.commit_message,
            actor: run.actor,
            event: run.event,
            started_at: opt_timestamp(run.started_at.as_ref()),
            duration_seconds: run.duration_seconds,
            html_url: run.html_url,
            failure: run.failure.map(Into::into),
        }
    }
}

/// Aggregate over the listed pipeline runs.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStats {
    /// Runs counted.
    pub total: u32,
    /// Succeeded runs.
    pub succeeded: u32,
    /// Failed runs.
    pub failed: u32,
    /// 0.0 – 1.0 over completed runs.
    pub success_rate: f64,
    /// Mean duration in seconds.
    pub avg_duration_seconds: u32,
}

impl From<proto::PipelineStats> for PipelineStats {
    fn from(stats: proto::PipelineStats) -> Self {
        Self {
            total: stats.total,
            succeeded: stats.succeeded,
            failed: stats.failed,
            success_rate: stats.success_rate,
            avg_duration_seconds: stats.avg_duration_seconds,
        }
    }
}

/// Query of `GET /projects/{id}/pipeline-runs` and `/deployments`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DeliveryHistoryQuery {
    /// Re-sync from the provider before answering.
    pub refresh: Option<bool>,
    /// Maximum number of items, newest first. 0 uses the service default.
    pub limit: Option<u32>,
}

/// Response of `GET /projects/{id}/pipeline-runs`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineRunsResponse {
    /// Runs, newest first.
    pub runs: Vec<PipelineRun>,
    /// Aggregate.
    pub stats: Option<PipelineStats>,
    /// When the runs were last synced from the provider.
    pub synced_at: Option<DateTime<Utc>>,
}

impl From<proto::ListPipelineRunsResponse> for PipelineRunsResponse {
    fn from(response: proto::ListPipelineRunsResponse) -> Self {
        Self {
            runs: response.runs.into_iter().map(Into::into).collect(),
            stats: response.stats.map(Into::into),
            synced_at: opt_timestamp(response.synced_at.as_ref()),
        }
    }
}

proto_enum! {
    /// Outcome of a deployment.
    DeploymentStatus => proto::DeploymentStatus { Pending, InProgress, Succeeded, Failed, Inactive }
}

/// One deployment observed at the provider.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    /// Deployment id.
    pub id: String,
    /// Project it belongs to.
    pub project_id: String,
    /// Target environment.
    pub environment: String,
    /// Git ref deployed.
    pub r#ref: String,
    /// Commit deployed.
    pub commit_sha: String,
    /// Outcome.
    pub status: DeploymentStatus,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// Who created it.
    pub creator: String,
    /// Environment or log URL.
    pub url: String,
    /// `github_deployment` or `workflow_run`.
    pub source: String,
    /// Description.
    pub description: String,
}

impl From<proto::Deployment> for Deployment {
    fn from(deployment: proto::Deployment) -> Self {
        Self {
            id: deployment.id,
            project_id: deployment.project_id,
            environment: deployment.environment,
            r#ref: deployment.r#ref,
            commit_sha: deployment.commit_sha,
            status: deployment.status.into(),
            created_at: timestamp(deployment.created_at.as_ref()),
            creator: deployment.creator,
            url: deployment.url,
            source: deployment.source,
            description: deployment.description,
        }
    }
}

/// Aggregate over the listed deployments.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentStats {
    /// Deployments counted.
    pub total: u32,
    /// Succeeded.
    pub succeeded: u32,
    /// Failed.
    pub failed: u32,
    /// 0.0 – 1.0 over finished deployments.
    pub success_rate: f64,
}

impl From<proto::DeploymentStats> for DeploymentStats {
    fn from(stats: proto::DeploymentStats) -> Self {
        Self {
            total: stats.total,
            succeeded: stats.succeeded,
            failed: stats.failed,
            success_rate: stats.success_rate,
        }
    }
}

/// Response of `GET /projects/{id}/deployments`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentsResponse {
    /// Deployments, newest first.
    pub deployments: Vec<Deployment>,
    /// Aggregate.
    pub stats: Option<DeploymentStats>,
}

impl From<proto::ListDeploymentsResponse> for DeploymentsResponse {
    fn from(response: proto::ListDeploymentsResponse) -> Self {
        Self {
            deployments: response.deployments.into_iter().map(Into::into).collect(),
            stats: response.stats.map(Into::into),
        }
    }
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

proto_enum! {
    /// Where a pattern came from.
    PatternSource => proto::PatternSource { Catalog, Learned }
}

proto_enum! {
    /// Whether a pattern is still offered.
    PatternStatus => proto::PatternStatus { Active, Deprecated }
}

/// A reusable, outcome-tracked way of doing something.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Pattern {
    /// Pattern id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Short summary.
    pub summary: String,
    /// `deployment`, `ci_cd`, `testing`, `infrastructure`, `security`, `documentation` or `dependency`.
    pub category: String,
    /// Technologies involved.
    pub technologies: Vec<String>,
    /// `azure`, `aws`, `gcp`, `any`, `self_hosted` or `github`.
    pub cloud: String,
    /// Runtime shape, e.g. `kubernetes`, `serverless_container`.
    pub runtime: String,
    /// Infrastructure-as-code tool.
    pub iac: String,
    /// Delivery mechanism, e.g. `GitHub Actions + OIDC`.
    pub delivery: String,
    /// Capability keys a project must have, e.g. `dockerfile`.
    pub requires: Vec<String>,
    /// Reuse scope.
    pub scope: KnowledgeScope,
    /// Catalog or learned.
    pub source: PatternSource,
    /// Active or deprecated.
    pub status: PatternStatus,
    /// Project names that use this pattern.
    pub used_by: Vec<String>,
    /// Deployments made with it.
    pub deployments: u32,
    /// 0.0 – 1.0.
    pub success_rate: f64,
    /// Incidents attributed to it.
    pub incidents: u32,
    /// Last time it was used.
    pub last_used_at: Option<DateTime<Utc>>,
    /// Estimated monthly cost in USD.
    pub estimated_monthly_cost_usd: f64,
    /// 0.0 – 1.0.
    pub security_score: f64,
    /// 1 (trivial) – 5 (heavy).
    pub complexity: u32,
    /// Advantages.
    pub pros: Vec<String>,
    /// Drawbacks.
    pub cons: Vec<String>,
    /// Plan template.
    pub plan: Vec<PlanStep>,
    /// When the pattern was created.
    pub created_at: DateTime<Utc>,
}

impl From<proto::Pattern> for Pattern {
    fn from(pattern: proto::Pattern) -> Self {
        Self {
            id: pattern.id,
            name: pattern.name,
            summary: pattern.summary,
            category: pattern.category,
            technologies: pattern.technologies,
            cloud: pattern.cloud,
            runtime: pattern.runtime,
            iac: pattern.iac,
            delivery: pattern.delivery,
            requires: pattern.requires,
            scope: pattern.scope.into(),
            source: pattern.source.into(),
            status: pattern.status.into(),
            used_by: pattern.used_by,
            deployments: pattern.deployments,
            success_rate: pattern.success_rate,
            incidents: pattern.incidents,
            last_used_at: opt_timestamp(pattern.last_used_at.as_ref()),
            estimated_monthly_cost_usd: pattern.estimated_monthly_cost_usd,
            security_score: pattern.security_score,
            complexity: pattern.complexity,
            pros: pattern.pros,
            cons: pattern.cons,
            plan: pattern.plan.into_iter().map(Into::into).collect(),
            created_at: timestamp(pattern.created_at.as_ref()),
        }
    }
}

/// Query of `GET /patterns`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListPatternsQuery {
    /// Only this category. Empty returns every category.
    pub category: Option<String>,
    /// Also return deprecated patterns.
    pub include_deprecated: Option<bool>,
}

// ---------------------------------------------------------------------------
// Semantic search and similarity
// ---------------------------------------------------------------------------

proto_enum! {
    /// What a knowledge chunk was indexed from.
    KnowledgeKind => proto::KnowledgeKind {
        Code, Iac, Document, CiCd, Architecture, Pattern, Decision, Conversation
    }
}

/// Query of `GET /knowledge/search`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct SearchKnowledgeQuery {
    /// Free-text query.
    pub q: String,
    /// Restrict to one project. Empty searches every project.
    pub project_id: Option<String>,
    /// Comma-separated knowledge kinds, e.g. `code,pattern`.
    pub kinds: Option<String>,
    /// Maximum hits. 0 uses the service default.
    pub limit: Option<u32>,
}

impl SearchKnowledgeQuery {
    /// Parses the comma list; unknown names are reported, not dropped.
    pub fn kinds(&self) -> Result<Vec<KnowledgeKind>, String> {
        self.kinds
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
            .map(|kind| {
                serde_json::from_value(serde_json::Value::String(kind.to_owned()))
                    .map_err(|_| format!("unknown knowledge kind: {kind}"))
            })
            .collect()
    }
}

/// One semantic search hit.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeHit {
    /// Chunk id.
    pub id: String,
    /// Chunk kind.
    pub kind: KnowledgeKind,
    /// Project the chunk belongs to.
    pub project_id: String,
    /// Project display name.
    pub project_name: String,
    /// Id of the object the chunk belongs to (pattern id, decision id, ...).
    pub ref_id: String,
    /// Title.
    pub title: String,
    /// Repository path, when any.
    pub path: String,
    /// Matching excerpt.
    pub snippet: String,
    /// Cosine similarity, 0.0 – 1.0.
    pub score: f64,
}

impl From<proto::KnowledgeHit> for KnowledgeHit {
    fn from(hit: proto::KnowledgeHit) -> Self {
        Self {
            id: hit.id,
            kind: hit.kind.into(),
            project_id: hit.project_id,
            project_name: hit.project_name,
            ref_id: hit.ref_id,
            title: hit.title,
            path: hit.path,
            snippet: hit.snippet,
            score: hit.score,
        }
    }
}

/// Per-dimension similarity between two projects.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SimilarityBreakdown {
    /// Architecture similarity.
    pub architecture: f64,
    /// Infrastructure-as-code similarity.
    pub iac: f64,
    /// Deployment similarity.
    pub deployment: f64,
    /// Cloud similarity.
    pub cloud: f64,
    /// Database similarity.
    pub database: f64,
    /// Weighted overall.
    pub overall: f64,
}

impl From<proto::SimilarityBreakdown> for SimilarityBreakdown {
    fn from(similarity: proto::SimilarityBreakdown) -> Self {
        Self {
            architecture: similarity.architecture,
            iac: similarity.iac,
            deployment: similarity.deployment,
            cloud: similarity.cloud,
            database: similarity.database,
            overall: similarity.overall,
        }
    }
}

/// A project that looks like the requested one.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SimilarProject {
    /// The similar project.
    pub project: Option<Project>,
    /// Similarity breakdown.
    pub similarity: Option<SimilarityBreakdown>,
    /// Technologies both projects use.
    pub shared_technologies: Vec<String>,
}

impl From<proto::SimilarProject> for SimilarProject {
    fn from(similar: proto::SimilarProject) -> Self {
        Self {
            project: similar.project.map(Into::into),
            similarity: similar.similarity.map(Into::into),
            shared_technologies: similar.shared_technologies,
        }
    }
}

/// Query of `GET /projects/{id}/similar`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListSimilarQuery {
    /// Maximum number of projects. 0 uses the service default.
    pub limit: Option<u32>,
}

/// Counters of the knowledge store.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeStats {
    /// Connected projects.
    pub projects: u32,
    /// Stored facts.
    pub facts: u32,
    /// Graph nodes.
    pub nodes: u32,
    /// Graph edges.
    pub edges: u32,
    /// Patterns.
    pub patterns: u32,
    /// Semantic chunks.
    pub chunks: u32,
    /// Unanswered questions.
    pub open_questions: u32,
}

impl From<proto::KnowledgeStats> for KnowledgeStats {
    fn from(stats: proto::KnowledgeStats) -> Self {
        Self {
            projects: stats.projects,
            facts: stats.facts,
            nodes: stats.nodes,
            edges: stats.edges,
            patterns: stats.patterns,
            chunks: stats.chunks,
            open_questions: stats.open_questions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_style_enum_names_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&RunStatus::InProgress).unwrap(),
            "\"in_progress\""
        );
        assert_eq!(
            serde_json::to_string(&FactCategory::CiCd).unwrap(),
            "\"ci_cd\""
        );
        assert_eq!(
            serde_json::to_string(&NodeKind::ExternalApi).unwrap(),
            "\"external_api\""
        );
    }

    #[test]
    fn search_kinds_parse_a_comma_list() {
        let query = SearchKnowledgeQuery {
            kinds: Some("code, pattern,".into()),
            ..Default::default()
        };
        assert_eq!(
            query.kinds().unwrap(),
            vec![KnowledgeKind::Code, KnowledgeKind::Pattern]
        );
        assert!(SearchKnowledgeQuery::default().kinds().unwrap().is_empty());

        let bad = SearchKnowledgeQuery {
            kinds: Some("nope".into()),
            ..Default::default()
        };
        assert!(bad.kinds().is_err());
    }
}
