//! Asks an image registry whether a release's image exists (CONTRACT.md §22,
//! `registry.verify_image`). Speaks the Docker Registry HTTP API v2, which
//! GHCR, Docker Hub, ACR, ECR, Artifact Registry and every generic registry
//! implement: `HEAD /v2/<name>/manifests/<reference>` answers with the
//! manifest's digest, after the token dance the registry asks for in its
//! `WWW-Authenticate` challenge.
//!
//! Credentials come from the registry's `auth`: `github_token` uses the
//! workspace's GitHub token as the password (GHCR), `secret` resolves the
//! two workspace secrets the registry names, and `oidc` registries cannot be
//! read from here at all (their credentials only exist inside CI), so the
//! step says it skipped and leaves the release unverified. Nothing is pulled
//! and nothing is written; the step records the digest.
//!
//! The GitHub token is the workspace's own key to GitHub, so it is only
//! ever sent to GitHub's registry: `ghcr.io`, or `containers.<host>` of the
//! GitHub Enterprise Server the workspace lives on, and to a token realm on
//! that same host. A registry row that says `github_token` for any other
//! host is refused before a request is made, whoever configured it.

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{Method, StatusCode, header};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    Adapter, RunContext, StepInput, StepOutcome, TOOL_REGISTRY,
    net::{self, Net},
};
use crate::domain::diagnosis;

/// Manifest media types a tag may resolve to: OCI and Docker, single
/// images and multi-platform indexes.
pub const ACCEPT: &str = "application/vnd.oci.image.index.v1+json, \
     application/vnd.oci.image.manifest.v1+json, \
     application/vnd.docker.distribution.manifest.list.v2+json, \
     application/vnd.docker.distribution.manifest.v2+json";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const STEP_TIMEOUT: Duration = Duration::from_secs(90);
/// What a GHCR login uses as the user name when the password is a token.
const TOKEN_USER: &str = "x-access-token";
pub const SKIPPED_OIDC: &str = "skipped: registry is only reachable from CI (oidc)";

/// The only host the GitHub token is sent to on github.com.
pub const GHCR_HOST: &str = "ghcr.io";

pub struct RegistryAdapter {
    net: Net,
    /// Process-wide dev fallback (`GITHUB_TOKEN`) for `github_token` auth.
    github_token: Option<String>,
    /// Registry hosts that may receive the GitHub token.
    token_hosts: Vec<String>,
}

/// User name and password for the registry, when it needs any.
#[derive(Clone)]
struct Credentials {
    username: String,
    password: String,
    /// The password is the workspace's GitHub token: it never leaves the
    /// registry's own host.
    github_token: bool,
}

/// `host[:port]` of a URL, lowercased.
fn authority(url: &reqwest::Url) -> String {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    }
}

/// One `WWW-Authenticate` challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// `bearer` or `basic`, lowercased.
    pub scheme: String,
    pub params: BTreeMap<String, String>,
}

/// Parses `Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:a/b:pull"`.
/// Quoted values may contain commas (`scope="repository:a/b:pull,push"`).
pub fn parse_challenge(raw: &str) -> Option<Challenge> {
    let raw = raw.trim();
    let (scheme, rest) = match raw.split_once(char::is_whitespace) {
        Some((scheme, rest)) => (scheme, rest.trim()),
        None => (raw, ""),
    };
    if scheme.is_empty() {
        return None;
    }
    let mut params = BTreeMap::new();
    let mut chars = rest.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| *c == ',' || c.is_whitespace()) {
            chars.next();
        }
        let mut key = String::new();
        while let Some(c) = chars.peek().copied() {
            if c == '=' || c == ',' {
                break;
            }
            key.push(c);
            chars.next();
        }
        if key.trim().is_empty() {
            break;
        }
        let mut value = String::new();
        if chars.peek() == Some(&'=') {
            chars.next();
            if chars.peek() == Some(&'"') {
                chars.next();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            if let Some(escaped) = chars.next() {
                                value.push(escaped);
                            }
                        }
                        '"' => break,
                        other => value.push(other),
                    }
                }
            } else {
                while let Some(c) = chars.peek().copied() {
                    if c == ',' {
                        break;
                    }
                    value.push(c);
                    chars.next();
                }
            }
        }
        params.insert(key.trim().to_ascii_lowercase(), value.trim().to_owned());
    }
    Some(Challenge {
        scheme: scheme.to_ascii_lowercase(),
        params,
    })
}

/// The host the registry API answers on: Docker Hub images are named
/// `docker.io/...` but served by `registry-1.docker.io`.
pub fn api_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('/').to_ascii_lowercase();
    match host.as_str() {
        "docker.io" | "index.docker.io" | "registry.hub.docker.com" => {
            "registry-1.docker.io".to_owned()
        }
        _ => host,
    }
}

/// `host/namespace/name` → the repository path the API wants. The host is
/// stripped when the image starts with the registry's host (or with any
/// first component that looks like one); a single-name Docker Hub image
/// gets its implicit `library/`.
pub fn repository_path(image: &str, registry_host: &str) -> String {
    let image = image.trim().trim_matches('/');
    let registry_host = registry_host.trim().trim_end_matches('/');
    let stripped = if !registry_host.is_empty()
        && image.len() > registry_host.len()
        && image[..registry_host.len()].eq_ignore_ascii_case(registry_host)
        && image.as_bytes()[registry_host.len()] == b'/'
    {
        &image[registry_host.len() + 1..]
    } else {
        match image.split_once('/') {
            Some((first, rest))
                if first.contains('.') || first.contains(':') || first == "localhost" =>
            {
                rest
            }
            _ => image,
        }
    };
    if api_host(registry_host) == "registry-1.docker.io" && !stripped.contains('/') {
        format!("library/{stripped}")
    } else {
        stripped.to_owned()
    }
}

/// `base64(user:password)`, what a Basic `Authorization` header carries.
fn basic_pair(credentials: &Credentials) -> String {
    STANDARD.encode(format!("{}:{}", credentials.username, credentials.password))
}

enum Probe {
    Found { digest: String },
    Missing { detail: String },
    Denied { detail: String },
    Throttled { detail: String },
    Failed { detail: String },
}

impl RegistryAdapter {
    pub fn new(net: Net, github_token: Option<String>) -> Self {
        Self {
            net,
            github_token,
            token_hosts: vec![GHCR_HOST.to_owned()],
        }
    }

    /// Another host that may receive the GitHub token (a fake GHCR in tests).
    #[cfg(test)]
    pub fn with_token_host(mut self, host: &str) -> Self {
        self.token_hosts.push(host.trim().to_ascii_lowercase());
        self
    }

    /// Whether `host` is GitHub's own registry for this run: `ghcr.io`, or
    /// `containers.<host>` of the GitHub Enterprise Server whose API the
    /// workspace's access points at.
    fn takes_github_token(&self, host: &str, ctx: &RunContext) -> bool {
        let host = api_host(host);
        if self.token_hosts.contains(&host) {
            return true;
        }
        ctx.github
            .as_ref()
            .and_then(|access| reqwest::Url::parse(&access.api_base_url).ok())
            .and_then(|api| api.host_str().map(str::to_ascii_lowercase))
            .filter(|api_host| api_host != "api.github.com")
            .is_some_and(|enterprise| host == format!("containers.{enterprise}"))
    }

    fn base_url(&self, host: &str) -> String {
        let api = api_host(host);
        let bare = api.split(':').next().unwrap_or(&api);
        // A registry on the private network is plain http the way the
        // Docker CLI treats localhost, and only when private targets are on.
        let scheme = if self.net.policy().allow_private && net::is_local_host(bare) {
            "http"
        } else {
            "https"
        };
        format!("{scheme}://{api}")
    }

    async fn credentials(
        &self,
        step: &StepInput,
        host: &str,
        ctx: &RunContext,
    ) -> Result<Option<Credentials>, StepOutcome> {
        let param = |key: &str| step.params.get(key).map(|v| v.trim()).unwrap_or_default();
        let credentials = match param("registry_auth") {
            "secret" => {
                let username = ctx.secrets.get(param("username_secret")).await?;
                let password = ctx.secrets.get(param("password_secret")).await?;
                Some(Credentials {
                    username,
                    password,
                    github_token: false,
                })
            }
            // GHCR: the GitHub token is the password; without one the
            // registry is asked anonymously, which works for public images.
            _ => {
                let token = ctx
                    .github
                    .as_ref()
                    .map(|access| access.token.clone())
                    .or_else(|| self.github_token.clone())
                    .filter(|t| !t.trim().is_empty());
                if token.is_some() && !self.takes_github_token(host, ctx) {
                    return Err(StepOutcome::failed(
                        diagnosis::UNKNOWN,
                        format!(
                            "the GitHub token is only sent to GitHub's own registry ({GHCR_HOST}); {host} is not it. Give this registry `auth: secret` with its own credentials."
                        ),
                    ));
                }
                token.map(|password| Credentials {
                    username: TOKEN_USER.to_owned(),
                    password,
                    github_token: true,
                })
            }
        };
        if let Some(credentials) = &credentials {
            // The password and the Basic pair it is sent as: a registry that
            // echoes a rejected header must not get either into the log.
            let redactor = ctx.secrets.redactor();
            redactor.protect(&credentials.password);
            redactor.protect(&basic_pair(credentials));
        }
        Ok(credentials)
    }

    /// One manifest request. `HEAD` asks for the digest header only; `GET`
    /// is the fallback for registries that omit it on `HEAD`.
    async fn manifest(
        &self,
        method: Method,
        url: &str,
        authorization: Option<&str>,
    ) -> Result<reqwest::Response, String> {
        let mut request = self
            .net
            .request(method, url, REQUEST_TIMEOUT)
            .await?
            .header(header::ACCEPT, ACCEPT);
        if let Some(value) = authorization {
            request = request.header(header::AUTHORIZATION, value);
        }
        request.send().await.map_err(net::transport_error)
    }

    /// Follows a `Bearer` challenge: asks the realm for a pull token.
    async fn bearer_token(
        &self,
        challenge: &Challenge,
        registry: &str,
        repository: &str,
        credentials: Option<&Credentials>,
        ctx: &RunContext,
    ) -> Result<String, Probe> {
        let Some(realm) = challenge.params.get("realm").filter(|r| !r.is_empty()) else {
            return Err(Probe::Failed {
                detail: "the registry's Bearer challenge names no realm".to_owned(),
            });
        };
        let mut url = match reqwest::Url::parse(realm) {
            Ok(url) => url,
            Err(error) => {
                return Err(Probe::Failed {
                    detail: format!("the registry's token realm is not a URL: {error}"),
                });
            }
        };
        let scope = challenge
            .params
            .get("scope")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("repository:{repository}:pull"));
        {
            let mut query = url.query_pairs_mut();
            if let Some(service) = challenge.params.get("service").filter(|s| !s.is_empty()) {
                query.append_pair("service", service);
            }
            query.append_pair("scope", &scope);
        }
        let shown = net::redact_url(&url);
        // The realm is whatever the registry said. The GitHub token only
        // goes to a realm on the registry's own host: a registry must not be
        // able to send the workspace's key to GitHub somewhere else.
        let same_host =
            reqwest::Url::parse(registry).is_ok_and(|base| authority(&base) == authority(&url));
        if credentials.is_some_and(|c| c.github_token) && !same_host {
            return Err(Probe::Failed {
                detail: format!(
                    "the registry's token realm ({shown}) is on another host than the registry; the GitHub token is not sent there"
                ),
            });
        }
        // It also goes through the same guard as the registry itself.
        let mut request = self
            .net
            .request(Method::GET, url.as_str(), REQUEST_TIMEOUT)
            .await
            .map_err(|detail| Probe::Failed {
                detail: format!("token realm refused: {detail}"),
            })?;
        if let Some(credentials) = credentials {
            request = request.basic_auth(&credentials.username, Some(&credentials.password));
        }
        let response = request.send().await.map_err(|e| Probe::Failed {
            detail: format!("GET {shown} failed: {}", net::transport_error(e)),
        })?;
        let status = response.status();
        ctx.logs.line(format!("GET {shown} -> {status}"));
        let body = net::body_text(response).await;
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(Probe::Denied {
                detail: format!("GET {shown} -> {status}: {}", net::excerpt(&body, 400)),
            });
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(Probe::Throttled {
                detail: format!("GET {shown} -> {status}"),
            });
        }
        if !status.is_success() {
            return Err(Probe::Failed {
                detail: format!("GET {shown} -> {status}: {}", net::excerpt(&body, 400)),
            });
        }
        let json: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let token = json
            .get("token")
            .or_else(|| json.get("access_token"))
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(str::to_owned);
        match token {
            Some(token) => {
                ctx.secrets.redactor().protect(&token);
                Ok(token)
            }
            None => Err(Probe::Failed {
                detail: format!("GET {shown} answered {status} without a token"),
            }),
        }
    }

    async fn probe(
        &self,
        base: &str,
        repository: &str,
        reference: &str,
        credentials: Option<&Credentials>,
        ctx: &RunContext,
    ) -> Probe {
        let url = format!("{base}/v2/{repository}/manifests/{reference}");
        let path = format!("/v2/{repository}/manifests/{reference}");
        let mut authorization: Option<String> = None;
        // At most one challenge is answered: anonymous, then authenticated.
        for attempt in 0..2 {
            let response = match self
                .manifest(Method::HEAD, &url, authorization.as_deref())
                .await
            {
                Ok(response) => response,
                Err(detail) => {
                    return Probe::Failed {
                        detail: format!("HEAD {path} failed: {detail}"),
                    };
                }
            };
            let status = response.status();
            ctx.logs.line(format!("HEAD {path} -> {status}"));
            match status {
                StatusCode::OK => {
                    let digest = response
                        .headers()
                        .get("docker-content-digest")
                        .and_then(|v| v.to_str().ok())
                        .map(str::trim)
                        .filter(|d| !d.is_empty())
                        .map(str::to_owned);
                    return match digest {
                        Some(digest) => Probe::Found { digest },
                        None => {
                            self.digest_from_body(&url, &path, authorization.as_deref(), ctx)
                                .await
                        }
                    };
                }
                StatusCode::NOT_FOUND => {
                    return Probe::Missing {
                        detail: format!("HEAD {path} -> {status}"),
                    };
                }
                StatusCode::UNAUTHORIZED if attempt == 0 => {
                    let challenge = response
                        .headers()
                        .get(header::WWW_AUTHENTICATE)
                        .and_then(|v| v.to_str().ok())
                        .and_then(parse_challenge);
                    match challenge {
                        Some(challenge) if challenge.scheme == "bearer" => {
                            match self
                                .bearer_token(&challenge, base, repository, credentials, ctx)
                                .await
                            {
                                Ok(token) => authorization = Some(format!("Bearer {token}")),
                                Err(probe) => return probe,
                            }
                        }
                        Some(challenge) if challenge.scheme == "basic" => match credentials {
                            Some(credentials) => {
                                authorization = Some(format!("Basic {}", basic_pair(credentials)));
                            }
                            None => {
                                return Probe::Denied {
                                    detail: format!(
                                        "HEAD {path} -> {status}: the registry asks for a user name and password and none is configured"
                                    ),
                                };
                            }
                        },
                        _ => {
                            return Probe::Denied {
                                detail: format!(
                                    "HEAD {path} -> {status} without a usable WWW-Authenticate challenge"
                                ),
                            };
                        }
                    }
                }
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    return Probe::Denied {
                        detail: format!("HEAD {path} -> {status}"),
                    };
                }
                StatusCode::TOO_MANY_REQUESTS => {
                    return Probe::Throttled {
                        detail: format!("HEAD {path} -> {status}"),
                    };
                }
                other => {
                    return Probe::Failed {
                        detail: format!("HEAD {path} -> {other}"),
                    };
                }
            }
        }
        Probe::Denied {
            detail: format!("HEAD {path} kept answering 401"),
        }
    }

    /// The registry did not send `Docker-Content-Digest` on `HEAD`: the
    /// digest is the SHA-256 of the manifest bytes.
    async fn digest_from_body(
        &self,
        url: &str,
        path: &str,
        authorization: Option<&str>,
        ctx: &RunContext,
    ) -> Probe {
        let response = match self.manifest(Method::GET, url, authorization).await {
            Ok(response) => response,
            Err(detail) => {
                return Probe::Failed {
                    detail: format!("GET {path} failed: {detail}"),
                };
            }
        };
        let status = response.status();
        ctx.logs.line(format!("GET {path} -> {status}"));
        if !status.is_success() {
            return Probe::Failed {
                detail: format!("GET {path} -> {status}"),
            };
        }
        if let Some(digest) = response
            .headers()
            .get("docker-content-digest")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|d| !d.is_empty())
        {
            return Probe::Found {
                digest: digest.to_owned(),
            };
        }
        match response.bytes().await {
            Ok(bytes) if bytes.len() <= net::MAX_BODY_BYTES => Probe::Found {
                digest: format!("sha256:{}", hex::encode(Sha256::digest(&bytes))),
            },
            Ok(_) => Probe::Failed {
                detail: format!("GET {path} answered a manifest too large to hash"),
            },
            Err(error) => Probe::Failed {
                detail: format!("GET {path} failed: {}", net::transport_error(error)),
            },
        }
    }

    async fn verify(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        let param = |key: &str| step.params.get(key).map(|v| v.trim()).unwrap_or_default();
        if param("registry_auth") == "oidc" {
            ctx.logs.line(SKIPPED_OIDC);
            return StepOutcome::Succeeded {
                output: SKIPPED_OIDC.to_owned(),
                outputs: BTreeMap::from([("verified".to_owned(), "false".to_owned())]),
            };
        }
        let image = param("image");
        if image.is_empty() {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                "params.image is required for registry.verify_image",
            );
        }
        let host = match param("registry_host") {
            "" => image.split('/').next().unwrap_or_default(),
            host => host,
        };
        if host.is_empty() {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                "the registry has no host and the image names none",
            );
        }
        let tag = param("tag");
        let expected = param("digest");
        let reference = if tag.is_empty() { expected } else { tag };
        if reference.is_empty() {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                "the release has neither a tag nor a digest to look up",
            );
        }
        let repository = repository_path(image, host);
        let base = self.base_url(host);
        let credentials = match self.credentials(step, host, ctx).await {
            Ok(credentials) => credentials,
            Err(outcome) => return outcome,
        };
        ctx.logs.line(format!(
            "looking up {image}:{reference} on {} ({})",
            api_host(host),
            if credentials.is_some() {
                "authenticated"
            } else {
                "anonymous"
            }
        ));

        let named = format!("{image}:{reference}");
        match self
            .probe(&base, &repository, reference, credentials.as_ref(), ctx)
            .await
        {
            Probe::Found { digest } => {
                if !expected.is_empty() && !tag.is_empty() && expected != digest {
                    return StepOutcome::failed(
                        diagnosis::IMAGE_NOT_FOUND,
                        format!(
                            "{named} now resolves to {digest}, but the release was recorded as {expected}: the tag was moved, so the released image is no longer what this tag names"
                        ),
                    );
                }
                StepOutcome::Succeeded {
                    output: format!("{named} exists in the registry ({digest})"),
                    outputs: BTreeMap::from([
                        ("digest".to_owned(), digest),
                        ("verified".to_owned(), "true".to_owned()),
                        ("image_ref".to_owned(), named),
                    ]),
                }
            }
            Probe::Missing { detail } => StepOutcome::failed(
                diagnosis::IMAGE_NOT_FOUND,
                format!("{named} is not in the registry: {detail}"),
            ),
            Probe::Denied { detail } => StepOutcome::Failed {
                code: diagnosis::PERMISSION_DENIED.to_owned(),
                raw_output: format!(
                    "the registry refused to show {named}: {detail}\n{}",
                    denied_hint(param("registry_kind"), param("registry_auth"))
                ),
                missing_permissions: vec!["registry:read".to_owned()],
            },
            Probe::Throttled { detail } => StepOutcome::failed(
                diagnosis::QUOTA_EXCEEDED,
                format!("the registry is rate limiting requests for {named}: {detail}"),
            ),
            Probe::Failed { detail } => {
                if detail.contains("timed out") {
                    StepOutcome::failed(diagnosis::TIMEOUT, detail)
                } else {
                    StepOutcome::failed(diagnosis::UNKNOWN, detail)
                }
            }
        }
    }
}

/// What a human has to fix when the registry says no.
fn denied_hint(kind: &str, auth: &str) -> &'static str {
    match (kind, auth) {
        ("ghcr", _) | (_, "github_token") => {
            "GHCR answers \"denied\" both for a package the token may not read and for one that does not exist yet. The GitHub token needs the `read:packages` scope (a GitHub App installation needs Packages: read), and the image must have been pushed under this namespace."
        }
        (_, "secret") => {
            "Check the user name and password the registry's secrets hold, and that the account may pull this repository."
        }
        _ => "The registry needs credentials this workspace does not have for it.",
    }
}

#[async_trait]
impl Adapter for RegistryAdapter {
    fn name(&self) -> &'static str {
        TOOL_REGISTRY
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        if step.action_type != super::VERIFY_IMAGE {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("the registry adapter cannot run {}", step.action_type),
            );
        }
        match tokio::time::timeout(STEP_TIMEOUT, self.verify(step, ctx)).await {
            Ok(outcome) => outcome,
            Err(_) => StepOutcome::failed(
                diagnosis::TIMEOUT,
                "the registry did not answer within the step's time limit",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        extract::{Path, Query, State},
        http::{HeaderMap, Method as HttpMethod, StatusCode as HttpStatus},
        response::{IntoResponse, Response},
        routing::get,
    };
    use serde_json::json;

    use super::*;
    use crate::adapters::{GithubAccess, LogSink, Redactor, StaticSecrets, net::NetPolicy};

    const MANIFEST: &str =
        r#"{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json"}"#;
    const DIGEST: &str = "sha256:5f0a3a1c9a1b0f1e3a7c9d2b4e6f8a0c1d3e5f7a9b1c3d5e7f9a1b3c5d7e9f10";

    /// A registry that speaks the v2 API the way the real ones do.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        /// `bearer` (token realm), `basic`, or empty for an open registry.
        auth: &'static str,
        /// Omit `Docker-Content-Digest` on `HEAD` (and on `GET`).
        no_digest_header: bool,
        /// The token realm answers 403 `DENIED` (GHCR for an unknown package).
        deny_token: bool,
        addr: Mutex<String>,
    }

    type Shared = Arc<Fake>;
    const USER: &str = "robot";
    const PASSWORD: &str = "registry-pass-4f9a2c7d";
    const TOKEN: &str = "pull-token-77aa91";

    fn basic(user: &str, password: &str) -> String {
        format!("Basic {}", STANDARD.encode(format!("{user}:{password}")))
    }

    fn authorization(headers: &HeaderMap) -> &str {
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    }

    async fn token(
        State(fake): State<Shared>,
        Query(query): Query<std::collections::HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        fake.calls.lock().unwrap().push(format!(
            "GET /token service={} scope={} auth={}",
            query.get("service").map_or("", String::as_str),
            query.get("scope").map_or("", String::as_str),
            if authorization(&headers).is_empty() {
                "none"
            } else {
                "basic"
            }
        ));
        if fake.deny_token {
            return (
                HttpStatus::FORBIDDEN,
                Json(json!({"errors": [{"code": "DENIED", "message": "requested access to the resource is denied"}]})),
            )
                .into_response();
        }
        let auth = authorization(&headers);
        // Anonymous pulls are for public images only; this one is private.
        if auth != basic(USER, PASSWORD) && auth != basic(TOKEN_USER, "gho_workspace_token_abcdef")
        {
            // A registry that echoes what it was sent, as some do.
            return (
                HttpStatus::UNAUTHORIZED,
                Json(json!({"errors": [{"code": "UNAUTHORIZED", "message": format!("bad credentials: {auth}")}]})),
            )
                .into_response();
        }
        Json(json!({ "token": TOKEN, "expires_in": 300 })).into_response()
    }

    async fn manifest(
        State(fake): State<Shared>,
        method: HttpMethod,
        Path(rest): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let accept = headers
            .get("accept")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        fake.calls
            .lock()
            .unwrap()
            .push(format!("{method} /v2/{rest}"));
        assert!(accept.contains("application/vnd.oci.image.index.v1+json"));
        assert!(accept.contains("application/vnd.docker.distribution.manifest.v2+json"));
        let auth = authorization(&headers);
        let addr = fake.addr.lock().unwrap().clone();
        let allowed = match fake.auth {
            "bearer" => auth == format!("Bearer {TOKEN}"),
            "basic" => auth == basic(USER, PASSWORD),
            _ => true,
        };
        if !allowed {
            let challenge = match fake.auth {
                "bearer" => format!(
                    "Bearer realm=\"http://{addr}/token\",service=\"registry.test\",scope=\"repository:acme/api:pull\""
                ),
                _ => "Basic realm=\"registry\"".to_owned(),
            };
            return (
                HttpStatus::UNAUTHORIZED,
                [("www-authenticate", challenge)],
                Json(json!({"errors": [{"code": "UNAUTHORIZED"}]})),
            )
                .into_response();
        }
        if rest != "acme/api/manifests/a1b2c3d" {
            return (
                HttpStatus::NOT_FOUND,
                Json(json!({"errors": [{"code": "MANIFEST_UNKNOWN", "message": "manifest unknown"}]})),
            )
                .into_response();
        }
        let body = if method == HttpMethod::HEAD {
            String::new()
        } else {
            MANIFEST.to_owned()
        };
        if fake.no_digest_header {
            (HttpStatus::OK, body).into_response()
        } else {
            (
                HttpStatus::OK,
                [("docker-content-digest", DIGEST.to_owned())],
                body,
            )
                .into_response()
        }
    }

    async fn serve(fake: Fake) -> (Shared, String) {
        let fake = Arc::new(fake);
        let app = Router::new()
            .route("/token", get(token))
            .route("/v2/{*rest}", get(manifest).head(manifest))
            .with_state(Arc::clone(&fake));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        *fake.addr.lock().unwrap() = addr.clone();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (fake, addr)
    }

    fn adapter() -> RegistryAdapter {
        RegistryAdapter::new(Net::new(NetPolicy::ALLOW_PRIVATE), None)
    }

    fn step(host: &str, auth: &str, tag: &str) -> StepInput {
        StepInput {
            key: "verify_image".into(),
            title: "Verify image".into(),
            action_type: "registry.verify_image".into(),
            adapter: "registry".into(),
            params: BTreeMap::from([
                ("registry_kind".to_owned(), "generic".to_owned()),
                ("registry_host".to_owned(), host.to_owned()),
                ("registry_auth".to_owned(), auth.to_owned()),
                ("username_secret".to_owned(), "REGISTRY_USER".to_owned()),
                ("password_secret".to_owned(), "REGISTRY_PASSWORD".to_owned()),
                ("image".to_owned(), format!("{host}/acme/api")),
                ("tag".to_owned(), tag.to_owned()),
            ]),
            attempt: 1,
        }
    }

    /// A context whose secrets and log lines share one redactor, like the
    /// runner builds it; the receiver collects the lines.
    fn context() -> (RunContext, tokio::sync::mpsc::Receiver<String>) {
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let redactor = Redactor::default();
        let secrets =
            StaticSecrets::with(&[("REGISTRY_USER", USER), ("REGISTRY_PASSWORD", PASSWORD)]);
        let ctx = RunContext {
            secrets: secrets.secrets(&redactor),
            logs: LogSink::new(tx).with_redactor(redactor),
            ..Default::default()
        };
        (ctx, rx)
    }

    fn drain(mut rx: tokio::sync::mpsc::Receiver<String>) -> Vec<String> {
        let mut lines = Vec::new();
        while let Ok(line) = rx.try_recv() {
            lines.push(line);
        }
        lines
    }

    #[tokio::test]
    async fn bearer_challenge_is_answered_with_the_secret_credentials() {
        let (fake, addr) = serve(Fake {
            auth: "bearer",
            ..Default::default()
        })
        .await;
        let (ctx, rx) = context();
        let outcome = adapter().run(&step(&addr, "secret", "a1b2c3d"), &ctx).await;
        let StepOutcome::Succeeded { output, outputs } = outcome else {
            panic!("expected success: {outcome:?}");
        };
        assert_eq!(outputs["digest"], DIGEST);
        assert_eq!(outputs["verified"], "true");
        assert_eq!(outputs["image_ref"], format!("{addr}/acme/api:a1b2c3d"));
        assert!(output.contains(DIGEST), "{output}");
        // Anonymous first, then the token realm with Basic, then again with
        // the bearer token: exactly the flow a Docker client performs.
        assert_eq!(
            *fake.calls.lock().unwrap(),
            [
                "HEAD /v2/acme/api/manifests/a1b2c3d",
                "GET /token service=registry.test scope=repository:acme/api:pull auth=basic",
                "HEAD /v2/acme/api/manifests/a1b2c3d",
            ]
        );
        // No credential, token or encoded pair reached a log line.
        let lines = drain(rx).join("\n");
        for secret in [PASSWORD, TOKEN, &basic(USER, PASSWORD)] {
            assert!(!lines.contains(secret), "{lines}");
        }
        assert!(lines.contains("HEAD /v2/acme/api/manifests/a1b2c3d -> 200 OK"));
    }

    #[tokio::test]
    async fn an_unpublished_tag_is_image_not_found() {
        let (_, addr) = serve(Fake {
            auth: "bearer",
            ..Default::default()
        })
        .await;
        let (ctx, _rx) = context();
        let outcome = adapter().run(&step(&addr, "secret", "0000000"), &ctx).await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure: {outcome:?}");
        };
        assert_eq!(code, diagnosis::IMAGE_NOT_FOUND);
        assert!(raw_output.contains("acme/api:0000000 is not in the registry"));
        assert!(raw_output.contains("404"), "{raw_output}");
    }

    #[tokio::test]
    async fn basic_challenges_and_missing_digest_headers_are_handled() {
        let (fake, addr) = serve(Fake {
            auth: "basic",
            no_digest_header: true,
            ..Default::default()
        })
        .await;
        let (ctx, _rx) = context();
        let outcome = adapter().run(&step(&addr, "secret", "a1b2c3d"), &ctx).await;
        let StepOutcome::Succeeded { outputs, .. } = outcome else {
            panic!("expected success: {outcome:?}");
        };
        // Without the header the digest is the hash of the manifest bytes.
        assert_eq!(
            outputs["digest"],
            format!(
                "sha256:{}",
                hex::encode(Sha256::digest(MANIFEST.as_bytes()))
            )
        );
        assert_eq!(
            *fake.calls.lock().unwrap(),
            [
                "HEAD /v2/acme/api/manifests/a1b2c3d",
                "HEAD /v2/acme/api/manifests/a1b2c3d",
                "GET /v2/acme/api/manifests/a1b2c3d",
            ]
        );
    }

    #[tokio::test]
    async fn the_github_token_is_the_password_for_ghcr_style_auth() {
        let (fake, addr) = serve(Fake {
            auth: "bearer",
            ..Default::default()
        })
        .await;
        let (mut ctx, rx) = context();
        ctx.github = Some(GithubAccess {
            token: "gho_workspace_token_abcdef".into(),
            api_base_url: "https://api.github.com".into(),
        });
        let mut input = step(&addr, "github_token", "a1b2c3d");
        input.params.insert("registry_kind".into(), "ghcr".into());
        // The fake stands in for ghcr.io.
        let outcome = adapter().with_token_host(&addr).run(&input, &ctx).await;
        assert!(
            matches!(outcome, StepOutcome::Succeeded { .. }),
            "{outcome:?}"
        );
        assert!(fake.calls.lock().unwrap()[1].ends_with("auth=basic"));
        assert!(!drain(rx).join("\n").contains("gho_workspace_token_abcdef"));
    }

    /// The workspace's GitHub token is its key to GitHub: no registry row
    /// and no registry answer can make this adapter send it anywhere but
    /// GitHub's own registry.
    #[tokio::test]
    async fn the_github_token_never_leaves_github_s_registry() {
        let token = "gho_workspace_token_abcdef";
        let with_token = |api: &str| {
            let (mut ctx, rx) = context();
            ctx.github = Some(GithubAccess {
                token: token.into(),
                api_base_url: api.to_owned(),
            });
            (ctx, rx)
        };

        // A registry row that says `github_token` for some other host:
        // refused before a single request is made.
        let (fake, addr) = serve(Fake {
            auth: "bearer",
            ..Default::default()
        })
        .await;
        let (ctx, _rx) = with_token("https://api.github.com");
        let mut input = step(&addr, "github_token", "a1b2c3d");
        input.params.insert("registry_kind".into(), "ghcr".into());
        let outcome = adapter().run(&input, &ctx).await;
        let StepOutcome::Failed { raw_output, .. } = outcome else {
            panic!("the token must not be sent to {addr}");
        };
        assert!(
            raw_output.contains("only sent to GitHub's own registry"),
            "{raw_output}"
        );
        assert!(fake.calls.lock().unwrap().is_empty(), "nothing was asked");

        // GitHub's registry naming a token realm on another host: the
        // manifest is asked for, the token is not sent to that realm.
        let (elsewhere, other_addr) = serve(Fake::default()).await;
        let (fake, addr) = serve(Fake {
            auth: "bearer",
            ..Default::default()
        })
        .await;
        *fake.addr.lock().unwrap() = other_addr;
        let mut input = step(&addr, "github_token", "a1b2c3d");
        input.params.insert("registry_kind".into(), "ghcr".into());
        let outcome = adapter().with_token_host(&addr).run(&input, &ctx).await;
        let StepOutcome::Failed { raw_output, .. } = outcome else {
            panic!("the token must not follow the realm to another host");
        };
        assert!(
            raw_output.contains("is on another host than the registry"),
            "{raw_output}"
        );
        assert_eq!(fake.calls.lock().unwrap().len(), 1, "only the first HEAD");
        assert!(elsewhere.calls.lock().unwrap().is_empty());

        // Secrets of the registry itself may follow its realm (Docker Hub's
        // lives on auth.docker.io), and without any token the lookup is
        // anonymous wherever the registry is.
        let adapter_for_hosts = adapter();
        let (ghes, _rx) = with_token("https://ghe.corp.example/api/v3");
        assert!(adapter_for_hosts.takes_github_token("ghcr.io", &ctx));
        assert!(adapter_for_hosts.takes_github_token("GHCR.io", &ctx));
        assert!(!adapter_for_hosts.takes_github_token("ghcr.io.evil.example", &ctx));
        assert!(!adapter_for_hosts.takes_github_token("containers.ghe.corp.example", &ctx));
        assert!(adapter_for_hosts.takes_github_token("containers.ghe.corp.example", &ghes));
        assert!(!adapter_for_hosts.takes_github_token("containers.evil.example", &ghes));
        assert!(!adapter_for_hosts.takes_github_token("ghe.corp.example", &ghes));
        let (anonymous, _rx) = context();
        let (open, addr) = serve(Fake::default()).await;
        let mut input = step(&addr, "github_token", "a1b2c3d");
        input.params.insert("registry_kind".into(), "ghcr".into());
        let outcome = adapter().run(&input, &anonymous).await;
        assert!(
            matches!(outcome, StepOutcome::Succeeded { .. }),
            "no token, nothing to protect: {outcome:?}"
        );
        assert_eq!(open.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn denied_and_wrong_credentials_are_permission_denied_without_leaking_them() {
        // GHCR's answer for a package the token cannot see (or that is not there).
        let (_, addr) = serve(Fake {
            auth: "bearer",
            deny_token: true,
            ..Default::default()
        })
        .await;
        let (ctx, _rx) = context();
        let mut input = step(&addr, "github_token", "a1b2c3d");
        input.params.insert("registry_kind".into(), "ghcr".into());
        let outcome = adapter().run(&input, &ctx).await;
        let StepOutcome::Failed {
            code,
            raw_output,
            missing_permissions,
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(missing_permissions, ["registry:read"]);
        assert!(raw_output.contains("read:packages"), "{raw_output}");

        // A registry that echoes the Authorization header it rejected.
        let (_, addr) = serve(Fake {
            auth: "bearer",
            ..Default::default()
        })
        .await;
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let redactor = Redactor::default();
        let wrong = "a-wrong-password-9c1e";
        let ctx = RunContext {
            secrets: StaticSecrets::with(&[("REGISTRY_USER", USER), ("REGISTRY_PASSWORD", wrong)])
                .secrets(&redactor),
            logs: LogSink::new(tx).with_redactor(redactor),
            ..Default::default()
        };
        let outcome = adapter()
            .run(&step(&addr, "secret", "a1b2c3d"), &ctx)
            .await
            .redacted(ctx.secrets.redactor());
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert!(raw_output.contains("bad credentials"), "{raw_output}");
        assert!(!raw_output.contains(wrong), "{raw_output}");
        assert!(!raw_output.contains(&STANDARD.encode(format!("{USER}:{wrong}"))));
        assert!(!drain(rx).join("\n").contains(wrong));

        // A secret the workspace does not hold fails before any request.
        let (fake, addr) = serve(Fake::default()).await;
        let ctx = RunContext {
            secrets: StaticSecrets::default().secrets(&Redactor::default()),
            ..Default::default()
        };
        let outcome = adapter().run(&step(&addr, "secret", "a1b2c3d"), &ctx).await;
        let StepOutcome::Failed { raw_output, .. } = outcome else {
            panic!("expected a failure");
        };
        assert!(
            raw_output.contains("secret REGISTRY_USER is not set in this workspace"),
            "{raw_output}"
        );
        assert!(fake.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn oidc_registries_are_skipped_and_a_moved_tag_is_refused() {
        let (fake, addr) = serve(Fake::default()).await;
        let (ctx, _rx) = context();
        let outcome = adapter().run(&step(&addr, "oidc", "a1b2c3d"), &ctx).await;
        assert_eq!(
            outcome,
            StepOutcome::Succeeded {
                output: "skipped: registry is only reachable from CI (oidc)".into(),
                outputs: BTreeMap::from([("verified".to_owned(), "false".to_owned())]),
            }
        );
        assert!(fake.calls.lock().unwrap().is_empty(), "nothing is asked");

        // An open registry: found without any challenge; the digest the
        // release recorded must still be what the tag names.
        let mut input = step(&addr, "github_token", "a1b2c3d");
        input.params.insert("digest".into(), DIGEST.into());
        let outcome = adapter().run(&input, &ctx).await;
        assert!(
            matches!(outcome, StepOutcome::Succeeded { .. }),
            "{outcome:?}"
        );
        input
            .params
            .insert("digest".into(), format!("sha256:{}", "0".repeat(64)));
        let outcome = adapter().run(&input, &ctx).await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::IMAGE_NOT_FOUND);
        assert!(raw_output.contains("the tag was moved"), "{raw_output}");
    }

    #[tokio::test]
    async fn private_registries_are_refused_unless_private_targets_are_allowed() {
        let (fake, addr) = serve(Fake::default()).await;
        let guarded = RegistryAdapter::new(Net::new(NetPolicy::PUBLIC_ONLY), None);
        let (ctx, _rx) = context();
        let outcome = guarded
            .run(&step(&addr, "github_token", "a1b2c3d"), &ctx)
            .await;
        let StepOutcome::Failed { raw_output, .. } = outcome else {
            panic!("a loopback registry must be refused");
        };
        assert!(raw_output.contains("loopback"), "{raw_output}");
        assert!(fake.calls.lock().unwrap().is_empty());
    }

    /// Read-only against the real GHCR, through the default (public-only)
    /// network policy:
    /// `SDI_TEST_GHCR=1 [GITHUB_TOKEN=...] [SDI_TEST_GHCR_MISSING=ghcr.io/owner/name:tag] cargo test -p sdi-execution real_ghcr -- --nocapture`.
    /// A public image must be found anonymously, with its digest; the
    /// image named in `SDI_TEST_GHCR_MISSING` (one that was never pushed)
    /// must not be. What GHCR answered is printed, so the run doubles as a
    /// report of how the real registry behaves.
    #[tokio::test]
    async fn real_ghcr_finds_a_public_image_and_not_an_unpublished_one() {
        if std::env::var("SDI_TEST_GHCR").ok().as_deref() != Some("1") {
            eprintln!("skipping: SDI_TEST_GHCR is not 1");
            return;
        }
        let token = std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty());
        let adapter = RegistryAdapter::new(Net::new(NetPolicy::PUBLIC_ONLY), None);
        let ghcr = |reference: &str| {
            let (image, tag) = reference.rsplit_once(':').expect("image:tag");
            let mut input = step("ghcr.io", "github_token", tag);
            input.params.insert("registry_kind".into(), "ghcr".into());
            input.params.insert("image".into(), image.to_owned());
            input
        };

        // Anonymous: the token realm hands out a pull token for public images.
        let public = std::env::var("SDI_TEST_GHCR_PUBLIC")
            .unwrap_or_else(|_| "ghcr.io/actions/actions-runner:latest".into());
        let (ctx, rx) = context();
        let outcome = adapter.run(&ghcr(&public), &ctx).await;
        eprintln!("{public} (anonymous): {outcome:?}");
        eprintln!("{}", drain(rx).join("\n"));
        let StepOutcome::Succeeded { outputs, .. } = outcome else {
            panic!("a public image must be found");
        };
        assert!(outputs["digest"].starts_with("sha256:"), "{outputs:?}");
        assert_eq!(outputs["verified"], "true");

        // With the workspace's token: an image that was never pushed.
        let missing = std::env::var("SDI_TEST_GHCR_MISSING")
            .unwrap_or_else(|_| "ghcr.io/f8fvwgzc/view-engine:0000000".into());
        let (mut ctx, rx) = context();
        ctx.github = token.clone().map(|token| GithubAccess {
            token,
            api_base_url: "https://api.github.com".into(),
        });
        let outcome = adapter
            .run(&ghcr(&missing), &ctx)
            .await
            .redacted(ctx.secrets.redactor());
        eprintln!(
            "{missing} ({}): {outcome:?}",
            if token.is_some() {
                "with GITHUB_TOKEN"
            } else {
                "anonymous"
            }
        );
        let lines = drain(rx).join("\n");
        eprintln!("{lines}");
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("an unpublished image must not be found");
        };
        // GHCR says "denied" both for a package the token may not read and
        // for one that does not exist; with a token that may read packages
        // it says 404.
        assert!(
            code == diagnosis::IMAGE_NOT_FOUND || code == diagnosis::PERMISSION_DENIED,
            "{code}: {raw_output}"
        );
        if let Some(token) = &token {
            assert!(!raw_output.contains(token.as_str()) && !lines.contains(token.as_str()));
        }
    }

    #[test]
    fn challenges_hosts_and_repository_paths_are_parsed() {
        let challenge = parse_challenge(
            "Bearer realm=\"https://ghcr.io/token\",service=\"ghcr.io\",scope=\"repository:acme/api:pull,push\"",
        )
        .unwrap();
        assert_eq!(challenge.scheme, "bearer");
        assert_eq!(challenge.params["realm"], "https://ghcr.io/token");
        assert_eq!(challenge.params["service"], "ghcr.io");
        assert_eq!(challenge.params["scope"], "repository:acme/api:pull,push");
        let basic = parse_challenge("Basic realm=\"Registry Realm\"").unwrap();
        assert_eq!(basic.scheme, "basic");
        assert_eq!(basic.params["realm"], "Registry Realm");
        let bare = parse_challenge("Bearer realm=https://r.example/token, service=r").unwrap();
        assert_eq!(bare.params["realm"], "https://r.example/token");
        assert_eq!(bare.params["service"], "r");
        assert_eq!(parse_challenge("Negotiate").unwrap().scheme, "negotiate");
        assert!(parse_challenge("").is_none());

        assert_eq!(api_host("docker.io"), "registry-1.docker.io");
        assert_eq!(api_host("GHCR.io/"), "ghcr.io");
        assert_eq!(repository_path("ghcr.io/acme/api", "ghcr.io"), "acme/api");
        assert_eq!(
            repository_path("myorg.azurecr.io/team/svc/api", "myorg.azurecr.io"),
            "team/svc/api"
        );
        assert_eq!(
            repository_path("docker.io/acme/api", "docker.io"),
            "acme/api"
        );
        assert_eq!(
            repository_path("docker.io/nginx", "docker.io"),
            "library/nginx"
        );
        assert_eq!(repository_path("acme/api", "ghcr.io"), "acme/api");
        assert_eq!(
            repository_path("127.0.0.1:5000/acme/api", "127.0.0.1:5000"),
            "acme/api"
        );
        assert_eq!(
            repository_path("other.example.com/acme/api", "ghcr.io"),
            "acme/api"
        );
    }
}
