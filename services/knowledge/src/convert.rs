//! Row ↔ proto conversions and the enum spelling used in the database.
//!
//! Enums are stored as the proto name without its prefix, lowercased
//! (`FACT_CATEGORY_CI_CD` → `ci_cd`), so a row reads naturally in SQL and
//! round-trips through `from_str_name` without a hand-written table.

use std::collections::{BTreeMap, HashMap};

use sdi_analyzer as an;
use sdi_common::time::{opt_to_proto, to_proto};
use sdi_proto::{
    common::v1::{CostLine, KnowledgeScope, PlanStep},
    knowledge::v1::{
        ChangedFile, Dependency, Deployment as DeploymentMsg, DeploymentStatus, DiffHunk, Fact,
        FactCategory, FactKind, GraphEdge, GraphNode, KnowledgeKind, KnowledgeSettings,
        MaturityAxis, MaturityScore, NodeKind, Pattern, PatternSource, PatternStatus, Pipeline,
        PipelineJob, PipelineRun, Project, ProjectConfig, ProjectMode, ProjectStatus,
        PullRequestSummary, Question, RelationCategory, RunFailure, RunStatus, Scan, ScanStage,
        ScanStatus, StageStatus, Understanding, UnderstandingSection,
    },
};

use crate::db::{
    config::ConfigRow,
    delivery::{DeploymentRow, RunRow},
    dependencies::DependencyRow,
    facts::FactRow,
    graph::{EdgeRow, NodeRow},
    maturity::MaturityRow,
    patterns::PatternRow,
    pipelines::PipelineRow,
    projects::ProjectRow,
    pull_requests::{FileRecord, PullRequestRow},
    questions::QuestionRow,
    scans::ScanRow,
    settings::SettingsRow,
    understanding::UnderstandingRow,
};

/// `PROJECT_STATUS_READY` with prefix `PROJECT_STATUS` → `ready`.
pub fn enum_to_db(proto_name: &str, prefix: &str) -> String {
    proto_name
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('_'))
        .unwrap_or(proto_name)
        .to_lowercase()
}

fn enum_from_db<T>(value: &str, prefix: &str, parse: fn(&str) -> Option<T>) -> i32
where
    T: Into<i32>,
{
    parse(&format!("{prefix}_{}", value.to_uppercase())).map_or(0, Into::into)
}

macro_rules! db_enum {
    ($to:ident, $from:ident, $ty:ty, $prefix:literal) => {
        pub fn $to(value: $ty) -> String {
            enum_to_db(value.as_str_name(), $prefix)
        }
        pub fn $from(value: &str) -> i32 {
            enum_from_db(value, $prefix, <$ty>::from_str_name)
        }
    };
}

db_enum!(
    project_status_to_db,
    project_status_from_db,
    ProjectStatus,
    "PROJECT_STATUS"
);
db_enum!(
    project_mode_to_db,
    project_mode_from_db,
    ProjectMode,
    "PROJECT_MODE"
);
db_enum!(
    scan_status_to_db,
    scan_status_from_db,
    ScanStatus,
    "SCAN_STATUS"
);
db_enum!(
    stage_status_to_db,
    stage_status_from_db,
    StageStatus,
    "STAGE_STATUS"
);
db_enum!(fact_kind_to_db, fact_kind_from_db, FactKind, "FACT_KIND");
db_enum!(
    fact_category_to_db,
    fact_category_from_db,
    FactCategory,
    "FACT_CATEGORY"
);
db_enum!(node_kind_to_db, node_kind_from_db, NodeKind, "NODE_KIND");
db_enum!(
    relation_category_to_db,
    relation_category_from_db,
    RelationCategory,
    "RELATION_CATEGORY"
);
db_enum!(
    run_status_to_db,
    run_status_from_db,
    RunStatus,
    "RUN_STATUS"
);
db_enum!(
    deployment_status_to_db,
    deployment_status_from_db,
    DeploymentStatus,
    "DEPLOYMENT_STATUS"
);
db_enum!(
    pattern_source_to_db,
    pattern_source_from_db,
    PatternSource,
    "PATTERN_SOURCE"
);
db_enum!(
    pattern_status_to_db,
    pattern_status_from_db,
    PatternStatus,
    "PATTERN_STATUS"
);
db_enum!(
    knowledge_kind_to_db,
    knowledge_kind_from_db,
    KnowledgeKind,
    "KNOWLEDGE_KIND"
);
db_enum!(
    knowledge_scope_to_db,
    knowledge_scope_from_db,
    KnowledgeScope,
    "KNOWLEDGE_SCOPE"
);

// ---------------------------------------------------------------------------
// Analyzer enums → proto enums
// ---------------------------------------------------------------------------

pub fn fact_kind(kind: an::FactKind) -> FactKind {
    match kind {
        an::FactKind::Detected => FactKind::Detected,
        an::FactKind::Inferred => FactKind::Inferred,
        an::FactKind::Configured => FactKind::Configured,
    }
}

pub fn fact_category(category: an::FactCategory) -> FactCategory {
    use an::FactCategory as A;
    match category {
        A::Repository => FactCategory::Repository,
        A::Language => FactCategory::Language,
        A::Framework => FactCategory::Framework,
        A::Datastore => FactCategory::Datastore,
        A::Container => FactCategory::Container,
        A::CiCd => FactCategory::CiCd,
        A::Iac => FactCategory::Iac,
        A::Cloud => FactCategory::Cloud,
        A::Testing => FactCategory::Testing,
        A::Dependency => FactCategory::Dependency,
        A::Security => FactCategory::Security,
        A::Documentation => FactCategory::Documentation,
        A::Deployment => FactCategory::Deployment,
        A::Migration => FactCategory::Migration,
        A::Runtime => FactCategory::Runtime,
        A::Configuration => FactCategory::Configuration,
    }
}

pub fn node_kind(kind: an::NodeKind) -> NodeKind {
    use an::NodeKind as A;
    match kind {
        A::Repository => NodeKind::Repository,
        A::Service => NodeKind::Service,
        A::Library => NodeKind::Library,
        A::Datastore => NodeKind::Datastore,
        A::Infrastructure => NodeKind::Infrastructure,
        A::Pipeline => NodeKind::Pipeline,
        A::Environment => NodeKind::Environment,
        A::Deployment => NodeKind::Deployment,
        A::ContainerImage => NodeKind::ContainerImage,
        A::IacModule => NodeKind::IacModule,
        A::TestSuite => NodeKind::TestSuite,
        A::Document => NodeKind::Document,
        A::ExternalApi => NodeKind::ExternalApi,
        A::Owner => NodeKind::Owner,
        A::Migration => NodeKind::Migration,
    }
}

pub fn relation_category(category: an::RelationCategory) -> RelationCategory {
    use an::RelationCategory as A;
    match category {
        A::Ownership => RelationCategory::Ownership,
        A::Structure => RelationCategory::Structure,
        A::Source => RelationCategory::Source,
        A::Execution => RelationCategory::Execution,
        A::Infrastructure => RelationCategory::Infrastructure,
        A::Validation => RelationCategory::Validation,
        A::Governance => RelationCategory::Governance,
        A::Outcome => RelationCategory::Outcome,
    }
}

pub fn chunk_kind(kind: an::ChunkKind) -> KnowledgeKind {
    use an::ChunkKind as A;
    match kind {
        A::Code => KnowledgeKind::Code,
        A::Iac => KnowledgeKind::Iac,
        A::Document => KnowledgeKind::Document,
        A::CiCd => KnowledgeKind::CiCd,
        A::Architecture => KnowledgeKind::Architecture,
    }
}

/// Chunk kinds a scan owns; everything else (decisions, conversations,
/// patterns) is indexed by other services and survives a rescan.
pub fn scan_chunk_kinds() -> Vec<String> {
    [
        KnowledgeKind::Code,
        KnowledgeKind::Iac,
        KnowledgeKind::Document,
        KnowledgeKind::CiCd,
        KnowledgeKind::Architecture,
    ]
    .into_iter()
    .map(knowledge_kind_to_db)
    .collect()
}

// ---------------------------------------------------------------------------
// Rows → proto messages
// ---------------------------------------------------------------------------

pub fn project(row: ProjectRow) -> Project {
    Project {
        id: row.id.to_string(),
        name: row.name,
        repo_full_name: row.repo_full_name,
        repo_url: row.repo_url,
        default_branch: row.default_branch,
        is_private: row.is_private,
        description: row.description,
        primary_language: row.primary_language,
        status: project_status_from_db(&row.status),
        mode: project_mode_from_db(&row.mode),
        last_scan_id: row
            .last_scan_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        last_scanned_at: opt_to_proto(row.last_scanned_at),
        created_at: Some(to_proto(row.created_at)),
        tldr: row.tldr,
        stack: row.stack,
        open_questions: row.open_questions.max(0) as u32,
        maturity_score: row.maturity_score.clamp(0, 100) as u32,
        has_config: row.has_config,
    }
}

pub fn scan(row: ScanRow) -> Scan {
    Scan {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        status: scan_status_from_db(&row.status),
        commit_sha: row.commit_sha,
        started_at: opt_to_proto(row.started_at),
        finished_at: opt_to_proto(row.finished_at),
        files_total: row.files_total.max(0) as u32,
        files_analyzed: row.files_analyzed.max(0) as u32,
        error: row.error,
        stages: row
            .stages
            .0
            .into_iter()
            .map(|s| ScanStage {
                key: s.key,
                label: s.label,
                status: stage_status_from_db(&s.status),
                detail: s.detail,
            })
            .collect(),
        facts_found: row.facts_found.max(0) as u32,
        nodes_found: row.nodes_found.max(0) as u32,
        edges_found: row.edges_found.max(0) as u32,
    }
}

pub fn fact(row: FactRow) -> Fact {
    Fact {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        kind: fact_kind_from_db(&row.kind),
        category: fact_category_from_db(&row.category),
        key: row.key,
        label: row.label,
        value: row.value,
        confidence: row.confidence,
        source_path: row.source_path,
        evidence: row.evidence,
        detected_at: Some(to_proto(row.detected_at)),
    }
}

pub fn question(row: QuestionRow) -> Question {
    Question {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        key: row.key,
        prompt: row.prompt,
        reason: row.reason,
        options: row.options,
        allow_free_text: row.allow_free_text,
        answer: row.answer,
        answered_at: opt_to_proto(row.answered_at),
        answered_by: row.answered_by,
    }
}

/// `maturity` is attached by the caller: the report lives in its own table
/// and only `GetUnderstanding` and `GetProjectContext` need it inline.
pub fn understanding(row: UnderstandingRow, maturity: Option<MaturityScore>) -> Understanding {
    Understanding {
        project_id: row.project_id.to_string(),
        tldr: row.tldr,
        architecture: row.architecture,
        stack: row.stack,
        sections: row
            .sections
            .0
            .into_iter()
            .map(|s| UnderstandingSection {
                key: s.key,
                title: s.title,
                summary: s.summary,
                items: s.items,
            })
            .collect(),
        request_flow: row.request_flow,
        commit_sha: row.commit_sha,
        generated_at: Some(to_proto(row.generated_at)),
        maturity,
    }
}

pub fn maturity(row: MaturityRow) -> MaturityScore {
    MaturityScore {
        project_id: row.project_id.to_string(),
        overall: row.overall.clamp(0, 100) as u32,
        stage: row.stage,
        axes: row
            .axes
            .0
            .into_iter()
            .map(|a| MaturityAxis {
                key: a.key,
                label: a.label,
                score: a.score.min(100),
                summary: a.summary,
                findings: a.findings,
                recommended_patterns: a.recommended_patterns,
            })
            .collect(),
        computed_at: Some(to_proto(row.computed_at)),
    }
}

/// A project without a stored config answers `present=false` rather than
/// nothing, so clients never have to special-case a missing message.
pub fn project_config(row: Option<ConfigRow>) -> ProjectConfig {
    let draft = row.map(|r| r.config.0).unwrap_or_default();
    ProjectConfig {
        present: draft.present,
        path: draft.path,
        deploy_target: draft.deploy_target,
        environments: draft.environments,
        region: draft.region,
        budget_monthly_usd: draft.budget_monthly_usd,
        test_command: draft.test_command,
        health_path: draft.health_path,
        port: draft.port,
        scan_exclude: draft.scan_exclude,
        errors: draft.errors,
        delivery_json: draft.delivery_json,
    }
}

pub fn settings(row: SettingsRow) -> KnowledgeSettings {
    KnowledgeSettings {
        scan_max_files: row.scan_max_files.max(0) as u32,
        scan_max_file_bytes: row.scan_max_file_bytes.max(0) as u32,
        scan_concurrency: row.scan_concurrency.max(0) as u32,
        pipeline_runs_limit: row.pipeline_runs_limit.max(0) as u32,
        cache_github: row.cache_github,
    }
}

pub fn node(row: NodeRow) -> GraphNode {
    GraphNode {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        kind: node_kind_from_db(&row.kind),
        key: row.key,
        label: row.label,
        description: row.description,
        source_path: row.source_path,
        attributes: to_hash_map(row.attributes.0),
    }
}

pub fn edge(row: EdgeRow) -> GraphEdge {
    GraphEdge {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        source_id: row.source_id.to_string(),
        target_id: row.target_id.to_string(),
        relation: row.relation,
        category: relation_category_from_db(&row.category),
        valid_from: Some(to_proto(row.valid_from)),
        valid_to: opt_to_proto(row.valid_to),
        attributes: to_hash_map(row.attributes.0),
    }
}

pub fn dependency(row: DependencyRow) -> Dependency {
    Dependency {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        ecosystem: row.ecosystem,
        name: row.name,
        version: row.version,
        manifest_path: row.manifest_path,
        scope: row.scope,
    }
}

pub fn pipeline(row: PipelineRow) -> Pipeline {
    Pipeline {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        provider: row.provider,
        name: row.name,
        path: row.path,
        triggers: row.triggers,
        jobs: row
            .jobs
            .0
            .into_iter()
            .map(|j| PipelineJob {
                key: j.key,
                name: j.name,
                runs_on: j.runs_on,
                needs: j.needs,
                steps: j.steps,
            })
            .collect(),
    }
}

pub fn pipeline_run(row: RunRow) -> PipelineRun {
    PipelineRun {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        pipeline_name: row.pipeline_name,
        pipeline_path: row.pipeline_path,
        run_number: row.run_number.max(0) as u64,
        status: run_status_from_db(&row.status),
        branch: row.branch,
        commit_sha: row.commit_sha,
        commit_message: row.commit_message,
        actor: row.actor,
        event: row.event,
        started_at: opt_to_proto(row.started_at),
        duration_seconds: row.duration_seconds.max(0) as u32,
        html_url: row.html_url,
        failure: row.failure.map(|f| RunFailure {
            job: f.0.job,
            step: f.0.step,
            summary: f.0.summary,
            suggested_fix: f.0.suggested_fix,
        }),
    }
}

pub fn deployment(row: DeploymentRow) -> DeploymentMsg {
    DeploymentMsg {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        environment: row.environment,
        r#ref: row.r#ref,
        commit_sha: row.commit_sha,
        status: deployment_status_from_db(&row.status),
        created_at: Some(to_proto(row.created_at)),
        creator: row.creator,
        url: row.url,
        source: row.source,
        description: row.description,
    }
}

pub fn pattern(row: PatternRow) -> Pattern {
    let deployments = row.deployments.max(0) as u32;
    let successes = row.successes.max(0) as u32;
    Pattern {
        id: row.id,
        name: row.name,
        summary: row.summary,
        category: row.category,
        technologies: row.technologies,
        cloud: row.cloud,
        runtime: row.runtime,
        iac: row.iac,
        delivery: row.delivery,
        requires: row.requires,
        scope: knowledge_scope_from_db(&row.scope),
        source: pattern_source_from_db(&row.source),
        status: pattern_status_from_db(&row.status),
        used_by: row.used_by,
        deployments,
        success_rate: crate::domain::stats::rate(successes, deployments),
        incidents: row.incidents.max(0) as u32,
        last_used_at: opt_to_proto(row.last_used_at),
        estimated_monthly_cost_usd: row.estimated_monthly_cost_usd,
        security_score: row.security_score,
        complexity: row.complexity.max(0) as u32,
        pros: row.pros,
        cons: row.cons,
        plan: row.plan.0.into_iter().map(plan_step).collect(),
        created_at: Some(to_proto(row.created_at)),
        targets_axes: string_list(&row.attributes.0, "targets_axes"),
        cost_lines: cost_lines(&row.attributes.0),
    }
}

/// `attributes.<key>` as strings; anything else in the list is dropped.
fn string_list(attributes: &serde_json::Value, key: &str) -> Vec<String> {
    attributes
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// `attributes.cost_lines` in the shape [`crate::catalog::attributes`] writes.
/// The catalog is the only writer, so a malformed line is dropped rather
/// than failing the read: a pattern without a cost model still ranks.
fn cost_lines(attributes: &serde_json::Value) -> Vec<CostLine> {
    attributes
        .get("cost_lines")
        .and_then(serde_json::Value::as_array)
        .map(|lines| {
            lines
                .iter()
                .filter_map(|line| {
                    Some(CostLine {
                        item: line.get("item")?.as_str()?.to_owned(),
                        monthly_usd: line.get("monthly_usd")?.as_f64()?,
                        note: line
                            .get("note")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn plan_step(step: crate::db::patterns::PlanStepRow) -> PlanStep {
    PlanStep {
        key: step.key,
        title: step.title,
        description: step.description,
        action_type: step.action_type,
        adapter: step.adapter,
        depends_on: step.depends_on,
        required_permissions: step.required_permissions,
        params: to_hash_map(step.params),
        verification: step.verification,
    }
}

fn to_hash_map(map: BTreeMap<String, String>) -> HashMap<String, String> {
    map.into_iter().collect()
}

/// `latest_review_id` stays empty: reviews live in intelligence and the
/// gateway fills it in (CONTRACT.md §20).
pub fn pull_request_summary(row: PullRequestRow) -> PullRequestSummary {
    PullRequestSummary {
        project_id: row.project_id.to_string(),
        number: row.number.max(0) as u64,
        title: row.title,
        author: row.author,
        author_avatar_url: row.author_avatar_url,
        state: row.state,
        base_ref: row.base_ref,
        head_ref: row.head_ref,
        head_sha: row.head_sha,
        changed_files: row.changed_files.max(0) as u32,
        additions: row.additions.max(0) as u32,
        deletions: row.deletions.max(0) as u32,
        html_url: row.html_url,
        created_at: opt_to_proto(row.pr_created_at),
        updated_at: opt_to_proto(row.pr_updated_at),
        latest_review_id: String::new(),
        head_repo: row.head_repo,
        is_fork: row.is_fork,
        is_bot: row.is_bot,
        is_dependabot: row.is_dependabot,
    }
}

pub fn changed_file(record: FileRecord) -> ChangedFile {
    ChangedFile {
        path: record.path,
        previous_path: record.previous_path,
        status: record.status,
        additions: record.additions,
        deletions: record.deletions,
        language: record.language,
        hunks: record
            .hunks
            .into_iter()
            .map(|h| DiffHunk {
                old_start: h.old_start,
                old_lines: h.old_lines,
                new_start: h.new_start,
                new_lines: h.new_lines,
                lines: h.lines,
            })
            .collect(),
        truncated: record.truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_round_trip_through_their_db_spelling() {
        assert_eq!(fact_category_to_db(FactCategory::CiCd), "ci_cd");
        assert_eq!(fact_category_from_db("ci_cd"), FactCategory::CiCd as i32);
        assert_eq!(project_status_to_db(ProjectStatus::Ready), "ready");
        assert_eq!(project_status_from_db("nonsense"), 0);
        assert_eq!(
            knowledge_scope_to_db(KnowledgeScope::PublicTemplate),
            "public_template"
        );
        assert_eq!(
            run_status_from_db("in_progress"),
            RunStatus::InProgress as i32
        );
    }

    #[test]
    fn pattern_attributes_become_cost_lines_and_axes() {
        let attributes = crate::catalog::attributes(
            &[
                ("Smallest managed instance", 12.0, "database"),
                ("Backups", 3.0, "database"),
            ],
            &["iac", "containerization"],
        );
        let lines = cost_lines(&attributes);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].item, "Smallest managed instance");
        assert!((lines[0].monthly_usd - 12.0).abs() < 1e-9);
        assert_eq!(lines[1].note, "database");
        assert_eq!(
            string_list(&attributes, "targets_axes"),
            vec!["iac", "containerization"]
        );
        // A learned pattern has no attributes at all.
        let none = serde_json::json!({});
        assert!(cost_lines(&none).is_empty());
        assert!(string_list(&none, "targets_axes").is_empty());
        // A malformed line is dropped, not fatal.
        let odd = serde_json::json!({ "cost_lines": [{ "item": "x" }, 7] });
        assert!(cost_lines(&odd).is_empty());
    }
}
