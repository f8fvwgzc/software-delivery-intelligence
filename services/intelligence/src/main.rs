//! Intelligence service: ranked, evidence-backed proposals, negotiation,
//! decision memory and generated documents over gRPC (CONTRACT.md §6).

mod config;
mod convert;
mod db;
mod domain;
mod events;
mod gateway;
mod generators;
mod github;
mod grpc;
mod osv;
mod reasoner;

use std::sync::Arc;

use anyhow::{Context, Result};
use sdi_proto::intelligence::v1::intelligence_service_server::IntelligenceServiceServer;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<()> {
    sdi_common::telemetry::init("intelligence");
    let settings = config::Settings::from_env()?;

    let pool = sdi_common::db::connect(&settings.database_url, "intelligence").await?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("migrations failed")?;

    let knowledge = Arc::new(gateway::GrpcKnowledge::new(sdi_common::grpc::lazy_channel(
        &settings.knowledge_url,
    )?));
    let execution = Arc::new(gateway::GrpcExecution::new(sdi_common::grpc::lazy_channel(
        &settings.execution_url,
    )?));
    let identity = Arc::new(gateway::GrpcIdentity::new(sdi_common::grpc::lazy_channel(
        &settings.identity_url,
    )?));
    if settings.reasoner.api_key.is_some() || !settings.reasoner.base_url.is_empty() {
        tracing::info!(
            provider = %crate::reasoner::Provider::parse(&settings.reasoner.provider, &settings.reasoner.base_url).label(),
            model = %settings.reasoner.model,
            "LLM_* / ANTHROPIC_* set: dev fallback for workspaces that allow the reasoner but configured none"
        );
    }
    let reasoner = grpc::ReasonerConfig {
        env: settings.reasoner.clone(),
    };

    let github_token = sdi_common::config::env_opt("GITHUB_TOKEN");
    if github_token.is_some() {
        tracing::info!(
            "GITHUB_TOKEN set: reviews are published with it while identity is unreachable (dev mode)"
        );
    }
    let osv_url = sdi_common::config::env_or("OSV_API_URL", osv::DEFAULT_BASE_URL);
    let osv = osv::OsvClient::new(&osv_url, pool.clone()).map(Arc::new);
    if osv.is_none() {
        tracing::info!("OSV_API_URL=off: the dependency vulnerability check is disabled");
    }
    let service = grpc::IntelligenceGrpc::new(pool, knowledge, execution, identity, reasoner)
        .with_github_dev_token(github_token)
        .with_osv(osv);
    let (reporter, health) = tonic_health::server::health_reporter();
    reporter
        .set_serving::<IntelligenceServiceServer<grpc::IntelligenceGrpc>>()
        .await;

    tracing::info!(listen = %settings.listen, "intelligence service listening");
    Server::builder()
        .add_service(health)
        .add_service(IntelligenceServiceServer::new(service))
        .serve_with_shutdown(settings.listen, sdi_common::shutdown::signal())
        .await
        .context("gRPC server failed")?;
    Ok(())
}
