use anyhow::{Context, Result};
use sdi_knowledge::{
    catalog, config::Settings, db, events::EventBus, github::GithubClient, grpc::KnowledgeState,
};
use sdi_proto::knowledge::v1::knowledge_service_server::KnowledgeServiceServer;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<()> {
    sdi_common::telemetry::init("knowledge");
    let settings = Settings::from_env()?;

    let pool = sdi_common::db::connect(&settings.database_url, "knowledge").await?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("knowledge migrations failed")?;

    // Scans that were mid-flight when the previous process died can never finish.
    let interrupted = db::scans::fail_interrupted(&pool).await?;
    if interrupted > 0 {
        tracing::warn!(interrupted, "marked interrupted scans as failed");
    }

    let seeded = catalog::seed(&pool).await?;
    tracing::info!(seeded, "catalog patterns seeded");

    let github = GithubClient::new(settings.github_token.clone())?;
    // The cache switch is the one setting that applies outside a scan.
    let stored = db::settings::get(&pool).await?;
    github.set_cache_enabled(stored.cache_github);
    let state = KnowledgeState::new(pool, github, EventBus::new());

    let (reporter, health) = tonic_health::server::health_reporter();
    reporter
        .set_serving::<KnowledgeServiceServer<KnowledgeState>>()
        .await;

    tracing::info!(listen = %settings.listen, "knowledge service listening");
    Server::builder()
        .add_service(health)
        .add_service(KnowledgeServiceServer::new(state))
        .serve_with_shutdown(settings.listen, sdi_common::shutdown::signal())
        .await
        .context("gRPC server failed")?;

    Ok(())
}
