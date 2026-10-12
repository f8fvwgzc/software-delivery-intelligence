//! Continuous delivery (CONTRACT.md §22): registries, environments,
//! releases and deployment runs.
//!
//! CI builds an image; this module moves that exact image through a
//! project's environments. It does not deploy anything itself: a
//! deployment run is an execution on the `delivery` runtime whose four
//! steps (verify image → deploy → verify health → record) drive the tools
//! that do (`crate::adapters`), behind the same gates as every other
//! execution: permission → policy → approval → act → verify → checkpoint,
//! stopping on failure.
//!
//! - `model`: vocabularies and the validation of registries and environments.
//! - `db`: the four tables.
//! - `image`: image references and their defaults.
//! - `plan`: the four-step plan of a run.
//! - `releases`: cutting a release (commit, tag, image, notes).
//! - `runs`: starting a promotion or a rollback, and what follows a success.
//! - `projection`: a run's status, derived from its execution inside the
//!   runner's own transaction.
//! - `pipeline`: the stage view with candidates, health and the 30-day numbers.
//! - `grpc`: the RPC handlers.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use sdi_common::grpc::db_status;
use tonic::Status;
use uuid::Uuid;

use crate::{
    adapters::{GithubAccess, SecretSource, net::NetPolicy},
    identity::WorkspaceDirectory,
    knowledge::ProjectSource,
    runner::Runtime,
};

pub mod convert;
pub mod db;
pub mod grpc;
pub mod image;
pub mod model;
pub mod pipeline;
pub mod plan;
pub mod projection;
pub mod releases;
pub mod runs;

use db::registries::RegistryRow;

/// Workspaces remembered as having their registry seed before the set is
/// reset (the seed is idempotent; it just runs again).
const SEEDED_MAX: usize = 10_000;

/// What the delivery module needs from outside the execution service.
pub struct Delivery {
    /// Projects, from the knowledge service.
    pub projects: Arc<dyn ProjectSource>,
    /// Workspace slugs and secret names, from the identity service.
    pub workspaces: Arc<dyn WorkspaceDirectory>,
    /// Secret values for the steps of a run; `None` leaves secret-backed
    /// deployers failing with a clear message.
    pub secrets: Option<Arc<dyn SecretSource>>,
    /// The process-wide dev fallback (`GITHUB_TOKEN` / `GITHUB_API_URL`)
    /// for reading commits when identity cannot mint a workspace token.
    pub github_fallback: Option<GithubAccess>,
    pub github: releases::GithubReader,
    /// Live health of environments for the pipeline view.
    pub health: pipeline::HealthProbe,
    /// Workspaces whose default registry was checked this process.
    seeded: Mutex<HashSet<Uuid>>,
}

impl Delivery {
    pub fn new(
        projects: Arc<dyn ProjectSource>,
        workspaces: Arc<dyn WorkspaceDirectory>,
        secrets: Option<Arc<dyn SecretSource>>,
        github_fallback: Option<GithubAccess>,
        policy: NetPolicy,
    ) -> Self {
        Self {
            projects,
            workspaces,
            secrets,
            github_fallback,
            github: releases::GithubReader::default(),
            health: pipeline::HealthProbe::new(policy),
            seeded: Mutex::new(HashSet::new()),
        }
    }

    /// Nothing connected: the delivery RPCs that need a peer answer
    /// UNAVAILABLE, everything else works (tests of the older RPCs).
    #[cfg(test)]
    pub fn disconnected() -> Self {
        Self::new(
            Arc::new(crate::knowledge::NoProjects),
            Arc::new(crate::identity::NoWorkspaceDirectory),
            None,
            None,
            NetPolicy::PUBLIC_ONLY,
        )
    }

    pub fn forget_workspace(&self, workspace_id: Uuid) {
        self.seeded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&workspace_id);
    }
}

/// Makes sure the workspace has its default registry (CONTRACT.md §22:
/// `ghcr.io/<workspace slug>`, GitHub token, nothing to configure). The
/// slug comes from the identity service the first time a workspace is
/// seen; afterwards this is a set lookup.
pub async fn ensure_default_registry(rt: &Runtime, workspace: Uuid) -> Result<(), Status> {
    let delivery = &rt.delivery;
    if delivery
        .seeded
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains(&workspace)
    {
        return Ok(());
    }
    if db::registries::count(&rt.pool, workspace)
        .await
        .map_err(db_status)?
        == 0
    {
        let slug = delivery.workspaces.slug(workspace).await?;
        let namespace = slug.trim().to_ascii_lowercase();
        if namespace.is_empty() {
            return Err(Status::failed_precondition(
                "the workspace has no GitHub login to derive its registry namespace from",
            ));
        }
        let seeded = db::registries::seed_default(&rt.pool, workspace, &namespace)
            .await
            .map_err(db_status)?;
        if seeded {
            tracing::info!(%workspace, namespace, "default ghcr registry seeded");
        }
    }
    let mut seeded = delivery
        .seeded
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if seeded.len() >= SEEDED_MAX {
        seeded.clear();
    }
    seeded.insert(workspace);
    Ok(())
}

/// The registry an environment's images live in: its own, or the
/// workspace default.
pub async fn registry_for(
    rt: &Runtime,
    workspace: Uuid,
    registry_id: Option<Uuid>,
) -> Result<RegistryRow, Status> {
    if let Some(id) = registry_id {
        match db::registries::get(&rt.pool, workspace, id).await {
            Ok(row) => return Ok(row),
            Err(sqlx::Error::RowNotFound) => {}
            Err(error) => return Err(db_status(error)),
        }
    }
    ensure_default_registry(rt, workspace).await?;
    db::registries::default_of(&rt.pool, workspace)
        .await
        .map_err(db_status)?
        .ok_or_else(|| {
            Status::failed_precondition(
                "the workspace has no default registry; mark one of its registries as the default",
            )
        })
}
