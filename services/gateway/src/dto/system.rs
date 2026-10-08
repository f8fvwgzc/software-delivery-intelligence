//! Gateway liveness and the health of the services behind it.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

/// Response of `GET /health`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Health {
    /// Always `ok` when the gateway can answer at all.
    pub status: String,
}

/// Result of one gRPC health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    /// The service answered `SERVING`.
    Serving,
    /// The service answered but is not ready.
    NotServing,
    /// No answer within the timeout, or the connection failed.
    Unreachable,
}

/// Health of one backing service.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceHealth {
    /// `knowledge`, `intelligence` or `execution`.
    pub name: String,
    /// gRPC endpoint the gateway talks to.
    pub url: String,
    /// Health status.
    pub status: ServiceStatus,
    /// Round trip of the health check in milliseconds.
    pub latency_ms: u64,
    /// Error text when the check failed.
    pub detail: Option<String>,
}

/// Response of `GET /system/status`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    /// `ok` when every service is serving, `degraded` otherwise.
    pub status: String,
    /// Gateway version.
    pub version: String,
    /// When the gateway process started.
    pub started_at: DateTime<Utc>,
    /// Seconds since the gateway started.
    pub uptime_seconds: u64,
    /// One entry per backing service.
    pub services: Vec<ServiceHealth>,
}
