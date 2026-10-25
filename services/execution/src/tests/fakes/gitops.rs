//! A stand-in for SDI's GitOps engine (CONTRACT.md §24): a real tonic
//! server on a random loopback port that keeps applications and their
//! history in memory, applies syncs and rollbacks the way the engine
//! describes them (overrides stored into the source, one history entry per
//! sync that is not a dry run), records every sync and rollback with the
//! principal that asked, and answers what a test scripts instead.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use futures::stream::BoxStream;
use sdi_common::principal::{Principal, Role};
use sdi_proto::{
    common::v1::{PlatformEvent, WatchEventsRequest},
    gitops::v1::{
        Application, ApplicationDiff, ApplicationSource, Cluster, DeleteApplicationRequest,
        DeleteApplicationResponse, DeleteClusterRequest, DeleteClusterResponse,
        DeleteWorkspaceDataRequest, DeleteWorkspaceDataResponse, DiffApplicationRequest,
        ExportWorkspaceDataRequest, GetApplicationRequest, GetManifestsRequest, HistoryEntry,
        ListApplicationsRequest, ListApplicationsResponse, ListClustersRequest,
        ListClustersResponse, ListHistoryRequest, ListHistoryResponse, Manifests, Operation,
        RefreshApplicationRequest, ResourceResult, ResourceStatus, RollbackApplicationRequest,
        SyncApplicationRequest, SyncResult, UpsertApplicationRequest, UpsertClusterRequest,
        WorkspaceDataRecord,
        gitops_service_server::{GitopsService, GitopsServiceServer},
    },
};
use tonic::{
    Request, Response, Status,
    transport::{Server, server::TcpIncoming},
};
use uuid::Uuid;

/// A kustomize application (`kustomize_app`).
pub const APP: &str = "0192f6a0-0000-7000-8000-00000000a991";
/// The id tests give a helm application.
pub const HELM_APP: &str = "0192f6a0-0000-7000-8000-00000000a992";
/// The commit every sync applies unless the request names a full SHA.
pub const REVISION: &str = "9f8e7d6c5b4a39281706f5e4d3c2b1a0f9e8d7c6";

/// What the next SyncApplication or RollbackApplication answers. A test
/// queues a handful at most, so the size of a result does not matter.
#[allow(clippy::large_enum_variant)]
pub enum Scripted {
    /// This result (a sync that is not a dry run still writes history).
    Result(SyncResult),
    /// This refusal.
    Status(Status),
}

#[derive(Default)]
struct State {
    apps: BTreeMap<String, Application>,
    /// Per application, newest first.
    history: BTreeMap<String, Vec<HistoryEntry>>,
    syncs: Vec<(Principal, SyncApplicationRequest)>,
    rollbacks: Vec<(Principal, RollbackApplicationRequest)>,
    script: VecDeque<Scripted>,
    /// Health GetApplication reports, one per call; the last one stays.
    health: VecDeque<String>,
    /// After this many more reads the running operation of the execution
    /// ends as succeeded at the revision.
    finish: Option<(usize, Uuid, String)>,
    /// What the next RefreshApplication calls answer instead of the
    /// application, one per call.
    refresh_errors: VecDeque<Status>,
    next: u64,
}

#[derive(Default)]
pub struct FakeGitops {
    state: Mutex<State>,
    /// GetApplication calls so far.
    pub gets: AtomicUsize,
    /// RefreshApplication calls so far.
    pub refreshes: AtomicUsize,
}

fn resource(group: &str, kind: &str, name: &str, sync: &str, health: &str) -> ResourceStatus {
    ResourceStatus {
        group: group.into(),
        version: "v1".into(),
        kind: kind.into(),
        namespace: "shop".into(),
        name: name.into(),
        sync_status: sync.into(),
        health_status: health.into(),
        ..Default::default()
    }
}

/// `api` in namespace `shop` of cluster `prod`, rendered with kustomize
/// from `acme/fleet@main:apps/api`: a Deployment that changed, a Service
/// not created yet, a ConfigMap no longer desired and a ServiceAccount in
/// sync.
pub fn kustomize_app() -> Application {
    let resources = vec![
        resource("apps", "Deployment", "api", "out_of_sync", "healthy"),
        resource("", "Service", "api", "missing", "missing"),
        resource("", "ConfigMap", "legacy", "extraneous", ""),
        resource("", "ServiceAccount", "api", "synced", "healthy"),
    ];
    Application {
        id: APP.into(),
        project_id: "0192f6a0-0000-7000-8000-0000000000a1".into(),
        name: "api".into(),
        cluster_id: "0192f6a0-0000-7000-8000-00000000c001".into(),
        cluster_name: "prod".into(),
        namespace: "shop".into(),
        source: Some(ApplicationSource {
            repo: "acme/fleet".into(),
            revision: "main".into(),
            path: "apps/api".into(),
            r#type: "kustomize".into(),
            ..Default::default()
        }),
        sync_status: "out_of_sync".into(),
        health_status: "healthy".into(),
        revision: REVISION.into(),
        resource_count: resources.len() as u32,
        out_of_sync_count: 3,
        resources,
        ..Default::default()
    }
}

/// The name part of a kustomize image override (`name=…`, `name:tag`,
/// `name@digest`), what the engine merges by.
fn image_name(image: &str) -> &str {
    if let Some((name, _)) = image.split_once('=') {
        return name;
    }
    let image = image.split_once('@').map_or(image, |(name, _)| name);
    match image.rsplit_once(':') {
        Some((name, tag)) if !tag.contains('/') => name,
        _ => image,
    }
}

fn full_sha(revision: &str) -> bool {
    revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit())
}

impl FakeGitops {
    pub fn with_app(self, app: Application) -> Self {
        self.lock().apps.insert(app.id.clone(), app);
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("fake gitops lock")
    }

    pub fn app(&self, id: &str) -> Application {
        self.lock()
            .apps
            .get(id)
            .cloned()
            .expect("no such application")
    }

    pub fn update(&self, id: &str, change: impl FnOnce(&mut Application)) {
        change(self.lock().apps.get_mut(id).expect("no such application"));
    }

    /// Newest first.
    pub fn history(&self, id: &str) -> Vec<HistoryEntry> {
        self.lock().history.get(id).cloned().unwrap_or_default()
    }

    pub fn syncs(&self) -> Vec<(Principal, SyncApplicationRequest)> {
        self.lock().syncs.clone()
    }

    /// The syncs that were not dry runs.
    pub fn applied(&self) -> Vec<(Principal, SyncApplicationRequest)> {
        self.syncs()
            .into_iter()
            .filter(|(_, r)| !r.dry_run)
            .collect()
    }

    pub fn rollbacks(&self) -> Vec<(Principal, RollbackApplicationRequest)> {
        self.lock().rollbacks.clone()
    }

    pub fn script(&self, answer: Scripted) {
        self.lock().script.push_back(answer);
    }

    pub fn health_sequence(&self, health: &[&str]) {
        self.lock().health = health.iter().map(|h| (*h).to_owned()).collect();
    }

    /// The running operation of `execution` ends after `gets` more reads.
    pub fn finish_operation_after(&self, gets: usize, execution: Uuid, revision: &str) {
        self.lock().finish = Some((gets, execution, revision.to_owned()));
    }

    /// The next RefreshApplication fails with `status`.
    pub fn fail_refresh(&self, status: Status) {
        self.lock().refresh_errors.push_back(status);
    }

    /// The application as a read (Get or Refresh) answers it: the scripted
    /// health of this read, and the end of an operation that was due.
    fn read(&self, id: &str) -> Result<Application, Status> {
        let mut state = self.lock();
        if let Some((left, execution, revision)) = state.finish.take() {
            if left <= 1 {
                if let Some(app) = state.apps.get_mut(id) {
                    app.operation = Some(Operation {
                        kind: "sync".into(),
                        phase: "succeeded".into(),
                        revision: revision.clone(),
                        execution_id: execution.to_string(),
                        ..Default::default()
                    });
                    app.synced_revision = revision.clone();
                }
                let principal = Principal::system("fake");
                state.record(
                    id,
                    &revision,
                    "succeeded",
                    &principal,
                    &execution.to_string(),
                );
            } else {
                state.finish = Some((left - 1, execution, revision));
            }
        }
        let health = if state.health.len() > 1 {
            state.health.pop_front()
        } else {
            state.health.front().cloned()
        };
        let Some(app) = state.apps.get_mut(id) else {
            return Err(Status::not_found(format!("no application {id}")));
        };
        if let Some(health) = health {
            app.health_status = health;
        }
        Ok(app.clone())
    }

    pub async fn serve(self) -> (Arc<Self>, String) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the fake gitops engine");
        let addr = listener.local_addr().expect("local address");
        let shared = Arc::new(self);
        let service = GitopsServiceServer::from_arc(Arc::clone(&shared));
        tokio::spawn(async move {
            Server::builder()
                .add_service(service)
                .serve_with_incoming(TcpIncoming::from(listener))
                .await
                .expect("fake gitops engine");
        });
        (shared, format!("http://{addr}"))
    }
}

impl State {
    fn next_id(&mut self) -> String {
        self.next += 1;
        format!("h-{}", self.next)
    }

    fn record(
        &mut self,
        app: &str,
        revision: &str,
        phase: &str,
        principal: &Principal,
        execution_id: &str,
    ) -> String {
        let id = self.next_id();
        let source = self.apps.get(app).and_then(|a| a.source.clone());
        self.history.entry(app.to_owned()).or_default().insert(
            0,
            HistoryEntry {
                id: id.clone(),
                application_id: app.to_owned(),
                revision: revision.to_owned(),
                source,
                phase: phase.to_owned(),
                initiated_by: principal.user_id.clone(),
                execution_id: execution_id.to_owned(),
                ..Default::default()
            },
        );
        id
    }

    /// What a sync of `app` does (or would do), resource by resource.
    fn results(app: &Application, prune: bool) -> Vec<ResourceResult> {
        app.resources
            .iter()
            .map(|r| ResourceResult {
                group: r.group.clone(),
                kind: r.kind.clone(),
                namespace: r.namespace.clone(),
                name: r.name.clone(),
                status: match (r.sync_status.as_str(), prune) {
                    ("extraneous", true) => "pruned",
                    ("extraneous", false) => "prune_skipped",
                    ("synced", _) => "unchanged",
                    _ => "synced",
                }
                .into(),
                ..Default::default()
            })
            .collect()
    }

    fn sync(
        &mut self,
        principal: &Principal,
        req: &SyncApplicationRequest,
    ) -> Result<SyncResult, Status> {
        let Some(app) = self.apps.get(&req.id) else {
            return Err(Status::not_found(format!("no application {}", req.id)));
        };
        if app
            .operation
            .as_ref()
            .is_some_and(|op| op.phase == "running")
        {
            return Err(Status::failed_precondition(format!(
                "an operation is already running on application {}",
                app.name
            )));
        }
        if let Some(answer) = self.script.pop_front() {
            let mut result = match answer {
                Scripted::Status(status) => return Err(status),
                Scripted::Result(result) => result,
            };
            result.application_id = req.id.clone();
            if !req.dry_run {
                let revision = result.revision.clone();
                result.history_id = self.record(
                    &req.id,
                    &revision,
                    &result.phase,
                    principal,
                    &req.execution_id,
                );
            }
            return Ok(result);
        }
        let revision = if full_sha(&req.revision) {
            req.revision.clone()
        } else {
            REVISION.to_owned()
        };
        let resources = Self::results(app, req.prune);
        if req.dry_run {
            return Ok(SyncResult {
                application_id: req.id.clone(),
                phase: "succeeded".into(),
                revision,
                resources,
                health_status: app.health_status.clone(),
                ..Default::default()
            });
        }
        let app = self.apps.get_mut(&req.id).expect("checked above");
        let source = app.source.get_or_insert_with(ApplicationSource::default);
        for image in &req.kustomize_images {
            source
                .kustomize_images
                .retain(|existing| image_name(existing) != image_name(image));
            source.kustomize_images.push(image.clone());
        }
        source.helm_parameters.extend(req.helm_parameters.clone());
        let prune = req.prune;
        app.resources
            .retain(|r| !(prune && r.sync_status == "extraneous"));
        for r in &mut app.resources {
            if r.sync_status != "extraneous" {
                r.sync_status = "synced".into();
                if r.health_status == "missing" {
                    r.health_status = "healthy".into();
                }
            }
        }
        app.sync_status = "synced".into();
        app.synced_revision = revision.clone();
        let health = app.health_status.clone();
        let history_id = self.record(
            &req.id,
            &revision,
            "succeeded",
            principal,
            &req.execution_id,
        );
        Ok(SyncResult {
            application_id: req.id.clone(),
            phase: "succeeded".into(),
            revision,
            resources,
            history_id,
            health_status: health,
            ..Default::default()
        })
    }

    fn rollback(
        &mut self,
        principal: &Principal,
        req: &RollbackApplicationRequest,
    ) -> Result<SyncResult, Status> {
        if !self.apps.contains_key(&req.id) {
            return Err(Status::not_found(format!("no application {}", req.id)));
        }
        if let Some(answer) = self.script.pop_front() {
            let mut result = match answer {
                Scripted::Status(status) => return Err(status),
                Scripted::Result(result) => result,
            };
            let revision = result.revision.clone();
            result.history_id = self.record(
                &req.id,
                &revision,
                &result.phase,
                principal,
                &req.execution_id,
            );
            return Ok(result);
        }
        let Some(target) = self
            .history
            .get(&req.id)
            .and_then(|entries| entries.iter().find(|e| e.id == req.history_id))
            .cloned()
        else {
            return Err(Status::not_found(format!(
                "no history entry {}",
                req.history_id
            )));
        };
        let app = self.apps.get_mut(&req.id).expect("checked above");
        app.source = target.source.clone();
        app.synced_revision = target.revision.clone();
        app.sync_status = "synced".into();
        let resources = Self::results(app, req.prune);
        let health = app.health_status.clone();
        let history_id = self.record(
            &req.id,
            &target.revision,
            "succeeded",
            principal,
            &req.execution_id,
        );
        Ok(SyncResult {
            application_id: req.id.clone(),
            phase: "succeeded".into(),
            revision: target.revision,
            resources,
            history_id,
            health_status: health,
            ..Default::default()
        })
    }
}

fn caller<T>(request: &Request<T>, min: Role) -> Result<Principal, Status> {
    let principal = Principal::from_request(request)?;
    principal.require(min)?;
    Ok(principal)
}

fn unimplemented<T>() -> Result<Response<T>, Status> {
    Err(Status::unimplemented("not in the fake GitOps engine"))
}

#[tonic::async_trait]
impl GitopsService for FakeGitops {
    async fn list_clusters(
        &self,
        _: Request<ListClustersRequest>,
    ) -> Result<Response<ListClustersResponse>, Status> {
        unimplemented()
    }

    async fn upsert_cluster(
        &self,
        _: Request<UpsertClusterRequest>,
    ) -> Result<Response<Cluster>, Status> {
        unimplemented()
    }

    async fn delete_cluster(
        &self,
        _: Request<DeleteClusterRequest>,
    ) -> Result<Response<DeleteClusterResponse>, Status> {
        unimplemented()
    }

    async fn list_applications(
        &self,
        _: Request<ListApplicationsRequest>,
    ) -> Result<Response<ListApplicationsResponse>, Status> {
        unimplemented()
    }

    async fn get_application(
        &self,
        request: Request<GetApplicationRequest>,
    ) -> Result<Response<Application>, Status> {
        caller(&request, Role::Viewer)?;
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.read(&request.into_inner().id).map(Response::new)
    }

    async fn upsert_application(
        &self,
        _: Request<UpsertApplicationRequest>,
    ) -> Result<Response<Application>, Status> {
        unimplemented()
    }

    async fn delete_application(
        &self,
        _: Request<DeleteApplicationRequest>,
    ) -> Result<Response<DeleteApplicationResponse>, Status> {
        unimplemented()
    }

    async fn refresh_application(
        &self,
        request: Request<RefreshApplicationRequest>,
    ) -> Result<Response<Application>, Status> {
        caller(&request, Role::Member)?;
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        if let Some(status) = self.lock().refresh_errors.pop_front() {
            return Err(status);
        }
        self.read(&request.into_inner().id).map(Response::new)
    }

    async fn get_manifests(
        &self,
        _: Request<GetManifestsRequest>,
    ) -> Result<Response<Manifests>, Status> {
        unimplemented()
    }

    async fn diff_application(
        &self,
        _: Request<DiffApplicationRequest>,
    ) -> Result<Response<ApplicationDiff>, Status> {
        unimplemented()
    }

    async fn sync_application(
        &self,
        request: Request<SyncApplicationRequest>,
    ) -> Result<Response<SyncResult>, Status> {
        let principal = caller(&request, Role::Approver)?;
        let req = request.into_inner();
        let mut state = self.lock();
        state.syncs.push((principal.clone(), req.clone()));
        state.sync(&principal, &req).map(Response::new)
    }

    async fn rollback_application(
        &self,
        request: Request<RollbackApplicationRequest>,
    ) -> Result<Response<SyncResult>, Status> {
        let principal = caller(&request, Role::Approver)?;
        let req = request.into_inner();
        let mut state = self.lock();
        state.rollbacks.push((principal.clone(), req.clone()));
        state.rollback(&principal, &req).map(Response::new)
    }

    async fn list_history(
        &self,
        request: Request<ListHistoryRequest>,
    ) -> Result<Response<ListHistoryResponse>, Status> {
        caller(&request, Role::Viewer)?;
        let req = request.into_inner();
        let state = self.lock();
        if !state.apps.contains_key(&req.id) {
            return Err(Status::not_found(format!("no application {}", req.id)));
        }
        let limit = if req.limit == 0 {
            50
        } else {
            req.limit as usize
        };
        let entries = state
            .history
            .get(&req.id)
            .map(|entries| entries.iter().take(limit).cloned().collect())
            .unwrap_or_default();
        Ok(Response::new(ListHistoryResponse { entries }))
    }

    type WatchEventsStream = BoxStream<'static, Result<PlatformEvent, Status>>;

    async fn watch_events(
        &self,
        _: Request<WatchEventsRequest>,
    ) -> Result<Response<Self::WatchEventsStream>, Status> {
        unimplemented()
    }

    async fn delete_workspace_data(
        &self,
        _: Request<DeleteWorkspaceDataRequest>,
    ) -> Result<Response<DeleteWorkspaceDataResponse>, Status> {
        unimplemented()
    }

    type ExportWorkspaceDataStream = BoxStream<'static, Result<WorkspaceDataRecord, Status>>;

    async fn export_workspace_data(
        &self,
        _: Request<ExportWorkspaceDataRequest>,
    ) -> Result<Response<Self::ExportWorkspaceDataStream>, Status> {
        unimplemented()
    }
}

#[test]
fn overrides_merge_by_image_name() {
    assert_eq!(image_name("ghcr.io/acme/api:1.2.3"), "ghcr.io/acme/api");
    assert_eq!(
        image_name("registry:5000/acme/api"),
        "registry:5000/acme/api"
    );
    assert_eq!(image_name("api=ghcr.io/acme/api:2"), "api");
    assert_eq!(image_name("ghcr.io/acme/api@sha256:00"), "ghcr.io/acme/api");
}
