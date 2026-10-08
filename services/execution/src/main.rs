//! Execution service: the flight controller of CONTRACT.md §7.

mod adapters;
mod config;
mod convert;
mod db;
mod domain;
mod grpc;
mod knowledge;
mod runner;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use sdi_proto::execution::v1::execution_service_server::ExecutionServiceServer;
use tonic::transport::Server;

use crate::{
    adapters::{Registry, SimulatedAdapter},
    config::{SCHEMA, Settings},
    grpc::ExecutionGrpc,
    knowledge::GrpcPatternOutcomes,
    runner::Runtime,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    sdi_common::telemetry::init("execution");
    let settings = Settings::from_env()?;

    let pool = sdi_common::db::connect(&settings.database_url, SCHEMA).await?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("execution migrations failed")?;

    let adapters = Registry::new(Arc::new(SimulatedAdapter::new(settings.sim_delay_ms)));
    let outcomes = Arc::new(GrpcPatternOutcomes::new(&settings.knowledge_url)?);
    let rt = Runtime::new(
        pool,
        adapters,
        outcomes,
        Duration::from_millis(settings.sim_delay_ms / 2),
    );
    let resumed = rt.recover().await.context("could not resume executions")?;

    let (reporter, health) = tonic_health::server::health_reporter();
    reporter
        .set_serving::<ExecutionServiceServer<ExecutionGrpc>>()
        .await;

    tracing::info!(addr = %settings.listen, resumed, "execution service listening");
    Server::builder()
        .add_service(health)
        .add_service(ExecutionServiceServer::new(ExecutionGrpc::new(rt)))
        .serve_with_shutdown(settings.listen, sdi_common::shutdown::signal())
        .await
        .context("gRPC server failed")?;
    Ok(())
}
