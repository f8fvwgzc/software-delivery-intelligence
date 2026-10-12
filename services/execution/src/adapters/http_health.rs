//! The health verifier of a deployment run (CONTRACT.md §22,
//! `deploy.verify_health`): polls the environment's `url + health_path`
//! until it answers 2xx or `health_timeout_seconds` (default 120) pass.
//! An environment without a URL has nothing to ask, so the step succeeds
//! saying it skipped. The same probe backs the pipeline view's live health
//! (one request, three seconds).

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use reqwest::{Method, StatusCode};

use super::{
    Adapter, RunContext, StepInput, StepOutcome, TOOL_HTTP,
    net::{self, Net},
};
use crate::domain::diagnosis;

pub const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
/// Longest a health check may be asked to wait.
pub const MAX_TIMEOUT_SECONDS: u64 = 3600;
const POLL: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
pub const SKIPPED_NO_URL: &str = "skipped: the environment has no URL to check";

/// `url` + `health_path`, with exactly one slash between them.
pub fn health_url(url: &str, health_path: &str) -> String {
    let base = url.trim().trim_end_matches('/');
    let path = health_path.trim();
    if path.is_empty() || path == "/" {
        format!("{base}/")
    } else {
        format!("{base}/{}", path.trim_start_matches('/'))
    }
}

/// One GET. `Ok` carries the status the target answered with, `Err` why no
/// answer came (including a URL the SSRF guard refuses).
pub async fn probe(net: &Net, url: &str, timeout: Duration) -> Result<StatusCode, String> {
    let response = net
        .request(Method::GET, url, timeout)
        .await?
        .send()
        .await
        .map_err(net::transport_error)?;
    Ok(response.status())
}

pub struct HttpHealthAdapter {
    net: Net,
    poll: Duration,
}

impl HttpHealthAdapter {
    pub fn new(net: Net) -> Self {
        Self { net, poll: POLL }
    }

    /// A shorter pause between polls (tests).
    #[cfg(test)]
    pub fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    async fn verify(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        let param = |key: &str| step.params.get(key).map(|v| v.trim()).unwrap_or_default();
        // A local Docker deployment only learns its address when it starts.
        let base = match param("url") {
            "" => ctx
                .outputs
                .get("container_url")
                .map(|url| url.trim())
                .unwrap_or_default(),
            url => url,
        };
        if base.is_empty() {
            ctx.logs.line(SKIPPED_NO_URL);
            return StepOutcome::Succeeded {
                output: SKIPPED_NO_URL.to_owned(),
                outputs: BTreeMap::from([("health".to_owned(), "unknown".to_owned())]),
            };
        }
        let url = health_url(base, param("health_path"));
        let limit = param("health_timeout_seconds")
            .parse::<u64>()
            .ok()
            .filter(|s| *s > 0)
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .min(MAX_TIMEOUT_SECONDS);
        // A URL the guard refuses will never pass: say so once, now.
        if let Err(reason) = self.net.check(&url).await {
            return StepOutcome::failed(
                diagnosis::HEALTH_CHECK_FAILED,
                format!("the health URL cannot be checked: {reason}"),
            );
        }

        let started = tokio::time::Instant::now();
        let deadline = started + Duration::from_secs(limit);
        let mut polls = 0u32;
        let mut last;
        loop {
            polls += 1;
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let timeout = REQUEST_TIMEOUT.min(remaining.max(Duration::from_secs(1)));
            match probe(&self.net, &url, timeout).await {
                Ok(status) if status.is_success() => {
                    ctx.logs
                        .line(format!("GET {url} -> {status} after {polls} polls"));
                    return StepOutcome::Succeeded {
                        output: format!(
                            "healthy: GET {url} -> {status} ({:.1}s, {polls} polls)",
                            started.elapsed().as_secs_f64()
                        ),
                        outputs: BTreeMap::from([("health".to_owned(), "healthy".to_owned())]),
                    };
                }
                Ok(status) => last = format!("GET {url} -> {status}"),
                Err(error) => last = format!("GET {url} failed: {error}"),
            }
            if polls == 1 || polls.is_multiple_of(10) {
                ctx.logs.line(format!("{last} (poll {polls})"));
            }
            if tokio::time::Instant::now() + self.poll >= deadline {
                break;
            }
            tokio::time::sleep(self.poll).await;
        }
        ctx.logs.line(format!("{last}; giving up after {limit}s"));
        StepOutcome::failed(
            diagnosis::HEALTH_CHECK_FAILED,
            format!("{last} after {polls} polls ({limit}s)"),
        )
    }
}

#[async_trait]
impl Adapter for HttpHealthAdapter {
    fn name(&self) -> &'static str {
        TOOL_HTTP
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        if step.action_type != super::VERIFY_HEALTH {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("the http adapter cannot run {}", step.action_type),
            );
        }
        self.verify(step, ctx).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{Router, extract::State, http::StatusCode as HttpStatus, routing::get};

    use super::*;
    use crate::adapters::net::NetPolicy;

    /// Answers 503 for the first `failures` requests, then 200.
    async fn serve(failures: usize) -> (Arc<AtomicUsize>, String) {
        let hits = Arc::new(AtomicUsize::new(0));
        async fn health(
            State((hits, failures)): State<(Arc<AtomicUsize>, usize)>,
        ) -> (HttpStatus, &'static str) {
            if hits.fetch_add(1, Ordering::SeqCst) < failures {
                (HttpStatus::SERVICE_UNAVAILABLE, "starting")
            } else {
                (HttpStatus::OK, "ok")
            }
        }
        let app = Router::new()
            .route("/healthz", get(health))
            .with_state((Arc::clone(&hits), failures));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (hits, format!("http://{addr}"))
    }

    fn adapter() -> HttpHealthAdapter {
        HttpHealthAdapter::new(Net::new(NetPolicy::ALLOW_PRIVATE))
            .with_poll(Duration::from_millis(20))
    }

    fn step(url: &str, path: &str, timeout: &str) -> StepInput {
        StepInput {
            key: "verify_health".into(),
            title: "Verify health".into(),
            action_type: "deploy.verify_health".into(),
            adapter: "http".into(),
            params: BTreeMap::from([
                ("url".to_owned(), url.to_owned()),
                ("health_path".to_owned(), path.to_owned()),
                ("health_timeout_seconds".to_owned(), timeout.to_owned()),
            ]),
            attempt: 1,
        }
    }

    #[tokio::test]
    async fn polls_until_the_target_turns_healthy() {
        let (hits, url) = serve(3).await;
        let outcome = adapter()
            .run(&step(&url, "healthz", "30"), &RunContext::default())
            .await;
        let StepOutcome::Succeeded { output, outputs } = outcome else {
            panic!("expected success: {outcome:?}");
        };
        assert_eq!(outputs["health"], "healthy");
        assert!(output.contains("4 polls"), "{output}");
        assert_eq!(hits.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn a_target_that_stays_down_fails_with_the_last_answer() {
        let (_, url) = serve(usize::MAX).await;
        let outcome = adapter()
            .run(&step(&url, "/healthz", "1"), &RunContext::default())
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure: {outcome:?}");
        };
        assert_eq!(code, diagnosis::HEALTH_CHECK_FAILED);
        assert!(
            raw_output.contains("/healthz -> 503 Service Unavailable after"),
            "{raw_output}"
        );
    }

    #[tokio::test]
    async fn no_url_is_a_skip_and_a_container_url_is_used_when_there_is_one() {
        let outcome = adapter()
            .run(&step("", "/healthz", ""), &RunContext::default())
            .await;
        assert_eq!(
            outcome,
            StepOutcome::Succeeded {
                output: SKIPPED_NO_URL.into(),
                outputs: BTreeMap::from([("health".to_owned(), "unknown".to_owned())]),
            }
        );
        let (hits, url) = serve(0).await;
        let ctx = RunContext {
            outputs: BTreeMap::from([("container_url".to_owned(), url)]),
            ..Default::default()
        };
        let outcome = adapter().run(&step("", "/healthz", ""), &ctx).await;
        assert!(
            matches!(outcome, StepOutcome::Succeeded { .. }),
            "{outcome:?}"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn private_health_urls_are_refused_by_default() {
        let (hits, url) = serve(0).await;
        let guarded = HttpHealthAdapter::new(Net::new(NetPolicy::PUBLIC_ONLY));
        let outcome = guarded
            .run(&step(&url, "/healthz", "5"), &RunContext::default())
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::HEALTH_CHECK_FAILED);
        assert!(raw_output.contains("must use https"), "{raw_output}");
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        // The metadata endpoint is refused whatever the scheme.
        let outcome = guarded
            .run(
                &step("https://169.254.169.254", "/latest/meta-data/", "5"),
                &RunContext::default(),
            )
            .await;
        let StepOutcome::Failed { raw_output, .. } = outcome else {
            panic!("expected a failure");
        };
        assert!(raw_output.contains("link-local"), "{raw_output}");
    }

    #[test]
    fn urls_and_paths_join_with_one_slash() {
        assert_eq!(
            health_url("https://a.example", "/health"),
            "https://a.example/health"
        );
        assert_eq!(
            health_url("https://a.example/", "health"),
            "https://a.example/health"
        );
        assert_eq!(
            health_url("https://a.example/app/", "/"),
            "https://a.example/app/"
        );
        assert_eq!(health_url("https://a.example", ""), "https://a.example/");
    }
}
