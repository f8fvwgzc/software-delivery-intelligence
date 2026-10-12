//! Typed settings. Read once at startup so a bad environment fails fast.

use std::net::SocketAddr;

use anyhow::Result;
use sdi_common::config;

#[derive(Debug, Clone)]
pub struct Settings {
    pub listen: SocketAddr,
    pub database_url: String,
    /// Dev-mode GitHub token: used for every workspace while the identity
    /// service cannot be reached (CONTRACT.md §18). `None` means a
    /// workspace without identity-minted access answers "not configured".
    pub github_token: Option<String>,
    /// Where workspace tokens, data controls and the audit log live.
    pub identity_url: String,
    /// Where the `.sdi.yml` environments are synced after a scan
    /// (CONTRACT.md §22).
    pub execution_url: String,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            listen: config::listen_addr("KNOWLEDGE_LISTEN", "0.0.0.0:50051")?,
            database_url: config::database_url(),
            github_token: config::env_opt("GITHUB_TOKEN"),
            identity_url: config::identity_url(),
            execution_url: config::execution_url(),
        })
    }
}
