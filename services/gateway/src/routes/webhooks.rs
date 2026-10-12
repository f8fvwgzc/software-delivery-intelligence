//! GitHub App webhooks (CONTRACT.md §18, §22): installation lifecycle,
//! membership changes, pushes to a project's default branch, pull requests,
//! and the two events that mean "a new image exists": a build workflow
//! succeeding on the default branch and a container package being
//! published. No session; the body is authenticated with
//! `X-Hub-Signature-256`.
//!
//! The handler only verifies, parses and classifies, then answers `202`:
//! GitHub times a delivery out after ten seconds and a sync, a rescan or a
//! review start can take several gRPC round trips. The work itself runs on
//! one bounded queue per replica ([`worker`]); when the queue is full the
//! handler answers `503` with `Retry-After` so GitHub redelivers instead of
//! the gateway growing a backlog.

use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use sdi_proto::{
    execution::v1 as execution, identity::v1 as identity, intelligence::v1 as intelligence,
    knowledge::v1 as knowledge,
};
use serde_json::Value;
use sha2::Sha256;
use tokio::sync::mpsc;
use tonic::Status;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::identity::WebhookAck,
    error::{ApiError, ApiResult, ErrorResponses},
    state::{AppState, WebhookDelivery, WebhookJob},
};

/// Largest delivery the handler parses; nginx caps the body at 1 MB too.
const MAX_BODY_BYTES: usize = 1024 * 1024;

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
/// present on `installation*` events, `organization` on the others. GitHub
/// Enterprise Server deliveries carry the same shape; the host is not in
/// the payload (`enterprise.*` only names the instance), so identity keeps
/// the host it recorded when the installation was first seen.
pub fn installation_of(payload: &Value) -> identity::RecordInstallationRequest {
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

/// What a delivery asks for, decided from the headers and payload alone.
/// `None` is "ignored": nothing to do, answered at once.
pub fn classify(event: &str, payload: &Value) -> Option<WebhookJob> {
    let action = str_of(payload, &["action"]);
    let installation_id = i64_of(payload, &["installation", "id"]);
    match (event, action) {
        ("installation", "created" | "unsuspend" | "new_permissions_accepted") => {
            Some(WebhookJob::RecordInstallation)
        }
        ("installation", "deleted" | "suspend") => {
            Some(WebhookJob::RemoveInstallation { installation_id })
        }
        ("installation_repositories", _) => Some(WebhookJob::RecordInstallation),
        ("member" | "team" | "membership" | "organization", _) if installation_id > 0 => {
            Some(WebhookJob::SyncMembers)
        }
        ("pull_request", "opened" | "synchronize" | "reopened" | "ready_for_review")
            if installation_id > 0 =>
        {
            let full_name = str_of(payload, &["repository", "full_name"]);
            let number = payload["pull_request"]["number"].as_u64().unwrap_or(0);
            let draft = payload["pull_request"]["draft"].as_bool().unwrap_or(false);
            (number > 0 && !draft && !full_name.is_empty()).then(|| WebhookJob::AutoReview {
                full_name: full_name.to_owned(),
                number,
            })
        }
        // A workflow finished successfully on the default branch. Whether
        // it is the project's *build* workflow is decided by the worker,
        // which can ask knowledge; pull-request runs never cut a release.
        ("workflow_run", "completed") if installation_id > 0 => {
            let run = &payload["workflow_run"];
            let full_name = str_of(payload, &["repository", "full_name"]);
            let default_branch = str_of(payload, &["repository", "default_branch"]);
            let head_sha = str_of(run, &["head_sha"]);
            let on_default = !default_branch.is_empty()
                && str_of(run, &["head_branch"]) == default_branch
                && !str_of(run, &["event"]).starts_with("pull_request");
            (str_of(run, &["conclusion"]) == "success"
                && on_default
                && !full_name.is_empty()
                && !head_sha.is_empty())
            .then(|| WebhookJob::BuildSucceeded {
                full_name: full_name.to_owned(),
                workflow_path: str_of(run, &["path"])
                    .split('@')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                workflow_name: str_of(run, &["name"]).to_owned(),
                head_sha: head_sha.to_owned(),
                run_id: run["id"]
                    .as_u64()
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
            })
        }
        // A container image was pushed to GHCR. Both events carry the same
        // package shape under their own key.
        ("registry_package" | "package", "published") if installation_id > 0 => {
            let package = if event == "package" {
                &payload["package"]
            } else {
                &payload["registry_package"]
            };
            published_image(payload, package)
        }
        ("push", _) if installation_id > 0 => {
            let git_ref = str_of(payload, &["ref"]);
            let default_branch = str_of(payload, &["repository", "default_branch"]);
            let full_name = str_of(payload, &["repository", "full_name"]);
            (!default_branch.is_empty()
                && !full_name.is_empty()
                && git_ref == format!("refs/heads/{default_branch}"))
            .then(|| WebhookJob::Rescan {
                full_name: full_name.to_owned(),
            })
        }
        _ => None,
    }
}

/// What a `registry_package` / `package` event says was published, when it
/// is a tagged container image of a repository: untagged manifests (the
/// per-platform images and attestations of a multi-arch push) and other
/// package types are nothing to release.
fn published_image(payload: &Value, package: &Value) -> Option<WebhookJob> {
    if !str_of(package, &["package_type"]).eq_ignore_ascii_case("container") {
        return None;
    }
    let version = &package["package_version"];
    let tag = str_of(version, &["container_metadata", "tag", "name"]);
    // The repository the package is linked to; an unlinked package belongs
    // to no project.
    let full_name = match str_of(payload, &["repository", "full_name"]) {
        "" => str_of(package, &["repository", "full_name"]),
        name => name,
    };
    if tag.is_empty() || full_name.is_empty() {
        return None;
    }
    let digest = match str_of(version, &["container_metadata", "tag", "digest"]) {
        "" => str_of(version, &["version"]),
        digest => digest,
    };
    // `ghcr.io/owner/name:tag` → the image without its tag; else the
    // registry's own naming.
    let url = str_of(version, &["package_url"]).trim();
    let image = match url.rsplit_once(':') {
        Some((image, suffix)) if !suffix.contains('/') && !image.is_empty() => image.to_owned(),
        _ if !url.is_empty() => url.to_owned(),
        _ => {
            let namespace = match str_of(package, &["namespace"]) {
                "" => str_of(package, &["owner", "login"]),
                namespace => namespace,
            };
            format!("ghcr.io/{namespace}/{}", str_of(package, &["name"]))
        }
    }
    .to_ascii_lowercase();
    let commit = str_of(version, &["target_oid"]);
    Some(WebhookJob::PackagePublished {
        full_name: full_name.to_owned(),
        image,
        tag: tag.to_owned(),
        digest: if digest.starts_with("sha256:") {
            digest.to_owned()
        } else {
            String::new()
        },
        commit_sha: if commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            commit.to_owned()
        } else {
            String::new()
        },
    })
}

/// Receives GitHub App events. Verified with `GITHUB_WEBHOOK_SECRET`; the
/// gateway answers 412 when none is configured. A delivery that needs work
/// is queued and answered `202 {outcome: "queued"}`; one that needs none is
/// answered `202 {outcome: "ignored"}`. `503` means the queue is full and
/// GitHub should redeliver.
#[utoipa::path(
    post,
    path = "/webhooks/github",
    tag = "webhooks",
    request_body(content = serde_json::Value, description = "GitHub webhook payload"),
    responses((status = 202, body = WebhookAck), ErrorResponses),
)]
async fn github(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
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
    if body.len() > MAX_BODY_BYTES {
        return Err(ApiError::bad_request("webhook payload too large"));
    }
    let header_text = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let event = header_text("x-github-event");
    let delivery_id = header_text("x-github-delivery");
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "invalid_body", e.to_string()))?;
    let action = str_of(&payload, &["action"]).to_owned();
    let installation_id = i64_of(&payload, &["installation", "id"]);
    tracing::info!(event, action, installation_id, delivery = %delivery_id, "github webhook");

    let outcome = match classify(&event, &payload) {
        None => "ignored",
        Some(job) => {
            let delivery = WebhookDelivery {
                delivery_id,
                event: event.clone(),
                job,
                installation: installation_of(&payload),
            };
            if !state.enqueue_webhook(delivery) {
                tracing::warn!(event, "webhook queue full; asking GitHub to redeliver");
                let mut response = ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "queue_full",
                    "webhook queue is full; redeliver later",
                )
                .into_response();
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, HeaderValue::from_static("30"));
                return Ok(response);
            }
            "queued"
        }
    };
    Ok((
        StatusCode::ACCEPTED,
        Json(WebhookAck {
            event,
            outcome: outcome.to_owned(),
        }),
    )
        .into_response())
}

/// Drains the queue, one delivery at a time, until the queue closes or the
/// process leaves. Every outcome is logged and kept in the bounded webhook
/// log; a failure never stops the worker.
pub async fn worker(state: AppState, mut inbox: mpsc::Receiver<WebhookDelivery>) {
    let leaving = sdi_common::shutdown::cancelled();
    tokio::pin!(leaving);
    loop {
        let delivery = tokio::select! {
            () = &mut leaving => return,
            next = inbox.recv() => match next {
                Some(delivery) => delivery,
                None => return,
            },
        };
        let delivery_id = delivery.delivery_id.clone();
        let event = delivery.event.clone();
        let outcome = match process(&state, delivery).await {
            Ok(outcome) => outcome,
            Err(error) => {
                tracing::warn!(%error, event, delivery = %delivery_id, "webhook delivery failed");
                "failed"
            }
        };
        tracing::info!(event, delivery = %delivery_id, outcome, "webhook processed");
        state.note_webhook(&delivery_id, outcome);
    }
}

async fn process(state: &AppState, delivery: WebhookDelivery) -> Result<&'static str, Status> {
    match delivery.job {
        WebhookJob::RecordInstallation => {
            state
                .identity()
                .record_installation(delivery.installation)
                .await?;
            Ok("recorded")
        }
        WebhookJob::RemoveInstallation { installation_id } => {
            state
                .identity()
                .remove_installation(identity::RemoveInstallationRequest { installation_id })
                .await?;
            Ok("removed")
        }
        WebhookJob::SyncMembers => {
            let workspace = state
                .identity()
                .record_installation(delivery.installation)
                .await?
                .into_inner();
            state
                .identity()
                .sync_members(identity::SyncMembersRequest {
                    workspace_id: workspace.id.clone(),
                })
                .await?;
            state.forget_auth(None, Some(&workspace.id));
            Ok("synced")
        }
        WebhookJob::Rescan { full_name } => {
            Ok(if rescan(state, delivery.installation, &full_name).await? {
                "scan_started"
            } else {
                "ignored"
            })
        }
        WebhookJob::AutoReview { full_name, number } => Ok(
            if auto_review(state, delivery.installation, &full_name, number).await? {
                "review_started"
            } else {
                "ignored"
            },
        ),
        WebhookJob::BuildSucceeded {
            full_name,
            workflow_path,
            workflow_name,
            head_sha,
            run_id,
        } => {
            let Some((scoped, project)) =
                connected_project(state, delivery.installation, &full_name).await?
            else {
                return Ok("ignored");
            };
            if !is_build_workflow(&scoped, &project.id, &workflow_path, &workflow_name).await? {
                return Ok("ignored");
            }
            scoped
                .execution()
                .create_release(execution::CreateReleaseRequest {
                    project_id: project.id,
                    commit_sha: head_sha,
                    source: "pipeline".to_owned(),
                    pipeline_run_id: run_id,
                    ..Default::default()
                })
                .await?;
            Ok("release_created")
        }
        WebhookJob::PackagePublished {
            full_name,
            image,
            tag,
            digest,
            commit_sha,
        } => {
            let Some((scoped, project)) =
                connected_project(state, delivery.installation, &full_name).await?
            else {
                return Ok("ignored");
            };
            scoped
                .execution()
                .create_release(execution::CreateReleaseRequest {
                    project_id: project.id,
                    commit_sha,
                    image,
                    tag,
                    digest,
                    source: "package".to_owned(),
                    ..Default::default()
                })
                .await?;
            Ok("release_created")
        }
    }
}

/// The connected project of a repository, with clients that act as
/// `system` in its workspace; `None` when the repository is not connected.
async fn connected_project(
    state: &AppState,
    installation: identity::RecordInstallationRequest,
    full_name: &str,
) -> Result<Option<(crate::state::Scoped, knowledge::Project)>, Status> {
    let workspace = state
        .identity()
        .record_installation(installation)
        .await?
        .into_inner();
    let scoped = state.system(&workspace.id);
    let projects = scoped
        .knowledge()
        .list_projects(knowledge::ListProjectsRequest {})
        .await?
        .into_inner()
        .projects;
    Ok(projects
        .into_iter()
        .find(|p| p.repo_full_name.eq_ignore_ascii_case(full_name))
        .map(|project| (scoped, project)))
}

/// The file name of a workflow path: `.github/workflows/ci.yml` → `ci.yml`.
fn workflow_file(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether `configured` (`delivery.build_workflow` of `.sdi.yml`: a file
/// name, a path, or the workflow's display name) names this workflow.
pub fn names_workflow(configured: &str, workflow_path: &str, workflow_name: &str) -> bool {
    let configured = configured.trim();
    !configured.is_empty()
        && (configured.eq_ignore_ascii_case(workflow_path)
            || workflow_file(configured).eq_ignore_ascii_case(workflow_file(workflow_path))
            || configured.eq_ignore_ascii_case(workflow_name.trim()))
}

/// Whether a successful run of this workflow means a new image exists for
/// the project (CONTRACT.md §22): the workflow `.sdi.yml` names as
/// `delivery.build_workflow`, or, when the file names none, any workflow
/// the scan found to build a container image (`ci:build_image:<path>`).
async fn is_build_workflow(
    scoped: &crate::state::Scoped,
    project_id: &str,
    workflow_path: &str,
    workflow_name: &str,
) -> Result<bool, Status> {
    let context = scoped
        .knowledge()
        .get_project_context(knowledge::GetProjectContextRequest {
            project_id: project_id.to_owned(),
        })
        .await?
        .into_inner();
    let configured = context
        .config
        .as_ref()
        .filter(|config| config.errors.is_empty())
        .and_then(|config| serde_json::from_str::<Value>(&config.delivery_json).ok())
        .and_then(|delivery| delivery["build_workflow"].as_str().map(str::to_owned))
        .unwrap_or_default();
    if !configured.trim().is_empty() {
        return Ok(names_workflow(&configured, workflow_path, workflow_name));
    }
    Ok(!workflow_path.is_empty()
        && context.facts.iter().any(|fact| {
            fact.key
                .strip_prefix("ci:build_image:")
                .is_some_and(|path| path.eq_ignore_ascii_case(workflow_path))
        }))
}

/// A push to the default branch of a connected project queues a scan as
/// `system`.
async fn rescan(
    state: &AppState,
    installation: identity::RecordInstallationRequest,
    full_name: &str,
) -> Result<bool, Status> {
    let workspace = state
        .identity()
        .record_installation(installation)
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

/// A pull request opened or pushed to starts a review as `system` when the
/// workspace's intelligence settings say `auto_review` (CONTRACT.md §20).
/// The review stops at `proposed`; nothing is ever published from here.
async fn auto_review(
    state: &AppState,
    installation: identity::RecordInstallationRequest,
    full_name: &str,
    number: u64,
) -> Result<bool, Status> {
    let workspace = state
        .identity()
        .record_installation(installation)
        .await?
        .into_inner();
    let scoped = state.system(&workspace.id);
    let settings = scoped
        .intelligence()
        .get_settings(intelligence::GetSettingsRequest {})
        .await?
        .into_inner();
    if !settings.auto_review {
        return Ok(false);
    }
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
        .intelligence()
        .start_review(intelligence::StartReviewRequest {
            project_id: project.id,
            pr_number: number,
        })
        .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

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
        let created = json!({
            "installation": {"id": 7, "account": {"login": "acme", "type": "Organization", "avatar_url": "a"}, "repository_selection": "selected"}
        });
        let req = installation_of(&created);
        assert_eq!(req.installation_id, 7);
        assert_eq!(req.account_login, "acme");
        assert_eq!(req.repository_selection, "selected");

        let member = json!({
            "installation": {"id": 7}, "organization": {"login": "acme", "avatar_url": "a"}
        });
        let req = installation_of(&member);
        assert_eq!(req.account_login, "acme");
        assert_eq!(req.account_type, "Organization");
        assert_eq!(req.repository_selection, "");
    }

    /// A GitHub Enterprise Server delivery: the same shape plus an
    /// `enterprise` block, which changes nothing about the classification.
    #[test]
    fn classification_covers_github_com_and_ghes_payloads() {
        let ghes_push = json!({
            "ref": "refs/heads/main",
            "installation": {"id": 9},
            "organization": {"login": "acme"},
            "enterprise": {"id": 1, "slug": "corp", "html_url": "https://ghe.corp.example/enterprises/corp"},
            "repository": {"full_name": "acme/repo", "default_branch": "main",
                           "html_url": "https://ghe.corp.example/acme/repo"}
        });
        assert_eq!(
            classify("push", &ghes_push),
            Some(WebhookJob::Rescan {
                full_name: "acme/repo".into()
            })
        );
        let mut feature = ghes_push.clone();
        feature["ref"] = json!("refs/heads/feature");
        assert_eq!(classify("push", &feature), None);

        let opened = json!({
            "action": "opened", "installation": {"id": 9}, "organization": {"login": "acme"},
            "repository": {"full_name": "acme/repo"}, "pull_request": {"number": 4, "draft": false}
        });
        assert_eq!(
            classify("pull_request", &opened),
            Some(WebhookJob::AutoReview {
                full_name: "acme/repo".into(),
                number: 4
            })
        );
        let mut draft = opened.clone();
        draft["pull_request"]["draft"] = json!(true);
        assert_eq!(classify("pull_request", &draft), None);
        let mut closed = opened.clone();
        closed["action"] = json!("closed");
        assert_eq!(classify("pull_request", &closed), None);

        let deleted =
            json!({"action": "deleted", "installation": {"id": 9, "account": {"login": "acme"}}});
        assert_eq!(
            classify("installation", &deleted),
            Some(WebhookJob::RemoveInstallation { installation_id: 9 })
        );
        let member = json!({"action": "added", "installation": {"id": 9}, "organization": {"login": "acme"}});
        assert_eq!(classify("member", &member), Some(WebhookJob::SyncMembers));
        assert_eq!(classify("ping", &json!({"zen": "x"})), None);
        // No installation id: nothing can be scoped, so nothing is queued.
        assert_eq!(
            classify(
                "push",
                &json!({"ref": "refs/heads/main", "repository": {"full_name": "a/b", "default_branch": "main"}})
            ),
            None
        );
    }
    fn workflow_run(branch: &str, conclusion: &str, event: &str) -> Value {
        json!({
            "action": "completed",
            "installation": {"id": 9},
            "organization": {"login": "acme"},
            "repository": {"full_name": "acme/repo", "default_branch": "main"},
            "workflow_run": {
                "id": 8841,
                "name": "CI",
                "path": ".github/workflows/ci.yml",
                "head_branch": branch,
                "head_sha": "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
                "event": event,
                "status": "completed",
                "conclusion": conclusion,
            }
        })
    }

    /// CONTRACT.md §22: a successful run on the default branch may be a
    /// release; everything else about a workflow run is not.
    #[test]
    fn only_successful_default_branch_runs_are_release_candidates() {
        assert_eq!(
            classify("workflow_run", &workflow_run("main", "success", "push")),
            Some(WebhookJob::BuildSucceeded {
                full_name: "acme/repo".into(),
                workflow_path: ".github/workflows/ci.yml".into(),
                workflow_name: "CI".into(),
                head_sha: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".into(),
                run_id: "8841".into(),
            })
        );
        for (branch, conclusion, event) in [
            ("main", "failure", "push"),
            ("main", "cancelled", "push"),
            ("feature/x", "success", "push"),
            // A fork's pull request whose branch is also called main.
            ("main", "success", "pull_request"),
            ("main", "success", "pull_request_target"),
        ] {
            assert_eq!(
                classify("workflow_run", &workflow_run(branch, conclusion, event)),
                None,
                "{branch} {conclusion} {event}"
            );
        }
        let mut requested = workflow_run("main", "success", "push");
        requested["action"] = json!("requested");
        assert_eq!(classify("workflow_run", &requested), None);
        let mut unscoped = workflow_run("main", "success", "push");
        unscoped["installation"] = json!(null);
        assert_eq!(classify("workflow_run", &unscoped), None);
        // GitHub appends the ref to the path of a reusable workflow's run.
        let mut pinned = workflow_run("main", "success", "push");
        pinned["workflow_run"]["path"] = json!(".github/workflows/ci.yml@refs/heads/main");
        assert!(matches!(
            classify("workflow_run", &pinned),
            Some(WebhookJob::BuildSucceeded { workflow_path, .. }) if workflow_path == ".github/workflows/ci.yml"
        ));
    }

    #[test]
    fn published_container_packages_carry_their_tag_and_digest() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let package = json!({
            "name": "repo",
            "namespace": "acme",
            "package_type": "CONTAINER",
            "package_version": {
                "version": digest,
                "target_oid": "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
                "package_url": "ghcr.io/Acme/Repo:a1b2c3d",
                "container_metadata": {"tag": {"name": "a1b2c3d", "digest": digest}},
            }
        });
        let expected = WebhookJob::PackagePublished {
            full_name: "acme/repo".into(),
            image: "ghcr.io/acme/repo".into(),
            tag: "a1b2c3d".into(),
            digest: digest.clone(),
            commit_sha: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".into(),
        };
        let registry_event = json!({
            "action": "published", "installation": {"id": 9}, "organization": {"login": "acme"},
            "repository": {"full_name": "acme/repo"}, "registry_package": package,
        });
        assert_eq!(
            classify("registry_package", &registry_event),
            Some(expected.clone())
        );
        let package_event = json!({
            "action": "published", "installation": {"id": 9}, "organization": {"login": "acme"},
            "repository": {"full_name": "acme/repo"}, "package": package,
        });
        assert_eq!(classify("package", &package_event), Some(expected));

        // The untagged manifests of a multi-arch push, other package
        // types, updates and unlinked packages are nothing to release.
        let mut untagged = registry_event.clone();
        untagged["registry_package"]["package_version"]["container_metadata"]["tag"]["name"] =
            json!("");
        assert_eq!(classify("registry_package", &untagged), None);
        let mut npm = registry_event.clone();
        npm["registry_package"]["package_type"] = json!("npm");
        assert_eq!(classify("registry_package", &npm), None);
        let mut updated = registry_event.clone();
        updated["action"] = json!("updated");
        assert_eq!(classify("registry_package", &updated), None);
        let mut unlinked = registry_event.clone();
        unlinked["repository"] = json!(null);
        assert_eq!(classify("registry_package", &unlinked), None);

        // Without a package URL the image is named the GHCR way; a version
        // that is not a digest and a commit that is not a SHA are left out.
        let mut bare = registry_event.clone();
        bare["registry_package"]["package_version"] = json!({
            "version": "1.4.2", "target_oid": "main",
            "container_metadata": {"tag": {"name": "v1.4.2", "digest": ""}},
        });
        assert_eq!(
            classify("registry_package", &bare),
            Some(WebhookJob::PackagePublished {
                full_name: "acme/repo".into(),
                image: "ghcr.io/acme/repo".into(),
                tag: "v1.4.2".into(),
                digest: String::new(),
                commit_sha: String::new(),
            })
        );
    }

    #[test]
    fn the_configured_build_workflow_is_matched_by_file_path_or_name() {
        let (path, name) = (".github/workflows/ci.yml", "CI");
        for configured in ["ci.yml", "CI.yml", ".github/workflows/ci.yml", "ci"] {
            assert!(names_workflow(configured, path, name), "{configured}");
        }
        for configured in [
            "",
            "  ",
            "build.yml",
            "release",
            ".github/workflows/other.yml",
        ] {
            assert!(!names_workflow(configured, path, name), "{configured:?}");
        }
    }
}
