//! The continuous-delivery RPCs (CONTRACT.md §22). The trait impl in
//! `crate::grpc` has already read the caller and checked the role: reads
//! VIEWER; `CreateRelease` and `SyncEnvironments` MEMBER; `Promote` and
//! `Rollback` APPROVER; registries and environment edits ADMIN.
//!
//! A row of another workspace is NOT_FOUND, never PERMISSION_DENIED: every
//! query is scoped by the caller's workspace.

use std::{collections::HashMap, sync::Arc};

use sdi_common::{grpc::db_status, time};
use sdi_proto::execution::v1::{
    CreateReleaseRequest, DeleteEnvironmentRequest, DeleteEnvironmentResponse,
    DeleteRegistryRequest, DeleteRegistryResponse, DeliveryPipeline, DeploymentRun, Environment,
    GetDeploymentRunRequest, GetPipelineRequest, GetReleaseRequest, ListDeploymentRunsRequest,
    ListDeploymentRunsResponse, ListEnvironmentsRequest, ListEnvironmentsResponse,
    ListRegistriesResponse, ListReleasesRequest, ListReleasesResponse, PromoteRequest, Registry,
    Release, RollbackRequest, SyncEnvironmentsRequest, UpsertEnvironmentRequest,
    UpsertRegistryRequest,
};
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
    model::{
        self, AUTH_SECRET, EnvironmentInput, RUN_PROMOTE, RUN_ROLLBACK, SOURCE_CONFIG, SOURCE_UI,
    },
    pipeline,
    runs::{self as flow, StartRun},
};
use crate::{
    adapters::net::{self, ALLOW_PRIVATE_ENV, NetPolicy},
    grpc::{Caller, parse_id},
    runner::Runtime,
};

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 200;
/// Environments one project may have (a pipeline, not an inventory).
pub const MAX_ENVIRONMENTS_PER_PROJECT: usize = 50;
/// Registries one workspace may have.
pub const MAX_REGISTRIES_PER_WORKSPACE: i64 = 50;

fn limit(raw: u32) -> i64 {
    match i64::from(raw) {
        0 => DEFAULT_LIST_LIMIT,
        n => n.min(MAX_LIST_LIMIT),
    }
}

fn project_of(raw: &str) -> Result<&str, Status> {
    let id = raw.trim();
    if id.is_empty() || id.len() > 128 {
        return Err(Status::invalid_argument("project_id is required"));
    }
    Ok(id)
}

/// A missing row of this workspace is NOT_FOUND with a word for what.
fn missing(what: &'static str) -> impl Fn(sqlx::Error) -> Status {
    move |error| match error {
        sqlx::Error::RowNotFound => Status::not_found(format!("no such {what}")),
        other => db_status(other),
    }
}

/// A unique violation (a name taken) as ALREADY_EXISTS.
fn conflict(message: String) -> impl Fn(sqlx::Error) -> Status {
    move |error| match &error {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            Status::already_exists(message.clone())
        }
        _ => db_status(error),
    }
}

/// Every name in `wanted` must be a secret of the caller's workspace.
/// `field` says where the name was written, for the message.
async fn require_secrets(
    rt: &Runtime,
    caller: &Caller,
    wanted: &[(String, String)],
) -> Result<(), Status> {
    if wanted.is_empty() {
        return Ok(());
    }
    let known = rt
        .delivery
        .workspaces
        .secret_names(&caller.principal)
        .await?;
    for (field, name) in wanted {
        if !known.iter().any(|k| k == name) {
            return Err(Status::invalid_argument(format!(
                "{field}: this workspace has no secret named {name}; add it under Workspace, Secrets first"
            )));
        }
    }
    Ok(())
}

/// The sentence a refused private target gets, whoever typed it.
fn private_target(field: &str, why: &str) -> Status {
    Status::invalid_argument(format!(
        "{field}: {why}; targets on private networks are refused unless the operator sets \
         {ALLOW_PRIVATE_ENV}=1 on the execution service"
    ))
}

/// Refuses, when it is saved, an environment whose deployer or health URL
/// plainly points at a private network: an IP literal that is not public,
/// or `localhost`. Names are resolved and judged when the deploy runs
/// (which also covers a name that later moves); this spares the admin a
/// first run that could only fail. `docker_local` is exempt: it runs on
/// this machine by definition and is only offered with the local-exec
/// profile.
fn refuse_private_environment(policy: NetPolicy, input: &EnvironmentInput) -> Result<(), Status> {
    if policy.allow_private || input.kind == model::ENV_DOCKER_LOCAL {
        return Ok(());
    }
    let targets = [
        ("config.url", input.config.get("url").map(String::as_str)),
        ("url", Some(input.url.as_str())),
    ];
    for (field, value) in targets {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        if let Some(why) = net::literal_refusal(value) {
            return Err(private_target(field, &why));
        }
        // The deploy would refuse it too: plain http is only ever accepted
        // inside a private network the operator has allowed.
        if value.to_ascii_lowercase().starts_with("http://") {
            return Err(Status::invalid_argument(format!(
                "{field}: must use https (http is only accepted for private addresses when the \
                 operator sets {ALLOW_PRIVATE_ENV}=1 on the execution service)"
            )));
        }
    }
    Ok(())
}

/// The same for a registry host (`host[:port]`, no scheme).
fn refuse_private_registry(policy: NetPolicy, host: &str) -> Result<(), Status> {
    if policy.allow_private {
        return Ok(());
    }
    match net::literal_refusal(&format!("https://{host}")) {
        Some(why) => Err(private_target("host", &why)),
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Registries
// ---------------------------------------------------------------------------

pub async fn list_registries(
    rt: &Runtime,
    caller: &Caller,
) -> Result<ListRegistriesResponse, Status> {
    ensure_default_registry(rt, caller.workspace).await?;
    let rows = registries::list(&rt.pool, caller.workspace)
        .await
        .map_err(db_status)?;
    Ok(ListRegistriesResponse {
        registries: rows.into_iter().map(convert::registry).collect(),
    })
}

pub async fn upsert_registry(
    rt: &Runtime,
    caller: &Caller,
    req: UpsertRegistryRequest,
) -> Result<Registry, Status> {
    let workspace = caller.workspace;
    let registry = req
        .registry
        .ok_or_else(|| Status::invalid_argument("registry is required"))?;
    let mut input = model::normalize_registry(&registry).map_err(Status::invalid_argument)?;
    refuse_private_registry(rt.delivery.policy, &input.host)?;
    // The seeded default must exist before another one can take its place.
    ensure_default_registry(rt, workspace).await?;
    let existing = match registry.id.trim() {
        "" => None,
        raw => Some(
            registries::get(&rt.pool, workspace, parse_id(raw, "id")?)
                .await
                .map_err(missing("registry"))?,
        ),
    };
    if existing.as_ref().is_some_and(|row| row.is_default) && !input.is_default {
        return Err(Status::failed_precondition(
            "a workspace always has one default registry: make another registry the default instead of unmarking this one",
        ));
    }
    if existing.is_none()
        && registries::count(&rt.pool, workspace)
            .await
            .map_err(db_status)?
            >= MAX_REGISTRIES_PER_WORKSPACE
    {
        return Err(Status::resource_exhausted(format!(
            "a workspace holds at most {MAX_REGISTRIES_PER_WORKSPACE} registries"
        )));
    }
    if input.auth == AUTH_SECRET {
        require_secrets(
            rt,
            caller,
            &[
                ("usernameSecret".to_owned(), input.username_secret.clone()),
                ("passwordSecret".to_owned(), input.password_secret.clone()),
            ],
        )
        .await?;
    }

    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    // A workspace without any default (only possible by hand) gets one.
    if registries::default_of(&mut *tx, workspace)
        .await
        .map_err(db_status)?
        .is_none()
    {
        input.is_default = true;
    }
    if input.is_default {
        registries::clear_default(&mut tx, workspace)
            .await
            .map_err(db_status)?;
    }
    let taken = conflict(format!(
        "name: this workspace already has a registry named {}",
        input.name
    ));
    let row = match &existing {
        Some(existing) => registries::update(&mut tx, workspace, existing.id, &input)
            .await
            .map_err(taken)?,
        None => registries::insert(&mut tx, workspace, Uuid::now_v7(), &input)
            .await
            .map_err(taken)?,
    };
    tx.commit().await.map_err(db_status)?;
    caller.audit(
        rt,
        "registry.upsert",
        "registry",
        &row.id.to_string(),
        format!(
            "{} {} ({}/{}, auth {}{})",
            if existing.is_some() {
                "updated"
            } else {
                "created"
            },
            row.name,
            row.host,
            row.namespace,
            row.auth,
            if row.is_default { ", default" } else { "" }
        ),
    );
    Ok(convert::registry(row))
}

pub async fn delete_registry(
    rt: &Runtime,
    caller: &Caller,
    req: DeleteRegistryRequest,
) -> Result<DeleteRegistryResponse, Status> {
    let workspace = caller.workspace;
    let id = parse_id(&req.id, "id")?;
    let row = registries::get(&rt.pool, workspace, id)
        .await
        .map_err(missing("registry"))?;
    if row.is_default {
        return Err(Status::failed_precondition(
            "the default registry cannot be deleted: make another registry the default first",
        ));
    }
    let used = registries::used_by(&rt.pool, workspace, id)
        .await
        .map_err(db_status)?;
    if used > 0 {
        return Err(Status::failed_precondition(format!(
            "{used} environment(s) use this registry: point them at another one first"
        )));
    }
    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    registries::delete(&mut tx, workspace, id)
        .await
        .map_err(db_status)?;
    tx.commit().await.map_err(db_status)?;
    caller.audit(
        rt,
        "registry.delete",
        "registry",
        &id.to_string(),
        format!("deleted {} ({})", row.name, row.host),
    );
    Ok(DeleteRegistryResponse {})
}

// ---------------------------------------------------------------------------
// Environments
// ---------------------------------------------------------------------------

pub async fn list_environments(
    rt: &Runtime,
    caller: &Caller,
    req: ListEnvironmentsRequest,
) -> Result<ListEnvironmentsResponse, Status> {
    let project_id = project_of(&req.project_id)?;
    let rows = environments::list_for_project(&rt.pool, caller.workspace, project_id)
        .await
        .map_err(db_status)?;
    Ok(ListEnvironmentsResponse {
        environments: rows.into_iter().map(convert::environment).collect(),
    })
}

/// The registry an environment names must be one of the workspace's.
async fn check_registry(
    rt: &Runtime,
    workspace: Uuid,
    input: &EnvironmentInput,
) -> Result<(), Status> {
    if let Some(id) = input.registry_id {
        registries::get(&rt.pool, workspace, id)
            .await
            .map_err(|error| match error {
                sqlx::Error::RowNotFound => {
                    Status::invalid_argument("registryId: this workspace has no such registry")
                }
                other => db_status(other),
            })?;
    }
    Ok(())
}

fn environment_event(rt: &Runtime, row: &EnvironmentRow, action: &str) {
    rt.platform.delivery(
        row.workspace_id,
        "environment",
        &row.id.to_string(),
        &row.project_id,
        action,
        &row.source,
        [("environment".to_owned(), row.name.clone())].into(),
    );
}

pub async fn upsert_environment(
    rt: &Runtime,
    caller: &Caller,
    req: UpsertEnvironmentRequest,
) -> Result<Environment, Status> {
    let workspace = caller.workspace;
    let environment = req
        .environment
        .ok_or_else(|| Status::invalid_argument("environment is required"))?;
    let mut input = model::normalize_environment(&environment).map_err(Status::invalid_argument)?;
    if environment.source.trim() == SOURCE_CONFIG {
        return Err(Status::failed_precondition(
            "config-managed environments are declared in .sdi.yml; they cannot be created or edited here",
        ));
    }

    let existing = match environment.id.trim() {
        "" => None,
        raw => Some(
            environments::get(&rt.pool, workspace, parse_id(raw, "id")?)
                .await
                .map_err(missing("environment"))?,
        ),
    };
    if existing
        .as_ref()
        .is_some_and(|row| row.source == SOURCE_CONFIG)
    {
        return Err(Status::failed_precondition(
            "this environment comes from .sdi.yml and is read-only here: change the file and rescan",
        ));
    }
    let project_id = match &existing {
        Some(row) => {
            let named = environment.project_id.trim();
            if !named.is_empty() && named != row.project_id {
                return Err(Status::invalid_argument(
                    "an environment cannot be moved to another project",
                ));
            }
            row.project_id.clone()
        }
        None => {
            let project_id = project_of(&environment.project_id)?.to_owned();
            // The project must be one of this workspace's (NOT_FOUND otherwise).
            rt.delivery
                .projects
                .project(&caller.principal, &project_id)
                .await?;
            let stages = environments::list_for_project(&rt.pool, workspace, &project_id)
                .await
                .map_err(db_status)?;
            if stages.len() >= MAX_ENVIRONMENTS_PER_PROJECT {
                return Err(Status::resource_exhausted(format!(
                    "a project holds at most {MAX_ENVIRONMENTS_PER_PROJECT} environments"
                )));
            }
            // A new environment that does not say where it goes is the
            // next stage after the last one.
            if environment.position == model::POSITION_UNSET {
                input.position = stages.iter().map(|s| s.position + 1).max().unwrap_or(0);
            }
            project_id
        }
    };
    // An edit that does not say where the stage goes leaves it in place.
    if let Some(row) = &existing
        && environment.position == model::POSITION_UNSET
    {
        input.position = row.position;
    }
    check_registry(rt, workspace, &input).await?;
    refuse_private_environment(rt.delivery.policy, &input)?;
    let wanted: Vec<(String, String)> = model::secret_keys(&input.kind)
        .iter()
        .filter_map(|key| {
            input
                .config
                .get(*key)
                .map(|name| (format!("config.{key}"), name.clone()))
        })
        .collect();
    require_secrets(rt, caller, &wanted).await?;

    let taken = conflict(format!(
        "name: this project already has an environment named {}",
        input.name
    ));
    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    let row = match &existing {
        Some(existing) => environments::update(&mut tx, workspace, existing.id, &input, SOURCE_UI)
            .await
            .map_err(taken)?,
        None => environments::insert(
            &mut tx,
            workspace,
            Uuid::now_v7(),
            &project_id,
            &input,
            SOURCE_UI,
        )
        .await
        .map_err(taken)?,
    };
    tx.commit().await.map_err(db_status)?;
    environment_event(
        rt,
        &row,
        if existing.is_some() {
            "updated"
        } else {
            "created"
        },
    );
    caller.audit(
        rt,
        "environment.upsert",
        "environment",
        &row.id.to_string(),
        format!(
            "{} {} ({}, stage {}{}{})",
            if existing.is_some() {
                "updated"
            } else {
                "created"
            },
            row.name,
            row.kind,
            row.position,
            if row.protected { ", protected" } else { "" },
            if row.auto_promote {
                ", auto-promote"
            } else {
                ""
            }
        ),
    );
    Ok(convert::environment(row))
}

pub async fn delete_environment(
    rt: &Runtime,
    caller: &Caller,
    req: DeleteEnvironmentRequest,
) -> Result<DeleteEnvironmentResponse, Status> {
    let workspace = caller.workspace;
    let id = parse_id(&req.id, "id")?;
    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    let row = environments::lock(&mut tx, workspace, id)
        .await
        .map_err(missing("environment"))?;
    if row.source == SOURCE_CONFIG {
        return Err(Status::failed_precondition(
            "this environment comes from .sdi.yml and is read-only here: remove it from the file and rescan",
        ));
    }
    if let Some(open) = runs::open_for_environment(&mut *tx, workspace, id)
        .await
        .map_err(db_status)?
    {
        return Err(Status::failed_precondition(format!(
            "a deployment of {} to {} is still open; finish or cancel it before deleting the environment",
            open.release_version, row.name
        )));
    }
    environments::delete(&mut tx, workspace, id)
        .await
        .map_err(db_status)?;
    tx.commit().await.map_err(db_status)?;
    environment_event(rt, &row, "deleted");
    caller.audit(
        rt,
        "environment.delete",
        "environment",
        &id.to_string(),
        format!("deleted {} ({})", row.name, row.kind),
    );
    Ok(DeleteEnvironmentResponse {})
}

/// Replaces the config-managed environments of a project with what
/// `.sdi.yml` declares, atomically: either every declared environment is
/// valid and the project's config rows become exactly that list, or nothing
/// changes. An environment that keeps its name keeps its id, its current
/// release and its history; one that disappears from the file is removed
/// unless a deployment to it is still open. A UI environment with the same
/// name becomes config-managed: the file wins.
///
/// Secret names are checked for shape only. The file may be committed
/// before an admin stores the secret, and a run that needs a missing secret
/// says so when it gets there.
pub async fn sync_environments(
    rt: &Runtime,
    caller: &Caller,
    req: SyncEnvironmentsRequest,
) -> Result<ListEnvironmentsResponse, Status> {
    let workspace = caller.workspace;
    let project_id = project_of(&req.project_id)?.to_owned();
    if req.environments.len() > MAX_ENVIRONMENTS_PER_PROJECT {
        return Err(Status::invalid_argument(format!(
            "a project holds at most {MAX_ENVIRONMENTS_PER_PROJECT} environments"
        )));
    }
    let explicit_positions = req.environments.iter().any(|e| e.position != 0);
    let mut declared: Vec<EnvironmentInput> = Vec::with_capacity(req.environments.len());
    for (index, environment) in req.environments.iter().enumerate() {
        let mut input = model::normalize_environment(environment).map_err(|error| {
            Status::invalid_argument(format!(
                "environment {}: {error}",
                match environment.name.trim() {
                    "" => format!("#{}", index + 1),
                    name => name.to_owned(),
                }
            ))
        })?;
        // The order of the file is the promotion order.
        if !explicit_positions {
            input.position = index as i32;
        }
        if declared.iter().any(|other| other.name == input.name) {
            return Err(Status::invalid_argument(format!(
                "environment {} is declared twice",
                input.name
            )));
        }
        check_registry(rt, workspace, &input)
            .await
            .map_err(|status| {
                Status::new(
                    status.code(),
                    format!("environment {}: {}", input.name, status.message()),
                )
            })?;
        declared.push(input);
    }

    let names: Vec<String> = declared.iter().map(|input| input.name.clone()).collect();
    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    let mut changed: Vec<(EnvironmentRow, &'static str)> = Vec::new();
    for gone in environments::config_rows_not_in(&mut tx, workspace, &project_id, &names)
        .await
        .map_err(db_status)?
    {
        if let Some(open) = runs::open_for_environment(&mut *tx, workspace, gone.id)
            .await
            .map_err(db_status)?
        {
            return Err(Status::failed_precondition(format!(
                "environment {} was removed from .sdi.yml but a deployment of {} to it is still open; finish or cancel it, then rescan",
                gone.name, open.release_version
            )));
        }
        environments::delete(&mut tx, workspace, gone.id)
            .await
            .map_err(db_status)?;
        changed.push((gone, "deleted"));
    }
    for input in &declared {
        let existing = environments::by_name(&mut *tx, workspace, &project_id, &input.name)
            .await
            .map_err(db_status)?;
        let (row, action) = match existing {
            Some(existing) => (
                environments::update(&mut tx, workspace, existing.id, input, SOURCE_CONFIG)
                    .await
                    .map_err(db_status)?,
                "updated",
            ),
            None => (
                environments::insert(
                    &mut tx,
                    workspace,
                    Uuid::now_v7(),
                    &project_id,
                    input,
                    SOURCE_CONFIG,
                )
                .await
                .map_err(db_status)?,
                "created",
            ),
        };
        changed.push((row, action));
    }
    let rows = environments::list_for_project(&mut *tx, workspace, &project_id)
        .await
        .map_err(db_status)?;
    tx.commit().await.map_err(db_status)?;

    for (row, action) in &changed {
        environment_event(rt, row, action);
    }
    caller.audit(
        rt,
        "environment.sync",
        "project",
        &project_id,
        format!("config environments: {}", names.join(", ")),
    );
    Ok(ListEnvironmentsResponse {
        environments: rows.into_iter().map(convert::environment).collect(),
    })
}

// ---------------------------------------------------------------------------
// Releases
// ---------------------------------------------------------------------------

/// Releases with the names of the environments each one currently runs in.
async fn with_deployed_to(
    rt: &Runtime,
    workspace: Uuid,
    rows: Vec<ReleaseRow>,
) -> Result<Vec<Release>, Status> {
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let mut running: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (release_id, name) in environments::running(&rt.pool, workspace, &ids)
        .await
        .map_err(db_status)?
    {
        running.entry(release_id).or_default().push(name);
    }
    Ok(rows
        .into_iter()
        .map(|row| {
            let deployed_to = running.remove(&row.id).unwrap_or_default();
            convert::release(row, deployed_to)
        })
        .collect())
}

pub async fn list_releases(
    rt: &Runtime,
    caller: &Caller,
    req: ListReleasesRequest,
) -> Result<ListReleasesResponse, Status> {
    let project_id = project_of(&req.project_id)?;
    let rows = releases::list(
        &rt.pool,
        caller.workspace,
        project_id,
        limit(req.limit),
        time::opt_from_proto(req.before.as_ref()),
    )
    .await
    .map_err(db_status)?;
    Ok(ListReleasesResponse {
        releases: with_deployed_to(rt, caller.workspace, rows).await?,
    })
}

pub async fn get_release(
    rt: &Runtime,
    caller: &Caller,
    req: GetReleaseRequest,
) -> Result<Release, Status> {
    let id = parse_id(&req.id, "id")?;
    let row = releases::get(&rt.pool, caller.workspace, id)
        .await
        .map_err(missing("release"))?;
    let mut releases = with_deployed_to(rt, caller.workspace, vec![row]).await?;
    let mut release = releases.remove(0);
    // The full report only here: lists carry the summary (CONTRACT.md §21).
    if let Some(inspection) = release.inspection.as_mut()
        && let Some(report) = releases::inspection_report(&rt.pool, caller.workspace, id)
            .await
            .map_err(db_status)?
    {
        inspection.report_json = report.to_string();
    }
    Ok(release)
}

pub async fn create_release(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: CreateReleaseRequest,
) -> Result<Release, Status> {
    let workspace = caller.workspace;
    let (row, created) = super::releases::create(rt, &caller.principal, workspace, req).await?;
    if created {
        rt.platform.delivery(
            workspace,
            "release",
            &row.id.to_string(),
            &row.project_id,
            "created",
            &row.source,
            [
                ("version".to_owned(), row.version.clone()),
                ("tag".to_owned(), row.tag.clone()),
            ]
            .into(),
        );
        caller.audit(
            rt,
            "release.create",
            "release",
            &row.id.to_string(),
            format!(
                "{} = {}:{} at {} ({})",
                row.version, row.image, row.tag, row.commit_sha, row.source
            ),
        );
        // A first stage that promotes automatically takes it from here.
        flow::after_release(rt, row.clone());
    }
    let mut releases = with_deployed_to(rt, workspace, vec![row]).await?;
    Ok(releases.remove(0))
}

// ---------------------------------------------------------------------------
// Pipeline and deployment runs
// ---------------------------------------------------------------------------

pub async fn get_pipeline(
    rt: &Runtime,
    caller: &Caller,
    req: GetPipelineRequest,
) -> Result<DeliveryPipeline, Status> {
    pipeline::get(rt, caller.workspace, project_of(&req.project_id)?).await
}

fn optional_id(raw: &str, what: &str) -> Result<Option<Uuid>, Status> {
    match raw.trim() {
        "" => Ok(None),
        raw => parse_id(raw, what).map(Some),
    }
}

pub async fn promote(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: PromoteRequest,
) -> Result<DeploymentRun, Status> {
    let run = flow::start(
        rt,
        &caller.principal,
        caller.workspace,
        StartRun {
            environment_id: parse_id(&req.environment_id, "environment_id")?,
            release_id: optional_id(&req.release_id, "release_id")?,
            kind: RUN_PROMOTE,
            reason: String::new(),
        },
    )
    .await?;
    Ok(convert::run(run))
}

pub async fn rollback(
    rt: &Arc<Runtime>,
    caller: &Caller,
    req: RollbackRequest,
) -> Result<DeploymentRun, Status> {
    if req.reason.chars().count() > 500 {
        return Err(Status::invalid_argument(
            "reason must be at most 500 characters",
        ));
    }
    let run = flow::start(
        rt,
        &caller.principal,
        caller.workspace,
        StartRun {
            environment_id: parse_id(&req.environment_id, "environment_id")?,
            release_id: optional_id(&req.release_id, "release_id")?,
            kind: RUN_ROLLBACK,
            reason: req.reason,
        },
    )
    .await?;
    Ok(convert::run(run))
}

pub async fn list_runs(
    rt: &Runtime,
    caller: &Caller,
    req: ListDeploymentRunsRequest,
) -> Result<ListDeploymentRunsResponse, Status> {
    let project_id = project_of(&req.project_id)?;
    let rows = runs::list(
        &rt.pool,
        caller.workspace,
        project_id,
        optional_id(&req.environment_id, "environment_id")?,
        limit(req.limit),
        time::opt_from_proto(req.before.as_ref()),
    )
    .await
    .map_err(db_status)?;
    Ok(ListDeploymentRunsResponse {
        runs: rows.into_iter().map(convert::run).collect(),
    })
}

pub async fn get_run(
    rt: &Runtime,
    caller: &Caller,
    req: GetDeploymentRunRequest,
) -> Result<DeploymentRun, Status> {
    runs::get(&rt.pool, caller.workspace, parse_id(&req.id, "id")?)
        .await
        .map(convert::run)
        .map_err(missing("deployment run"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tonic::Code;

    use super::*;

    fn environment(kind: &str, config_url: Option<&str>, url: &str) -> EnvironmentInput {
        EnvironmentInput {
            name: "staging".into(),
            position: 0,
            kind: kind.into(),
            config: config_url
                .map(|value| BTreeMap::from([("url".to_owned(), value.to_owned())]))
                .unwrap_or_default(),
            url: url.into(),
            health_path: String::new(),
            registry_id: None,
            protected: false,
            auto_promote: false,
        }
    }

    /// A private target is refused when it is saved, with the field it was
    /// typed into and the switch that would allow it; the same input is
    /// accepted once the operator allows private targets.
    #[test]
    fn private_targets_are_refused_at_save_unless_the_operator_allows_them() {
        let cases = [
            (
                environment(
                    model::ENV_WEBHOOK,
                    Some("http://169.254.169.254/latest"),
                    "",
                ),
                "config.url: 169.254.169.254 is a link-local (cloud metadata) address",
            ),
            (
                environment(model::ENV_JENKINS, Some("http://localhost:8080"), ""),
                "config.url: localhost is the machine SDI runs on",
            ),
            (
                environment(model::ENV_GITHUB_ACTIONS, None, "http://10.0.0.7:3000"),
                "url: 10.0.0.7 is a private address",
            ),
        ];
        for (input, expected) in &cases {
            let refused = refuse_private_environment(NetPolicy::PUBLIC_ONLY, input).unwrap_err();
            assert_eq!(refused.code(), Code::InvalidArgument);
            assert!(
                refused.message().starts_with(expected),
                "{}",
                refused.message()
            );
            assert!(
                refused.message().contains(ALLOW_PRIVATE_ENV),
                "{}",
                refused.message()
            );
            refuse_private_environment(NetPolicy::ALLOW_PRIVATE, input).unwrap();
        }
        // Plain http to a public name would be refused by the deploy as well.
        let plain = refuse_private_environment(
            NetPolicy::PUBLIC_ONLY,
            &environment(
                model::ENV_WEBHOOK,
                Some("http://deploy.example.com/hook"),
                "",
            ),
        )
        .unwrap_err();
        assert!(
            plain.message().starts_with("config.url: must use https"),
            "{}",
            plain.message()
        );
        refuse_private_environment(
            NetPolicy::ALLOW_PRIVATE,
            &environment(model::ENV_WEBHOOK, Some("http://deploy.internal/hook"), ""),
        )
        .unwrap();
        // Names are judged when the deploy runs, not here.
        refuse_private_environment(
            NetPolicy::PUBLIC_ONLY,
            &environment(
                model::ENV_JENKINS,
                Some("https://ci.example.com"),
                "https://staging.example.com",
            ),
        )
        .unwrap();
        // Local Docker runs on this machine by definition.
        refuse_private_environment(
            NetPolicy::PUBLIC_ONLY,
            &environment(model::ENV_DOCKER_LOCAL, None, "http://localhost:8080"),
        )
        .unwrap();

        let refused = refuse_private_registry(NetPolicy::PUBLIC_ONLY, "10.0.0.5:5000").unwrap_err();
        assert!(
            refused
                .message()
                .starts_with("host: 10.0.0.5 is a private address"),
            "{}",
            refused.message()
        );
        refuse_private_registry(NetPolicy::PUBLIC_ONLY, "localhost:5000").unwrap_err();
        refuse_private_registry(NetPolicy::ALLOW_PRIVATE, "localhost:5000").unwrap();
        refuse_private_registry(NetPolicy::PUBLIC_ONLY, "ghcr.io").unwrap();
        refuse_private_registry(NetPolicy::PUBLIC_ONLY, "myorg.azurecr.io").unwrap();
    }
}
