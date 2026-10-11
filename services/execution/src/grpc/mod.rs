//! tonic service. Each RPC group lives in its own file; this module only
//! reads the caller (CONTRACT.md §18), checks the role, wires the trait to
//! them and holds the shared helpers.
//!
//! Roles: reads need VIEWER; create / start / resolve / cancel of a
//! `simulated` run need MEMBER, of a `docker` or `github` run APPROVER;
//! DecideApproval and GrantPermission need APPROVER; UpdatePolicy needs
//! ADMIN; the workspace export needs ADMIN and the delete needs OWNER.

use std::sync::Arc;

use sdi_common::{
    grpc::db_status,
    principal::{Principal, Role},
};
use sdi_proto::{
    common::v1::{PlatformEvent, WatchEventsRequest},
    execution::v1::{
        Approval, CancelExecutionRequest, CreateExecutionRequest, DecideApprovalRequest,
        DeleteWorkspaceDataRequest, DeleteWorkspaceDataResponse, DeliveryStats, Execution,
        ExecutionEvent, ExportWorkspaceDataRequest, GetDeliveryStatsRequest, GetExecutionRequest,
        GrantPermissionRequest, Identity, ListApprovalsRequest, ListApprovalsResponse,
        ListEventsRequest, ListEventsResponse, ListExecutionsRequest, ListExecutionsResponse,
        ListIdentitiesRequest, ListIdentitiesResponse, ListPoliciesRequest, ListPoliciesResponse,
        Policy, ResolveFailureRequest, StartExecutionRequest, UpdatePolicyRequest,
        WatchExecutionRequest, WorkspaceDataRecord, execution_service_server::ExecutionService,
    },
};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{adapters, convert, db, identity, runner::Runtime};

mod approvals;
mod events;
mod executions;
mod identities;
mod policies;
mod stats;
mod workspace;

#[derive(Clone)]
pub struct ExecutionGrpc {
    rt: Arc<Runtime>,
}

impl ExecutionGrpc {
    pub fn new(rt: Arc<Runtime>) -> Self {
        Self { rt }
    }

    /// Reads the caller, checks the minimum role and makes sure the
    /// workspace has its policy and identity seed before anything else
    /// looks at the request body.
    async fn caller<T>(&self, request: &Request<T>, min: Role) -> Result<Caller, Status> {
        let caller = Caller::from_request(request)?;
        caller.require(min)?;
        self.rt.ensure_workspace(caller.workspace).await?;
        Ok(caller)
    }
}

/// The caller of one RPC: the forwarded principal plus its workspace as the
/// uuid every repository function takes.
#[derive(Debug, Clone)]
pub(crate) struct Caller {
    pub principal: Principal,
    pub workspace: Uuid,
}

impl Caller {
    pub fn from_request<T>(request: &Request<T>) -> Result<Self, Status> {
        let principal = Principal::from_request(request)?;
        let workspace = Uuid::parse_str(principal.workspace_id.trim())
            .map_err(|_| Status::unauthenticated("workspace id is not a valid id"))?;
        Ok(Self {
            principal,
            workspace,
        })
    }

    pub fn require(&self, min: Role) -> Result<(), Status> {
        self.principal.require(min)
    }

    /// The role an action on a run of `adapter` needs: dry runs are a
    /// member's, runs that touch Docker or GitHub an approver's.
    pub fn require_adapter(&self, adapter: &str) -> Result<(), Status> {
        self.require(if adapter == adapters::SIMULATED {
            Role::Member
        } else {
            Role::Approver
        })
    }

    /// Fire-and-forget audit entry for this caller's action (CONTRACT.md §19).
    pub fn audit(&self, rt: &Runtime, action: &str, entity: &str, entity_id: &str, detail: String) {
        rt.audit.record(
            &self.principal,
            identity::entry(&self.principal, action, entity, entity_id, detail),
        );
    }
}

pub(crate) fn parse_id(raw: &str, what: &str) -> Result<Uuid, Status> {
    Uuid::parse_str(raw.trim())
        .map_err(|_| Status::invalid_argument(format!("{what} is not a valid id")))
}

/// Execution with its steps; the failure body only on single reads.
pub(crate) async fn load_execution(
    rt: &Runtime,
    workspace: Uuid,
    id: Uuid,
    include_failure: bool,
) -> Result<Execution, Status> {
    let row = db::executions::get(&rt.pool, workspace, id)
        .await
        .map_err(db_status)?;
    let steps = db::steps::list_for(&rt.pool, workspace, id)
        .await
        .map_err(db_status)?;
    Ok(convert::execution_to_proto(row, steps, include_failure))
}

#[tonic::async_trait]
impl ExecutionService for ExecutionGrpc {
    async fn create_execution(
        &self,
        request: Request<CreateExecutionRequest>,
    ) -> Result<Response<Execution>, Status> {
        let caller = self.caller(&request, Role::Member).await?;
        executions::create(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_executions(
        &self,
        request: Request<ListExecutionsRequest>,
    ) -> Result<Response<ListExecutionsResponse>, Status> {
        let caller = self.caller(&request, Role::Viewer).await?;
        executions::list(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_execution(
        &self,
        request: Request<GetExecutionRequest>,
    ) -> Result<Response<Execution>, Status> {
        let caller = self.caller(&request, Role::Viewer).await?;
        let id = parse_id(&request.into_inner().id, "id")?;
        load_execution(&self.rt, caller.workspace, id, true)
            .await
            .map(Response::new)
    }

    async fn start_execution(
        &self,
        request: Request<StartExecutionRequest>,
    ) -> Result<Response<Execution>, Status> {
        let caller = self.caller(&request, Role::Member).await?;
        executions::start(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn resolve_failure(
        &self,
        request: Request<ResolveFailureRequest>,
    ) -> Result<Response<Execution>, Status> {
        let caller = self.caller(&request, Role::Member).await?;
        executions::resolve(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn cancel_execution(
        &self,
        request: Request<CancelExecutionRequest>,
    ) -> Result<Response<Execution>, Status> {
        let caller = self.caller(&request, Role::Member).await?;
        executions::cancel(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_events(
        &self,
        request: Request<ListEventsRequest>,
    ) -> Result<Response<ListEventsResponse>, Status> {
        let caller = self.caller(&request, Role::Viewer).await?;
        events::list(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    type WatchExecutionStream = ReceiverStream<Result<ExecutionEvent, Status>>;

    async fn watch_execution(
        &self,
        request: Request<WatchExecutionRequest>,
    ) -> Result<Response<Self::WatchExecutionStream>, Status> {
        let caller = self.caller(&request, Role::Viewer).await?;
        events::watch(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_approvals(
        &self,
        request: Request<ListApprovalsRequest>,
    ) -> Result<Response<ListApprovalsResponse>, Status> {
        let caller = self.caller(&request, Role::Viewer).await?;
        approvals::list(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn decide_approval(
        &self,
        request: Request<DecideApprovalRequest>,
    ) -> Result<Response<Approval>, Status> {
        let caller = self.caller(&request, Role::Approver).await?;
        approvals::decide(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_policies(
        &self,
        request: Request<ListPoliciesRequest>,
    ) -> Result<Response<ListPoliciesResponse>, Status> {
        let caller = self.caller(&request, Role::Viewer).await?;
        policies::list(&self.rt, &caller).await.map(Response::new)
    }

    async fn update_policy(
        &self,
        request: Request<UpdatePolicyRequest>,
    ) -> Result<Response<Policy>, Status> {
        let caller = self.caller(&request, Role::Admin).await?;
        policies::update(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_identities(
        &self,
        request: Request<ListIdentitiesRequest>,
    ) -> Result<Response<ListIdentitiesResponse>, Status> {
        let caller = self.caller(&request, Role::Viewer).await?;
        identities::list(&self.rt, &caller).await.map(Response::new)
    }

    async fn grant_permission(
        &self,
        request: Request<GrantPermissionRequest>,
    ) -> Result<Response<Identity>, Status> {
        let caller = self.caller(&request, Role::Approver).await?;
        identities::grant(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_delivery_stats(
        &self,
        request: Request<GetDeliveryStatsRequest>,
    ) -> Result<Response<DeliveryStats>, Status> {
        let caller = self.caller(&request, Role::Viewer).await?;
        stats::get(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    type WatchEventsStream = ReceiverStream<Result<PlatformEvent, Status>>;

    async fn watch_events(
        &self,
        request: Request<WatchEventsRequest>,
    ) -> Result<Response<Self::WatchEventsStream>, Status> {
        let caller = self.caller(&request, Role::Viewer).await?;
        Ok(Response::new(events::watch_platform(
            &self.rt,
            caller.workspace,
        )))
    }

    async fn delete_workspace_data(
        &self,
        request: Request<DeleteWorkspaceDataRequest>,
    ) -> Result<Response<DeleteWorkspaceDataResponse>, Status> {
        let caller = self.caller(&request, Role::Owner).await?;
        workspace::delete(&self.rt, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    type ExportWorkspaceDataStream = ReceiverStream<Result<WorkspaceDataRecord, Status>>;

    async fn export_workspace_data(
        &self,
        request: Request<ExportWorkspaceDataRequest>,
    ) -> Result<Response<Self::ExportWorkspaceDataStream>, Status> {
        let caller = self.caller(&request, Role::Admin).await?;
        workspace::export(&self.rt, &caller, request.into_inner()).map(Response::new)
    }
}
