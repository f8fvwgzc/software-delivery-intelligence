//! Environment-driven configuration helpers. Every service reads its
//! settings from the environment so the same binary runs locally and in
//! Docker without a config file.

use std::{env, net::SocketAddr, str::FromStr};

use anyhow::{Context, Result};

/// Returns the variable, or `default` when it is unset or empty.
pub fn env_or(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

/// Returns the variable when it is set and not empty.
pub fn env_opt(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

/// Parses the variable, falling back to `default` when it is unset.
pub fn env_parse<T>(key: &str, default: T) -> Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env_opt(key) {
        Some(raw) => raw
            .parse()
            .with_context(|| format!("{key} has an invalid value: {raw}")),
        None => Ok(default),
    }
}

/// Listen address from `key`, e.g. `0.0.0.0:50051`.
pub fn listen_addr(key: &str, default: &str) -> Result<SocketAddr> {
    let raw = env_or(key, default);
    raw.parse()
        .with_context(|| format!("{key} is not a socket address: {raw}"))
}

pub const DEFAULT_DATABASE_URL: &str = "postgres://sdi:sdi@127.0.0.1:5440/sdi";
pub const DEFAULT_KNOWLEDGE_URL: &str = "http://127.0.0.1:50051";
pub const DEFAULT_INTELLIGENCE_URL: &str = "http://127.0.0.1:50052";
pub const DEFAULT_EXECUTION_URL: &str = "http://127.0.0.1:50053";
pub const DEFAULT_IDENTITY_URL: &str = "http://127.0.0.1:50054";
/// Where the browser reaches the app in development (`SDI_PUBLIC_URL`).
/// `localhost`, not `127.0.0.1`: the Vite dev server binds the name, which
/// on current Node resolves to the IPv6 loopback only, so a redirect to the
/// IPv4 literal would land on a closed port.
pub const DEFAULT_PUBLIC_URL: &str = "http://localhost:5173";

pub fn database_url() -> String {
    env_or("DATABASE_URL", DEFAULT_DATABASE_URL)
}

pub fn knowledge_url() -> String {
    env_or("KNOWLEDGE_GRPC_URL", DEFAULT_KNOWLEDGE_URL)
}

pub fn intelligence_url() -> String {
    env_or("INTELLIGENCE_GRPC_URL", DEFAULT_INTELLIGENCE_URL)
}

pub fn execution_url() -> String {
    env_or("EXECUTION_GRPC_URL", DEFAULT_EXECUTION_URL)
}

pub fn identity_url() -> String {
    env_or("IDENTITY_GRPC_URL", DEFAULT_IDENTITY_URL)
}

/// `SDI_PUBLIC_URL` without a trailing slash: the origin the browser uses,
/// what redirects are made absolute against and what decides whether the
/// session cookie is `Secure` (CONTRACT.md §18).
pub fn public_url() -> String {
    env_or("SDI_PUBLIC_URL", DEFAULT_PUBLIC_URL)
        .trim_end_matches('/')
        .to_owned()
}

/// `SHUTDOWN_GRACE_SECS` (default 20): how long a service waits for its
/// tracked background tasks once the listener closed.
pub fn shutdown_grace() -> std::time::Duration {
    std::time::Duration::from_secs(env_parse("SHUTDOWN_GRACE_SECS", 20).unwrap_or(20))
}
