//! tonic service: one file per RPC group, the trait impl here only delegates.

mod decisions;
mod documents;
mod negotiation;
mod proposals;
mod stats;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use sdi_proto::intelligence::v1::{
    AcceptProposalRequest, CreateDecisionRequest, Decision, Document, GenerateDocumentRequest,
    GenerateProposalRequest, GetDocumentRequest, GetIntelligenceStatsRequest, GetProposalRequest,
    IntelligenceStats, ListDecisionsRequest, ListDecisionsResponse, ListDocumentsRequest,
    ListDocumentsResponse, ListMessagesRequest, ListMessagesResponse, ListProposalsRequest,
    ListProposalsResponse, NegotiateRequest, NegotiateResponse, Proposal, RejectProposalRequest,
    SupersedeDecisionRequest, intelligence_service_server::IntelligenceService,
};
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{
    gateway::{ExecutionGateway, KnowledgeGateway},
    reasoner::Reasoner,
};

pub struct IntelligenceGrpc {
    pool: PgPool,
    knowledge: Arc<dyn KnowledgeGateway>,
    execution: Arc<dyn ExecutionGateway>,
    reasoner: Arc<dyn Reasoner>,
}

impl IntelligenceGrpc {
    pub fn new(
        pool: PgPool,
        knowledge: Arc<dyn KnowledgeGateway>,
        execution: Arc<dyn ExecutionGateway>,
        reasoner: Arc<dyn Reasoner>,
    ) -> Self {
        Self {
            pool,
            knowledge,
            execution,
            reasoner,
        }
    }
}

/// Our own ids are uuids; a malformed one is a bad request, not a miss.
pub(crate) fn parse_id(field: &str, value: &str) -> Result<Uuid, Status> {
    if value.trim().is_empty() {
        return Err(Status::invalid_argument(format!("{field} is required")));
    }
    Uuid::parse_str(value.trim())
        .map_err(|_| Status::invalid_argument(format!("{field} is not a valid id")))
}

pub(crate) fn require(field: &str, value: &str) -> Result<String, Status> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Status::invalid_argument(format!("{field} is required")));
    }
    Ok(trimmed.to_owned())
}

#[tonic::async_trait]
impl IntelligenceService for IntelligenceGrpc {
    async fn generate_proposal(
        &self,
        request: Request<GenerateProposalRequest>,
    ) -> Result<Response<Proposal>, Status> {
        proposals::generate(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_proposals(
        &self,
        request: Request<ListProposalsRequest>,
    ) -> Result<Response<ListProposalsResponse>, Status> {
        proposals::list(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_proposal(
        &self,
        request: Request<GetProposalRequest>,
    ) -> Result<Response<Proposal>, Status> {
        proposals::get(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn accept_proposal(
        &self,
        request: Request<AcceptProposalRequest>,
    ) -> Result<Response<Proposal>, Status> {
        proposals::accept(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn reject_proposal(
        &self,
        request: Request<RejectProposalRequest>,
    ) -> Result<Response<Proposal>, Status> {
        proposals::reject(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn negotiate(
        &self,
        request: Request<NegotiateRequest>,
    ) -> Result<Response<NegotiateResponse>, Status> {
        negotiation::negotiate(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_messages(
        &self,
        request: Request<ListMessagesRequest>,
    ) -> Result<Response<ListMessagesResponse>, Status> {
        negotiation::list_messages(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_decisions(
        &self,
        request: Request<ListDecisionsRequest>,
    ) -> Result<Response<ListDecisionsResponse>, Status> {
        decisions::list(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn create_decision(
        &self,
        request: Request<CreateDecisionRequest>,
    ) -> Result<Response<Decision>, Status> {
        decisions::create(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn supersede_decision(
        &self,
        request: Request<SupersedeDecisionRequest>,
    ) -> Result<Response<Decision>, Status> {
        decisions::supersede(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn generate_document(
        &self,
        request: Request<GenerateDocumentRequest>,
    ) -> Result<Response<Document>, Status> {
        documents::generate(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_documents(
        &self,
        request: Request<ListDocumentsRequest>,
    ) -> Result<Response<ListDocumentsResponse>, Status> {
        documents::list(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_document(
        &self,
        request: Request<GetDocumentRequest>,
    ) -> Result<Response<Document>, Status> {
        documents::get(self, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_intelligence_stats(
        &self,
        _request: Request<GetIntelligenceStatsRequest>,
    ) -> Result<Response<IntelligenceStats>, Status> {
        stats::get(self).await.map(Response::new)
    }
}
