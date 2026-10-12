//! Typed settings. Everything comes from the environment (CONTRACT.md §3).

use std::net::SocketAddr;

use anyhow::Result;
use sdi_common::config;

pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";

/// The reasoner's environment fallbacks (CONTRACT.md §3, §19): provider,
/// endpoint, key and model, used only for a workspace whose data controls
/// allow the reasoner and that configured nothing of its own. The
/// `ANTHROPIC_*` names are kept as aliases of `LLM_API_KEY` / `LLM_MODEL`.
#[derive(Clone, Default)]
pub struct ReasonerEnv {
    /// `LLM_PROVIDER`: "anthropic" (default) | "openai_compatible".
    pub provider: String,
    /// `LLM_BASE_URL`: OpenAI-compatible endpoint (Ollama, vLLM, Azure, ...).
    pub base_url: String,
    /// `LLM_API_KEY`, else `ANTHROPIC_API_KEY`.
    pub api_key: Option<String>,
    /// `LLM_MODEL`, else `ANTHROPIC_MODEL`, else [`DEFAULT_MODEL`].
    pub model: String,
}

impl ReasonerEnv {
    pub fn from_env() -> Self {
        Self {
            provider: config::env_or("LLM_PROVIDER", ""),
            base_url: config::env_or("LLM_BASE_URL", ""),
            api_key: config::env_opt("LLM_API_KEY")
                .or_else(|| config::env_opt("ANTHROPIC_API_KEY")),
            model: config::env_opt("LLM_MODEL")
                .unwrap_or_else(|| config::env_or("ANTHROPIC_MODEL", DEFAULT_MODEL)),
        }
    }
}

#[derive(Clone)]
pub struct Settings {
    pub listen: SocketAddr,
    pub database_url: String,
    pub knowledge_url: String,
    pub execution_url: String,
    pub identity_url: String,
    /// Optional dev fallback for the LLM reasoner: used only for a
    /// workspace whose data controls allow the reasoner and that stored no
    /// key of its own (CONTRACT.md §19).
    pub reasoner: ReasonerEnv,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            listen: config::listen_addr("INTELLIGENCE_LISTEN", "0.0.0.0:50052")?,
            database_url: config::database_url(),
            knowledge_url: config::knowledge_url(),
            execution_url: config::execution_url(),
            identity_url: config::identity_url(),
            reasoner: ReasonerEnv::from_env(),
        })
    }
}
