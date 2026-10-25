//! Shared handler state: one lazy channel per backing service. Lazy means
//! the gateway boots (and serves `/health`, docs and the OpenAPI document)
//! before any service is up, and reconnects on its own once they are.
//!
//! Every call to knowledge, intelligence, execution or gitops carries the caller's
//! `Principal` as gRPC metadata (CONTRACT.md §18): handlers get their
//! clients from a [`Scoped`] extractor, never from the bare state.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use axum::{extract::FromRequestParts, http::request::Parts};
use chrono::{DateTime, Utc};
use sdi_common::{cache::TtlCache, grpc::MAX_MESSAGE_BYTES, principal::Principal};
use sdi_proto::{
    execution::v1::execution_service_client::ExecutionServiceClient,
    gitops::v1::gitops_service_client::GitopsServiceClient,
    identity::v1::identity_service_client::IdentityServiceClient,
    intelligence::v1::intelligence_service_client::IntelligenceServiceClient,
    knowledge::v1::knowledge_service_client::KnowledgeServiceClient,
};
use tonic::{Request, Status, service::interceptor::InterceptedService, transport::Channel};

use crate::{
    dto::identity::{User, Workspace},
    error::ApiError,
};

/// How long one `Identity.Authorize` answer is reused (CONTRACT.md §18).
pub const AUTH_CACHE_TTL: Duration = Duration::from_secs(60);
/// `GATEWAY_AUTH_CACHE_MAX`: entries the per-replica authorize cache holds
/// (one per session × workspace); the oldest goes when it is full.
pub const DEFAULT_AUTH_CACHE_MAX: usize = 10_000;
/// `SSE_MAX_PER_WORKSPACE`: open event streams one workspace may hold on
/// this replica before new ones are answered 429.
pub const DEFAULT_SSE_MAX_PER_WORKSPACE: usize = 50;
/// `WEBHOOK_QUEUE_MAX`: deliveries waiting for the webhook worker before
/// GitHub is told to retry (503).
pub const DEFAULT_WEBHOOK_QUEUE_MAX: usize = 256;
/// Outcomes of the latest webhook deliveries kept for diagnostics.
const WEBHOOK_LOG_MAX: usize = 256;
/// `GITOPS_GRPC_URL` when unset: the GitOps engine's port (CONTRACT.md §24.1).
pub const DEFAULT_GITOPS_URL: &str = "http://127.0.0.1:50055";

/// One backing service, as the system status page reports it.
#[derive(Debug, Clone)]
pub struct ServiceLink {
    pub name: &'static str,
    pub url: String,
    pub channel: Channel,
}

/// What the gateway needs to know about its own deployment (§18).
#[derive(Debug, Clone)]
pub struct GatewaySettings {
    /// `SDI_PUBLIC_URL`: the origin the browser uses. `https` makes the
    /// session cookie `Secure`.
    pub public_url: String,
    /// `GITHUB_WEBHOOK_SECRET`: HMAC key of `POST /webhooks/github`.
    pub webhook_secret: Option<String>,
    /// `GITHUB_APP_SLUG`: builds the install URL.
    pub app_slug: Option<String>,
    /// `SDI_DEV_LOGIN=1`: the callback accepts the local `state=dev` flow.
    pub dev_login: bool,
    /// Bound of the authorize cache (`GATEWAY_AUTH_CACHE_MAX`).
    pub auth_cache_max: usize,
    /// Open SSE streams per workspace on this replica (`SSE_MAX_PER_WORKSPACE`).
    pub sse_max_per_workspace: usize,
    /// Webhook deliveries that may wait for the worker (`WEBHOOK_QUEUE_MAX`).
    pub webhook_queue_max: usize,
}

impl Default for GatewaySettings {
    fn default() -> Self {
        Self {
            public_url: String::new(),
            webhook_secret: None,
            app_slug: None,
            dev_login: false,
            auth_cache_max: DEFAULT_AUTH_CACHE_MAX,
            sse_max_per_workspace: DEFAULT_SSE_MAX_PER_WORKSPACE,
            webhook_queue_max: DEFAULT_WEBHOOK_QUEUE_MAX,
        }
    }
}

/// The resolved caller of one request, as the auth middleware leaves it in
/// the request extensions.
#[derive(Debug, Clone)]
pub struct Auth {
    pub principal: Principal,
    pub user: User,
    pub workspace: Workspace,
    /// The raw session token (for logout).
    pub token: String,
}

/// What one webhook delivery asks the worker to do, decided synchronously
/// in the handler so the 202 can say "queued" or "ignored" honestly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookJob {
    RecordInstallation,
    RemoveInstallation {
        installation_id: i64,
    },
    SyncMembers,
    Rescan {
        full_name: String,
    },
    AutoReview {
        full_name: String,
        number: u64,
    },
    /// A workflow run succeeded on the default branch: a release, when the
    /// workflow is the project's build workflow (CONTRACT.md §22).
    BuildSucceeded {
        full_name: String,
        /// `.github/workflows/ci.yml`.
        workflow_path: String,
        workflow_name: String,
        head_sha: String,
        run_id: String,
    },
    /// A container image was published to the registry: a release with the
    /// tag and digest the event carries.
    PackagePublished {
        full_name: String,
        /// `host/namespace/name`, without tag.
        image: String,
        tag: String,
        digest: String,
        /// The commit the package was built from, when the event says.
        commit_sha: String,
    },
}

/// A delivery waiting for the worker: the job plus what it needs from the
/// payload (the installation record, re-read by every job).
#[derive(Debug, Clone)]
pub struct WebhookDelivery {
    pub delivery_id: String,
    pub event: String,
    pub job: WebhookJob,
    pub installation: sdi_proto::identity::v1::RecordInstallationRequest,
}

/// The gateway's per-replica memory. Every piece is bounded: the authorize
/// cache by `auth_cache_max`, the stream counters by the number of
/// workspaces currently streaming, the webhook queue by `webhook_queue_max`
/// and the webhook log by [`WEBHOOK_LOG_MAX`]. Nothing is shared between
/// replicas, which is why they need no sticky sessions: a cache miss costs
/// one `Identity.Authorize` (docs/self-hosting.md).
#[derive(Clone)]
pub struct AppState {
    /// knowledge, intelligence, execution, identity, gitops (in that order).
    pub services: Arc<[ServiceLink; 5]>,
    pub settings: Arc<GatewaySettings>,
    pub started_at: DateTime<Utc>,
    pub version: &'static str,
    /// `Authorize` answers keyed by (session token, requested workspace).
    auth_cache: Arc<Mutex<TtlCache<(String, String), Auth>>>,
    /// Open SSE streams per workspace on this replica.
    streams: Arc<Mutex<HashMap<String, usize>>>,
    /// Deliveries for the webhook worker (bounded; see `webhook_queue_max`).
    webhooks: tokio::sync::mpsc::Sender<WebhookDelivery>,
    /// `(delivery id, outcome)` of the latest processed deliveries.
    webhook_log: Arc<Mutex<VecDeque<(String, String)>>>,
}

impl AppState {
    /// Reads the five `*_GRPC_URL` variables and the gateway's own settings
    /// (CONTRACT.md §3, §18, §24.1).
    pub fn from_env() -> Result<Self> {
        use sdi_common::config;
        let settings = GatewaySettings {
            public_url: config::public_url(),
            webhook_secret: config::env_opt("GITHUB_WEBHOOK_SECRET"),
            app_slug: config::env_opt("GITHUB_APP_SLUG"),
            dev_login: config::env_or("SDI_DEV_LOGIN", "0") == "1",
            auth_cache_max: config::env_parse("GATEWAY_AUTH_CACHE_MAX", DEFAULT_AUTH_CACHE_MAX)?,
            sse_max_per_workspace: config::env_parse(
                "SSE_MAX_PER_WORKSPACE",
                DEFAULT_SSE_MAX_PER_WORKSPACE,
            )?,
            webhook_queue_max: config::env_parse("WEBHOOK_QUEUE_MAX", DEFAULT_WEBHOOK_QUEUE_MAX)?,
        };
        Self::with_urls(
            &config::knowledge_url(),
            &config::intelligence_url(),
            &config::execution_url(),
            &config::identity_url(),
            &config::env_or("GITOPS_GRPC_URL", DEFAULT_GITOPS_URL),
            settings,
        )
    }

    pub fn with_urls(
        knowledge: &str,
        intelligence: &str,
        execution: &str,
        identity: &str,
        gitops: &str,
        settings: GatewaySettings,
    ) -> Result<Self> {
        let link = |name, url: &str| -> Result<ServiceLink> {
            Ok(ServiceLink {
                name,
                url: url.to_owned(),
                channel: sdi_common::grpc::lazy_channel(url)?,
            })
        };
        let (webhooks, inbox) =
            tokio::sync::mpsc::channel::<WebhookDelivery>(settings.webhook_queue_max.max(1));
        let state = Self {
            services: Arc::new([
                link("knowledge", knowledge)?,
                link("intelligence", intelligence)?,
                link("execution", execution)?,
                link("identity", identity)?,
                link("gitops", gitops)?,
            ]),
            auth_cache: Arc::new(Mutex::new(TtlCache::new(
                settings.auth_cache_max,
                AUTH_CACHE_TTL,
            ))),
            settings: Arc::new(settings),
            started_at: Utc::now(),
            version: env!("CARGO_PKG_VERSION"),
            streams: Arc::new(Mutex::new(HashMap::new())),
            webhooks,
            webhook_log: Arc::new(Mutex::new(VecDeque::new())),
        };
        // One worker per replica drains deliveries in order; GitHub only
        // needs the 202, the work itself may take a few gRPC round trips.
        sdi_common::tasks::spawn(
            "webhook-worker",
            crate::routes::webhooks::worker(state.clone(), inbox),
        );
        Ok(state)
    }

    /// Identity is the one service called without a principal: it is the
    /// service that produces them.
    pub fn identity(&self) -> IdentityServiceClient<Channel> {
        IdentityServiceClient::new(self.services[3].channel.clone())
            .max_decoding_message_size(MAX_MESSAGE_BYTES)
            .max_encoding_message_size(MAX_MESSAGE_BYTES)
    }

    /// Hands a delivery to the worker without waiting. `false` means the
    /// queue is full (the delivery is dropped): the caller answers 503 so
    /// GitHub redelivers later.
    pub fn enqueue_webhook(&self, delivery: WebhookDelivery) -> bool {
        self.webhooks.try_send(delivery).is_ok()
    }

    /// Records what a delivery ended as (bounded log, newest last).
    pub fn note_webhook(&self, delivery_id: &str, outcome: &str) {
        let mut log = self.webhook_log.lock().expect("webhook log lock");
        if log.len() >= WEBHOOK_LOG_MAX {
            log.pop_front();
        }
        log.push_back((delivery_id.to_owned(), outcome.to_owned()));
    }

    /// The recorded outcome of a delivery, when the worker has reached it.
    #[cfg(test)]
    pub fn webhook_outcome(&self, delivery_id: &str) -> Option<String> {
        self.webhook_log
            .lock()
            .expect("webhook log lock")
            .iter()
            .rev()
            .find(|(id, _)| id == delivery_id)
            .map(|(_, outcome)| outcome.clone())
    }

    /// Claims one SSE slot for the workspace, or `None` once the per-replica
    /// cap is reached. The guard releases the slot when the stream ends.
    pub fn open_stream(&self, workspace_id: &str) -> Option<StreamSlot> {
        let mut streams = self.streams.lock().expect("stream counter lock");
        let count = streams.entry(workspace_id.to_owned()).or_insert(0);
        if *count >= self.settings.sse_max_per_workspace {
            if *count == 0 {
                streams.remove(workspace_id);
            }
            return None;
        }
        *count += 1;
        Some(StreamSlot {
            streams: Arc::clone(&self.streams),
            workspace_id: workspace_id.to_owned(),
        })
    }

    /// Open streams of a workspace on this replica (diagnostics, tests).
    #[cfg(test)]
    pub fn open_streams(&self, workspace_id: &str) -> usize {
        self.streams
            .lock()
            .expect("stream counter lock")
            .get(workspace_id)
            .copied()
            .unwrap_or(0)
    }

    /// Clients scoped to a principal. Handlers use the [`Scoped`] extractor;
    /// this is for work done on nobody's behalf (webhooks, exports).
    pub fn scoped(&self, principal: Principal) -> Scoped {
        Scoped {
            state: self.clone(),
            principal,
        }
    }

    /// Internal jobs run as `system` in one workspace (CONTRACT.md §18).
    pub fn system(&self, workspace_id: &str) -> Scoped {
        self.scoped(Principal::system(workspace_id))
    }

    pub fn cached_auth(&self, token: &str, workspace: &str) -> Option<Auth> {
        let mut cache = self.auth_cache.lock().expect("auth cache lock");
        cache
            .get(&(token.to_owned(), workspace.to_owned()))
            .cloned()
    }

    pub fn remember_auth(&self, token: &str, workspace: &str, auth: Auth) {
        let mut cache = self.auth_cache.lock().expect("auth cache lock");
        cache.insert((token.to_owned(), workspace.to_owned()), auth);
    }

    /// Entries currently held (diagnostics, tests).
    #[cfg(test)]
    pub fn cached_auths(&self) -> usize {
        self.auth_cache.lock().expect("auth cache lock").len()
    }

    /// Every cached answer for a session (logout) or a workspace (deletion).
    pub fn forget_auth(&self, token: Option<&str>, workspace: Option<&str>) {
        let mut cache = self.auth_cache.lock().expect("auth cache lock");
        cache.retain(|(t, w), auth| {
            token.is_none_or(|token| t != token)
                && workspace.is_none_or(|ws| auth.principal.workspace_id != ws && w != ws)
        });
    }
}

/// One claimed SSE slot; dropping it frees the slot.
pub struct StreamSlot {
    streams: Arc<Mutex<HashMap<String, usize>>>,
    workspace_id: String,
}

impl Drop for StreamSlot {
    fn drop(&mut self) {
        let mut streams = self.streams.lock().expect("stream counter lock");
        if let Some(count) = streams.get_mut(&self.workspace_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                streams.remove(&self.workspace_id);
            }
        }
    }
}

/// Copies the three principal headers onto every outgoing call.
#[derive(Clone)]
pub struct Forward(pub Principal);

impl tonic::service::Interceptor for Forward {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        self.0.apply(&mut request);
        Ok(request)
    }
}

pub type KnowledgeClient = KnowledgeServiceClient<InterceptedService<Channel, Forward>>;
pub type IdentityCallerClient = IdentityServiceClient<InterceptedService<Channel, Forward>>;
pub type IntelligenceClient = IntelligenceServiceClient<InterceptedService<Channel, Forward>>;
pub type ExecutionClient = ExecutionServiceClient<InterceptedService<Channel, Forward>>;
pub type GitopsClient = GitopsServiceClient<InterceptedService<Channel, Forward>>;

/// The state plus the caller: the only way handlers reach a service.
#[derive(Clone)]
pub struct Scoped {
    pub state: AppState,
    pub principal: Principal,
}

impl Scoped {
    pub fn knowledge(&self) -> KnowledgeClient {
        KnowledgeServiceClient::with_interceptor(
            self.state.services[0].channel.clone(),
            Forward(self.principal.clone()),
        )
        .max_decoding_message_size(MAX_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_MESSAGE_BYTES)
    }

    pub fn intelligence(&self) -> IntelligenceClient {
        IntelligenceServiceClient::with_interceptor(
            self.state.services[1].channel.clone(),
            Forward(self.principal.clone()),
        )
        .max_decoding_message_size(MAX_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_MESSAGE_BYTES)
    }

    pub fn execution(&self) -> ExecutionClient {
        ExecutionServiceClient::with_interceptor(
            self.state.services[2].channel.clone(),
            Forward(self.principal.clone()),
        )
        .max_decoding_message_size(MAX_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_MESSAGE_BYTES)
    }

    /// The GitOps engine (CONTRACT.md §24). It checks the forwarded role
    /// itself, like every service.
    pub fn gitops(&self) -> GitopsClient {
        GitopsServiceClient::with_interceptor(
            self.state.services[4].channel.clone(),
            Forward(self.principal.clone()),
        )
        .max_decoding_message_size(MAX_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_MESSAGE_BYTES)
    }

    pub fn identity(&self) -> IdentityServiceClient<Channel> {
        self.state.identity()
    }

    /// Identity with the caller's principal attached. Workspace secrets
    /// (CONTRACT.md §22) are the one part of identity that checks the role
    /// itself: an admin of the workspace manages them, whoever asks.
    pub fn identity_as_caller(&self) -> IdentityCallerClient {
        IdentityServiceClient::with_interceptor(
            self.state.services[3].channel.clone(),
            Forward(self.principal.clone()),
        )
        .max_decoding_message_size(MAX_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_MESSAGE_BYTES)
    }

    pub fn workspace_id(&self) -> &str {
        &self.principal.workspace_id
    }
}

impl FromRequestParts<AppState> for Scoped {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth = Auth::from_request_parts(parts, state).await?;
        Ok(state.scoped(auth.principal))
    }
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &AppState) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Auth>()
            .cloned()
            .ok_or_else(ApiError::unauthenticated)
    }
}
