//! tonic service. Each RPC group lives in its own file; this module only
//! wires the trait to them and holds the shared helpers.

use std::sync::Arc;

use sdi_common::grpc::db_status;
use sdi_proto::execution::v1::{
    Approval, CancelExecutionRequest, CreateExecutionRequest, DecideApprovalRequest, DeliveryStats,
    Execution, ExecutionEvent, GetDeliveryStatsRequest, GetExecutionRequest,
    GrantPermissionRequest, Identity, ListApprovalsRequest, ListApprovalsResponse,
    ListEventsRequest, ListEventsResponse, ListExecutionsRequest, ListExecutionsResponse,
    ListIdentitiesRequest, ListIdentitiesResponse, ListPoliciesRequest, ListPoliciesResponse,
    Policy, ResolveFailureRequest, StartExecutionRequest, UpdatePolicyRequest,
    WatchExecutionRequest, execution_service_server::ExecutionService,
};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{convert, db, runner::Runtime};

mod approvals;
mod events;
mod executions;
mod identities;
mod policies;
mod stats;

#[derive(Clone)]
pub struct ExecutionGrpc {
    rt: Arc<Runtime>,
}

impl ExecutionGrpc {
    pub fn new(rt: Arc<Runtime>) -> Self {
        Self { rt }
    }
}

pub(crate) fn parse_id(raw: &str, what: &str) -> Result<Uuid, Status> {
    Uuid::parse_str(raw.trim())
        .map_err(|_| Status::invalid_argument(format!("{what} is not a valid id")))
}

/// Execution with its steps; the failure body only on single reads.
pub(crate) async fn load_execution(
    rt: &Runtime,
    id: Uuid,
    include_failure: bool,
) -> Result<Execution, Status> {
    let row = db::executions::get(&rt.pool, id).await.map_err(db_status)?;
    let steps = db::steps::list_for(&rt.pool, id).await.map_err(db_status)?;
    Ok(convert::execution_to_proto(row, steps, include_failure))
}

#[tonic::async_trait]
impl ExecutionService for ExecutionGrpc {
    async fn create_execution(
        &self,
        request: Request<CreateExecutionRequest>,
    ) -> Result<Response<Execution>, Status> {
        executions::create(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_executions(
        &self,
        request: Request<ListExecutionsRequest>,
    ) -> Result<Response<ListExecutionsResponse>, Status> {
        executions::list(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_execution(
        &self,
        request: Request<GetExecutionRequest>,
    ) -> Result<Response<Execution>, Status> {
        let id = parse_id(&request.into_inner().id, "id")?;
        load_execution(&self.rt, id, true).await.map(Response::new)
    }

    async fn start_execution(
        &self,
        request: Request<StartExecutionRequest>,
    ) -> Result<Response<Execution>, Status> {
        executions::start(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn resolve_failure(
        &self,
        request: Request<ResolveFailureRequest>,
    ) -> Result<Response<Execution>, Status> {
        executions::resolve(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn cancel_execution(
        &self,
        request: Request<CancelExecutionRequest>,
    ) -> Result<Response<Execution>, Status> {
        executions::cancel(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_events(
        &self,
        request: Request<ListEventsRequest>,
    ) -> Result<Response<ListEventsResponse>, Status> {
        events::list(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }

    type WatchExecutionStream = ReceiverStream<Result<ExecutionEvent, Status>>;

    async fn watch_execution(
        &self,
        request: Request<WatchExecutionRequest>,
    ) -> Result<Response<Self::WatchExecutionStream>, Status> {
        events::watch(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_approvals(
        &self,
        request: Request<ListApprovalsRequest>,
    ) -> Result<Response<ListApprovalsResponse>, Status> {
        approvals::list(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn decide_approval(
        &self,
        request: Request<DecideApprovalRequest>,
    ) -> Result<Response<Approval>, Status> {
        approvals::decide(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_policies(
        &self,
        _request: Request<ListPoliciesRequest>,
    ) -> Result<Response<ListPoliciesResponse>, Status> {
        policies::list(&self.rt).await.map(Response::new)
    }

    async fn update_policy(
        &self,
        request: Request<UpdatePolicyRequest>,
    ) -> Result<Response<Policy>, Status> {
        policies::update(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_identities(
        &self,
        _request: Request<ListIdentitiesRequest>,
    ) -> Result<Response<ListIdentitiesResponse>, Status> {
        identities::list(&self.rt).await.map(Response::new)
    }

    async fn grant_permission(
        &self,
        request: Request<GrantPermissionRequest>,
    ) -> Result<Response<Identity>, Status> {
        identities::grant(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_delivery_stats(
        &self,
        request: Request<GetDeliveryStatsRequest>,
    ) -> Result<Response<DeliveryStats>, Status> {
        stats::get(&self.rt, request.into_inner())
            .await
            .map(Response::new)
    }
}
