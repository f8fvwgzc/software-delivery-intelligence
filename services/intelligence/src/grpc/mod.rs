//! tonic service: one file per RPC group, the trait impl here only reads the
//! caller (CONTRACT.md §18), checks the role and delegates.
//!
//! Roles: reads need VIEWER (the delivery setup files are generated on
//! read and stored nowhere, so they count as one); generate / negotiate /
//! accept / reject, documents, diagrams and decisions need MEMBER; settings
//! need ADMIN; the workspace export needs ADMIN and the delete needs OWNER.

mod decisions;
mod delivery;
mod diagrams;
mod documents;
mod negotiation;
mod proposals;
mod review_runner;
mod reviews;
mod settings;
mod stats;
#[cfg(test)]
mod tests;
mod workspace;

use std::sync::Arc;

use chrono::Utc;
use sdi_common::{
    grpc::db_status,
    principal::{Principal, Role},
};
use sdi_proto::{
    common::v1::{PlatformEvent, WatchEventsRequest},
    identity::v1::{AuditEntry, ReasonerKey},
    intelligence::v1::{
        AcceptProposalRequest, CreateDecisionRequest, DecideFindingRequest, Decision,
        DeleteDiagramRequest, DeleteDiagramResponse, DeleteWorkspaceDataRequest,
        DeleteWorkspaceDataResponse, DeliveryArtifacts, Diagram, Document,
        ExportWorkspaceDataRequest, Finding, GenerateDeliveryArtifactsRequest,
        GenerateDiagramRequest, GenerateDocumentRequest, GenerateProposalRequest,
        GetDiagramRequest, GetDocumentRequest, GetIntelligenceStatsRequest, GetProposalRequest,
        GetReviewRequest, GetSettingsRequest, IntelligenceSettings, IntelligenceStats,
        ListDecisionsRequest, ListDecisionsResponse, ListDiagramsRequest, ListDiagramsResponse,
        ListDocumentsRequest, ListDocumentsResponse, ListMessagesRequest, ListMessagesResponse,
        ListProposalsRequest, ListProposalsResponse, ListReviewActivityRequest,
        ListReviewActivityResponse, ListReviewsRequest, ListReviewsResponse, NegotiateRequest,
        NegotiateResponse, Proposal, PublishReviewRequest, RejectProposalRequest, Review,
        StartReviewRequest, SupersedeDecisionRequest, UpdateDiagramRequest, UpdateSettingsRequest,
        WorkspaceDataRecord, intelligence_service_server::IntelligenceService,
    },
};
use sqlx::PgPool;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{
    config::ReasonerEnv,
    db::{settings::SettingsRow, usage::PgUsageLedger},
    events::EventBus,
    gateway::{ExecutionGateway, IdentityGateway, KnowledgeGateway},
    github::{GithubPublisher, GithubTokens},
    reasoner::{HeuristicReasoner, LlmReasoner, Provider, Reasoner},
};

/// What the environment allows the reasoner to be (CONTRACT.md §3, §19).
/// The environment is the development fallback only: it is used when the
/// workspace allows the reasoner but configured no provider or key of its own.
#[derive(Clone, Default)]
pub struct ReasonerConfig {
    pub env: ReasonerEnv,
}

impl std::fmt::Debug for ReasonerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the key.
        f.debug_struct("ReasonerConfig")
            .field("provider", &self.env.provider)
            .field("base_url", &self.env.base_url)
            .field("api_key", &self.env.api_key.as_ref().map(|_| "<set>"))
            .field("model", &self.env.model)
            .finish()
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
}

/// Whether, and with which provider and key, the workspace may use the
/// LLM reasoner.
#[derive(Clone, Default)]
pub(crate) struct ReasonerAccess {
    /// The workspace's data controls allow the reasoner (anything but `off`).
    pub allowed: bool,
    /// The key to call with: the workspace's own, else the dev fallback.
    /// An OpenAI-compatible endpoint without a key (Ollama, vLLM) carries
    /// an empty `api_key`.
    pub key: Option<ReasonerKey>,
    /// The provider the settings or the environment chose.
    pub provider: Provider,
}

impl ReasonerAccess {
    pub fn configured(&self) -> bool {
        self.allowed && self.key.is_some()
    }
}

pub struct IntelligenceGrpc {
    pool: PgPool,
    knowledge: Arc<dyn KnowledgeGateway>,
    execution: Arc<dyn ExecutionGateway>,
    identity: Arc<dyn IdentityGateway>,
    reasoner: ReasonerConfig,
    events: EventBus,
    /// GitHub access per workspace for `PublishReview` (CONTRACT.md §20).
    github_tokens: Arc<GithubTokens>,
    publisher: Arc<GithubPublisher>,
    /// The vulnerability check of added dependencies; `None` = off.
    osv: Option<Arc<crate::osv::OsvClient>>,
}

impl IntelligenceGrpc {
    pub fn new(
        pool: PgPool,
        knowledge: Arc<dyn KnowledgeGateway>,
        execution: Arc<dyn ExecutionGateway>,
        identity: Arc<dyn IdentityGateway>,
        reasoner: ReasonerConfig,
    ) -> Self {
        Self {
            pool,
            knowledge,
            execution,
            identity,
            reasoner,
            events: EventBus::new(),
            github_tokens: Arc::new(GithubTokens::new(None)),
            publisher: Arc::new(GithubPublisher::new()),
            osv: None,
        }
    }

    /// The OSV client reviews ask about added dependencies (`OSV_API_URL`).
    pub fn with_osv(mut self, osv: Option<Arc<crate::osv::OsvClient>>) -> Self {
        self.osv = osv;
        self
    }

    /// `GITHUB_TOKEN`: used to publish reviews only while identity cannot
    /// be reached (dev mode), like the knowledge service does.
    pub fn with_github_dev_token(mut self, token: Option<String>) -> Self {
        self.github_tokens = Arc::new(GithubTokens::new(token));
        self
    }

    /// Stored settings of the workspace, or the defaults when the row is missing.
    pub(crate) async fn settings(&self, caller: &Caller) -> Result<SettingsRow, Status> {
        Ok(crate::db::settings::get(&self.pool, caller.workspace)
            .await
            .map_err(db_status)?
            .unwrap_or_else(SettingsRow::defaults))
    }

    /// The provider a workspace's calls go to: the settings row's choice,
    /// else the environment's (`LLM_PROVIDER` / `LLM_BASE_URL`), else Anthropic.
    pub(crate) fn provider_for(&self, settings: &SettingsRow) -> Provider {
        if !settings.reasoner_provider.trim().is_empty()
            || !settings.reasoner_base_url.trim().is_empty()
        {
            let base_url = if settings.reasoner_base_url.trim().is_empty() {
                self.reasoner.env.base_url.as_str()
            } else {
                settings.reasoner_base_url.as_str()
            };
            return Provider::parse(&settings.reasoner_provider, base_url);
        }
        Provider::parse(&self.reasoner.env.provider, &self.reasoner.env.base_url)
    }

    /// Resolves the reasoner the workspace may use (CONTRACT.md §19):
    /// `GetDataControls.reasoner` must not be `off`, then the workspace's
    /// own key is used, and only when it has none the environment's. An
    /// OpenAI-compatible endpoint needs no key. A missing identity service
    /// means "off": the heuristic always works.
    pub(crate) async fn reasoner_access(
        &self,
        caller: &Caller,
        settings: &SettingsRow,
    ) -> ReasonerAccess {
        let provider = self.provider_for(settings);
        let controls = match self.identity.data_controls(&caller.principal).await {
            Ok(controls) => controls,
            Err(error) => {
                tracing::warn!(%error, "data controls unavailable; reasoner stays off");
                return ReasonerAccess {
                    provider,
                    ..ReasonerAccess::default()
                };
            }
        };
        if matches!(
            controls.reasoner.trim().to_ascii_lowercase().as_str(),
            "" | "off" | "none" | "heuristic"
        ) {
            return ReasonerAccess {
                allowed: false,
                key: None,
                provider,
            };
        }
        let key = match self.identity.reasoner_key(&caller.principal).await {
            Ok(key) => key,
            Err(error) => {
                tracing::warn!(%error, "workspace reasoner key unavailable");
                None
            }
        };
        let key = key
            .filter(|k| !k.api_key.trim().is_empty())
            .or_else(|| {
                self.reasoner
                    .env
                    .api_key
                    .clone()
                    .map(|api_key| ReasonerKey {
                        api_key,
                        model: String::new(),
                    })
            })
            .or_else(|| {
                // A local OpenAI-compatible endpoint (Ollama, vLLM) has no key.
                matches!(provider, Provider::OpenAiCompatible { .. }).then(ReasonerKey::default)
            });
        ReasonerAccess {
            allowed: true,
            key,
            provider,
        }
    }

    /// The model the provider is called with: the settings' choice, else
    /// the one stored with the workspace key, else the environment's.
    pub(crate) fn effective_model(
        &self,
        settings: &SettingsRow,
        access: &ReasonerAccess,
    ) -> String {
        if !settings.model.trim().is_empty() {
            return settings.model.clone();
        }
        access
            .key
            .as_ref()
            .map(|k| k.model.trim())
            .filter(|m| !m.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| self.reasoner.env.model.clone())
    }

    /// The settings choose the reasoner; the LLM only works when the
    /// workspace allows it and a key (or keyless endpoint) exists, and falls
    /// back to the heuristic otherwise. Built per call so a changed key,
    /// provider or model applies to the next turn.
    pub(crate) fn reasoner_for(
        &self,
        caller: &Caller,
        settings: &SettingsRow,
        access: &ReasonerAccess,
    ) -> Arc<dyn Reasoner> {
        match &access.key {
            Some(key) if access.allowed && settings.wants_llm() => Arc::new(LlmReasoner::new(
                access.provider.clone(),
                key.api_key.clone(),
                self.effective_model(settings, access),
                caller.workspace,
                settings.daily_token_cap(),
                Arc::new(PgUsageLedger {
                    pool: self.pool.clone(),
                }),
            )),
            _ => Arc::new(HeuristicReasoner),
        }
    }

    /// Fire-and-forget audit entry for the caller's action (CONTRACT.md §19).
    pub(crate) fn audit(
        &self,
        caller: &Caller,
        action: &str,
        entity: &str,
        entity_id: &str,
        detail: String,
    ) {
        self.identity.record_audit(
            &caller.principal,
            AuditEntry {
                workspace_id: caller.principal.workspace_id.clone(),
                user_id: caller.principal.user_id.clone(),
                action: action.to_owned(),
                entity: entity.to_owned(),
                entity_id: entity_id.to_owned(),
                detail,
                at: Some(sdi_common::time::to_proto(Utc::now())),
                ..Default::default()
            },
        );
    }
}

/// Page size of every list RPC (CONTRACT.md §21): 50 unless asked, never
/// more than `MAX_PAGE`.
pub(crate) const DEFAULT_PAGE: i64 = 50;
pub(crate) const MAX_PAGE: i64 = 200;

pub(crate) fn page_limit(raw: u32) -> i64 {
    if raw == 0 {
        DEFAULT_PAGE
    } else {
        i64::from(raw).min(MAX_PAGE)
    }
}

/// The `before` cursor of a list RPC, when set.
pub(crate) fn before(raw: Option<&prost_types::Timestamp>) -> Option<chrono::DateTime<Utc>> {
    sdi_common::time::opt_from_proto(raw)
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

/// Reads the caller and checks the role before the request body is looked at.
fn caller<T>(request: &Request<T>, min: Role) -> Result<Caller, Status> {
    let caller = Caller::from_request(request)?;
    caller.require(min)?;
    Ok(caller)
}

#[tonic::async_trait]
impl IntelligenceService for IntelligenceGrpc {
    async fn generate_proposal(
        &self,
        request: Request<GenerateProposalRequest>,
    ) -> Result<Response<Proposal>, Status> {
        let caller = caller(&request, Role::Member)?;
        proposals::generate(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_proposals(
        &self,
        request: Request<ListProposalsRequest>,
    ) -> Result<Response<ListProposalsResponse>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        proposals::list(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_proposal(
        &self,
        request: Request<GetProposalRequest>,
    ) -> Result<Response<Proposal>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        proposals::get(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn accept_proposal(
        &self,
        request: Request<AcceptProposalRequest>,
    ) -> Result<Response<Proposal>, Status> {
        let caller = caller(&request, Role::Member)?;
        proposals::accept(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn reject_proposal(
        &self,
        request: Request<RejectProposalRequest>,
    ) -> Result<Response<Proposal>, Status> {
        let caller = caller(&request, Role::Member)?;
        proposals::reject(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn negotiate(
        &self,
        request: Request<NegotiateRequest>,
    ) -> Result<Response<NegotiateResponse>, Status> {
        let caller = caller(&request, Role::Member)?;
        negotiation::negotiate(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_messages(
        &self,
        request: Request<ListMessagesRequest>,
    ) -> Result<Response<ListMessagesResponse>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        negotiation::list_messages(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_decisions(
        &self,
        request: Request<ListDecisionsRequest>,
    ) -> Result<Response<ListDecisionsResponse>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        decisions::list(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn create_decision(
        &self,
        request: Request<CreateDecisionRequest>,
    ) -> Result<Response<Decision>, Status> {
        let caller = caller(&request, Role::Member)?;
        decisions::create(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn supersede_decision(
        &self,
        request: Request<SupersedeDecisionRequest>,
    ) -> Result<Response<Decision>, Status> {
        let caller = caller(&request, Role::Member)?;
        decisions::supersede(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn generate_document(
        &self,
        request: Request<GenerateDocumentRequest>,
    ) -> Result<Response<Document>, Status> {
        let caller = caller(&request, Role::Member)?;
        documents::generate(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_documents(
        &self,
        request: Request<ListDocumentsRequest>,
    ) -> Result<Response<ListDocumentsResponse>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        documents::list(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_document(
        &self,
        request: Request<GetDocumentRequest>,
    ) -> Result<Response<Document>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        documents::get(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_intelligence_stats(
        &self,
        request: Request<GetIntelligenceStatsRequest>,
    ) -> Result<Response<IntelligenceStats>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        stats::get(self, &caller).await.map(Response::new)
    }

    async fn generate_diagram(
        &self,
        request: Request<GenerateDiagramRequest>,
    ) -> Result<Response<Diagram>, Status> {
        let caller = caller(&request, Role::Member)?;
        diagrams::generate(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_diagrams(
        &self,
        request: Request<ListDiagramsRequest>,
    ) -> Result<Response<ListDiagramsResponse>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        diagrams::list(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_diagram(
        &self,
        request: Request<GetDiagramRequest>,
    ) -> Result<Response<Diagram>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        diagrams::get(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn update_diagram(
        &self,
        request: Request<UpdateDiagramRequest>,
    ) -> Result<Response<Diagram>, Status> {
        let caller = caller(&request, Role::Member)?;
        diagrams::update(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn delete_diagram(
        &self,
        request: Request<DeleteDiagramRequest>,
    ) -> Result<Response<DeleteDiagramResponse>, Status> {
        let caller = caller(&request, Role::Member)?;
        diagrams::delete(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn start_review(
        &self,
        request: Request<StartReviewRequest>,
    ) -> Result<Response<Review>, Status> {
        let caller = caller(&request, Role::Member)?;
        reviews::start(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_reviews(
        &self,
        request: Request<ListReviewsRequest>,
    ) -> Result<Response<ListReviewsResponse>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        reviews::list(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_review(
        &self,
        request: Request<GetReviewRequest>,
    ) -> Result<Response<Review>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        reviews::get(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn decide_finding(
        &self,
        request: Request<DecideFindingRequest>,
    ) -> Result<Response<Finding>, Status> {
        let caller = caller(&request, Role::Member)?;
        reviews::decide_finding(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn publish_review(
        &self,
        request: Request<PublishReviewRequest>,
    ) -> Result<Response<Review>, Status> {
        let caller = caller(&request, Role::Approver)?;
        reviews::publish(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_review_activity(
        &self,
        request: Request<ListReviewActivityRequest>,
    ) -> Result<Response<ListReviewActivityResponse>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        reviews::list_activity(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn generate_delivery_artifacts(
        &self,
        request: Request<GenerateDeliveryArtifactsRequest>,
    ) -> Result<Response<DeliveryArtifacts>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        delivery::generate(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn get_settings(
        &self,
        request: Request<GetSettingsRequest>,
    ) -> Result<Response<IntelligenceSettings>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        settings::get(self, &caller).await.map(Response::new)
    }

    async fn update_settings(
        &self,
        request: Request<UpdateSettingsRequest>,
    ) -> Result<Response<IntelligenceSettings>, Status> {
        let caller = caller(&request, Role::Admin)?;
        settings::update(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    type WatchEventsStream = ReceiverStream<Result<PlatformEvent, Status>>;

    async fn watch_events(
        &self,
        request: Request<WatchEventsRequest>,
    ) -> Result<Response<Self::WatchEventsStream>, Status> {
        let caller = caller(&request, Role::Viewer)?;
        Ok(Response::new(self.events.watch(caller.workspace)))
    }

    async fn delete_workspace_data(
        &self,
        request: Request<DeleteWorkspaceDataRequest>,
    ) -> Result<Response<DeleteWorkspaceDataResponse>, Status> {
        let caller = caller(&request, Role::Owner)?;
        workspace::delete(self, &caller, request.into_inner())
            .await
            .map(Response::new)
    }

    type ExportWorkspaceDataStream = ReceiverStream<Result<WorkspaceDataRecord, Status>>;

    async fn export_workspace_data(
        &self,
        request: Request<ExportWorkspaceDataRequest>,
    ) -> Result<Response<Self::ExportWorkspaceDataStream>, Status> {
        let caller = caller(&request, Role::Admin)?;
        workspace::export(self, &caller, request.into_inner()).map(Response::new)
    }
}
