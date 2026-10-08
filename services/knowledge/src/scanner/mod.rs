//! The scan pipeline. One tokio task per scan walks the stages and records
//! progress in the `scans` row so the UI can follow along; any error fails
//! the scan and the project, never leaving a half-written model behind
//! (the knowledge replacement runs in one transaction).

pub mod delivery;

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};
use futures::{StreamExt, stream};
use sdi_analyzer::{Analysis, RepoSnapshot, TreeEntry};
use sdi_common::embed::embed;
use sqlx::PgPool;
use tracing::Instrument;
use uuid::Uuid;

use crate::{
    convert::{chunk_kind, knowledge_kind_to_db, scan_chunk_kinds, stage_status_to_db},
    db::{
        self,
        chunks::NewChunk,
        patterns::PatternUpsert,
        projects::ProjectRow,
        scans::{ScanRow, Stage},
    },
    github::{GithubClient, split_full_name},
};
use sdi_proto::knowledge::v1::StageStatus;

const STAGES: [(&str, &str); 7] = [
    ("metadata", "Repository metadata"),
    ("tree", "File tree"),
    ("files", "Download files"),
    ("analyze", "Analyze"),
    ("graph", "Store knowledge"),
    ("embeddings", "Embeddings"),
    ("delivery", "Delivery sync"),
];

/// How many file downloads run at once; GitHub tolerates this without secondary limits.
const DOWNLOAD_CONCURRENCY: usize = 6;

pub fn initial_stages() -> Vec<Stage> {
    STAGES
        .iter()
        .map(|(key, label)| Stage {
            key: (*key).to_owned(),
            label: (*label).to_owned(),
            status: stage_status_to_db(StageStatus::Pending),
            detail: String::new(),
        })
        .collect()
}

/// Creates the scan row and spawns the worker. The caller has already
/// checked that no scan is active for the project.
pub async fn start(
    pool: &PgPool,
    github: &GithubClient,
    project: ProjectRow,
) -> sqlx::Result<ScanRow> {
    let scan = db::scans::insert(pool, project.id, &initial_stages()).await?;
    let pool = pool.clone();
    let github = github.clone();
    let scan_id = scan.id;
    let span = tracing::info_span!("scan", %scan_id, project = %project.repo_full_name);
    tokio::spawn(run(pool, github, project, scan_id).instrument(span));
    Ok(scan)
}

async fn run(pool: PgPool, github: GithubClient, project: ProjectRow, scan_id: Uuid) {
    let mut tracker = Tracker {
        pool: pool.clone(),
        scan_id,
        stages: initial_stages(),
        current: None,
    };
    match pipeline(&pool, &github, &project, scan_id, &mut tracker).await {
        Ok(()) => {
            if let Err(error) = db::scans::set_succeeded(&pool, scan_id, &tracker.stages).await {
                tracing::error!(%error, "could not mark scan succeeded");
            }
            tracing::info!("scan succeeded");
        }
        Err(error) => {
            let message = format!("{error:#}");
            tracing::warn!(error = %message, "scan failed");
            tracker.fail(&message).await;
            if let Err(error) =
                db::scans::set_failed(&pool, scan_id, &message, &tracker.stages).await
            {
                tracing::error!(%error, "could not mark scan failed");
            }
            if let Err(error) = db::projects::set_status(&pool, project.id, "failed").await {
                tracing::error!(%error, "could not mark project failed");
            }
        }
    }
}

struct Tracker {
    pool: PgPool,
    scan_id: Uuid,
    stages: Vec<Stage>,
    current: Option<usize>,
}

impl Tracker {
    async fn start(&mut self, key: &str) -> Result<()> {
        let index = self
            .stages
            .iter()
            .position(|s| s.key == key)
            .ok_or_else(|| anyhow!("unknown stage {key}"))?;
        self.current = Some(index);
        self.stages[index].status = stage_status_to_db(StageStatus::Running);
        self.flush().await
    }

    async fn done(&mut self, detail: impl Into<String>) -> Result<()> {
        if let Some(index) = self.current.take() {
            self.stages[index].status = stage_status_to_db(StageStatus::Done);
            self.stages[index].detail = detail.into();
        }
        self.flush().await
    }

    async fn fail(&mut self, detail: &str) {
        if let Some(index) = self.current.take() {
            self.stages[index].status = stage_status_to_db(StageStatus::Failed);
            self.stages[index].detail = detail.to_owned();
        }
        for stage in &mut self.stages {
            if stage.status == stage_status_to_db(StageStatus::Pending) {
                stage.status = stage_status_to_db(StageStatus::Skipped);
            }
        }
    }

    async fn flush(&self) -> Result<()> {
        db::scans::set_stages(&self.pool, self.scan_id, &self.stages)
            .await
            .context("could not record scan progress")
    }
}

async fn pipeline(
    pool: &PgPool,
    github: &GithubClient,
    project: &ProjectRow,
    scan_id: Uuid,
    tracker: &mut Tracker,
) -> Result<()> {
    let (owner, name) = split_full_name(&project.repo_full_name)
        .ok_or_else(|| anyhow!("invalid repository name {}", project.repo_full_name))?;
    db::projects::set_status(pool, project.id, "scanning").await?;
    db::scans::set_running(pool, scan_id).await?;

    // metadata
    tracker.start("metadata").await?;
    let repo = github
        .repo(owner, name)
        .await
        .context("repository metadata")?;
    let languages = github.languages(owner, name).await.unwrap_or_default();
    let commit_sha = github
        .branch_sha(owner, name, &repo.default_branch)
        .await
        .with_context(|| format!("head of {}", repo.default_branch))?;
    db::projects::set_metadata(
        pool,
        project.id,
        repo.description.as_deref().unwrap_or_default(),
        &repo.default_branch,
        repo.language.as_deref().unwrap_or_default(),
        repo.private,
    )
    .await?;
    db::scans::set_commit(pool, scan_id, &commit_sha).await?;
    tracker
        .done(format!(
            "{} @ {}",
            repo.default_branch,
            &commit_sha[..commit_sha.len().min(7)]
        ))
        .await?;

    // tree
    tracker.start("tree").await?;
    let tree = github
        .tree(owner, name, &commit_sha)
        .await
        .context("repository tree")?;
    let entries: Vec<TreeEntry> = tree
        .tree
        .iter()
        .map(|node| TreeEntry {
            path: node.path.clone(),
            size: node.size.unwrap_or(0),
            is_dir: node.kind == "tree",
        })
        .collect();
    let files_total = entries.iter().filter(|e| !e.is_dir).count();
    let truncated = if tree.truncated {
        " (truncated by GitHub)"
    } else {
        ""
    };
    tracker
        .done(format!("{files_total} files{truncated}"))
        .await?;

    // files
    tracker.start("files").await?;
    let wanted = sdi_analyzer::wanted_files(&entries);
    let files = download(github, owner, name, &commit_sha, &wanted).await;
    db::scans::set_files(pool, scan_id, files_total as i32, files.len() as i32).await?;
    tracker
        .done(format!("{} of {} wanted files", files.len(), wanted.len()))
        .await?;

    // analyze
    tracker.start("analyze").await?;
    let snapshot = RepoSnapshot {
        full_name: repo.full_name.clone(),
        description: repo.description.clone().unwrap_or_default(),
        default_branch: repo.default_branch.clone(),
        commit_sha: commit_sha.clone(),
        is_private: repo.private,
        primary_language: repo.language.clone().unwrap_or_default(),
        languages,
        topics: repo.topics.clone(),
        tree: entries,
        files,
    };
    let analysis = tokio::task::spawn_blocking(move || sdi_analyzer::analyze(&snapshot))
        .await
        .context("analyzer panicked")?;
    tracker
        .done(format!(
            "{} facts, {} nodes, {} edges, {} questions",
            analysis.facts.len(),
            analysis.nodes.len(),
            analysis.edges.len(),
            analysis.questions.len()
        ))
        .await?;

    // graph: everything SQL-shaped, in one transaction
    tracker.start("graph").await?;
    let dependabot_prs = match github.open_pulls(owner, name).await {
        Ok(pulls) => pulls
            .iter()
            .filter(|p| {
                p.user
                    .as_ref()
                    .is_some_and(|u| u.login == "dependabot[bot]")
            })
            .count() as i32,
        Err(error) => {
            tracing::warn!(%error, "could not count dependabot pull requests");
            0
        }
    };
    let (facts, nodes, edges) =
        store_knowledge(pool, project.id, &analysis, &commit_sha, dependabot_prs).await?;
    db::scans::set_counts(pool, scan_id, facts as i32, nodes as i32, edges as i32).await?;
    tracker
        .done(format!("{facts} facts, {nodes} nodes, {edges} edges"))
        .await?;

    // embeddings
    tracker.start("embeddings").await?;
    let chunks = store_embeddings(pool, project.id, &analysis).await?;
    let learned = store_learned_patterns(pool, project.id, &analysis).await?;
    tracker
        .done(format!("{chunks} chunks, {learned} learned patterns"))
        .await?;

    // delivery
    tracker.start("delivery").await?;
    let runs = delivery::sync_runs(pool, github, project).await;
    let deployments = delivery::sync_deployments(pool, github, project).await;
    let detail = match (runs, deployments) {
        (Ok(runs), Ok(deployments)) => format!("{runs} runs, {deployments} deployments"),
        (runs, deployments) => {
            // Delivery data is observational; a missing Actions permission must not fail the scan.
            let problems: Vec<String> = [runs.err(), deployments.err()]
                .into_iter()
                .flatten()
                .map(|e| format!("{e:#}"))
                .collect();
            tracing::warn!(problems = ?problems, "delivery sync incomplete");
            format!("partial: {}", problems.join("; "))
        }
    };
    tracker.done(detail).await?;

    db::projects::finish_scan(
        pool,
        project.id,
        scan_id,
        &analysis.understanding.tldr,
        &analysis.understanding.stack,
    )
    .await?;
    Ok(())
}

async fn download(
    github: &GithubClient,
    owner: &str,
    name: &str,
    reference: &str,
    paths: &[String],
) -> BTreeMap<String, String> {
    stream::iter(paths.iter().cloned())
        .map(|path| async move {
            match github.file(owner, name, &path, reference).await {
                Ok(content) => Some((path, content)),
                Err(error) => {
                    tracing::warn!(path, %error, "skipping file");
                    None
                }
            }
        })
        .buffer_unordered(DOWNLOAD_CONCURRENCY)
        .filter_map(std::future::ready)
        .collect()
        .await
}

/// Returns `(facts, nodes, edges)` stored.
async fn store_knowledge(
    pool: &PgPool,
    project_id: Uuid,
    analysis: &Analysis,
    commit_sha: &str,
    dependabot_prs: i32,
) -> Result<(usize, usize, usize)> {
    let mut tx = pool.begin().await?;
    let facts = db::facts::replace(&mut tx, project_id, &analysis.facts).await?;
    db::questions::sync(&mut tx, project_id, &analysis.questions).await?;
    db::understanding::upsert(&mut *tx, project_id, &analysis.understanding, commit_sha).await?;
    let (nodes, edges) =
        db::graph::sync(&mut tx, project_id, &analysis.nodes, &analysis.edges).await?;
    db::dependencies::replace(&mut tx, project_id, &analysis.dependencies).await?;
    db::dependencies::upsert_dependabot(&mut *tx, project_id, &analysis.dependabot, dependabot_prs)
        .await?;
    db::pipelines::replace(&mut tx, project_id, &analysis.pipelines).await?;
    tx.commit().await?;
    Ok((facts, nodes, edges))
}

async fn store_embeddings(pool: &PgPool, project_id: Uuid, analysis: &Analysis) -> Result<usize> {
    let drafts = analysis.chunks.clone();
    let profile_text = analysis.profile_text.clone();
    let (chunks, profile) = tokio::task::spawn_blocking(move || {
        let chunks: Vec<NewChunk> = drafts
            .into_iter()
            .filter(|c| !c.content.trim().is_empty())
            .map(|c| NewChunk {
                kind: knowledge_kind_to_db(chunk_kind(c.kind)),
                project_id: Some(project_id),
                ref_id: String::new(),
                title: c.title,
                path: c.path,
                embedding: embed(&c.content),
                content: c.content,
            })
            .collect();
        (chunks, embed(&profile_text))
    })
    .await
    .context("embedding panicked")?;

    let mut tx = pool.begin().await?;
    db::chunks::replace_kinds(&mut tx, project_id, &scan_chunk_kinds(), &chunks).await?;
    db::profiles::upsert(
        &mut *tx,
        project_id,
        &analysis.profile_text,
        &analysis.technologies,
        &analysis.capabilities,
        &profile,
    )
    .await?;
    tx.commit().await?;
    Ok(chunks.len())
}

async fn store_learned_patterns(
    pool: &PgPool,
    project_id: Uuid,
    analysis: &Analysis,
) -> Result<usize> {
    for draft in &analysis.learned_patterns {
        if draft.key.trim().is_empty() {
            continue;
        }
        let id = format!("learned:{}", draft.key.trim());
        let pattern = PatternUpsert {
            id: id.clone(),
            name: draft.name.clone(),
            summary: draft.summary.clone(),
            category: draft.category.clone(),
            technologies: draft.technologies.clone(),
            cloud: if draft.cloud.is_empty() {
                "any".to_owned()
            } else {
                draft.cloud.clone()
            },
            runtime: draft.runtime.clone(),
            iac: draft.iac.clone(),
            delivery: draft.delivery.clone(),
            requires: draft.requires.clone(),
            scope: "organization".to_owned(),
            source: "learned".to_owned(),
            estimated_monthly_cost_usd: 0.0,
            security_score: 0.5,
            complexity: 2,
            pros: vec!["Already proven in a connected repository".to_owned()],
            cons: Vec::new(),
            plan: Vec::new(),
            embedding: embed(&format!(
                "{} {} {}",
                draft.name,
                draft.summary,
                draft.technologies.join(" ")
            )),
        };
        db::patterns::upsert(pool, &pattern).await?;
        db::patterns::add_used_by(pool, &id, project_id).await?;
    }
    Ok(analysis.learned_patterns.len())
}
