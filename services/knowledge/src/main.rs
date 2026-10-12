use anyhow::{Context, Result};
use sdi_common::grpc::MAX_MESSAGE_BYTES;
use sdi_knowledge::{
    auth, catalog,
    config::Settings,
    db,
    events::EventBus,
    github::{Github, TokenProvider},
    grpc::KnowledgeState,
    identity::Identity,
    sweeper,
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

    // GitHub access comes from identity per workspace; GITHUB_TOKEN is the
    // dev-mode fallback when identity cannot be reached.
    let identity = Identity::connect(&settings.identity_url)?;
    let github = Github::new(TokenProvider::new(
        identity.clone(),
        settings.github_token.clone(),
    ))?;
    if github.dev_token_configured() {
        tracing::info!("GITHUB_TOKEN set: used for every workspace while identity is unreachable");
    }
    let state = KnowledgeState::new(pool.clone(), github, EventBus::new(), identity);

    // Session code caches expire; reclaim them in the background.
    let _sweeper = sweeper::spawn(pool);

    let (reporter, health) = tonic_health::server::health_reporter();
    reporter
        .set_serving::<KnowledgeServiceServer<KnowledgeState>>()
        .await;

    tracing::info!(listen = %settings.listen, identity = %settings.identity_url, "knowledge service listening");
    Server::builder()
        .add_service(health)
        .add_service(tonic::service::interceptor::InterceptedService::new(
            KnowledgeServiceServer::new(state)
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES),
            auth::interceptor,
        ))
        .serve_with_shutdown(settings.listen, sdi_common::shutdown::signal())
        .await
        .context("gRPC server failed")?;

    // Scans, audit entries and exports in flight get a moment to finish;
    // a scan cut short is marked failed by the next start (`fail_interrupted`).
    let abandoned = sdi_common::tasks::shutdown(sdi_common::config::shutdown_grace()).await;
    tracing::info!(abandoned, "knowledge service stopped");
    Ok(())
}
