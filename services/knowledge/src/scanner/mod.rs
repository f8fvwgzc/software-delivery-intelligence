//! The scan pipeline. One tokio task per scan walks the stages and records
//! progress in the `scans` row so the UI can follow along; any error fails
//! the scan and the project, never leaving a half-written model behind
//! (the knowledge replacement runs in one transaction).
//!
//! Every stage update is also broadcast on the event bus, which is what
//! `WatchScan` and `WatchEvents` forward, so the UI learns about progress
//! the moment it is written instead of on its next poll.
//!
//! Data controls (CONTRACT.md §19): the scanner only ever holds file
//! contents in memory, whatever `code_cache` says; nothing it stores is a
//! raw file (facts carry short evidence, chunks carry what `minimal_chunks`
//! allows). The caller resolves the workspace's GitHub client, settings and
//! controls so the scan itself never talks to identity.

pub mod delivery;
pub mod tarball;

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Context, Result, anyhow};
use futures::{StreamExt, stream};
use sdi_analyzer::{Analysis, RepoSnapshot, TreeEntry};
use sdi_common::embed::embed;
use sqlx::PgPool;
use tracing::Instrument;
use uuid::Uuid;

use crate::{
    controls::Controls,
    convert::{chunk_kind, knowledge_kind_to_db, scan_chunk_kinds, stage_status_to_db},
    db::{
        self,
        chunks::NewChunk,
        patterns::{PatternUpsert, learned_id},
        projects::{ProjectRow, ScanOutcome},
        scans::{ScanRow, Stage},
        settings::SettingsRow,
    },
    events::EventBus,
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

/// Everything a scan needs that the caller resolved per workspace: the
/// GitHub client bound to the workspace's token, the settings read once (a
/// change applies to the next scan) and the data controls.
pub struct ScanInputs {
    pub github: GithubClient,
    pub settings: SettingsRow,
    pub controls: Controls,
}

/// Creates the scan row and spawns the worker. The caller has already
/// checked that no scan is active for the project.
///
/// The scan row is committed together with the project's switch to
/// `scanning`, so a client that sees the project scanning can always list
/// the scan (v1 flipped the status first and lost that race).
pub async fn start(
    pool: &PgPool,
    events: &EventBus,
    inputs: ScanInputs,
    project: ProjectRow,
) -> sqlx::Result<ScanRow> {
    let ws = project.workspace_id;
    let mut tx = pool.begin().await?;
    let scan = db::scans::insert(&mut *tx, ws, project.id, &initial_stages()).await?;
    db::projects::set_status(&mut *tx, ws, project.id, "scanning").await?;
    tx.commit().await?;
    events.project(ws, project.id, "updated", "scanning");
    events.scan(ws, scan.id, project.id, "created", "queued", HashMap::new());

    let pool = pool.clone();
    let events = events.clone();
    let scan_id = scan.id;
    let span =
        tracing::info_span!("scan", %scan_id, workspace = %ws, project = %project.repo_full_name);
    tokio::spawn(run(pool, events, inputs, project, scan_id).instrument(span));
    Ok(scan)
}

async fn run(
    pool: PgPool,
    events: EventBus,
    inputs: ScanInputs,
    project: ProjectRow,
    scan_id: Uuid,
) {
    let ws = project.workspace_id;
    let mut tracker = Tracker {
        pool: pool.clone(),
        events: events.clone(),
        workspace_id: ws,
        scan_id,
        project_id: project.id,
        stages: initial_stages(),
        current: None,
        files_total: 0,
        files_analyzed: 0,
    };
    match pipeline(&pool, &inputs, &project, scan_id, &mut tracker).await {
        Ok(()) => {
            if let Err(error) = db::scans::set_succeeded(&pool, ws, scan_id, &tracker.stages).await
            {
                tracing::error!(%error, "could not mark scan succeeded");
            }
            tracker.emit("updated", "succeeded");
            events.project(ws, project.id, "updated", "ready");
            tracing::info!("scan succeeded");
        }
        Err(error) => {
            let message = format!("{error:#}");
            tracing::warn!(error = %message, "scan failed");
            tracker.fail(&message).await;
            if let Err(error) =
                db::scans::set_failed(&pool, ws, scan_id, &message, &tracker.stages).await
            {
                tracing::error!(%error, "could not mark scan failed");
            }
            if let Err(error) = db::projects::set_status(&pool, ws, project.id, "failed").await {
                tracing::error!(%error, "could not mark project failed");
            }
            tracker.emit("updated", "failed");
            events.project(ws, project.id, "updated", "failed");
        }
    }
}

struct Tracker {
    pool: PgPool,
    events: EventBus,
    workspace_id: Uuid,
    scan_id: Uuid,
    project_id: Uuid,
    stages: Vec<Stage>,
    current: Option<usize>,
    files_total: usize,
    files_analyzed: usize,
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
        db::scans::set_stages(&self.pool, self.workspace_id, self.scan_id, &self.stages)
            .await
            .context("could not record scan progress")?;
        self.emit("progress", "running");
        Ok(())
    }

    /// `data` carries the stage in flight (or the one just finished, between
    /// stages) and the file counters so a list view can render progress
    /// without fetching the scan.
    fn emit(&self, action: &str, status: &str) {
        let running = stage_status_to_db(StageStatus::Running);
        let pending = stage_status_to_db(StageStatus::Pending);
        let stage = self
            .stages
            .iter()
            .find(|s| s.status == running)
            .or_else(|| self.stages.iter().rev().find(|s| s.status != pending))
            .map(|s| s.key.clone())
            .unwrap_or_default();
        let data = HashMap::from([
            ("stage".to_owned(), stage),
            ("files".to_owned(), self.files_analyzed.to_string()),
            ("files_total".to_owned(), self.files_total.to_string()),
        ]);
        self.events.scan(
            self.workspace_id,
            self.scan_id,
            self.project_id,
            action,
            status,
            data,
        );
    }
}

async fn pipeline(
    pool: &PgPool,
    inputs: &ScanInputs,
    project: &ProjectRow,
    scan_id: Uuid,
    tracker: &mut Tracker,
) -> Result<()> {
    let ScanInputs {
        github,
        settings,
        controls,
    } = inputs;
    let ws = project.workspace_id;
    let (owner, name) = split_full_name(&project.repo_full_name)
        .ok_or_else(|| anyhow!("invalid repository name {}", project.repo_full_name))?;
    db::scans::set_running(pool, ws, scan_id).await?;

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
        ws,
        project.id,
        repo.description.as_deref().unwrap_or_default(),
        &repo.default_branch,
        repo.language.as_deref().unwrap_or_default(),
        repo.private,
    )
    .await?;
    db::scans::set_commit(pool, ws, scan_id, &commit_sha).await?;
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
    tracker.files_total = files_total;
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
    let (files, wanted, how) =
        fetch_files(github, owner, name, &commit_sha, &entries, settings).await;
    tracker.files_analyzed = files.len();
    db::scans::set_files(pool, ws, scan_id, files_total as i32, files.len() as i32).await?;
    tracker
        .done(format!("{} of {wanted} wanted files ({how})", files.len()))
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
    let analysis = tokio::task::spawn_blocking(move || analyze(&snapshot))
        .await
        .context("analyzer panicked")?;
    tracker
        .done(format!(
            "{} facts, {} nodes, {} edges, {} questions, maturity {} ({})",
            analysis.facts.len(),
            analysis.nodes.len(),
            analysis.edges.len(),
            analysis.questions.len(),
            analysis.maturity.overall,
            analysis.maturity.stage
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
        store_knowledge(pool, ws, project.id, &analysis, &commit_sha, dependabot_prs).await?;
    db::scans::set_counts(pool, ws, scan_id, facts as i32, nodes as i32, edges as i32).await?;
    tracker
        .done(format!("{facts} facts, {nodes} nodes, {edges} edges"))
        .await?;

    // embeddings
    tracker.start("embeddings").await?;
    let chunks = store_embeddings(pool, ws, project.id, &analysis, controls).await?;
    let learned = store_learned_patterns(pool, &tracker.events, ws, project.id, &analysis).await?;
    tracker
        .done(format!("{chunks} chunks, {learned} learned patterns"))
        .await?;

    // delivery
    tracker.start("delivery").await?;
    let runs_limit = settings.pipeline_runs_limit.max(1) as u32;
    let runs = delivery::sync_runs(pool, github, project, runs_limit).await;
    let deployments = delivery::sync_deployments(pool, github, project, runs_limit).await;
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
        ws,
        project.id,
        scan_id,
        &ScanOutcome {
            tldr: &analysis.understanding.tldr,
            stack: &analysis.understanding.stack,
            maturity_score: analysis.maturity.overall,
            has_config: analysis.config.present,
        },
    )
    .await?;
    prune_code_caches(pool, ws, project.id, &commit_sha).await;
    Ok(())
}

/// The code browser caches (§15) only ever hold the latest scanned commit;
/// rows of older commits go once this scan is the latest. Housekeeping: a
/// failure here is logged, not a failed scan.
async fn prune_code_caches(pool: &PgPool, workspace_id: Uuid, project_id: Uuid, commit_sha: &str) {
    let pruned = match pool.acquire().await {
        Ok(mut conn) => db::code::prune(&mut conn, workspace_id, project_id, commit_sha).await,
        Err(error) => Err(error),
    };
    match pruned {
        Ok(0) => {}
        Ok(rows) => tracing::info!(rows, "pruned code browser caches of older commits"),
        Err(error) => tracing::warn!(%error, "could not prune code browser caches"),
    }
}

/// Runs the analyzer as of today, so runtime end-of-life findings are
/// judged against the real calendar rather than the analyzer's fixed date.
fn analyze(snapshot: &RepoSnapshot) -> Analysis {
    use chrono::Datelike;
    let today = chrono::Utc::now().date_naive();
    sdi_analyzer::analyze_at(snapshot, (today.year(), today.month(), today.day()))
}

/// The analyzer picks the files worth reading (minus the repository's own
/// `scan.exclude` globs); the settings cap how many and how large, so an
/// operator can trade depth for speed without a build.
fn select_files(entries: &[TreeEntry], settings: &SettingsRow, exclude: &[String]) -> Vec<String> {
    let max_bytes = settings.scan_max_file_bytes.max(1) as u64;
    let sizes: HashMap<&str, u64> = entries
        .iter()
        .filter(|e| !e.is_dir)
        .map(|e| (e.path.as_str(), e.size))
        .collect();
    let mut wanted: Vec<String> = sdi_analyzer::wanted_files_with(entries, exclude)
        .into_iter()
        .filter(|p| sizes.get(p.as_str()).is_none_or(|size| *size <= max_bytes))
        .collect();
    wanted.truncate(settings.scan_max_files.max(1) as usize);
    wanted
}

/// `scan.exclude` from a downloaded `.sdi.yml`; empty when the file is
/// absent or unusable (an unparsable config is ignored entirely, §12).
fn config_exclude(files: &BTreeMap<String, String>) -> Vec<String> {
    files
        .get(sdi_analyzer::CONFIG_PATH)
        .map(|content| sdi_analyzer::parse_config(content))
        .filter(sdi_analyzer::config_is_usable)
        .map(|config| config.scan_exclude)
        .unwrap_or_default()
}

/// Everything the analyzer wants, with how many files were wanted and how
/// they were fetched. One archive download, unpacked off the runtime; the
/// archive holds the whole tree, so applying `scan.exclude` afterwards costs
/// nothing. Per-file requests only when the archive is too large to hold or
/// GitHub could not serve it.
async fn fetch_files(
    github: &GithubClient,
    owner: &str,
    name: &str,
    reference: &str,
    entries: &[TreeEntry],
    settings: &SettingsRow,
) -> (BTreeMap<String, String>, usize, &'static str) {
    let wanted = select_files(entries, settings, &[]);
    if wanted.is_empty() {
        return (BTreeMap::new(), 0, "nothing wanted");
    }
    match github
        .tarball(owner, name, reference, tarball::MAX_TARBALL_BYTES)
        .await
    {
        Ok(Some(bytes)) => {
            let wanted_set: HashSet<String> = wanted.iter().cloned().collect();
            let max_file_bytes = settings.scan_max_file_bytes.max(1) as usize;
            let size = bytes.len();
            let unpacked = tokio::task::spawn_blocking(move || {
                tarball::unpack(&bytes, &wanted_set, max_file_bytes)
            })
            .await;
            match unpacked {
                Ok(Ok(mut files)) => {
                    tracing::info!(bytes = size, files = files.len(), "unpacked tarball");
                    let exclude = config_exclude(&files);
                    let mut wanted_len = wanted.len();
                    if !exclude.is_empty() {
                        let kept = select_files(entries, settings, &exclude);
                        let keep: HashSet<&str> = kept.iter().map(String::as_str).collect();
                        files.retain(|path, _| keep.contains(path.as_str()));
                        wanted_len = kept.len();
                    }
                    return (files, wanted_len, "tarball");
                }
                Ok(Err(error)) => tracing::warn!(%error, "tarball unusable, downloading files"),
                Err(error) => tracing::warn!(%error, "tarball unpack panicked, downloading files"),
            }
        }
        Ok(None) => tracing::info!("tarball over the size limit, downloading files"),
        Err(error) => tracing::warn!(%error, "tarball download failed, downloading files"),
    }
    let (files, wanted_len) = download(github, owner, name, reference, entries, settings).await;
    (files, wanted_len, "per file")
}

/// The v1 path, one `/contents` request per file. `.sdi.yml` goes first so
/// its `scan.exclude` can shrink the list before the rest is fetched.
async fn download(
    github: &GithubClient,
    owner: &str,
    name: &str,
    reference: &str,
    entries: &[TreeEntry],
    settings: &SettingsRow,
) -> (BTreeMap<String, String>, usize) {
    let mut wanted = select_files(entries, settings, &[]);
    let mut files = BTreeMap::new();
    if wanted.iter().any(|p| p == sdi_analyzer::CONFIG_PATH)
        && let Ok(content) = github
            .file(owner, name, sdi_analyzer::CONFIG_PATH, reference)
            .await
    {
        files.insert(sdi_analyzer::CONFIG_PATH.to_owned(), content);
        let exclude = config_exclude(&files);
        if !exclude.is_empty() {
            wanted = select_files(entries, settings, &exclude);
        }
    }
    let wanted_len = wanted.len();
    let rest: Vec<String> = wanted
        .into_iter()
        .filter(|p| !files.contains_key(p))
        .collect();
    let fetched: BTreeMap<String, String> = stream::iter(rest)
        .map(|path| async move {
            match github.file(owner, name, &path, reference).await {
                Ok(content) => Some((path, content)),
                Err(error) => {
                    tracing::warn!(path, %error, "skipping file");
                    None
                }
            }
        })
        .buffer_unordered(settings.scan_concurrency.clamp(1, 32) as usize)
        .filter_map(std::future::ready)
        .collect()
        .await;
    files.extend(fetched);
    (files, wanted_len)
}

/// Returns `(facts, nodes, edges)` stored.
async fn store_knowledge(
    pool: &PgPool,
    ws: Uuid,
    project_id: Uuid,
    analysis: &Analysis,
    commit_sha: &str,
    dependabot_prs: i32,
) -> Result<(usize, usize, usize)> {
    let mut tx = pool.begin().await?;
    let facts = db::facts::replace(&mut tx, ws, project_id, &analysis.facts).await?;
    db::questions::sync(&mut tx, ws, project_id, &analysis.questions).await?;
    db::understanding::upsert(
        &mut *tx,
        ws,
        project_id,
        &analysis.understanding,
        commit_sha,
    )
    .await?;
    db::maturity::upsert(&mut *tx, ws, project_id, &analysis.maturity).await?;
    db::config::upsert(&mut *tx, ws, project_id, &analysis.config).await?;
    let (nodes, edges) =
        db::graph::sync(&mut tx, ws, project_id, &analysis.nodes, &analysis.edges).await?;
    db::dependencies::replace(&mut tx, ws, project_id, &analysis.dependencies).await?;
    db::dependencies::upsert_dependabot(
        &mut *tx,
        ws,
        project_id,
        &analysis.dependabot,
        dependabot_prs,
    )
    .await?;
    db::pipelines::replace(&mut tx, ws, project_id, &analysis.pipelines).await?;
    tx.commit().await?;
    Ok((facts, nodes, edges))
}

/// Chunk content is cut to the workspace's `minimal_chunks` excerpt before
/// it is embedded, so neither the vector nor the row sees more.
async fn store_embeddings(
    pool: &PgPool,
    ws: Uuid,
    project_id: Uuid,
    analysis: &Analysis,
    controls: &Controls,
) -> Result<usize> {
    let drafts = analysis.chunks.clone();
    let profile_text = analysis.profile_text.clone();
    let controls = controls.clone();
    let (chunks, profile) = tokio::task::spawn_blocking(move || {
        let chunks: Vec<NewChunk> = drafts
            .into_iter()
            .filter(|c| !c.content.trim().is_empty())
            .map(|c| {
                let content = controls.chunk_content(c.content);
                NewChunk {
                    kind: knowledge_kind_to_db(chunk_kind(c.kind)),
                    project_id: Some(project_id),
                    ref_id: String::new(),
                    title: c.title,
                    path: c.path,
                    embedding: embed(&content),
                    content,
                }
            })
            .collect();
        (chunks, embed(&profile_text))
    })
    .await
    .context("embedding panicked")?;

    let mut tx = pool.begin().await?;
    db::chunks::replace_kinds(&mut tx, ws, project_id, &scan_chunk_kinds(), &chunks).await?;
    db::profiles::upsert(
        &mut *tx,
        ws,
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

/// Learned patterns are upserted by slug, so a rescan refreshes them in
/// place; each upsert is announced so pattern views refresh too.
async fn store_learned_patterns(
    pool: &PgPool,
    events: &EventBus,
    ws: Uuid,
    project_id: Uuid,
    analysis: &Analysis,
) -> Result<usize> {
    for draft in &analysis.learned_patterns {
        if draft.key.trim().is_empty() {
            continue;
        }
        let id = learned_id(ws, &draft.key);
        let pattern = PatternUpsert {
            id: id.clone(),
            workspace_id: Some(ws),
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
            attributes: serde_json::json!({}),
            embedding: embed(&format!(
                "{} {} {}",
                draft.name,
                draft.summary,
                draft.technologies.join(" ")
            )),
        };
        db::patterns::upsert(pool, &pattern).await?;
        db::patterns::add_used_by(pool, ws, &id, project_id).await?;
        events.pattern(ws, &id, "updated", "active");
    }
    Ok(analysis.learned_patterns.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, size: u64) -> TreeEntry {
        TreeEntry {
            path: path.to_owned(),
            size,
            is_dir: false,
        }
    }

    #[test]
    fn settings_cap_count_and_size_of_wanted_files() {
        let entries = vec![
            entry("Cargo.toml", 100),
            entry("package.json", 50_000),
            entry("README.md", 10),
            entry("Dockerfile", 20),
        ];
        let settings = SettingsRow {
            scan_max_files: 2,
            scan_max_file_bytes: 1_000,
            ..SettingsRow::default()
        };
        let wanted = select_files(&entries, &settings, &[]);
        assert_eq!(wanted.len(), 2);
        assert!(!wanted.iter().any(|p| p == "package.json"));
        let excluded = select_files(&entries, &settings, &["Cargo.toml".to_owned()]);
        assert!(!excluded.iter().any(|p| p == "Cargo.toml"));
    }

    #[test]
    fn exclude_comes_only_from_a_usable_config() {
        let mut files = BTreeMap::new();
        assert!(config_exclude(&files).is_empty());
        files.insert(
            sdi_analyzer::CONFIG_PATH.to_owned(),
            "version: 1\nscan:\n  exclude: [\"vendor/**\"]\n".to_owned(),
        );
        assert_eq!(config_exclude(&files), vec!["vendor/**"]);
        files.insert(
            sdi_analyzer::CONFIG_PATH.to_owned(),
            ": not yaml [".to_owned(),
        );
        assert!(config_exclude(&files).is_empty());
    }
}
