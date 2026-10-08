//! Typed settings. Everything comes from the environment (CONTRACT.md §3).

use std::net::SocketAddr;

use anyhow::Result;
use sdi_common::config;

pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";

#[derive(Debug, Clone)]
pub struct Settings {
    pub listen: SocketAddr,
    pub database_url: String,
    pub knowledge_url: String,
    pub execution_url: String,
    /// Optional: enables the `claude` reasoner.
    pub anthropic_api_key: Option<String>,
    pub anthropic_model: String,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            listen: config::listen_addr("INTELLIGENCE_LISTEN", "0.0.0.0:50052")?,
            database_url: config::database_url(),
            knowledge_url: config::knowledge_url(),
            execution_url: config::execution_url(),
            anthropic_api_key: config::env_opt("ANTHROPIC_API_KEY"),
            anthropic_model: config::env_or("ANTHROPIC_MODEL", DEFAULT_MODEL),
        })
    }
}
