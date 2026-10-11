//! Typed settings. Everything comes from the environment (CONTRACT.md §3).

use std::{net::SocketAddr, path::PathBuf};

use anyhow::Result;
use sdi_common::config;

use crate::adapters::{github::DEFAULT_API, simulated::DEFAULT_DELAY_MS};

pub const SCHEMA: &str = "execution";

#[derive(Clone)]
pub struct Settings {
    pub listen: SocketAddr,
    pub database_url: String,
    pub knowledge_url: String,
    /// The identity service, for the audit log (`IDENTITY_GRPC_URL`).
    pub identity_url: String,
    /// Floor of the simulated adapter's per-step delay. Tests set it to 1.
    pub sim_delay_ms: u64,
    /// Where the docker adapter keeps one workdir per execution (`SDI_RUN_DIR`).
    pub run_dir: PathBuf,
    /// The connected token (`GITHUB_TOKEN`); `None` disables the github
    /// adapter and withdraws the `github-token` identity.
    pub github_token: Option<String>,
    /// GitHub API base (`GITHUB_API_URL`); only tests and GHES change it.
    pub github_api_url: String,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            listen: config::listen_addr("EXECUTION_LISTEN", "0.0.0.0:50053")?,
            database_url: config::database_url(),
            knowledge_url: config::knowledge_url(),
            identity_url: config::identity_url(),
            sim_delay_ms: config::env_parse("EXECUTION_SIM_DELAY_MS", DEFAULT_DELAY_MS)?,
            run_dir: PathBuf::from(config::env_or("SDI_RUN_DIR", ".run")),
            github_token: config::env_opt("GITHUB_TOKEN").map(|t| t.trim().to_owned()),
            github_api_url: config::env_or("GITHUB_API_URL", DEFAULT_API),
        })
    }
}
