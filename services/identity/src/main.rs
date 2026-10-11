//! Identity service: GitHub OAuth sessions, GitHub App installations,
//! workspaces, membership and roles, audit and data controls
//! (CONTRACT.md §18, §19).

mod config;
mod convert;
mod crypto;
mod db;
mod github;
mod grpc;
mod roles;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use sdi_proto::identity::v1::identity_service_server::IdentityServiceServer;
use tonic::transport::Server;

use crate::{
    config::{SCHEMA, Settings},
    grpc::{IdentityGrpc, IdentityState},
};

/// How often expired audit entries are pruned (§19: `audit_retention_days`).
const PRUNE_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    sdi_common::telemetry::init("identity");
    let settings = Settings::from_env()?;

    let pool = sdi_common::db::connect(&settings.database_url, SCHEMA).await?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("identity migrations failed")?;

    let state = Arc::new(IdentityState::new(pool.clone(), settings.clone()));

    tokio::spawn(async move {
        loop {
            match db::audit::prune(&pool).await {
                Ok(0) => {}
                Ok(pruned) => tracing::info!(pruned, "expired audit entries pruned"),
                Err(error) => tracing::warn!(%error, "audit prune failed"),
            }
            tokio::time::sleep(PRUNE_INTERVAL).await;
        }
    });

    let (reporter, health) = tonic_health::server::health_reporter();
    reporter
        .set_serving::<IdentityServiceServer<IdentityGrpc>>()
        .await;

    tracing::info!(
        addr = %settings.listen,
        public_url = %settings.public_url,
        oauth = settings.oauth.is_some(),
        github_app = settings.app.is_some(),
        dev_login = settings.dev_login,
        "identity service listening"
    );
    if settings.dev_login && settings.github_token.is_none() {
        tracing::warn!("SDI_DEV_LOGIN=1 but GITHUB_TOKEN is unset: dev login will fail");
    }
    Server::builder()
        .add_service(health)
        .add_service(IdentityServiceServer::new(IdentityGrpc::new(state)))
        .serve_with_shutdown(settings.listen, sdi_common::shutdown::signal())
        .await
        .context("gRPC server failed")?;
    Ok(())
}
