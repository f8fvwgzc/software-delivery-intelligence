//! Intelligence service: ranked, evidence-backed proposals, negotiation,
//! decision memory and generated documents over gRPC (CONTRACT.md §6).

mod config;
mod convert;
mod db;
mod domain;
mod gateway;
mod grpc;
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
    let reasoner = reasoner::from_settings(&settings);

    let service = grpc::IntelligenceGrpc::new(pool, knowledge, execution, reasoner);
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
