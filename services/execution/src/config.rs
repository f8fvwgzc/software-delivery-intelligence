//! Typed settings. Everything comes from the environment (CONTRACT.md §3).

use std::net::SocketAddr;

use anyhow::Result;
use sdi_common::config;

use crate::adapters::simulated::DEFAULT_DELAY_MS;

pub const SCHEMA: &str = "execution";

#[derive(Debug, Clone)]
pub struct Settings {
    pub listen: SocketAddr,
    pub database_url: String,
    pub knowledge_url: String,
    /// Floor of the simulated adapter's per-step delay. Tests set it to 1.
    pub sim_delay_ms: u64,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            listen: config::listen_addr("EXECUTION_LISTEN", "0.0.0.0:50053")?,
            database_url: config::database_url(),
            knowledge_url: config::knowledge_url(),
            sim_delay_ms: config::env_parse("EXECUTION_SIM_DELAY_MS", DEFAULT_DELAY_MS)?,
        })
    }
}
