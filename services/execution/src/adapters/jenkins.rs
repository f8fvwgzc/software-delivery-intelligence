//! The `jenkins` deployer (CONTRACT.md §22): `POST <url>/job/<job>/buildWithParameters`
//! with the release as build parameters, then follows the queued item to its
//! build and the build to its result. Authenticates with the user name and
//! API token held in two workspace secrets (API tokens need no CSRF crumb).
//!
//! Jenkins answers with absolute URLs for the queue item and the build.
//! Only their paths are used, on the origin the environment configured: a
//! Jenkins behind a proxy often reports an internal address, and a server
//! must not be able to point the poller at another host.

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use reqwest::{Method, StatusCode, Url, header};
use serde_json::Value;

use super::{
    Adapter, RunContext, StepInput, StepOutcome, TOOL_JENKINS, is_deploy_action,
    net::{self, Net},
};
use crate::domain::diagnosis;

pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;
pub const MAX_TIMEOUT_SECONDS: u64 = 6 * 60 * 60;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_secs(5);

/// The step's time limit: `params.timeout_seconds`, bounded.
pub(super) fn step_limit(step: &StepInput, default: u64, max: u64) -> Duration {
    Duration::from_secs(
        step.params
            .get("timeout_seconds")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|s| *s > 0)
            .unwrap_or(default)
            .min(max),
    )
}

/// `team/api` → `/job/team/job/api` (folders nest as jobs).
pub fn job_path(job: &str) -> Vec<String> {
    job.split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty() && *segment != "job")
        .flat_map(|segment| ["job".to_owned(), segment.to_owned()])
        .collect()
}

pub struct JenkinsAdapter {
    net: Net,
    poll: Duration,
}

/// A step that ended early: the failure is already its outcome.
type Fail = StepOutcome;

fn failed(detail: impl Into<String>) -> Fail {
    StepOutcome::failed(diagnosis::DEPLOYER_FAILED, detail)
}

impl JenkinsAdapter {
    pub fn new(net: Net) -> Self {
        Self { net, poll: POLL }
    }

    #[cfg(test)]
    pub fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    /// `base` with `segments` appended to its path.
    fn extend(base: &Url, segments: &[String]) -> Result<Url, Fail> {
        let mut url = base.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|()| failed("the Jenkins url cannot carry a path"))?;
            path.pop_if_empty();
            for segment in segments {
                path.push(segment);
            }
        }
        Ok(url)
    }

    /// The path of a URL Jenkins reported, on the configured origin.
    fn same_origin(base: &Url, reported: &str, suffix: &str) -> Result<Url, Fail> {
        let path = match Url::parse(reported) {
            Ok(url) => url.path().to_owned(),
            Err(_) if reported.starts_with('/') => reported.to_owned(),
            Err(_) => {
                return Err(failed(format!(
                    "Jenkins reported an unusable URL: {reported}"
                )));
            }
        };
        let mut url = base.clone();
        url.set_query(None);
        url.set_path(&format!("{}/{suffix}", path.trim_end_matches('/')));
        Ok(url)
    }

    async fn get_json(
        &self,
        url: &Url,
        user: &str,
        token: &str,
        ctx: &RunContext,
    ) -> Result<Value, Fail> {
        let shown = net::redact_url(url);
        let response = self
            .net
            .request(Method::GET, url.as_str(), REQUEST_TIMEOUT)
            .await
            .map_err(|reason| failed(format!("GET {shown} is not allowed: {reason}")))?
            .basic_auth(user, Some(token))
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| failed(format!("GET {shown} failed: {}", net::transport_error(e))))?;
        let status = response.status();
        let text = net::body_text(response).await;
        if !status.is_success() {
            ctx.logs.line(format!("GET {shown} -> {status}"));
            return Err(failed(format!(
                "GET {shown} -> {status}: {}",
                net::excerpt(&text, 300)
            )));
        }
        serde_json::from_str(&text).map_err(|_| failed(format!("GET {shown} did not answer JSON")))
    }

    async fn deploy(&self, step: &StepInput, ctx: &RunContext) -> Result<StepOutcome, Fail> {
        let param = |key: &str| step.params.get(key).map(|v| v.trim()).unwrap_or_default();
        let base = Url::parse(param("url"))
            .map_err(|e| failed(format!("the Jenkins url is not a URL: {e}")))?;
        let job = job_path(param("job"));
        if job.is_empty() {
            return Err(failed("the jenkins environment has no job configured"));
        }
        let user = ctx.secrets.get(param("user_secret")).await?;
        let token = ctx.secrets.get(param("token_secret")).await?;
        let limit = step_limit(step, DEFAULT_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS);
        let deadline = tokio::time::Instant::now() + limit;
        let digest = ctx
            .outputs
            .get("digest")
            .map(String::as_str)
            .filter(|d| !d.is_empty())
            .unwrap_or(param("digest"));

        // 1. Trigger.
        let mut segments = job.clone();
        segments.push("buildWithParameters".to_owned());
        let mut trigger = Self::extend(&base, &segments)?;
        trigger
            .query_pairs_mut()
            .append_pair("ENVIRONMENT", param("environment"))
            .append_pair("IMAGE", param("image"))
            .append_pair("TAG", param("tag"))
            .append_pair("DIGEST", digest)
            .append_pair("VERSION", param("version"));
        let shown = net::redact_url(&trigger);
        let response = self
            .net
            .request(Method::POST, trigger.as_str(), REQUEST_TIMEOUT)
            .await
            .map_err(|reason| failed(format!("the Jenkins url cannot be called: {reason}")))?
            .basic_auth(&user, Some(&token))
            .send()
            .await
            .map_err(|e| failed(format!("POST {shown} failed: {}", net::transport_error(e))))?;
        let status = response.status();
        ctx.logs.line(format!("POST {shown} -> {status}"));
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(failed(format!(
                "POST {shown} -> {status}: Jenkins refused the credentials in the secrets {} and {} (the user needs Job/Build on this job)",
                param("user_secret"),
                param("token_secret")
            )));
        }
        if status == StatusCode::NOT_FOUND {
            return Err(failed(format!(
                "POST {shown} -> {status}: Jenkins has no job {} (or the user cannot see it)",
                param("job")
            )));
        }
        if !(status.is_success() || status.is_redirection()) {
            let text = net::body_text(response).await;
            return Err(failed(format!(
                "POST {shown} -> {status}: {}",
                net::excerpt(&text, 300)
            )));
        }
        let Some(location) = location else {
            return Err(failed(format!(
                "POST {shown} -> {status} without a Location for the queued build"
            )));
        };

        // 2. Queue item → build.
        let queue = Self::same_origin(&base, &location, "api/json")?;
        let mut waits = 0u32;
        let build_url = loop {
            let item = self.get_json(&queue, &user, &token, ctx).await?;
            if item.get("cancelled").and_then(Value::as_bool) == Some(true) {
                return Err(failed(
                    "the queued Jenkins build was cancelled before it started",
                ));
            }
            if let Some(url) = item
                .get("executable")
                .and_then(|e| e.get("url"))
                .and_then(Value::as_str)
            {
                break url.to_owned();
            }
            if waits.is_multiple_of(6) {
                let why = item.get("why").and_then(Value::as_str).unwrap_or("queued");
                ctx.logs
                    .line(format!("waiting in the Jenkins queue: {why}"));
            }
            waits += 1;
            if tokio::time::Instant::now() + self.poll >= deadline {
                return Ok(StepOutcome::failed(
                    diagnosis::DEPLOYER_TIMEOUT,
                    format!(
                        "the Jenkins build was still queued after {}s",
                        limit.as_secs()
                    ),
                ));
            }
            tokio::time::sleep(self.poll).await;
        };
        let build = Self::same_origin(&base, &build_url, "api/json")?;
        let mut external = build.clone();
        external.set_path(build.path().trim_end_matches("api/json"));
        let external = net::redact_url(&external);
        ctx.logs.line(format!("Jenkins build started: {external}"));
        ctx.progress.emit("external_url", external.clone());

        // 3. Build → result.
        loop {
            let status = self.get_json(&build, &user, &token, ctx).await?;
            let building = status
                .get("building")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let result = status.get("result").and_then(Value::as_str);
            if let (false, Some(result)) = (building, result) {
                let number = status.get("number").and_then(Value::as_u64).unwrap_or(0);
                ctx.logs
                    .line(format!("Jenkins build #{number} finished {result}"));
                let outputs = BTreeMap::from([("external_url".to_owned(), external.clone())]);
                return Ok(if result == "SUCCESS" {
                    StepOutcome::Succeeded {
                        output: format!("Jenkins build #{number} succeeded: {external}"),
                        outputs,
                    }
                } else {
                    StepOutcome::failed(
                        diagnosis::DEPLOYER_FAILED,
                        format!("Jenkins build #{number} finished {result}: {external}console"),
                    )
                });
            }
            if tokio::time::Instant::now() + self.poll >= deadline {
                return Ok(StepOutcome::failed(
                    diagnosis::DEPLOYER_TIMEOUT,
                    format!(
                        "the Jenkins build was still running after {}s: {external}",
                        limit.as_secs()
                    ),
                ));
            }
            tokio::time::sleep(self.poll).await;
        }
    }
}

#[async_trait]
impl Adapter for JenkinsAdapter {
    fn name(&self) -> &'static str {
        TOOL_JENKINS
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        if !is_deploy_action(&step.action_type) {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("the jenkins adapter cannot run {}", step.action_type),
            );
        }
        match self.deploy(step, ctx).await {
            Ok(outcome) | Err(outcome) => outcome,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Json, Router,
        extract::{Path, Query, State},
        http::{HeaderMap, StatusCode as HttpStatus},
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde_json::json;

    use super::*;
    use crate::adapters::{LogSink, OutputSink, Redactor, StaticSecrets, net::NetPolicy};

    const USER: &str = "sdi-deployer";
    const TOKEN: &str = "11aa22bb33cc44dd55ee66ff77";

    struct Fake {
        calls: Mutex<Vec<String>>,
        queue_polls: AtomicUsize,
        build_polls: AtomicUsize,
        /// What the build ends as.
        result: &'static str,
        /// Never finish the build.
        hang: bool,
    }

    type Shared = Arc<Fake>;

    fn authorized(headers: &HeaderMap) -> bool {
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == format!("Basic {}", STANDARD.encode(format!("{USER}:{TOKEN}"))))
    }

    async fn trigger(
        State(fake): State<Shared>,
        Path(job): Path<String>,
        Query(query): Query<BTreeMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        if !authorized(&headers) {
            return (HttpStatus::UNAUTHORIZED, "bad credentials").into_response();
        }
        let params: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
        fake.calls
            .lock()
            .unwrap()
            .push(format!("POST /job/{job} {}", params.join("&")));
        if !job.ends_with("deploy-api/buildWithParameters") {
            return (HttpStatus::NOT_FOUND, "no such job").into_response();
        }
        // An internal address, as a proxied Jenkins reports it.
        (
            HttpStatus::CREATED,
            [("location", "http://jenkins.internal:8080/queue/item/77/")],
        )
            .into_response()
    }

    async fn queue_item(State(fake): State<Shared>, headers: HeaderMap) -> Response {
        if !authorized(&headers) {
            return (HttpStatus::UNAUTHORIZED, "bad credentials").into_response();
        }
        let polls = fake.queue_polls.fetch_add(1, Ordering::SeqCst);
        if polls < 2 {
            return Json(json!({"why": "Waiting for next available executor", "cancelled": false}))
                .into_response();
        }
        Json(json!({
            "cancelled": false,
            "executable": {"number": 41, "url": "http://jenkins.internal:8080/job/team/job/deploy-api/41/"}
        }))
        .into_response()
    }

    async fn build(State(fake): State<Shared>, headers: HeaderMap) -> Response {
        if !authorized(&headers) {
            return (HttpStatus::UNAUTHORIZED, "bad credentials").into_response();
        }
        let polls = fake.build_polls.fetch_add(1, Ordering::SeqCst);
        if fake.hang || polls < 2 {
            return Json(json!({"number": 41, "building": true, "result": null})).into_response();
        }
        Json(json!({"number": 41, "building": false, "result": fake.result})).into_response()
    }

    async fn serve(result: &'static str, hang: bool) -> (Shared, String) {
        let fake = Arc::new(Fake {
            calls: Mutex::default(),
            queue_polls: AtomicUsize::new(0),
            build_polls: AtomicUsize::new(0),
            result,
            hang,
        });
        let app = Router::new()
            .route("/job/{*job}", post(trigger))
            .route("/queue/item/77/api/json", get(queue_item))
            .route("/job/team/job/deploy-api/41/api/json", get(build))
            .with_state(Arc::clone(&fake));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (fake, format!("http://{addr}"))
    }

    fn step(url: &str, job: &str, timeout: &str) -> StepInput {
        let params = [
            ("deployer", "jenkins"),
            ("url", url),
            ("job", job),
            ("user_secret", "JENKINS_USER"),
            ("token_secret", "JENKINS_TOKEN"),
            ("timeout_seconds", timeout),
            ("environment", "staging"),
            ("image", "ghcr.io/acme/api"),
            ("tag", "a1b2c3d"),
            ("digest", "sha256:abc"),
            ("version", "a1b2c3d"),
        ];
        StepInput {
            key: "deploy".into(),
            title: "Deploy to staging".into(),
            action_type: "deploy.staging".into(),
            adapter: "jenkins".into(),
            params: params
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            attempt: 1,
        }
    }

    struct Run {
        ctx: RunContext,
        logs: tokio::sync::mpsc::Receiver<String>,
        progress: tokio::sync::mpsc::Receiver<(String, String)>,
    }

    fn run_context(token: &str) -> Run {
        let (tx, logs) = tokio::sync::mpsc::channel(256);
        let (ptx, progress) = tokio::sync::mpsc::channel(16);
        let redactor = Redactor::default();
        let ctx = RunContext {
            secrets: StaticSecrets::with(&[("JENKINS_USER", USER), ("JENKINS_TOKEN", token)])
                .secrets(&redactor),
            logs: LogSink::new(tx).with_redactor(redactor),
            progress: OutputSink::new(ptx),
            ..Default::default()
        };
        Run {
            ctx,
            logs,
            progress,
        }
    }

    fn adapter() -> JenkinsAdapter {
        JenkinsAdapter::new(Net::new(NetPolicy::ALLOW_PRIVATE)).with_poll(Duration::from_millis(10))
    }

    #[tokio::test]
    async fn triggers_the_job_and_follows_the_queue_to_a_successful_build() {
        let (fake, base) = serve("SUCCESS", false).await;
        let mut run = run_context(TOKEN);
        let outcome = adapter()
            .run(&step(&base, "team/deploy-api", ""), &run.ctx)
            .await;
        let StepOutcome::Succeeded { output, outputs } = outcome else {
            panic!("expected success: {outcome:?}");
        };
        let build_url = format!("{base}/job/team/job/deploy-api/41/");
        assert_eq!(outputs["external_url"], build_url);
        assert!(output.contains("build #41 succeeded"), "{output}");
        // Folders nest as jobs and the release travels as build parameters.
        assert_eq!(
            *fake.calls.lock().unwrap(),
            [
                "POST /job/team/job/deploy-api/buildWithParameters DIGEST=sha256:abc&ENVIRONMENT=staging&IMAGE=ghcr.io/acme/api&TAG=a1b2c3d&VERSION=a1b2c3d"
            ]
        );
        // The queue and the build were polled on the configured origin, not
        // on the internal host Jenkins named.
        assert_eq!(fake.queue_polls.load(Ordering::SeqCst), 3);
        assert_eq!(fake.build_polls.load(Ordering::SeqCst), 3);
        // The build URL was reported as soon as the build existed.
        assert_eq!(
            run.progress.try_recv().unwrap(),
            ("external_url".to_owned(), build_url)
        );
        let mut lines = Vec::new();
        while let Ok(line) = run.logs.try_recv() {
            lines.push(line);
        }
        let lines = lines.join("\n");
        assert!(lines.contains("waiting in the Jenkins queue"), "{lines}");
        assert!(!lines.contains(TOKEN), "{lines}");
        assert!(
            !lines.contains("IMAGE="),
            "the query is not logged: {lines}"
        );
    }

    #[tokio::test]
    async fn a_failed_build_a_hung_build_and_bad_credentials_are_reported() {
        let (_, base) = serve("FAILURE", false).await;
        let run = run_context(TOKEN);
        let outcome = adapter()
            .run(&step(&base, "team/deploy-api", ""), &run.ctx)
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(
            raw_output.contains("build #41 finished FAILURE")
                && raw_output.ends_with("/job/team/job/deploy-api/41/console"),
            "{raw_output}"
        );

        let (_, base) = serve("SUCCESS", true).await;
        let outcome = adapter()
            .run(&step(&base, "team/deploy-api", "1"), &run.ctx)
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_TIMEOUT);
        assert!(
            raw_output.contains("still running after 1s"),
            "{raw_output}"
        );

        let (fake, base) = serve("SUCCESS", false).await;
        let wrong = run_context("not-the-token-000000");
        let outcome = adapter()
            .run(&step(&base, "team/deploy-api", ""), &wrong.ctx)
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(
            raw_output
                .contains("refused the credentials in the secrets JENKINS_USER and JENKINS_TOKEN"),
            "{raw_output}"
        );
        assert!(!raw_output.contains("not-the-token-000000"));
        assert!(fake.calls.lock().unwrap().is_empty());

        let outcome = adapter()
            .run(&step(&base, "team/other", ""), &run.ctx)
            .await;
        let StepOutcome::Failed { raw_output, .. } = outcome else {
            panic!("expected a failure");
        };
        assert!(raw_output.contains("no job team/other"), "{raw_output}");
    }

    #[tokio::test]
    async fn the_ssrf_guard_applies_to_jenkins_urls() {
        let (fake, base) = serve("SUCCESS", false).await;
        let guarded = JenkinsAdapter::new(Net::new(NetPolicy::PUBLIC_ONLY));
        let run = run_context(TOKEN);
        let outcome = guarded
            .run(&step(&base, "team/deploy-api", ""), &run.ctx)
            .await;
        let StepOutcome::Failed { raw_output, .. } = outcome else {
            panic!("a loopback Jenkins must be refused");
        };
        assert!(
            raw_output.contains("the Jenkins url cannot be called"),
            "{raw_output}"
        );
        assert!(fake.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn job_names_become_nested_job_paths() {
        assert_eq!(job_path("deploy"), ["job", "deploy"]);
        assert_eq!(
            job_path("team/api/deploy"),
            ["job", "team", "job", "api", "job", "deploy"]
        );
        // Already written the Jenkins way.
        assert_eq!(
            job_path("/job/team/job/deploy/"),
            ["job", "team", "job", "deploy"]
        );
        assert!(job_path(" / ").is_empty());
    }
}
