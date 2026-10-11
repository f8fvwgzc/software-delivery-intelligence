//! Liveness of the gateway and health of the services behind it.

use std::time::{Duration, Instant};

use axum::{Json, extract::State};
use chrono::Utc;
use tonic_health::pb::{
    HealthCheckRequest, health_check_response::ServingStatus, health_client::HealthClient,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::system::{Health, ServiceHealth, ServiceStatus, SystemStatus},
    state::{AppState, ServiceLink},
};

/// A service that cannot answer a health check in this time is reported as
/// unreachable instead of holding up the whole status page.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(health))
        .routes(routes!(system_status))
}

/// Gateway liveness. Answers without touching any service.
#[utoipa::path(get, path = "/health", tag = "system", responses((status = 200, body = Health)))]
async fn health() -> Json<Health> {
    Json(Health {
        status: "ok".into(),
    })
}

/// gRPC health of every backing service (identity included), checked concurrently.
#[utoipa::path(get, path = "/system/status", tag = "system", responses((status = 200, body = SystemStatus)))]
async fn system_status(State(state): State<AppState>) -> Json<SystemStatus> {
    let services = futures::future::join_all(state.services.iter().map(check)).await;
    let all_serving = services
        .iter()
        .all(|service| service.status == ServiceStatus::Serving);
    let now = Utc::now();
    Json(SystemStatus {
        status: if all_serving { "ok" } else { "degraded" }.into(),
        version: state.version.into(),
        started_at: state.started_at,
        uptime_seconds: (now - state.started_at).num_seconds().max(0) as u64,
        services,
    })
}

async fn check(link: &ServiceLink) -> ServiceHealth {
    let started = Instant::now();
    let mut client = HealthClient::new(link.channel.clone());
    let request = HealthCheckRequest {
        service: String::new(),
    };
    let (status, detail) = match tokio::time::timeout(HEALTH_TIMEOUT, client.check(request)).await {
        Ok(Ok(response)) => match response.into_inner().status() {
            ServingStatus::Serving => (ServiceStatus::Serving, None),
            other => (ServiceStatus::NotServing, Some(format!("{other:?}"))),
        },
        Ok(Err(status)) => (
            ServiceStatus::Unreachable,
            Some(status.message().to_owned()),
        ),
        Err(_) => (
            ServiceStatus::Unreachable,
            Some(format!("no answer within {}s", HEALTH_TIMEOUT.as_secs())),
        ),
    };
    ServiceHealth {
        name: link.name.into(),
        url: link.url.clone(),
        status,
        latency_ms: started.elapsed().as_millis() as u64,
        detail,
    }
}
