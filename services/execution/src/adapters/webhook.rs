//! The `webhook` deployer (CONTRACT.md §22): a signed `POST` with the
//! release as JSON; any 2xx answer is success. The body is signed with
//! HMAC-SHA256 under the workspace secret the environment names and the
//! signature travels in `X-SDI-Signature-256: sha256=<hex>`, the scheme
//! GitHub uses for its own webhooks, so a receiver verifies it the same
//! way. The secret is resolved when the step runs and never leaves this
//! function except as that signature.

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use chrono::{SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use reqwest::{Method, header};
use serde_json::{Value, json};
use sha2::Sha256;

use super::{
    Adapter, RunContext, StepInput, StepOutcome, TOOL_WEBHOOK, is_deploy_action,
    net::{self, Net},
};
use crate::domain::diagnosis;

pub const SIGNATURE_HEADER: &str = "X-SDI-Signature-256";
pub const EVENT_HEADER: &str = "X-SDI-Event";
pub const DELIVERY_HEADER: &str = "X-SDI-Delivery";
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
pub const MAX_TIMEOUT_SECONDS: u64 = 600;

/// `sha256=<hex of HMAC-SHA256(secret, body)>`.
pub fn signature(secret: &str, body: &[u8]) -> String {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).expect("hmac takes any key");
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

/// The release as the receiver gets it.
pub fn payload(step: &StepInput, ctx: &RunContext) -> Value {
    let param = |key: &str| step.params.get(key).cloned().unwrap_or_default();
    // The digest the registry confirmed in this run wins over the recorded one.
    let digest = ctx
        .outputs
        .get("digest")
        .cloned()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| param("digest"));
    json!({
        "event": "deployment",
        "kind": param("run_kind"),
        "environment": param("environment"),
        "project": {
            "id": param("project_id"),
            "repository": param("repo"),
        },
        "release": {
            "id": param("release_id"),
            "version": param("version"),
            "image": param("image"),
            "tag": param("tag"),
            "digest": digest,
            "commit_sha": param("commit_sha"),
        },
        "execution_id": ctx.execution_id.to_string(),
        "attempt": step.attempt,
        "sent_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    })
}

pub struct WebhookAdapter {
    net: Net,
}

impl WebhookAdapter {
    pub fn new(net: Net) -> Self {
        Self { net }
    }

    async fn deploy(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        let param = |key: &str| step.params.get(key).map(|v| v.trim()).unwrap_or_default();
        let url = param("url");
        if url.is_empty() {
            return StepOutcome::failed(
                diagnosis::DEPLOYER_FAILED,
                "the webhook environment has no url configured",
            );
        }
        let secret = match ctx.secrets.get(param("secret")).await {
            Ok(secret) => secret,
            Err(outcome) => return outcome,
        };
        let limit = param("timeout_seconds")
            .parse::<u64>()
            .ok()
            .filter(|s| *s > 0)
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .min(MAX_TIMEOUT_SECONDS);
        let body = serde_json::to_vec(&payload(step, ctx)).expect("payload serialises");
        let signed = signature(&secret, &body);
        ctx.secrets.redactor().protect(&signed);

        let request = match self
            .net
            .request(Method::POST, url, Duration::from_secs(limit))
            .await
        {
            Ok(request) => request,
            Err(reason) => {
                return StepOutcome::failed(
                    diagnosis::DEPLOYER_FAILED,
                    format!("the webhook URL cannot be called: {reason}"),
                );
            }
        };
        let shown = reqwest::Url::parse(url)
            .map(|u| net::redact_url(&u))
            .unwrap_or_else(|_| "the webhook".to_owned());
        ctx.logs.line(format!(
            "POST {shown} ({} bytes, signed, timeout {limit}s)",
            body.len()
        ));
        let response = request
            .header(header::CONTENT_TYPE, "application/json")
            .header(SIGNATURE_HEADER, &signed)
            .header(EVENT_HEADER, "deployment")
            .header(
                DELIVERY_HEADER,
                format!("{}:{}", ctx.execution_id, step.attempt),
            )
            .body(body)
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) if error.is_timeout() => {
                return StepOutcome::failed(
                    diagnosis::DEPLOYER_TIMEOUT,
                    format!("POST {shown} did not answer within {limit}s"),
                );
            }
            Err(error) => {
                return StepOutcome::failed(
                    diagnosis::DEPLOYER_FAILED,
                    format!("POST {shown} failed: {}", net::transport_error(error)),
                );
            }
        };
        let status = response.status();
        ctx.logs.line(format!("POST {shown} -> {status}"));
        let text = net::body_text(response).await;
        if !status.is_success() {
            return StepOutcome::failed(
                diagnosis::DEPLOYER_FAILED,
                format!("POST {shown} -> {status}: {}", net::excerpt(&text, 600)),
            );
        }
        // A receiver may point at where the deployment can be followed.
        let mut outputs = BTreeMap::new();
        if let Some(link) = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|json| {
                ["url", "external_url", "html_url"]
                    .iter()
                    .find_map(|key| json.get(*key)?.as_str().map(str::to_owned))
            })
            .filter(|link| link.len() <= 2048 && link.starts_with("https://"))
        {
            outputs.insert("external_url".to_owned(), link);
        }
        StepOutcome::Succeeded {
            output: format!("webhook accepted the release: POST {shown} -> {status}"),
            outputs,
        }
    }
}

#[async_trait]
impl Adapter for WebhookAdapter {
    fn name(&self) -> &'static str {
        TOOL_WEBHOOK
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        if !is_deploy_action(&step.action_type) {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("the webhook adapter cannot run {}", step.action_type),
            );
        }
        self.deploy(step, ctx).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, StatusCode as HttpStatus},
        response::{IntoResponse, Response},
        routing::post,
    };

    use super::*;
    use crate::adapters::{LogSink, Redactor, StaticSecrets, net::NetPolicy};

    const SECRET: &str = "whsec_5d41402abc4b2a76b9719d911017c592";

    #[derive(Default)]
    struct Received {
        bodies: Mutex<Vec<Value>>,
        headers: Mutex<Vec<(String, String)>>,
    }

    /// Verifies the signature the way a receiver would, over the raw body.
    async fn receive(
        State(received): State<Arc<Received>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned()
        };
        if header("x-sdi-signature-256") != signature(SECRET, &body) {
            return (HttpStatus::UNAUTHORIZED, "signature mismatch").into_response();
        }
        received.headers.lock().unwrap().extend([
            ("event".to_owned(), header("x-sdi-event")),
            ("delivery".to_owned(), header("x-sdi-delivery")),
            ("content-type".to_owned(), header("content-type")),
        ]);
        received
            .bodies
            .lock()
            .unwrap()
            .push(serde_json::from_slice(&body).unwrap());
        Json(json!({"accepted": true, "url": "https://deploy.example.com/runs/42"})).into_response()
    }

    async fn broken() -> Response {
        // A receiver that leaks what it was configured with.
        (
            HttpStatus::INTERNAL_SERVER_ERROR,
            format!("deploy script crashed; HOOK_SECRET={SECRET}"),
        )
            .into_response()
    }

    async fn slow() -> &'static str {
        tokio::time::sleep(Duration::from_secs(5)).await;
        "late"
    }

    async fn serve() -> (Arc<Received>, String) {
        let received = Arc::new(Received::default());
        let app = Router::new()
            .route("/deploy", post(receive))
            .route("/broken", post(broken))
            .route("/slow", post(slow))
            .with_state(Arc::clone(&received));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (received, format!("http://{addr}"))
    }

    fn step(url: &str, timeout: &str) -> StepInput {
        let params = [
            ("deployer", "webhook"),
            ("url", url),
            ("secret", "DEPLOY_HOOK_SECRET"),
            ("timeout_seconds", timeout),
            ("run_kind", "promote"),
            ("environment", "production"),
            ("project_id", "p-1"),
            ("repo", "acme/api"),
            ("release_id", "r-1"),
            ("version", "a1b2c3d"),
            ("image", "ghcr.io/acme/api"),
            ("tag", "a1b2c3d"),
            ("digest", ""),
            ("commit_sha", "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678"),
        ];
        StepInput {
            key: "deploy".into(),
            title: "Deploy to production".into(),
            action_type: "deploy.production".into(),
            adapter: "webhook".into(),
            params: params
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            attempt: 2,
        }
    }

    fn context(secret: &str) -> (RunContext, tokio::sync::mpsc::Receiver<String>) {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let redactor = Redactor::default();
        let ctx = RunContext {
            execution_id: uuid::Uuid::now_v7(),
            outputs: BTreeMap::from([("digest".to_owned(), format!("sha256:{}", "a".repeat(64)))]),
            secrets: StaticSecrets::with(&[("DEPLOY_HOOK_SECRET", secret)]).secrets(&redactor),
            logs: LogSink::new(tx).with_redactor(redactor),
            ..Default::default()
        };
        (ctx, rx)
    }

    fn adapter() -> WebhookAdapter {
        WebhookAdapter::new(Net::new(NetPolicy::ALLOW_PRIVATE))
    }

    #[tokio::test]
    async fn posts_the_signed_release_and_any_2xx_is_success() {
        let (received, base) = serve().await;
        let (ctx, mut rx) = context(SECRET);
        let outcome = adapter()
            .run(&step(&format!("{base}/deploy?team=core"), ""), &ctx)
            .await;
        let StepOutcome::Succeeded { output, outputs } = outcome else {
            panic!("expected success: {outcome:?}");
        };
        assert!(output.contains("-> 200 OK"), "{output}");
        assert!(
            !output.contains("team=core"),
            "the query never shows: {output}"
        );
        assert_eq!(
            outputs["external_url"],
            "https://deploy.example.com/runs/42"
        );

        let bodies = received.bodies.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        let body = &bodies[0];
        assert_eq!(body["event"], "deployment");
        assert_eq!(body["kind"], "promote");
        assert_eq!(body["environment"], "production");
        assert_eq!(body["project"]["repository"], "acme/api");
        assert_eq!(body["release"]["image"], "ghcr.io/acme/api");
        assert_eq!(body["release"]["tag"], "a1b2c3d");
        // The digest verified earlier in the run, not the empty recorded one.
        assert_eq!(
            body["release"]["digest"],
            format!("sha256:{}", "a".repeat(64))
        );
        assert_eq!(body["execution_id"], ctx.execution_id.to_string());
        assert_eq!(body["attempt"], 2);
        let headers = received.headers.lock().unwrap();
        assert!(headers.contains(&("event".to_owned(), "deployment".to_owned())));
        assert!(headers.contains(&("delivery".to_owned(), format!("{}:2", ctx.execution_id))));
        assert!(headers.contains(&("content-type".to_owned(), "application/json".to_owned())));

        let mut lines = Vec::new();
        while let Ok(line) = rx.try_recv() {
            lines.push(line);
        }
        let lines = lines.join("\n");
        assert!(!lines.contains(SECRET), "{lines}");
        assert!(
            !lines.contains("sha256="),
            "the signature is not logged: {lines}"
        );
    }

    #[tokio::test]
    async fn a_wrong_secret_a_crash_and_a_timeout_map_to_deployer_failures() {
        let (received, base) = serve().await;

        // The receiver rejects a signature made with another key.
        let (ctx, _rx) = context("another-secret-entirely");
        let outcome = adapter()
            .run(&step(&format!("{base}/deploy"), ""), &ctx)
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(
            raw_output.contains("-> 401 Unauthorized: signature mismatch"),
            "{raw_output}"
        );
        assert!(received.bodies.lock().unwrap().is_empty());

        // A receiver that prints the secret in its error: masked on the way out.
        let (ctx, _rx) = context(SECRET);
        let outcome = adapter()
            .run(&step(&format!("{base}/broken"), ""), &ctx)
            .await
            .redacted(ctx.secrets.redactor());
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(
            raw_output.contains("500 Internal Server Error"),
            "{raw_output}"
        );
        assert!(
            raw_output.contains("HOOK_SECRET=[redacted]"),
            "{raw_output}"
        );
        assert!(!raw_output.contains(SECRET));

        // No answer within timeout_seconds.
        let outcome = adapter()
            .run(&step(&format!("{base}/slow"), "1"), &ctx)
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_TIMEOUT);
        assert!(
            raw_output.contains("did not answer within 1s"),
            "{raw_output}"
        );

        // A secret the workspace does not hold: nothing is sent.
        let ctx = RunContext {
            secrets: StaticSecrets::default().secrets(&Redactor::default()),
            ..Default::default()
        };
        let outcome = adapter()
            .run(&step(&format!("{base}/deploy"), ""), &ctx)
            .await;
        let StepOutcome::Failed { raw_output, .. } = outcome else {
            panic!("expected a failure");
        };
        assert!(raw_output.contains("secret DEPLOY_HOOK_SECRET is not set"));
    }

    #[tokio::test]
    async fn the_ssrf_guard_applies_to_webhook_urls() {
        let (received, base) = serve().await;
        let guarded = WebhookAdapter::new(Net::new(NetPolicy::PUBLIC_ONLY));
        let (ctx, _rx) = context(SECRET);
        for url in [
            format!("{base}/deploy"),
            "https://127.0.0.1/deploy".to_owned(),
            "https://169.254.169.254/latest/api/token".to_owned(),
            "https://10.0.0.8/deploy".to_owned(),
        ] {
            let outcome = guarded.run(&step(&url, ""), &ctx).await;
            let StepOutcome::Failed {
                code, raw_output, ..
            } = outcome
            else {
                panic!("{url} must be refused");
            };
            assert_eq!(code, diagnosis::DEPLOYER_FAILED);
            assert!(
                raw_output.contains("the webhook URL cannot be called"),
                "{raw_output}"
            );
        }
        assert!(received.bodies.lock().unwrap().is_empty());
    }

    #[test]
    fn signatures_match_the_reference_hmac() {
        // RFC 4231 test case 2.
        assert_eq!(
            signature("Jefe", b"what do ya want for nothing?"),
            "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }
}
