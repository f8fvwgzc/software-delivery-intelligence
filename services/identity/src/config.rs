//! Typed settings. Everything comes from the environment (CONTRACT.md §3, §18).

use std::net::SocketAddr;

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use sdi_common::config;

pub const SCHEMA: &str = "identity";
/// The public GitHub API; every other host is a GitHub Enterprise Server
/// reached at `https://<host>/api/v3`.
pub const DEFAULT_GITHUB_API: &str = "https://api.github.com";
/// Host users of the public API sign in to.
pub const PUBLIC_HOST: &str = "github.com";

/// Registered GitHub OAuth app (individual sign-in).
#[derive(Debug, Clone)]
pub struct OAuthApp {
    pub client_id: String,
    pub client_secret: String,
}

/// Registered GitHub App (organisation installations).
#[derive(Debug, Clone)]
pub struct GithubApp {
    pub app_id: String,
    /// PEM text of the RSA private key.
    pub private_key_pem: String,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub listen: SocketAddr,
    pub database_url: String,
    /// `SDI_PUBLIC_URL`: where GitHub sends users back and what `return_to`
    /// is validated against.
    pub public_url: String,
    /// `SDI_SESSION_SECRET`, 32+ bytes.
    pub session_secret: Vec<u8>,
    pub oauth: Option<OAuthApp>,
    pub app: Option<GithubApp>,
    /// Dev mode (§18): no OAuth app and `SDI_DEV_LOGIN=1`.
    pub dev_login: bool,
    /// The dev-mode account's token (`GITHUB_TOKEN`).
    pub github_token: Option<String>,
    /// API base for github.com (`GITHUB_API_URL`; tests point it at a fake).
    pub github_api_url: String,
    /// Host name users of that API sign in to ("github.com").
    pub github_host: String,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        let oauth = match (
            config::env_opt("GITHUB_OAUTH_CLIENT_ID"),
            config::env_opt("GITHUB_OAUTH_CLIENT_SECRET"),
        ) {
            (Some(client_id), Some(client_secret)) => Some(OAuthApp {
                client_id,
                client_secret,
            }),
            (Some(_), None) => {
                bail!("GITHUB_OAUTH_CLIENT_SECRET is required with GITHUB_OAUTH_CLIENT_ID")
            }
            _ => None,
        };
        let app = match (
            config::env_opt("GITHUB_APP_ID"),
            config::env_opt("GITHUB_APP_PRIVATE_KEY"),
        ) {
            (Some(app_id), Some(key)) => Some(GithubApp {
                app_id,
                private_key_pem: decode_pem(&key)?,
            }),
            (Some(_), None) => bail!("GITHUB_APP_PRIVATE_KEY is required with GITHUB_APP_ID"),
            _ => None,
        };
        let dev_login = oauth.is_none() && config::env_or("SDI_DEV_LOGIN", "0") == "1";
        let session_secret = config::env_opt("SDI_SESSION_SECRET")
            .context("SDI_SESSION_SECRET is required (32+ bytes; `openssl rand -hex 32`)")?
            .into_bytes();
        if session_secret.len() < 32 {
            bail!("SDI_SESSION_SECRET must be at least 32 bytes");
        }
        let github_api_url = config::env_or("GITHUB_API_URL", DEFAULT_GITHUB_API)
            .trim_end_matches('/')
            .to_owned();
        Ok(Self {
            listen: config::listen_addr("IDENTITY_LISTEN", "0.0.0.0:50054")?,
            database_url: config::database_url(),
            public_url: config::public_url(),
            session_secret,
            oauth,
            app,
            dev_login,
            github_token: config::env_opt("GITHUB_TOKEN").map(|t| t.trim().to_owned()),
            github_host: host_of(&github_api_url),
            github_api_url,
        })
    }

    /// The `https://<host>` users are sent to for OAuth (`/login/oauth/...`
    /// lives on the web host, not under `/api/v3`, on GHES too).
    pub fn github_web_url(&self) -> String {
        github_web_url_of(&self.github_api_url, &self.github_host)
    }
}

/// `<scheme>://<host>`: the scheme is the API base's (a fake API over plain
/// `http` in a test keeps `http`; every real host is `https`).
pub fn github_web_url_of(api_url: &str, host: &str) -> String {
    let scheme = if api_url.starts_with("http://") {
        "http"
    } else {
        "https"
    };
    format!("{scheme}://{host}")
}

/// `GITHUB_APP_PRIVATE_KEY` is the PEM itself or the PEM base64-encoded on
/// one line (easier to put in `.env`).
pub fn decode_pem(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.starts_with("-----BEGIN") {
        return Ok(raw.replace("\\n", "\n"));
    }
    let bytes = STANDARD
        .decode(raw)
        .context("GITHUB_APP_PRIVATE_KEY is neither PEM nor base64")?;
    let pem = String::from_utf8(bytes).context("GITHUB_APP_PRIVATE_KEY is not UTF-8")?;
    if !pem.trim_start().starts_with("-----BEGIN") {
        bail!("GITHUB_APP_PRIVATE_KEY does not decode to a PEM key");
    }
    Ok(pem)
}

/// "github.com" for the public API; the GHES host (`ghe.example.com`) for
/// `https://ghe.example.com/api/v3`.
pub fn host_of(api_url: &str) -> String {
    let without_scheme = api_url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = without_scheme.split('/').next().unwrap_or_default();
    match host.strip_prefix("api.") {
        Some(PUBLIC_HOST) => PUBLIC_HOST.to_owned(),
        _ => host.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_is_derived_from_the_api_url() {
        assert_eq!(host_of("https://api.github.com"), "github.com");
        assert_eq!(host_of("https://ghe.example.com/api/v3"), "ghe.example.com");
        assert_eq!(host_of("http://127.0.0.1:5555"), "127.0.0.1:5555");
    }

    #[test]
    fn oauth_lives_on_the_web_host_of_an_enterprise_server() {
        assert_eq!(
            github_web_url_of(DEFAULT_GITHUB_API, "github.com"),
            "https://github.com"
        );
        assert_eq!(
            github_web_url_of("https://ghe.corp.example/api/v3", "ghe.corp.example"),
            "https://ghe.corp.example"
        );
        assert_eq!(
            github_web_url_of("http://fake.test:5555", "fake.test:5555"),
            "http://fake.test:5555"
        );
    }

    #[test]
    fn pem_accepts_text_or_base64() {
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nabc\n-----END RSA PRIVATE KEY-----";
        assert_eq!(decode_pem(pem).unwrap(), pem);
        assert_eq!(decode_pem(&STANDARD.encode(pem)).unwrap(), pem);
        assert!(decode_pem("not a key").is_err());
        assert!(decode_pem(&STANDARD.encode("hello")).is_err());
    }
}
