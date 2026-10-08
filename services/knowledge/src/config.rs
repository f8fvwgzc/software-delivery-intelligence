//! Typed settings. Read once at startup so a bad environment fails fast.

use std::net::SocketAddr;

use anyhow::Result;
use sdi_common::config;

#[derive(Debug, Clone)]
pub struct Settings {
    pub listen: SocketAddr,
    pub database_url: String,
    /// Read-only GitHub token. `None` keeps the service up but every
    /// connector call answers "token not configured".
    pub github_token: Option<String>,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            listen: config::listen_addr("KNOWLEDGE_LISTEN", "0.0.0.0:50051")?,
            database_url: config::database_url(),
            github_token: config::env_opt("GITHUB_TOKEN"),
        })
    }
}
