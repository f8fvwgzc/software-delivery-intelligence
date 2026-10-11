//! tonic service: one file per RPC group, the trait impl here only delegates.

mod decisions;
mod documents;
mod negotiation;
mod proposals;
mod settings;
mod stats;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use sdi_common::grpc::db_status;
use sdi_proto::{
    common::v1::{PlatformEvent, WatchEventsRequest},
    intelligence::v1::{
        AcceptProposalRequest, CreateDecisionRequest, Decision, Document, GenerateDocumentRequest,
        GenerateProposalRequest, GetDocumentRequest, GetIntelligenceStatsRequest,
        GetProposalRequest, GetSettingsRequest, IntelligenceSettings, IntelligenceStats,
        ListDecisionsRequest, ListDecisionsResponse, ListDocumentsRequest, ListDocumentsResponse,
        ListMessagesRequest, ListMessagesResponse, ListProposalsRequest, ListProposalsResponse,
        NegotiateRequest, NegotiateResponse, Proposal, RejectProposalRequest,
        SupersedeDecisionRequest, UpdateSettingsRequest,
        intelligence_service_server::IntelligenceService,
    },
};
use sqlx::PgPool;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{
    db::settings::SettingsRow,
    events::EventBus,
    gateway::{ExecutionGateway, KnowledgeGateway},
    reasoner::{ClaudeReasoner, HeuristicReasoner, Reasoner},
};

/// What the environment allows the reasoner to be (CONTRACT.md §3).
#[derive(Debug, Clone, Default)]
pub struct ReasonerConfig {
    pub anthropic_api_key: Option<String>,
    /// `ANTHROPIC_MODEL`: used when the settings row names no model.
    pub anthropic_model: String,
}

pub struct IntelligenceGrpc {
    pool: PgPool,
    knowledge: Arc<dyn KnowledgeGateway>,
    execution: Arc<dyn ExecutionGateway>,
    reasoner: ReasonerConfig,
    events: EventBus,
}

impl IntelligenceGrpc {
    pub fn new(
        pool: PgPool,
        knowledge: Arc<dyn KnowledgeGateway>,
        execution: Arc<dyn ExecutionGateway>,
        reasoner: ReasonerConfig,
    ) -> Self {
        Self {
            pool,
            knowledge,
            execution,
            reasoner,
            events: EventBus::new(),
        }
    }

    /// Stored settings, or the defaults when the row is missing.
    pub(crate) async fn settings(&self) -> Result<SettingsRow, Status> {
        Ok(crate::db::settings::get(&self.pool)
            .await
            .map_err(db_status)?
            .unwrap_or_else(SettingsRow::defaults))
    }

    pub(crate) fn api_key_configured(&self) -> bool {
        self.reasoner.anthropic_api_key.is_some()
    }

    /// The model Claude would be called with: the stored one, else the environment's.
    pub(crate) fn effective_model(&self, settings: &SettingsRow) -> String {
        if settings.model.trim().is_empty() {
            self.reasoner.anthropic_model.clone()
        } else {
            settings.model.clone()
        }
    }

    /// The settings choose the reasoner; "claude" only works when the key is
    /// configured and falls back to the heuristic otherwise. Built per call so
    /// a model changed on the settings page applies to the next turn.
    pub(crate) fn reasoner_for(&self, settings: &SettingsRow) -> Arc<dyn Reasoner> {
        match &self.reasoner.anthropic_api_key {
            Some(key) if settings.reasoner == "claude" => Arc::new(ClaudeReasoner::new(
                key.clone(),
                self.effective_model(settings),
            )),
            _ => Arc::new(HeuristicReasoner),
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

    async fn get_settings(
        &self,
        _request: Request<GetSettingsRequest>,
    ) -> Result<Response<IntelligenceSettings>, Status> {
        settings::get(self).await.map(Response::new)
    }

    async fn update_settings(
        &self,
        request: Request<UpdateSettingsRequest>,
    ) -> Result<Response<IntelligenceSettings>, Status> {
        settings::update(self, request.into_inner())
            .await
            .map(Response::new)
    }

    type WatchEventsStream = ReceiverStream<Result<PlatformEvent, Status>>;

    async fn watch_events(
        &self,
        _request: Request<WatchEventsRequest>,
    ) -> Result<Response<Self::WatchEventsStream>, Status> {
        Ok(Response::new(self.events.watch()))
    }
}
