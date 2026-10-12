//! The `azure_devops` deployer (CONTRACT.md §22): starts a run of an Azure
//! Pipeline through `POST <organization>/<project>/_apis/pipelines/<id>/runs`
//! with the release as template parameters, then polls the run until it
//! completes. Authenticates with a personal access token held in a
//! workspace secret (Basic auth with an empty user name, as Azure DevOps
//! expects).

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use reqwest::{Method, StatusCode, Url, header};
use serde_json::{Value, json};

use super::{
    Adapter, RunContext, StepInput, StepOutcome, TOOL_AZURE_DEVOPS, is_deploy_action,
    jenkins::step_limit,
    net::{self, Net},
};
use crate::domain::diagnosis;

pub const DEFAULT_BASE: &str = "https://dev.azure.com";
pub const API_VERSION: &str = "7.1";
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;
pub const MAX_TIMEOUT_SECONDS: u64 = 6 * 60 * 60;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_secs(5);

pub struct AzureDevopsAdapter {
    net: Net,
    base: String,
    poll: Duration,
}

type Fail = StepOutcome;

fn failed(detail: impl Into<String>) -> Fail {
    StepOutcome::failed(diagnosis::DEPLOYER_FAILED, detail)
}

impl AzureDevopsAdapter {
    pub fn new(net: Net, base: &str) -> Self {
        Self {
            net,
            base: base.trim().trim_end_matches('/').to_owned(),
            poll: POLL,
        }
    }

    #[cfg(test)]
    pub fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    /// `<base>/<organization>/<project>/_apis/pipelines/<id>/runs[/<run>]`.
    fn runs_url(
        &self,
        organization: &str,
        project: &str,
        pipeline_id: &str,
        run: Option<u64>,
    ) -> Result<Url, Fail> {
        let mut url = Url::parse(&self.base)
            .map_err(|e| failed(format!("the Azure DevOps base URL is invalid: {e}")))?;
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|()| failed("the Azure DevOps base URL cannot carry a path"))?;
            path.pop_if_empty();
            path.extend([
                organization,
                project,
                "_apis",
                "pipelines",
                pipeline_id,
                "runs",
            ]);
            if let Some(run) = run {
                path.push(&run.to_string());
            }
        }
        url.query_pairs_mut()
            .append_pair("api-version", API_VERSION);
        Ok(url)
    }

    /// Sends one call and parses the JSON answer. Azure DevOps answers a
    /// rejected token with a sign-in page (203 or 302), not a 401.
    async fn call(
        &self,
        method: Method,
        url: &Url,
        pat: &str,
        body: Option<&Value>,
        ctx: &RunContext,
        pat_secret: &str,
    ) -> Result<Value, Fail> {
        let shown = net::redact_url(url);
        let mut request = self
            .net
            .request(method.clone(), url.as_str(), REQUEST_TIMEOUT)
            .await
            .map_err(|reason| failed(format!("{method} {shown} is not allowed: {reason}")))?
            .basic_auth("", Some(pat))
            .header(header::ACCEPT, "application/json");
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|e| {
            failed(format!(
                "{method} {shown} failed: {}",
                net::transport_error(e)
            ))
        })?;
        let status = response.status();
        ctx.logs.line(format!("{method} {shown} -> {status}"));
        let text = net::body_text(response).await;
        if matches!(
            status,
            StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::NON_AUTHORITATIVE_INFORMATION
                | StatusCode::FOUND
        ) {
            return Err(failed(format!(
                "{method} {shown} -> {status}: Azure DevOps refused the token in the secret {pat_secret} (it needs Build: Read & execute on this pipeline, and must not be expired)"
            )));
        }
        let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if !status.is_success() {
            let message = json
                .get("message")
                .and_then(Value::as_str)
                .map_or_else(|| net::excerpt(&text, 300), str::to_owned);
            return Err(failed(format!("{method} {shown} -> {status}: {message}")));
        }
        if json.is_null() {
            return Err(failed(format!("{method} {shown} did not answer JSON")));
        }
        Ok(json)
    }

    async fn deploy(&self, step: &StepInput, ctx: &RunContext) -> Result<StepOutcome, Fail> {
        let param = |key: &str| step.params.get(key).map(|v| v.trim()).unwrap_or_default();
        let (organization, project, pipeline_id) = (
            param("organization"),
            param("project"),
            param("pipeline_id"),
        );
        if organization.is_empty() || project.is_empty() || pipeline_id.is_empty() {
            return Err(failed(
                "the azure_devops environment needs organization, project and pipeline_id",
            ));
        }
        let pat_secret = param("pat_secret");
        let pat = ctx.secrets.get(pat_secret).await?;
        let limit = step_limit(step, DEFAULT_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS);
        let deadline = tokio::time::Instant::now() + limit;
        let digest = ctx
            .outputs
            .get("digest")
            .map(String::as_str)
            .filter(|d| !d.is_empty())
            .unwrap_or(param("digest"));

        let started = self
            .call(
                Method::POST,
                &self.runs_url(organization, project, pipeline_id, None)?,
                &pat,
                Some(&json!({
                    "templateParameters": {
                        "environment": param("environment"),
                        "image": param("image"),
                        "digest": digest,
                        "version": param("version"),
                    }
                })),
                ctx,
                pat_secret,
            )
            .await?;
        let Some(run_id) = started.get("id").and_then(Value::as_u64) else {
            return Err(failed("Azure DevOps started a run without an id"));
        };
        let external = started
            .get("_links")
            .and_then(|links| links.get("web"))
            .and_then(|web| web.get("href"))
            .and_then(Value::as_str)
            .filter(|href| href.starts_with("https://") || href.starts_with("http://"))
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "{}/{organization}/{project}/_build/results?buildId={run_id}",
                    self.base
                )
            });
        ctx.logs
            .line(format!("pipeline run {run_id} started: {external}"));
        ctx.progress.emit("external_url", external.clone());

        let status_url = self.runs_url(organization, project, pipeline_id, Some(run_id))?;
        let mut run = started;
        loop {
            let state = run.get("state").and_then(Value::as_str).unwrap_or_default();
            if state.eq_ignore_ascii_case("completed") {
                let result = run
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let outputs = BTreeMap::from([("external_url".to_owned(), external.clone())]);
                return Ok(if result.eq_ignore_ascii_case("succeeded") {
                    StepOutcome::Succeeded {
                        output: format!("pipeline run {run_id} succeeded: {external}"),
                        outputs,
                    }
                } else {
                    StepOutcome::failed(
                        diagnosis::DEPLOYER_FAILED,
                        format!("pipeline run {run_id} finished {result}: {external}"),
                    )
                });
            }
            if tokio::time::Instant::now() + self.poll >= deadline {
                return Ok(StepOutcome::failed(
                    diagnosis::DEPLOYER_TIMEOUT,
                    format!(
                        "pipeline run {run_id} was still {} after {}s: {external}",
                        if state.is_empty() { "running" } else { state },
                        limit.as_secs()
                    ),
                ));
            }
            tokio::time::sleep(self.poll).await;
            run = self
                .call(Method::GET, &status_url, &pat, None, ctx, pat_secret)
                .await?;
        }
    }
}

#[async_trait]
impl Adapter for AzureDevopsAdapter {
    fn name(&self) -> &'static str {
        TOOL_AZURE_DEVOPS
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        if !is_deploy_action(&step.action_type) {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("the azure_devops adapter cannot run {}", step.action_type),
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

    use super::*;
    use crate::adapters::{LogSink, OutputSink, Redactor, StaticSecrets, net::NetPolicy};

    const PAT: &str = "azdo-pat-9d8c7b6a5f4e3d2c1b0a";

    struct Fake {
        started: Mutex<Vec<Value>>,
        polls: AtomicUsize,
        result: &'static str,
        hang: bool,
    }

    type Shared = Arc<Fake>;

    fn authorized(headers: &HeaderMap) -> bool {
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == format!("Basic {}", STANDARD.encode(format!(":{PAT}"))))
    }

    async fn start(
        State(fake): State<Shared>,
        Path((organization, project, pipeline)): Path<(String, String, String)>,
        Query(query): Query<BTreeMap<String, String>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        if !authorized(&headers) {
            // What Azure DevOps really does with a bad token.
            return (
                HttpStatus::NON_AUTHORITATIVE_INFORMATION,
                "<html>Sign in</html>",
            )
                .into_response();
        }
        assert_eq!(query["api-version"], "7.1");
        if pipeline != "12" {
            return (
                HttpStatus::NOT_FOUND,
                Json(json!({"message": format!("Pipeline {pipeline} was not found in {organization}/{project}.")})),
            )
                .into_response();
        }
        fake.started.lock().unwrap().push(body);
        Json(json!({
            "id": 501,
            "state": "inProgress",
            "_links": {"web": {"href": "https://dev.azure.com/acme/Shop%20Api/_build/results?buildId=501"}}
        }))
        .into_response()
    }

    async fn status(State(fake): State<Shared>, headers: HeaderMap) -> Response {
        if !authorized(&headers) {
            return (HttpStatus::UNAUTHORIZED, "").into_response();
        }
        let polls = fake.polls.fetch_add(1, Ordering::SeqCst);
        if fake.hang || polls < 2 {
            return Json(json!({"id": 501, "state": "inProgress"})).into_response();
        }
        Json(json!({"id": 501, "state": "completed", "result": fake.result})).into_response()
    }

    async fn serve(result: &'static str, hang: bool) -> (Shared, String) {
        let fake = Arc::new(Fake {
            started: Mutex::default(),
            polls: AtomicUsize::new(0),
            result,
            hang,
        });
        let app = Router::new()
            .route(
                "/{org}/{project}/_apis/pipelines/{pipeline}/runs",
                post(start),
            )
            .route(
                "/{org}/{project}/_apis/pipelines/{pipeline}/runs/{run}",
                get(status),
            )
            .with_state(Arc::clone(&fake));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (fake, format!("http://{addr}"))
    }

    fn step(pipeline: &str, timeout: &str) -> StepInput {
        let params = [
            ("deployer", "azure_devops"),
            ("organization", "acme"),
            ("project", "Shop Api"),
            ("pipeline_id", pipeline),
            ("pat_secret", "AZDO_PAT"),
            ("timeout_seconds", timeout),
            ("environment", "production"),
            ("image", "acme.azurecr.io/shop/api"),
            ("tag", "a1b2c3d"),
            ("digest", ""),
            ("version", "v1.4.2"),
        ];
        StepInput {
            key: "deploy".into(),
            title: "Deploy to production".into(),
            action_type: "deploy.production".into(),
            adapter: "azure_devops".into(),
            params: params
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            attempt: 1,
        }
    }

    fn context(pat: &str) -> (RunContext, tokio::sync::mpsc::Receiver<(String, String)>) {
        let (ptx, progress) = tokio::sync::mpsc::channel(16);
        let redactor = Redactor::default();
        let ctx = RunContext {
            outputs: BTreeMap::from([("digest".to_owned(), "sha256:feed".to_owned())]),
            secrets: StaticSecrets::with(&[("AZDO_PAT", pat)]).secrets(&redactor),
            logs: LogSink::default().with_redactor(redactor),
            progress: OutputSink::new(ptx),
            ..Default::default()
        };
        (ctx, progress)
    }

    fn adapter(base: &str) -> AzureDevopsAdapter {
        AzureDevopsAdapter::new(Net::new(NetPolicy::ALLOW_PRIVATE), base)
            .with_poll(Duration::from_millis(10))
    }

    #[tokio::test]
    async fn starts_the_pipeline_with_the_release_and_follows_the_run() {
        let (fake, base) = serve("succeeded", false).await;
        let (ctx, mut progress) = context(PAT);
        let outcome = adapter(&base).run(&step("12", ""), &ctx).await;
        let StepOutcome::Succeeded { output, outputs } = outcome else {
            panic!("expected success: {outcome:?}");
        };
        let web = "https://dev.azure.com/acme/Shop%20Api/_build/results?buildId=501";
        assert_eq!(outputs["external_url"], web);
        assert!(output.contains("run 501 succeeded"), "{output}");
        assert_eq!(
            *fake.started.lock().unwrap(),
            [json!({"templateParameters": {
                "environment": "production",
                "image": "acme.azurecr.io/shop/api",
                "digest": "sha256:feed",
                "version": "v1.4.2",
            }})]
        );
        assert_eq!(fake.polls.load(Ordering::SeqCst), 3);
        assert_eq!(
            progress.try_recv().unwrap(),
            ("external_url".to_owned(), web.to_owned())
        );
    }

    #[tokio::test]
    async fn failed_runs_timeouts_bad_tokens_and_missing_pipelines_are_reported() {
        let (_, base) = serve("failed", false).await;
        let (ctx, _progress) = context(PAT);
        let outcome = adapter(&base).run(&step("12", ""), &ctx).await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(
            raw_output.contains("run 501 finished failed"),
            "{raw_output}"
        );

        let (_, base) = serve("succeeded", true).await;
        let outcome = adapter(&base).run(&step("12", "1"), &ctx).await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_TIMEOUT);
        assert!(
            raw_output.contains("still inProgress after 1s"),
            "{raw_output}"
        );

        let (fake, base) = serve("succeeded", false).await;
        let (wrong, _progress) = context("an-expired-token-0000");
        let outcome = adapter(&base).run(&step("12", ""), &wrong).await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(
            raw_output.contains("refused the token in the secret AZDO_PAT"),
            "{raw_output}"
        );
        assert!(!raw_output.contains("an-expired-token-0000"));
        assert!(fake.started.lock().unwrap().is_empty());

        let outcome = adapter(&base).run(&step("99", ""), &ctx).await;
        let StepOutcome::Failed { raw_output, .. } = outcome else {
            panic!("expected a failure");
        };
        assert!(
            raw_output.contains("Pipeline 99 was not found in acme/Shop Api"),
            "{raw_output}"
        );
    }

    #[test]
    fn urls_escape_their_segments_and_carry_the_api_version() {
        let adapter = AzureDevopsAdapter::new(Net::new(NetPolicy::PUBLIC_ONLY), DEFAULT_BASE);
        assert_eq!(
            adapter
                .runs_url("acme", "Shop Api", "12", None)
                .unwrap()
                .as_str(),
            "https://dev.azure.com/acme/Shop%20Api/_apis/pipelines/12/runs?api-version=7.1"
        );
        assert_eq!(
            adapter
                .runs_url("acme", "a/b", "12", Some(501))
                .unwrap()
                .as_str(),
            "https://dev.azure.com/acme/a%2Fb/_apis/pipelines/12/runs/501?api-version=7.1"
        );
    }
}
