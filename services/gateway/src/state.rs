//! Shared handler state: one lazy channel per backing service. Lazy means
//! the gateway boots (and serves `/health`, docs and the OpenAPI document)
//! before any service is up, and reconnects on its own once they are.
//!
//! Every call to knowledge, intelligence or execution carries the caller's
//! `Principal` as gRPC metadata (CONTRACT.md §18): handlers get their
//! clients from a [`Scoped`] extractor, never from the bare state.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::Result;
use axum::{extract::FromRequestParts, http::request::Parts};
use chrono::{DateTime, Utc};
use sdi_common::principal::Principal;
use sdi_proto::{
    execution::v1::execution_service_client::ExecutionServiceClient,
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

/// One backing service, as the system status page reports it.
#[derive(Debug, Clone)]
pub struct ServiceLink {
    pub name: &'static str,
    pub url: String,
    pub channel: Channel,
}

/// What the gateway needs to know about its own deployment (§18).
#[derive(Debug, Clone, Default)]
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

struct CacheEntry {
    at: Instant,
    auth: Auth,
}

#[derive(Clone)]
pub struct AppState {
    pub services: Arc<[ServiceLink; 4]>,
    pub settings: Arc<GatewaySettings>,
    pub started_at: DateTime<Utc>,
    pub version: &'static str,
    /// `Authorize` answers keyed by (session token, requested workspace).
    auth_cache: Arc<Mutex<HashMap<(String, String), CacheEntry>>>,
}

impl AppState {
    /// Reads the four `*_GRPC_URL` variables and the gateway's own settings
    /// (CONTRACT.md §3, §18).
    pub fn from_env() -> Result<Self> {
        use sdi_common::config;
        let settings = GatewaySettings {
            public_url: config::env_or("SDI_PUBLIC_URL", "http://127.0.0.1:5173")
                .trim_end_matches('/')
                .to_owned(),
            webhook_secret: config::env_opt("GITHUB_WEBHOOK_SECRET"),
            app_slug: config::env_opt("GITHUB_APP_SLUG"),
            dev_login: config::env_or("SDI_DEV_LOGIN", "0") == "1",
        };
        Self::with_urls(
            &config::knowledge_url(),
            &config::intelligence_url(),
            &config::execution_url(),
            &config::identity_url(),
            settings,
        )
    }

    pub fn with_urls(
        knowledge: &str,
        intelligence: &str,
        execution: &str,
        identity: &str,
        settings: GatewaySettings,
    ) -> Result<Self> {
        let link = |name, url: &str| -> Result<ServiceLink> {
            Ok(ServiceLink {
                name,
                url: url.to_owned(),
                channel: sdi_common::grpc::lazy_channel(url)?,
            })
        };
        Ok(Self {
            services: Arc::new([
                link("knowledge", knowledge)?,
                link("intelligence", intelligence)?,
                link("execution", execution)?,
                link("identity", identity)?,
            ]),
            settings: Arc::new(settings),
            started_at: Utc::now(),
            version: env!("CARGO_PKG_VERSION"),
            auth_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Identity is the one service called without a principal: it is the
    /// service that produces them.
    pub fn identity(&self) -> IdentityServiceClient<Channel> {
        IdentityServiceClient::new(self.services[3].channel.clone())
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
        let key = (token.to_owned(), workspace.to_owned());
        match cache.get(&key) {
            Some(entry) if entry.at.elapsed() < AUTH_CACHE_TTL => Some(entry.auth.clone()),
            Some(_) => {
                cache.remove(&key);
                None
            }
            None => None,
        }
    }

    pub fn remember_auth(&self, token: &str, workspace: &str, auth: Auth) {
        let mut cache = self.auth_cache.lock().expect("auth cache lock");
        // Keep the map bounded: drop everything stale once it grows.
        if cache.len() >= 10_000 {
            cache.retain(|_, entry| entry.at.elapsed() < AUTH_CACHE_TTL);
        }
        cache.insert(
            (token.to_owned(), workspace.to_owned()),
            CacheEntry {
                at: Instant::now(),
                auth,
            },
        );
    }

    /// Every cached answer for a session (logout) or a workspace (deletion).
    pub fn forget_auth(&self, token: Option<&str>, workspace: Option<&str>) {
        let mut cache = self.auth_cache.lock().expect("auth cache lock");
        cache.retain(|(t, w), entry| {
            token.is_none_or(|token| t != token)
                && workspace.is_none_or(|ws| entry.auth.principal.workspace_id != ws && w != ws)
        });
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
pub type IntelligenceClient = IntelligenceServiceClient<InterceptedService<Channel, Forward>>;
pub type ExecutionClient = ExecutionServiceClient<InterceptedService<Channel, Forward>>;

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
    }

    pub fn intelligence(&self) -> IntelligenceClient {
        IntelligenceServiceClient::with_interceptor(
            self.state.services[1].channel.clone(),
            Forward(self.principal.clone()),
        )
    }

    pub fn execution(&self) -> ExecutionClient {
        ExecutionServiceClient::with_interceptor(
            self.state.services[2].channel.clone(),
            Forward(self.principal.clone()),
        )
    }

    pub fn identity(&self) -> IdentityServiceClient<Channel> {
        self.state.identity()
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
