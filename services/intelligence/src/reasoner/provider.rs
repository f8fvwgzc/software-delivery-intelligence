//! Any LLM behind one of two wire protocols (CONTRACT.md §19): the
//! Anthropic Messages API, or OpenAI-compatible chat completions (OpenAI,
//! Azure OpenAI, Ollama, vLLM, OpenRouter, Gemini's OpenAI endpoint). One
//! [`ReasonerRequest`] shape, a context trimmed to the token budget, capped
//! replies, JSON mode where the protocol has one, retries with backoff on
//! 429/5xx, a 20 s timeout, and a per-workspace daily token cap kept in the
//! usage ledger. Every failure, including the cap, falls back to the
//! templated draft so the user always gets an answer.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    Reasoner, ReasonerRequest,
    budget::{self, CONTEXT_TOKENS},
    diagrams::RefineRequest,
};
use crate::db::usage::UsageLedger;

pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const TIMEOUT: Duration = Duration::from_secs(20);
const ATTEMPTS: u32 = 3;
const RETRY_BASE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Provider {
    #[default]
    Anthropic,
    OpenAiCompatible {
        base_url: String,
    },
}

impl Provider {
    /// `name` as the settings or `LLM_PROVIDER` spell it; `base_url` from
    /// the settings or `LLM_BASE_URL`. Unknown names with a base URL are
    /// OpenAI-compatible (that is what Ollama, vLLM and friends speak);
    /// everything else is Anthropic.
    pub fn parse(name: &str, base_url: &str) -> Self {
        let base_url = base_url.trim().trim_end_matches('/').to_owned();
        match name.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "" | "anthropic" | "claude" => Provider::Anthropic,
            "openai" | "openai_compatible" | "azure" | "azure_openai" | "ollama" | "vllm"
            | "openrouter" | "gemini" | "compatible" => Provider::OpenAiCompatible {
                base_url: if base_url.is_empty() {
                    OPENAI_BASE_URL.to_owned()
                } else {
                    base_url
                },
            },
            _ if !base_url.is_empty() => Provider::OpenAiCompatible { base_url },
            _ => Provider::Anthropic,
        }
    }

    /// The stored reasoner name: "claude" for Anthropic, "llm" otherwise.
    pub fn reasoner_name(&self) -> &'static str {
        match self {
            Provider::Anthropic => "claude",
            Provider::OpenAiCompatible { .. } => "llm",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAiCompatible { .. } => "openai_compatible",
        }
    }
}

/// `https://x/v1` → `https://x/v1/chat/completions`; an Azure deployment
/// URL keeps its `api-version` query; a URL that already names the
/// endpoint is used as is.
pub fn chat_completions_url(base_url: &str) -> String {
    let (path, query) = match base_url.split_once('?') {
        Some((p, q)) => (p.trim_end_matches('/'), Some(q)),
        None => (base_url.trim_end_matches('/'), None),
    };
    let path = if path.ends_with("/chat/completions") {
        path.to_owned()
    } else {
        format!("{path}/chat/completions")
    };
    match query {
        Some(q) if !q.is_empty() => format!("{path}?{q}"),
        _ => path,
    }
}

fn is_azure(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains(".openai.azure.com") || lower.contains("api-version=")
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// A reply with the usage the provider reported (estimated when it did not).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub text: String,
    pub usage: Usage,
}

pub struct LlmReasoner {
    http: reqwest::Client,
    provider: Provider,
    api_key: String,
    model: String,
    workspace_id: Uuid,
    daily_token_cap: u64,
    ledger: Arc<dyn UsageLedger>,
    anthropic_base_url: String,
    retry_base: Duration,
}

impl LlmReasoner {
    pub fn new(
        provider: Provider,
        api_key: String,
        model: String,
        workspace_id: Uuid,
        daily_token_cap: u64,
        ledger: Arc<dyn UsageLedger>,
    ) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .build()
                .expect("reqwest client"),
            provider,
            api_key,
            model,
            workspace_id,
            daily_token_cap,
            ledger,
            anthropic_base_url: ANTHROPIC_BASE_URL.to_owned(),
            retry_base: RETRY_BASE,
        }
    }

    /// Tests point the Anthropic protocol at a fake server.
    #[cfg(test)]
    pub fn with_anthropic_base_url(mut self, base_url: &str) -> Self {
        self.anthropic_base_url = base_url.trim_end_matches('/').to_owned();
        self
    }

    #[cfg(test)]
    pub fn with_retry_base(mut self, base: Duration) -> Self {
        self.retry_base = base;
        self
    }

    /// The user turn: the trimmed context, then the instruction.
    fn user_content(request: &ReasonerRequest) -> (String, bool) {
        let (context, trimmed) = budget::trim_context(&request.context_json, CONTEXT_TOKENS);
        let context = serde_json::to_string(&context).unwrap_or_default();
        (
            format!(
                "Structured context (JSON{}):\n{context}\n\n{}",
                if trimmed {
                    ", trimmed to the token budget"
                } else {
                    ""
                },
                request.instruction
            ),
            trimmed,
        )
    }

    /// One completion under the budget: refused before any call when the
    /// workspace's day is spent, recorded in the ledger after one.
    pub async fn complete(&self, request: &ReasonerRequest) -> anyhow::Result<Completion> {
        let (content, trimmed) = Self::user_content(request);
        if trimmed {
            tracing::debug!(workspace = %self.workspace_id, "reasoner context trimmed to the token budget");
        }
        let estimated_input =
            (budget::estimate_tokens(request.system) + budget::estimate_tokens(&content)) as u64;
        let used = self.ledger.used_today(self.workspace_id).await;
        let needed = estimated_input + u64::from(request.max_tokens);
        if used.saturating_add(needed) > self.daily_token_cap {
            anyhow::bail!(
                "daily token cap reached ({used} used of {} today; this call needs about {needed})",
                self.daily_token_cap
            );
        }
        let result = self.send(request, &content).await;
        match &result {
            Ok(completion) => {
                self.ledger
                    .record(
                        self.workspace_id,
                        completion.usage.input_tokens,
                        completion.usage.output_tokens,
                    )
                    .await;
            }
            Err(_) => {
                // A failed call still cost the request tokens on most
                // providers; count the estimate so retries cannot run away.
                self.ledger
                    .record(self.workspace_id, estimated_input, 0)
                    .await;
            }
        }
        result
    }

    async fn send(&self, request: &ReasonerRequest, content: &str) -> anyhow::Result<Completion> {
        let estimated_input =
            (budget::estimate_tokens(request.system) + budget::estimate_tokens(content)) as u64;
        let mut attempt = 0;
        loop {
            attempt += 1;
            let outcome = match &self.provider {
                Provider::Anthropic => self.anthropic(request, content).await,
                Provider::OpenAiCompatible { base_url } => {
                    self.openai(base_url, request, content).await
                }
            };
            match outcome {
                Ok(mut completion) => {
                    if completion.usage == Usage::default() {
                        completion.usage = Usage {
                            input_tokens: estimated_input,
                            output_tokens: budget::estimate_tokens(&completion.text) as u64,
                        };
                    }
                    return Ok(completion);
                }
                Err(Transient(message)) if attempt < ATTEMPTS => {
                    let delay = self.retry_base * 2u32.pow(attempt - 1);
                    tracing::debug!(attempt, %message, "reasoner call retried");
                    tokio::time::sleep(delay).await;
                }
                Err(Transient(message)) => anyhow::bail!("{message} (after {attempt} attempts)"),
                Err(Fatal(message)) => anyhow::bail!("{message}"),
            }
        }
    }

    async fn anthropic(
        &self,
        request: &ReasonerRequest,
        content: &str,
    ) -> Result<Completion, CallError> {
        #[derive(Deserialize)]
        struct Reply {
            #[serde(default)]
            content: Vec<Block>,
            #[serde(default)]
            stop_reason: Option<String>,
            #[serde(default)]
            usage: Option<AnthropicUsage>,
        }
        #[derive(Deserialize)]
        struct Block {
            #[serde(rename = "type")]
            kind: String,
            #[serde(default)]
            text: String,
        }
        #[derive(Deserialize)]
        struct AnthropicUsage {
            #[serde(default)]
            input_tokens: u64,
            #[serde(default)]
            output_tokens: u64,
        }
        let body = json!({
            "model": self.model,
            "max_tokens": request.max_tokens,
            "system": request.system,
            "messages": [{"role": "user", "content": content}],
        });
        let response = self
            .http
            .post(format!("{}/v1/messages", self.anthropic_base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(transport)?;
        let reply: Reply = parse_response("anthropic", response).await?;
        if reply.stop_reason.as_deref() == Some("refusal") {
            return Err(Fatal("anthropic refused the request".into()));
        }
        let text = reply
            .content
            .iter()
            .filter(|b| b.kind == "text")
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        finish(
            "anthropic",
            text,
            reply.usage.map(|u| Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
            }),
        )
    }

    async fn openai(
        &self,
        base_url: &str,
        request: &ReasonerRequest,
        content: &str,
    ) -> Result<Completion, CallError> {
        #[derive(Deserialize)]
        struct Reply {
            #[serde(default)]
            choices: Vec<Choice>,
            #[serde(default)]
            usage: Option<OpenAiUsage>,
        }
        #[derive(Deserialize)]
        struct Choice {
            #[serde(default)]
            message: Message,
            #[serde(default)]
            finish_reason: Option<String>,
        }
        #[derive(Deserialize, Default)]
        struct Message {
            #[serde(default)]
            content: Option<Value>,
        }
        #[derive(Deserialize)]
        struct OpenAiUsage {
            #[serde(default)]
            prompt_tokens: u64,
            #[serde(default)]
            completion_tokens: u64,
        }
        let mut body = json!({
            "model": self.model,
            "max_tokens": request.max_tokens,
            "temperature": 0.2,
            "messages": [
                {"role": "system", "content": request.system},
                {"role": "user", "content": content},
            ],
        });
        if request.json {
            body["response_format"] = json!({"type": "json_object"});
        }
        let url = chat_completions_url(base_url);
        let mut call = self
            .http
            .post(&url)
            .header("content-type", "application/json");
        if !self.api_key.is_empty() {
            call = call.header("authorization", format!("Bearer {}", self.api_key));
            if is_azure(&url) {
                call = call.header("api-key", &self.api_key);
            }
        }
        let response = call.json(&body).send().await.map_err(transport)?;
        let reply: Reply = parse_response("openai-compatible", response).await?;
        let choice = reply
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| Fatal("openai-compatible reply has no choices".into()))?;
        if choice.finish_reason.as_deref() == Some("content_filter") {
            return Err(Fatal(
                "openai-compatible endpoint filtered the reply".into(),
            ));
        }
        // `content` is a string, or an array of `{type: "text", text}` parts.
        let text = match choice.message.content {
            Some(Value::String(s)) => s,
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        finish(
            "openai-compatible",
            text,
            reply.usage.map(|u| Usage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            }),
        )
    }
}

enum CallError {
    /// 429, 5xx, transport: worth a retry.
    Transient(String),
    Fatal(String),
}
use CallError::{Fatal, Transient};

fn transport(error: reqwest::Error) -> CallError {
    Transient(format!("request failed: {error}"))
}

async fn parse_response<T: serde::de::DeserializeOwned>(
    provider: &str,
    response: reqwest::Response,
) -> Result<T, CallError> {
    let status = response.status();
    if status.as_u16() == 429 || status.is_server_error() {
        let text = response.text().await.unwrap_or_default();
        return Err(Transient(format!(
            "{provider} returned {status}: {}",
            text.chars().take(200).collect::<String>()
        )));
    }
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(Fatal(format!(
            "{provider} returned {status}: {}",
            text.chars().take(300).collect::<String>()
        )));
    }
    response
        .json()
        .await
        .map_err(|e| Fatal(format!("{provider} reply is not JSON: {e}")))
}

fn finish(provider: &str, text: String, usage: Option<Usage>) -> Result<Completion, CallError> {
    if text.trim().is_empty() {
        return Err(Fatal(format!("{provider} returned no text")));
    }
    Ok(Completion {
        text: text.trim().to_owned(),
        usage: usage.unwrap_or_default(),
    })
}

#[async_trait]
impl Reasoner for LlmReasoner {
    fn name(&self) -> &'static str {
        self.provider.reasoner_name()
    }

    async fn write_reply(&self, draft: &str, context: &Value) -> String {
        let request = ReasonerRequest {
            system: super::REPLY_SYSTEM_PROMPT,
            context_json: context.clone(),
            instruction: format!("Templated draft:\n{draft}\n\nRewrite the draft as prose."),
            max_tokens: budget::NEGOTIATION_MAX_TOKENS,
            json: false,
        };
        match self.complete(&request).await {
            Ok(completion) => completion.text,
            Err(error) => {
                tracing::warn!(%error, "reasoner failed; using heuristic reply");
                draft.to_owned()
            }
        }
    }

    async fn write_review_summary(&self, draft: &str, context: &Value) -> String {
        let request = ReasonerRequest {
            system: super::REVIEW_SYSTEM_PROMPT,
            context_json: context.clone(),
            instruction: format!("Templated walkthrough:\n{draft}\n\nRewrite the walkthrough."),
            max_tokens: budget::SUMMARY_MAX_TOKENS,
            json: false,
        };
        match self.complete(&request).await {
            Ok(completion) if completion.text.contains("## Intent") => completion.text,
            Ok(_) => {
                tracing::warn!(
                    "reasoner dropped the walkthrough sections; keeping the templated summary"
                );
                draft.to_owned()
            }
            Err(error) => {
                tracing::warn!(%error, "reasoner failed; keeping the templated summary");
                draft.to_owned()
            }
        }
    }

    async fn refine_diagram(&self, request: &RefineRequest) -> Option<String> {
        let request = ReasonerRequest {
            system: super::diagrams::SYSTEM_PROMPT,
            context_json: request.context_json(),
            instruction: request.instruction(),
            max_tokens: budget::DIAGRAM_MAX_TOKENS,
            json: true,
        };
        match self.complete(&request).await {
            Ok(completion) => Some(completion.text),
            Err(error) => {
                tracing::warn!(%error, "reasoner failed; keeping the templated diagram text");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::{Arc, Mutex},
    };

    use axum::{
        Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use serde_json::json;

    use super::*;
    use crate::db::usage::MemoryLedger;

    /// What the fake provider saw: headers and bodies, in order, and the
    /// scripted answers it hands out (status, JSON), one per request.
    #[derive(Default)]
    struct Seen {
        requests: Vec<(String, HeaderMap, Value)>,
        answers: std::collections::VecDeque<(StatusCode, Value)>,
    }

    type Shared = Arc<Mutex<Seen>>;

    async fn handle(
        State(seen): State<Shared>,
        headers: HeaderMap,
        uri: axum::http::Uri,
        axum::Json(body): axum::Json<Value>,
    ) -> (StatusCode, axum::Json<Value>) {
        let mut seen = seen.lock().unwrap();
        seen.requests.push((uri.to_string(), headers, body));
        let (status, reply) = seen
            .answers
            .pop_front()
            .unwrap_or((StatusCode::OK, json!({})));
        (status, axum::Json(reply))
    }

    async fn fake_server(answers: Vec<(StatusCode, Value)>) -> (String, Shared) {
        let seen: Shared = Arc::new(Mutex::new(Seen {
            requests: Vec::new(),
            answers: answers.into_iter().collect(),
        }));
        let app = Router::new()
            .route("/v1/messages", post(handle))
            .route("/v1/chat/completions", post(handle))
            .route("/openai/deployments/{d}/chat/completions", post(handle))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), seen)
    }

    fn anthropic_reply(text: &str) -> Value {
        json!({"content": [{"type": "text", "text": text}], "stop_reason": "end_turn",
               "usage": {"input_tokens": 120, "output_tokens": 30}})
    }

    fn openai_reply(text: &str) -> Value {
        json!({"choices": [{"message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
               "usage": {"prompt_tokens": 100, "completion_tokens": 20}})
    }

    fn request(context: Value) -> ReasonerRequest {
        ReasonerRequest {
            system: "sys",
            context_json: context,
            instruction: "Rewrite.".into(),
            max_tokens: 100,
            json: false,
        }
    }

    #[tokio::test]
    async fn anthropic_request_shape_auth_and_usage() {
        let (url, seen) = fake_server(vec![(StatusCode::OK, anthropic_reply("  prose  "))]).await;
        let ledger = Arc::new(MemoryLedger::default());
        let ws = Uuid::now_v7();
        let reasoner = LlmReasoner::new(
            Provider::Anthropic,
            "sk-test".into(),
            "claude-x".into(),
            ws,
            200_000,
            ledger.clone(),
        )
        .with_anthropic_base_url(&url);
        assert_eq!(reasoner.name(), "claude");
        let out = reasoner
            .complete(&request(json!({"options": [1]})))
            .await
            .unwrap();
        assert_eq!(out.text, "prose");
        assert_eq!(
            out.usage,
            Usage {
                input_tokens: 120,
                output_tokens: 30
            }
        );
        let seen = seen.lock().unwrap();
        let (path, headers, body) = &seen.requests[0];
        assert_eq!(path, "/v1/messages");
        assert_eq!(headers.get("x-api-key").unwrap(), "sk-test");
        assert_eq!(headers.get("anthropic-version").unwrap(), ANTHROPIC_VERSION);
        assert!(headers.get("authorization").is_none());
        assert_eq!(body["model"], "claude-x");
        assert_eq!(body["max_tokens"], 100);
        assert_eq!(body["system"], "sys");
        assert_eq!(body["messages"][0]["role"], "user");
        let content = body["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("{\"options\":[1]}"));
        assert!(content.ends_with("Rewrite."));
        assert_eq!(ledger.used.lock().unwrap()[&ws], (120, 30, 1));
    }

    #[tokio::test]
    async fn openai_compatible_request_shape_json_mode_and_azure_header() {
        let (url, seen) = fake_server(vec![
            (StatusCode::OK, openai_reply("{\"explanation\": \"x\"}")),
            (StatusCode::OK, openai_reply("plain")),
            (
                StatusCode::OK,
                json!({"choices": [{"message": {"content": [{"type": "text", "text": "parts"}]}}]}),
            ),
        ])
        .await;
        let ledger = Arc::new(MemoryLedger::default());
        let ws = Uuid::now_v7();
        let reasoner = LlmReasoner::new(
            Provider::parse("openai", &format!("{url}/v1")),
            "key-1".into(),
            "gpt-x".into(),
            ws,
            200_000,
            ledger.clone(),
        );
        assert_eq!(reasoner.name(), "llm");
        let mut req = request(json!({"diagram": {"nodes": []}}));
        req.json = true;
        let out = reasoner.complete(&req).await.unwrap();
        assert_eq!(out.text, "{\"explanation\": \"x\"}");
        assert_eq!(
            out.usage,
            Usage {
                input_tokens: 100,
                output_tokens: 20
            }
        );
        {
            let seen = seen.lock().unwrap();
            let (path, headers, body) = &seen.requests[0];
            assert_eq!(path, "/v1/chat/completions");
            assert_eq!(headers.get("authorization").unwrap(), "Bearer key-1");
            assert!(headers.get("api-key").is_none());
            assert_eq!(body["model"], "gpt-x");
            assert_eq!(body["max_tokens"], 100);
            assert_eq!(body["response_format"]["type"], "json_object");
            assert_eq!(body["messages"][0]["role"], "system");
            assert_eq!(body["messages"][0]["content"], "sys");
            assert_eq!(body["messages"][1]["role"], "user");
        }
        // Without JSON mode there is no response_format at all.
        let out = reasoner.complete(&request(json!({}))).await.unwrap();
        assert_eq!(out.text, "plain");
        assert!(
            seen.lock().unwrap().requests[1]
                .2
                .get("response_format")
                .is_none()
        );
        // Array content parts and missing usage (estimated instead).
        let out = reasoner.complete(&request(json!({}))).await.unwrap();
        assert_eq!(out.text, "parts");
        assert!(out.usage.input_tokens > 0 && out.usage.output_tokens > 0);

        // Azure: deployment URL keeps api-version, api-key header is added.
        let azure = LlmReasoner::new(
            Provider::parse(
                "azure",
                &format!("{url}/openai/deployments/dep?api-version=2024-10-21"),
            ),
            "az-key".into(),
            "dep".into(),
            ws,
            200_000,
            ledger.clone(),
        );
        seen.lock()
            .unwrap()
            .answers
            .push_back((StatusCode::OK, openai_reply("hi")));
        azure.complete(&request(json!({}))).await.unwrap();
        let seen = seen.lock().unwrap();
        let (path, headers, _) = seen.requests.last().unwrap();
        assert_eq!(
            path,
            "/openai/deployments/dep/chat/completions?api-version=2024-10-21"
        );
        assert_eq!(headers.get("api-key").unwrap(), "az-key");
        assert_eq!(headers.get("authorization").unwrap(), "Bearer az-key");
    }

    #[tokio::test]
    async fn context_is_trimmed_to_the_budget_before_sending() {
        let (url, seen) = fake_server(vec![(StatusCode::OK, anthropic_reply("ok"))]).await;
        let reasoner = LlmReasoner::new(
            Provider::Anthropic,
            "k".into(),
            "m".into(),
            Uuid::now_v7(),
            1_000_000,
            Arc::new(MemoryLedger::default()),
        )
        .with_anthropic_base_url(&url);
        let graph: Vec<Value> = (0..5000)
            .map(|i| json!({"n": i, "label": "node-label"}))
            .collect();
        let context = json!({"graph": graph, "findings": [{"rule": "r.x"}]});
        reasoner.complete(&request(context)).await.unwrap();
        let seen = seen.lock().unwrap();
        let content = seen.requests[0].2["messages"][0]["content"]
            .as_str()
            .unwrap();
        assert!(
            budget::estimate_tokens(content) <= CONTEXT_TOKENS + 100,
            "{}",
            content.len()
        );
        assert!(content.contains("trimmed to the token budget"));
        assert!(content.contains("\"findings\":[{\"rule\":\"r.x\"}]"));
        assert!(content.contains("\"_truncated\":[\"graph\"]"));
    }

    #[tokio::test]
    async fn daily_cap_refuses_before_calling_and_counts_failures() {
        let (url, seen) = fake_server(vec![(StatusCode::OK, anthropic_reply("ok"))]).await;
        let ledger = Arc::new(MemoryLedger::default());
        let ws = Uuid::now_v7();
        let reasoner = LlmReasoner::new(
            Provider::Anthropic,
            "k".into(),
            "m".into(),
            ws,
            300,
            ledger.clone(),
        )
        .with_anthropic_base_url(&url);
        // Fits: ~10 input tokens + 100 reply < 300.
        reasoner.complete(&request(json!({}))).await.unwrap();
        assert_eq!(ledger.used_today(ws).await, 150);
        // 150 used + ~110 needed = 260 < 300: still fits, and the ledger grows.
        seen.lock()
            .unwrap()
            .answers
            .push_back((StatusCode::OK, anthropic_reply("ok")));
        reasoner.complete(&request(json!({}))).await.unwrap();
        assert_eq!(ledger.used_today(ws).await, 300);
        // The cap is reached: refused without a request.
        let before = seen.lock().unwrap().requests.len();
        let error = reasoner.complete(&request(json!({}))).await.unwrap_err();
        assert!(error.to_string().contains("daily token cap"), "{error}");
        assert_eq!(seen.lock().unwrap().requests.len(), before);
        // The trait falls back to the draft.
        assert_eq!(reasoner.write_reply("draft", &json!({})).await, "draft");
    }

    #[tokio::test]
    async fn retries_on_429_and_5xx_then_gives_up_on_4xx() {
        let (url, seen) = fake_server(vec![
            (StatusCode::TOO_MANY_REQUESTS, json!({"error": "slow down"})),
            (StatusCode::BAD_GATEWAY, json!({"error": "bad"})),
            (StatusCode::OK, openai_reply("third time")),
            (StatusCode::BAD_REQUEST, json!({"error": "nope"})),
            (StatusCode::INTERNAL_SERVER_ERROR, json!({})),
            (StatusCode::INTERNAL_SERVER_ERROR, json!({})),
            (StatusCode::INTERNAL_SERVER_ERROR, json!({})),
        ])
        .await;
        let ledger = Arc::new(MemoryLedger::default());
        let reasoner = LlmReasoner::new(
            Provider::OpenAiCompatible {
                base_url: format!("{url}/v1"),
            },
            String::new(),
            "local".into(),
            Uuid::now_v7(),
            1_000_000,
            ledger,
        )
        .with_retry_base(Duration::from_millis(1));
        let out = reasoner.complete(&request(json!({}))).await.unwrap();
        assert_eq!(out.text, "third time");
        assert_eq!(seen.lock().unwrap().requests.len(), 3);
        // No key: no authorization header (Ollama-style).
        assert!(
            seen.lock().unwrap().requests[0]
                .1
                .get("authorization")
                .is_none()
        );
        let error = reasoner.complete(&request(json!({}))).await.unwrap_err();
        assert!(error.to_string().contains("400"), "{error}");
        assert_eq!(seen.lock().unwrap().requests.len(), 4);
        let error = reasoner.complete(&request(json!({}))).await.unwrap_err();
        assert!(error.to_string().contains("after 3 attempts"), "{error}");
        assert_eq!(seen.lock().unwrap().requests.len(), 7);
    }

    #[test]
    fn provider_parsing_and_urls() {
        assert_eq!(Provider::parse("", ""), Provider::Anthropic);
        assert_eq!(
            Provider::parse("Anthropic", "https://x"),
            Provider::Anthropic
        );
        assert_eq!(
            Provider::parse("openai_compatible", ""),
            Provider::OpenAiCompatible {
                base_url: OPENAI_BASE_URL.into()
            }
        );
        assert_eq!(
            Provider::parse("ollama", "http://ollama:11434/v1/"),
            Provider::OpenAiCompatible {
                base_url: "http://ollama:11434/v1".into()
            }
        );
        assert_eq!(
            Provider::parse("something-else", "http://vllm:8000/v1"),
            Provider::OpenAiCompatible {
                base_url: "http://vllm:8000/v1".into()
            }
        );
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://openrouter.ai/api/v1/chat/completions"),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url(
                "https://r.openai.azure.com/openai/deployments/d?api-version=2024-10-21"
            ),
            "https://r.openai.azure.com/openai/deployments/d/chat/completions?api-version=2024-10-21"
        );
        assert_eq!(
            chat_completions_url("https://generativelanguage.googleapis.com/v1beta/openai/"),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
    }
}
