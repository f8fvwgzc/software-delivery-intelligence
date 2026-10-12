//! The pipeline view of a project (`GetPipeline`, CONTRACT.md §22): its
//! environments in promotion order, what runs in each, the last run, the
//! release that could be promoted next (the candidate), whether the
//! environment answers its health URL right now, and three numbers over the
//! last 30 days: success rate, deployment count and median lead time.

use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
    time::Duration,
};

use chrono::{DateTime, Utc};
use sdi_common::{cache::TtlCache, grpc::db_status};
use sdi_proto::execution::v1::{DeliveryPipeline, PipelineStage};
use sqlx::PgPool;
use tonic::Status;
use uuid::Uuid;

use super::{
    convert,
    db::{
        environments::{self, EnvironmentRow},
        registries,
        releases::{self, ReleaseRow},
        runs,
    },
    ensure_default_registry,
};
use crate::{
    adapters::{
        http_health,
        net::{Net, NetPolicy},
    },
    runner::Runtime,
};

pub const HEALTHY: &str = "healthy";
pub const UNHEALTHY: &str = "unhealthy";
/// No URL configured (or one this service is not allowed to call).
pub const UNKNOWN: &str = "unknown";

/// One GET per check, and it may take this long.
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);
/// A verdict is reused for this long, so a page that polls the pipeline
/// does not turn into a load test of the environment.
pub const HEALTH_TTL: Duration = Duration::from_secs(30);
/// Health URLs remembered at once; the oldest goes when it is full.
pub const HEALTH_CACHE_MAX: usize = 2048;
/// Releases considered when looking for a candidate.
const CANDIDATE_WINDOW: i64 = 200;

/// Live health of environments, cached and bounded.
pub struct HealthProbe {
    net: Net,
    cache: Mutex<TtlCache<String, &'static str>>,
}

impl HealthProbe {
    pub fn new(policy: NetPolicy) -> Self {
        Self {
            net: Net::new(policy),
            cache: Mutex::new(TtlCache::new(HEALTH_CACHE_MAX, HEALTH_TTL)),
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, TtlCache<String, &'static str>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Verdicts held right now (tests).
    #[cfg(test)]
    pub fn cached(&self) -> usize {
        self.cache().len()
    }

    /// `healthy` when `url + health_path` answers 2xx within three seconds,
    /// `unhealthy` when it answers anything else or not at all, `unknown`
    /// when there is no URL or the SSRF guard refuses it.
    pub async fn check(&self, url: &str, health_path: &str) -> &'static str {
        if url.trim().is_empty() {
            return UNKNOWN;
        }
        let target = http_health::health_url(url, health_path);
        if let Some(verdict) = self.cache().get(&target).copied() {
            return verdict;
        }
        let verdict = match self.net.check(&target).await {
            Err(reason) => {
                tracing::debug!(url = %url, %reason, "health URL not checked");
                UNKNOWN
            }
            Ok(_) => match http_health::probe(&self.net, &target, HEALTH_TIMEOUT).await {
                Ok(status) if status.is_success() => HEALTHY,
                _ => UNHEALTHY,
            },
        };
        self.cache().insert(target, verdict);
        verdict
    }
}

/// Where a release sits in time: creation, then id (v7 ids are ordered).
type Age = (DateTime<Utc>, Uuid);

fn age(release: &ReleaseRow) -> Age {
    (release.created_at, release.id)
}

/// The candidate of every stage: the newest release that is ahead of what
/// the stage runs and already succeeded in the stage before it (any
/// release for the first stage). `releases` is newest first; `current`
/// maps a stage's current release to its age; `succeeded` holds every
/// `(environment, release)` that ever ran successfully.
pub fn candidates<'a>(
    stages: &[EnvironmentRow],
    releases: &'a [ReleaseRow],
    current: &HashMap<Uuid, Age>,
    succeeded: &HashSet<(Uuid, Uuid)>,
) -> Vec<Option<&'a ReleaseRow>> {
    stages
        .iter()
        .enumerate()
        .map(|(index, stage)| {
            let running = stage
                .current_release_id
                .and_then(|id| current.get(&id).copied());
            releases.iter().find(|release| {
                let ahead = running.is_none_or(|running| age(release) > running);
                let ready = match index.checked_sub(1).and_then(|i| stages.get(i)) {
                    Some(previous) => succeeded.contains(&(previous.id, release.id)),
                    None => true,
                };
                ahead && ready
            })
        })
        .collect()
}

struct Board {
    stages: Vec<EnvironmentRow>,
    releases: Vec<ReleaseRow>,
    current: HashMap<Uuid, ReleaseRow>,
    succeeded: HashSet<(Uuid, Uuid)>,
}

async fn board(pool: &PgPool, workspace: Uuid, project_id: &str) -> sqlx::Result<Board> {
    let stages = environments::list_for_project(pool, workspace, project_id).await?;
    let releases = releases::list(pool, workspace, project_id, CANDIDATE_WINDOW, None).await?;
    // What a stage runs may be older than the window.
    let current_ids: Vec<Uuid> = stages.iter().filter_map(|s| s.current_release_id).collect();
    let mut current: HashMap<Uuid, ReleaseRow> = releases
        .iter()
        .filter(|r| current_ids.contains(&r.id))
        .map(|r| (r.id, r.clone()))
        .collect();
    let missing: Vec<Uuid> = current_ids
        .iter()
        .copied()
        .filter(|id| !current.contains_key(id))
        .collect();
    if !missing.is_empty() {
        for release in releases::by_ids(pool, workspace, &missing).await? {
            current.insert(release.id, release);
        }
    }
    let succeeded = runs::succeeded_pairs(pool, workspace, project_id)
        .await?
        .into_iter()
        .collect();
    Ok(Board {
        stages,
        releases,
        current,
        succeeded,
    })
}

impl Board {
    fn candidates(&self) -> Vec<Option<&ReleaseRow>> {
        let ages: HashMap<Uuid, Age> = self.current.iter().map(|(id, r)| (*id, age(r))).collect();
        candidates(&self.stages, &self.releases, &ages, &self.succeeded)
    }
}

/// The candidate of one environment: what `Promote` without a release takes.
pub async fn candidate_for(
    pool: &PgPool,
    workspace: Uuid,
    environment: &EnvironmentRow,
) -> sqlx::Result<Option<ReleaseRow>> {
    let board = board(pool, workspace, &environment.project_id).await?;
    let Some(index) = board.stages.iter().position(|s| s.id == environment.id) else {
        return Ok(None);
    };
    Ok(board.candidates()[index].cloned())
}

pub async fn get(
    rt: &Runtime,
    workspace: Uuid,
    project_id: &str,
) -> Result<DeliveryPipeline, Status> {
    ensure_default_registry(rt, workspace).await?;
    let board = board(&rt.pool, workspace, project_id)
        .await
        .map_err(db_status)?;
    let latest: HashMap<Uuid, runs::RunRow> =
        runs::latest_per_environment(&rt.pool, workspace, project_id)
            .await
            .map_err(db_status)?
            .into_iter()
            .map(|run| (run.environment_id, run))
            .collect();
    let stats = runs::stats(&rt.pool, workspace, project_id)
        .await
        .map_err(db_status)?;
    let lead_time = match board.stages.last() {
        Some(last) => runs::median_lead_time(&rt.pool, workspace, project_id, last.id)
            .await
            .map_err(db_status)?,
        None => None,
    };
    let registry = registries::default_of(&rt.pool, workspace)
        .await
        .map_err(db_status)?;

    // Which stages run which release, for `deployed_to` on the cards.
    let deployed_to = |release: &ReleaseRow| -> Vec<String> {
        board
            .stages
            .iter()
            .filter(|stage| stage.current_release_id == Some(release.id))
            .map(|stage| stage.name.clone())
            .collect()
    };
    // Every stage is asked at once; each check is bounded by its timeout.
    let health = futures::future::join_all(
        board
            .stages
            .iter()
            .map(|stage| rt.delivery.health.check(&stage.url, &stage.health_path)),
    )
    .await;

    let candidates = board.candidates();
    let stages = board
        .stages
        .iter()
        .zip(candidates)
        .zip(health)
        .map(|((stage, candidate), health)| PipelineStage {
            environment: Some(convert::environment(stage.clone())),
            current: stage
                .current_release_id
                .and_then(|id| board.current.get(&id))
                .map(|release| convert::release(release.clone(), deployed_to(release))),
            last_run: latest.get(&stage.id).cloned().map(convert::run),
            candidate: candidate
                .map(|release| convert::release(release.clone(), deployed_to(release))),
            health: health.to_owned(),
        })
        .collect();

    Ok(DeliveryPipeline {
        project_id: project_id.to_owned(),
        stages,
        registry: registry.map(convert::registry),
        success_rate: if stats.finished > 0 {
            stats.succeeded as f64 / stats.finished as f64
        } else {
            0.0
        },
        deployments_30d: u32::try_from(stats.deployments.max(0)).unwrap_or(u32::MAX),
        finished_30d: u32::try_from(stats.finished.max(0)).unwrap_or(u32::MAX),
        lead_time_seconds: lead_time
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
            .map_or(0, |seconds| seconds.round().min(f64::from(u32::MAX)) as u32),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{Router, extract::State, http::StatusCode, routing::get};
    use chrono::Duration as ChronoDuration;
    use sqlx::types::Json;

    use super::*;

    fn stage(position: i32, current: Option<Uuid>) -> EnvironmentRow {
        EnvironmentRow {
            id: Uuid::now_v7(),
            workspace_id: Uuid::nil(),
            project_id: "p".into(),
            name: format!("stage-{position}"),
            position,
            kind: "webhook".into(),
            config: Json(Default::default()),
            url: String::new(),
            health_path: String::new(),
            registry_id: None,
            protected: false,
            auto_promote: false,
            source: "ui".into(),
            current_release_id: current,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Releases r1 (oldest) to r4 (newest), returned newest first.
    fn four_releases() -> Vec<ReleaseRow> {
        let base = Utc::now() - ChronoDuration::hours(10);
        (1..=4)
            .rev()
            .map(|n| ReleaseRow {
                id: Uuid::now_v7(),
                workspace_id: Uuid::nil(),
                project_id: "p".into(),
                version: format!("r{n}"),
                commit_sha: format!("{n:040}"),
                image: "ghcr.io/acme/api".into(),
                tag: format!("r{n}"),
                digest: String::new(),
                source: "manual".into(),
                pipeline_run_id: String::new(),
                notes: String::new(),
                verified: false,
                created_by: String::new(),
                repo_full_name: "acme/api".into(),
                project_name: "api".into(),
                created_at: base + ChronoDuration::hours(n),
            })
            .collect()
    }

    fn version<'a>(candidate: &Option<&'a ReleaseRow>) -> Option<&'a str> {
        candidate.map(|release| release.version.as_str())
    }

    #[test]
    fn candidates_are_ahead_of_current_and_proven_in_the_previous_stage() {
        let releases = four_releases();
        let by_version = |v: &str| releases.iter().find(|r| r.version == v).unwrap();
        let (r1, r2, r3) = (by_version("r1"), by_version("r2"), by_version("r3"));
        // staging runs r3, production runs r1, a third stage runs nothing.
        let stages = vec![stage(0, Some(r3.id)), stage(1, Some(r1.id)), stage(2, None)];
        let current: HashMap<Uuid, Age> = [r1, r3].iter().map(|r| (r.id, age(r))).collect();
        // r1, r2 and r3 succeeded in staging; r1 in production.
        let succeeded: HashSet<(Uuid, Uuid)> = [
            (stages[0].id, r1.id),
            (stages[0].id, r2.id),
            (stages[0].id, r3.id),
            (stages[1].id, r1.id),
        ]
        .into();
        let found = candidates(&stages, &releases, &current, &succeeded);
        // First stage: the newest release at all that is ahead of r3.
        assert_eq!(version(&found[0]), Some("r4"));
        // Second stage: the newest one proven in staging and ahead of r1
        // (r4 is newer but has not run in staging).
        assert_eq!(version(&found[1]), Some("r3"));
        // Third stage: only r1 succeeded in production.
        assert_eq!(version(&found[2]), Some("r1"));

        // Once production runs r3 it has nothing to take until staging proves more.
        let stages = vec![stage(0, Some(r3.id)), stage(1, Some(r3.id))];
        let current: HashMap<Uuid, Age> = [(r3.id, age(r3))].into();
        let succeeded: HashSet<(Uuid, Uuid)> =
            [(stages[0].id, r3.id), (stages[1].id, r3.id)].into();
        let found = candidates(&stages, &releases, &current, &succeeded);
        assert_eq!(version(&found[0]), Some("r4"));
        assert_eq!(version(&found[1]), None);

        // A first stage that runs the newest release has no candidate, and
        // a project without releases has none anywhere.
        let newest = by_version("r4");
        let stages = vec![stage(0, Some(newest.id))];
        let current: HashMap<Uuid, Age> = [(newest.id, age(newest))].into();
        assert_eq!(
            version(&candidates(&stages, &releases, &current, &HashSet::new())[0]),
            None
        );
        let stages = vec![stage(0, None), stage(1, None)];
        let found = candidates(&stages, &[], &HashMap::new(), &HashSet::new());
        assert!(found.iter().all(Option::is_none));
        // A stage whose current release is unknown (deleted) takes the newest proven one.
        let stages = vec![stage(0, Some(Uuid::now_v7()))];
        assert_eq!(
            version(&candidates(&stages, &releases, &HashMap::new(), &HashSet::new())[0]),
            Some("r4")
        );
    }

    #[tokio::test]
    async fn health_is_checked_live_cached_and_unknown_without_a_url() {
        let hits = Arc::new(AtomicUsize::new(0));
        async fn health(State(hits): State<Arc<AtomicUsize>>) -> &'static str {
            hits.fetch_add(1, Ordering::SeqCst);
            "ok"
        }
        async fn down() -> StatusCode {
            StatusCode::SERVICE_UNAVAILABLE
        }
        async fn slow() -> &'static str {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "late"
        }
        let app = Router::new()
            .route("/health", get(health))
            .route("/down", get(down))
            .route("/slow", get(slow))
            .with_state(Arc::clone(&hits));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let probe = HealthProbe::new(NetPolicy::ALLOW_PRIVATE);
        assert_eq!(probe.check("", "/health").await, UNKNOWN);
        assert_eq!(probe.cached(), 0, "no URL, nothing to remember");
        assert_eq!(probe.check(&url, "/health").await, HEALTHY);
        assert_eq!(probe.check(&url, "health").await, HEALTHY);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the second answer came from the cache"
        );
        assert_eq!(probe.check(&url, "/down").await, UNHEALTHY);
        // Nothing listens there: unhealthy, not an error.
        assert_eq!(probe.check("http://127.0.0.1:1", "/").await, UNHEALTHY);
        // An answer slower than three seconds is unhealthy.
        let started = std::time::Instant::now();
        assert_eq!(probe.check(&url, "/slow").await, UNHEALTHY);
        assert!(started.elapsed() < Duration::from_secs(6));
        assert_eq!(probe.cached(), 4);

        // Without the private-targets flag a loopback URL is never called.
        let guarded = HealthProbe::new(NetPolicy::PUBLIC_ONLY);
        assert_eq!(guarded.check(&url, "/health").await, UNKNOWN);
        assert_eq!(
            guarded
                .check("https://169.254.169.254", "/latest/meta-data/")
                .await,
            UNKNOWN
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_health_cache_is_bounded() {
        let probe = HealthProbe::new(NetPolicy::PUBLIC_ONLY);
        for i in 0..(HEALTH_CACHE_MAX + 50) {
            probe
                .cache()
                .insert(format!("https://e{i}.example/health"), HEALTHY);
        }
        assert_eq!(probe.cached(), HEALTH_CACHE_MAX);
    }
}
