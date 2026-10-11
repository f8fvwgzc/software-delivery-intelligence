//! GitHub App webhooks (CONTRACT.md §18): installation lifecycle, membership
//! changes and pushes to a project's default branch. No session; the body
//! is authenticated with `X-Hub-Signature-256`.

use axum::{Json, body::Bytes, extract::State, http::HeaderMap};
use hmac::{Hmac, Mac};
use sdi_proto::{identity::v1 as identity, knowledge::v1 as knowledge};
use serde_json::Value;
use sha2::Sha256;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::identity::WebhookAck,
    error::{ApiError, ApiResult, ErrorResponses},
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(github))
}

/// Constant-time check of `sha256=<hex>` over the raw body.
pub fn verify_signature(secret: &str, signature: Option<&str>, body: &[u8]) -> bool {
    let Some(hex_digest) = signature.and_then(|s| s.strip_prefix("sha256=")) else {
        return false;
    };
    let Ok(expected) = hex::decode(hex_digest) else {
        return false;
    };
    let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

fn str_of<'a>(value: &'a Value, path: &[&str]) -> &'a str {
    let mut current = value;
    for key in path {
        current = &current[*key];
    }
    current.as_str().unwrap_or_default()
}

fn i64_of(value: &Value, path: &[&str]) -> i64 {
    let mut current = value;
    for key in path {
        current = &current[*key];
    }
    current.as_i64().unwrap_or(0)
}

/// The installation record a payload describes; `installation.account` is
/// present on `installation*` events, `organization` on the others.
fn installation_of(payload: &Value) -> identity::RecordInstallationRequest {
    let account = if payload["installation"]["account"].is_object() {
        &payload["installation"]["account"]
    } else {
        &payload["organization"]
    };
    identity::RecordInstallationRequest {
        installation_id: i64_of(payload, &["installation", "id"]),
        account_login: str_of(account, &["login"]).to_owned(),
        account_type: {
            let t = str_of(account, &["type"]);
            if t.is_empty() { "Organization" } else { t }.to_owned()
        },
        account_avatar_url: str_of(account, &["avatar_url"]).to_owned(),
        github_host: String::new(),
        repository_selection: str_of(payload, &["installation", "repository_selection"]).to_owned(),
        installed_by_user_id: String::new(),
    }
}

/// Receives GitHub App events. Verified with `GITHUB_WEBHOOK_SECRET`; the
/// gateway answers 412 when none is configured.
#[utoipa::path(
    post,
    path = "/webhooks/github",
    tag = "webhooks",
    request_body(content = serde_json::Value, description = "GitHub webhook payload"),
    responses((status = 200, body = WebhookAck), ErrorResponses),
)]
async fn github(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<WebhookAck>> {
    let Some(secret) = state.settings.webhook_secret.as_deref() else {
        return Err(ApiError::precondition(
            "GITHUB_WEBHOOK_SECRET is not configured on the gateway",
        ));
    };
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok());
    if !verify_signature(secret, signature, &body) {
        return Err(ApiError::forbidden("webhook signature does not verify"));
    }
    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let payload: Value = serde_json::from_slice(&body).map_err(|e| {
        ApiError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "invalid_body",
            e.to_string(),
        )
    })?;
    let action = str_of(&payload, &["action"]).to_owned();
    let installation_id = i64_of(&payload, &["installation", "id"]);
    tracing::info!(event, action, installation_id, "github webhook");

    let outcome = match (event.as_str(), action.as_str()) {
        ("installation", "created" | "unsuspend" | "new_permissions_accepted") => {
            state
                .identity()
                .record_installation(installation_of(&payload))
                .await?;
            "recorded"
        }
        ("installation", "deleted" | "suspend") => {
            state
                .identity()
                .remove_installation(identity::RemoveInstallationRequest { installation_id })
                .await?;
            "removed"
        }
        ("installation_repositories", _) => {
            state
                .identity()
                .record_installation(installation_of(&payload))
                .await?;
            "recorded"
        }
        ("member" | "team" | "membership" | "organization", _) if installation_id > 0 => {
            let workspace = state
                .identity()
                .record_installation(installation_of(&payload))
                .await?
                .into_inner();
            state
                .identity()
                .sync_members(identity::SyncMembersRequest {
                    workspace_id: workspace.id.clone(),
                })
                .await?;
            state.forget_auth(None, Some(&workspace.id));
            "synced"
        }
        ("push", _) if installation_id > 0 => {
            let git_ref = str_of(&payload, &["ref"]);
            let default_branch = str_of(&payload, &["repository", "default_branch"]);
            let full_name = str_of(&payload, &["repository", "full_name"]);
            if !default_branch.is_empty() && git_ref == format!("refs/heads/{default_branch}") {
                match rescan(&state, &payload, full_name).await {
                    Ok(true) => "scan_started",
                    Ok(false) => "ignored",
                    Err(error) => {
                        tracing::warn!(%error, full_name, "push rescan not started");
                        "ignored"
                    }
                }
            } else {
                "ignored"
            }
        }
        _ => "ignored",
    };
    Ok(Json(WebhookAck {
        event,
        outcome: outcome.to_owned(),
    }))
}

/// A push to the default branch of a connected project queues a scan as
/// `system`. Errors are the caller's to ignore.
async fn rescan(state: &AppState, payload: &Value, full_name: &str) -> Result<bool, tonic::Status> {
    let workspace = state
        .identity()
        .record_installation(installation_of(payload))
        .await?
        .into_inner();
    let scoped = state.system(&workspace.id);
    let projects = scoped
        .knowledge()
        .list_projects(knowledge::ListProjectsRequest {})
        .await?
        .into_inner()
        .projects;
    let Some(project) = projects
        .into_iter()
        .find(|p| p.repo_full_name.eq_ignore_ascii_case(full_name))
    else {
        return Ok(false);
    };
    scoped
        .knowledge()
        .start_scan(knowledge::StartScanRequest {
            project_id: project.id,
        })
        .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_verifies_only_with_the_right_secret() {
        let body = br#"{"action":"created"}"#;
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(b"s3cret").unwrap();
        mac.update(body);
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(verify_signature("s3cret", Some(&signature), body));
        assert!(!verify_signature("other", Some(&signature), body));
        assert!(!verify_signature("s3cret", Some(&signature), b"{}"));
        assert!(!verify_signature("s3cret", Some("sha1=abc"), body));
        assert!(!verify_signature("s3cret", Some("sha256=zz"), body));
        assert!(!verify_signature("s3cret", None, body));
    }

    #[test]
    fn installation_is_read_from_either_payload_shape() {
        let created = serde_json::json!({
            "installation": {"id": 7, "account": {"login": "acme", "type": "Organization", "avatar_url": "a"}, "repository_selection": "selected"}
        });
        let req = installation_of(&created);
        assert_eq!(req.installation_id, 7);
        assert_eq!(req.account_login, "acme");
        assert_eq!(req.repository_selection, "selected");

        let member = serde_json::json!({
            "installation": {"id": 7}, "organization": {"login": "acme", "avatar_url": "a"}
        });
        let req = installation_of(&member);
        assert_eq!(req.account_login, "acme");
        assert_eq!(req.account_type, "Organization");
        assert_eq!(req.repository_selection, "");
    }
}
