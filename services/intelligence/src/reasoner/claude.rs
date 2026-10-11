//! Anthropic Messages API over raw HTTP (no official Rust SDK). Any failure,
//! including a refusal, falls back to the heuristic draft so the user always
//! gets an answer.

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{Reasoner, diagrams::RefineRequest};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 800;

const SYSTEM_PROMPT: &str = "You rewrite a delivery assistant's templated reply into clear, natural prose for an \
engineer. You are given a JSON context (ranked options with scores, evidence, the effects of the user's message, \
any recorded decision) and the templated draft. Rules: use only facts, numbers, option names and evidence present \
in the context or the draft; never invent services, costs, scores, technologies or history; keep every score and \
evidence reference that the draft cites; if the recommendation changed, say so explicitly with the before and after \
scores; keep it under 250 words; plain text or light Markdown, no preamble.";

pub struct ClaudeReasoner {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
struct MessageRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<ChatMessage<'a>>,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: String,
}

#[derive(Deserialize)]
struct MessageResponse {
    #[serde(default)]
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

impl ClaudeReasoner {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
            api_key,
            model,
        }
    }

    async fn rewrite(&self, draft: &str, context: &serde_json::Value) -> anyhow::Result<String> {
        self.complete(
            SYSTEM_PROMPT,
            format!(
                "Structured context (JSON):\n{}\n\nTemplated draft:\n{}\n\nRewrite the draft as prose.",
                serde_json::to_string(context)?,
                draft
            ),
            MAX_TOKENS,
        )
        .await
    }

    /// One user turn against the Messages API; the text blocks joined.
    async fn complete(
        &self,
        system: &str,
        content: String,
        max_tokens: u32,
    ) -> anyhow::Result<String> {
        let body = MessageRequest {
            model: &self.model,
            max_tokens,
            system,
            messages: vec![ChatMessage {
                role: "user",
                content,
            }],
        };
        let response = self
            .http
            .post(ENDPOINT)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "anthropic returned {status}: {}",
                text.chars().take(300).collect::<String>()
            );
        }
        let message: MessageResponse = response.json().await?;
        if message.stop_reason.as_deref() == Some("refusal") {
            anyhow::bail!("anthropic refused the request");
        }
        let text: String = message
            .content
            .iter()
            .filter(|b| b.kind == "text")
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            anyhow::bail!("anthropic returned no text");
        }
        Ok(text.trim().to_owned())
    }
}

#[async_trait]
impl Reasoner for ClaudeReasoner {
    fn name(&self) -> &'static str {
        "claude"
    }

    async fn write_reply(&self, draft: &str, context: &serde_json::Value) -> String {
        match self.rewrite(draft, context).await {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!(%error, "claude reasoner failed; using heuristic reply");
                draft.to_owned()
            }
        }
    }

    async fn refine_diagram(&self, request: &RefineRequest) -> Option<String> {
        match self
            .complete(
                super::diagrams::SYSTEM_PROMPT,
                request.user_message(),
                super::diagrams::MAX_TOKENS,
            )
            .await
        {
            Ok(text) => Some(text),
            Err(error) => {
                tracing::warn!(%error, "claude reasoner failed; keeping the templated diagram text");
                None
            }
        }
    }
}
