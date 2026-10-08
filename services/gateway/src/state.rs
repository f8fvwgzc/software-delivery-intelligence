//! Shared handler state: one lazy channel per backing service. Lazy means
//! the gateway boots (and serves `/health`, docs and the OpenAPI document)
//! before any service is up, and reconnects on its own once they are.

use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use sdi_proto::{
    execution::v1::execution_service_client::ExecutionServiceClient,
    intelligence::v1::intelligence_service_client::IntelligenceServiceClient,
    knowledge::v1::knowledge_service_client::KnowledgeServiceClient,
};
use tonic::transport::Channel;

/// One backing service, as the system status page reports it.
#[derive(Debug, Clone)]
pub struct ServiceLink {
    pub name: &'static str,
    pub url: String,
    pub channel: Channel,
}

#[derive(Clone)]
pub struct AppState {
    pub services: Arc<[ServiceLink; 3]>,
    pub started_at: DateTime<Utc>,
    pub version: &'static str,
}

impl AppState {
    /// Reads the three `*_GRPC_URL` variables (CONTRACT.md §3).
    pub fn from_env() -> Result<Self> {
        Self::with_urls(
            &sdi_common::config::knowledge_url(),
            &sdi_common::config::intelligence_url(),
            &sdi_common::config::execution_url(),
        )
    }

    pub fn with_urls(knowledge: &str, intelligence: &str, execution: &str) -> Result<Self> {
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
            ]),
            started_at: Utc::now(),
            version: env!("CARGO_PKG_VERSION"),
        })
    }

    /// Clients are cheap handles over the shared channel, so every handler
    /// gets its own `&mut` client without locking.
    pub fn knowledge(&self) -> KnowledgeServiceClient<Channel> {
        KnowledgeServiceClient::new(self.services[0].channel.clone())
    }

    pub fn intelligence(&self) -> IntelligenceServiceClient<Channel> {
        IntelligenceServiceClient::new(self.services[1].channel.clone())
    }

    pub fn execution(&self) -> ExecutionServiceClient<Channel> {
        ExecutionServiceClient::new(self.services[2].channel.clone())
    }
}
