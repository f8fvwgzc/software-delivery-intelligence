//! Execution service: the flight controller of CONTRACT.md §7.

mod adapters;
mod config;
mod convert;
mod db;
mod domain;
mod events;
mod grpc;
mod identity;
mod knowledge;
mod runner;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use sdi_proto::execution::v1::execution_service_server::ExecutionServiceServer;
use tonic::transport::Server;

use crate::{
    adapters::{DockerAdapter, GithubAdapter, Registry, SimulatedAdapter},
    config::{SCHEMA, Settings},
    grpc::ExecutionGrpc,
    identity::{GrpcAudit, GrpcGithubAccess},
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
    // The github-token identity mirrors the environment, not the migration
    // history: it is (re)seeded or withdrawn on every start (§5.3).
    db::identities::seed_github_token(&pool, settings.github_token.is_some())
        .await
        .context("could not seed the github-token identity")?;

    let adapters = Registry::new(
        Arc::new(SimulatedAdapter::new(settings.sim_delay_ms)),
        Arc::new(
            DockerAdapter::new(
                settings.run_dir.clone(),
                settings.github_token.clone(),
                &settings.github_api_url,
            )
            .with_publish_host(&settings.docker_publish_host),
        ),
        Arc::new(GithubAdapter::new(
            settings.github_token.clone(),
            &settings.github_api_url,
        )),
    );
    let outcomes = Arc::new(GrpcPatternOutcomes::new(&settings.knowledge_url)?);
    let audit = Arc::new(GrpcAudit::new(&settings.identity_url)?);
    let github_access = Arc::new(GrpcGithubAccess::new(&settings.identity_url)?);
    let rt = Runtime::new(
        pool,
        adapters,
        outcomes,
        audit,
        Duration::from_millis(settings.sim_delay_ms / 2),
        settings.github_token.is_some(),
        github_access,
    );
    let resumed = rt.recover().await.context("could not resume executions")?;

    let (reporter, health) = tonic_health::server::health_reporter();
    reporter
        .set_serving::<ExecutionServiceServer<ExecutionGrpc>>()
        .await;

    tracing::info!(
        addr = %settings.listen,
        resumed,
        run_dir = %settings.run_dir.display(),
        github_token = settings.github_token.is_some(),
        "execution service listening"
    );
    Server::builder()
        .add_service(health)
        .add_service(
            ExecutionServiceServer::new(ExecutionGrpc::new(rt))
                .max_decoding_message_size(sdi_common::grpc::MAX_MESSAGE_BYTES)
                .max_encoding_message_size(sdi_common::grpc::MAX_MESSAGE_BYTES),
        )
        .serve_with_shutdown(settings.listen, sdi_common::shutdown::signal())
        .await
        .context("gRPC server failed")?;
    // Runners checkpoint every step; whatever is mid-step when the grace
    // period ends is resumed by `recover` on the next start.
    let abandoned = sdi_common::tasks::shutdown(sdi_common::config::shutdown_grace()).await;
    tracing::info!(abandoned, "execution service stopped");
    Ok(())
}
