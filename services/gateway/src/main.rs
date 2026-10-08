//! Public REST gateway: HTTP + JSON in, gRPC out. The only HTTP surface of
//! the platform (CONTRACT.md §2 and §8).

mod app;
mod dto;
mod error;
mod routes;
mod sse;
mod state;

use anyhow::Context;

const DEFAULT_LISTEN: &str = "0.0.0.0:8080";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // The contract file is generated from the binary itself so it can never
    // drift from the routes; this path must work with nothing else running.
    if std::env::args().any(|arg| arg == "--print-openapi") {
        let json = app::openapi()
            .to_pretty_json()
            .context("could not serialise the OpenAPI document")?;
        println!("{json}");
        return Ok(());
    }

    sdi_common::telemetry::init("gateway");

    let state = state::AppState::from_env()?;
    for link in state.services.iter() {
        tracing::info!(service = link.name, url = %link.url, "upstream configured");
    }

    let addr = sdi_common::config::listen_addr("GATEWAY_LISTEN", DEFAULT_LISTEN)?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("could not bind {addr}"))?;
    tracing::info!(%addr, version = state.version, "gateway listening");

    axum::serve(listener, app::router(state))
        .with_graceful_shutdown(sdi_common::shutdown::signal())
        .await
        .context("server error")?;

    tracing::info!("gateway stopped");
    Ok(())
}
