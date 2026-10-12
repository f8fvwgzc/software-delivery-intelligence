//! One module per API area. Each exposes `router()` returning an
//! `OpenApiRouter` so the OpenAPI document is assembled from the same
//! registrations that serve traffic and can never drift from them.

use utoipa_axum::router::OpenApiRouter;

use crate::state::AppState;

pub mod approvals;
pub mod auth;
pub mod code;
pub mod decisions;
pub mod diagrams;
pub mod documents;
pub mod executions;
pub mod export;
pub mod github;
pub mod github_app;
pub mod knowledge;
pub mod policies;
pub mod projects;
pub mod proposals;
pub mod reviews;
pub mod settings;
pub mod system;
pub mod webhooks;
pub mod workspaces;

/// Every request/response route under `/api/v1` (the SSE routes live in
/// `crate::sse`, outside the request timeout).
pub fn rest() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(system::router())
        .merge(auth::router())
        .merge(workspaces::router())
        .merge(webhooks::router())
        .merge(settings::router())
        .merge(github::router())
        .merge(github_app::router())
        .merge(projects::router())
        .merge(code::router())
        .merge(knowledge::router())
        .merge(proposals::router())
        .merge(decisions::router())
        .merge(documents::router())
        .merge(diagrams::router())
        .merge(reviews::router())
        .merge(executions::router())
        .merge(approvals::router())
        .merge(policies::router())
}

/// `Vec<proto>` to `Vec<dto>`, the shape every list route returns.
pub(crate) fn map_all<T, U: From<T>>(items: Vec<T>) -> Vec<U> {
    items.into_iter().map(Into::into).collect()
}
