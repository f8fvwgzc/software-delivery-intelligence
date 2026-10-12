//! gRPC client channels and error mapping for service-to-service calls.

use std::time::Duration;

use anyhow::{Context, Result};
use tonic::{
    Status,
    transport::{Channel, Endpoint},
};

/// Largest gRPC message a service accepts or sends (16 MiB). Trees of big
/// monorepos and export records are the largest payloads; anything beyond
/// this is a bug or an attack, and tonic's 4 MiB default was too small for
/// the former. Every generated server and client applies it through its
/// `max_decoding_message_size` / `max_encoding_message_size` setters.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// A lazily connected channel: the service starts even when its peers are
/// not up yet, and reconnects on its own once they are.
pub fn lazy_channel(url: &str) -> Result<Channel> {
    let endpoint = Endpoint::from_shared(url.to_owned())
        .with_context(|| format!("invalid gRPC endpoint: {url}"))?
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(60))
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .http2_keep_alive_interval(Duration::from_secs(30))
        .keep_alive_while_idle(true);
    Ok(endpoint.connect_lazy())
}

/// Maps a database error to a gRPC status without leaking SQL details.
pub fn db_status(error: sqlx::Error) -> Status {
    match error {
        sqlx::Error::RowNotFound => Status::not_found("not found"),
        other => {
            tracing::error!(error = %other, "database error");
            Status::internal("database error")
        }
    }
}

/// Maps any internal failure to `INTERNAL`, logging the cause.
pub fn internal(error: impl std::fmt::Display) -> Status {
    tracing::error!(%error, "internal error");
    Status::internal("internal error")
}
