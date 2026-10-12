//! Outbound HTTP for deployers, with a server-side request forgery guard.
//!
//! Registry hosts, Jenkins and webhook URLs and environment health URLs are
//! typed by workspace admins (or come from `.sdi.yml`), so the execution
//! service must not let them reach what only it can reach: its own
//! loopback, the private network it runs in, or a cloud metadata endpoint.
//!
//! The rule (CONTRACT.md §22): `https` only, and every address the host
//! resolves to must be public. `SDI_ALLOW_PRIVATE_DEPLOY_TARGETS=1` lifts
//! both for self-hosted installs whose deployers live on a private network
//! (`http` is then accepted for loopback and private addresses only, never
//! for a public host).
//!
//! Two layers enforce it. [`Net::check`] validates a URL before a request
//! is built, which covers IP literals and gives a readable error. The
//! client's DNS resolver ([`GuardedResolver`]) applies the same address
//! test at connection time, so a name that answers a public address to the
//! check and a private one to the connection (DNS rebinding) is refused
//! too. Redirects are never followed: a public host cannot bounce a
//! request inward.
//!
//! The GitHub API base is not routed through here: it is operator
//! configuration (`GITHUB_API_URL` or the host identity recorded for the
//! installation), not something a workspace types, and a GitHub Enterprise
//! Server on a private network has to keep working.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use reqwest::{
    Method, RequestBuilder, Url,
    dns::{Addrs, Name, Resolve, Resolving},
};

pub const ALLOW_PRIVATE_ENV: &str = "SDI_ALLOW_PRIVATE_DEPLOY_TARGETS";
const USER_AGENT: &str = concat!("sdi-execution/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);
/// Longest response body an adapter reads into memory (manifests, build
/// status documents and error excerpts are all far smaller).
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Whether deploy targets on private networks are reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetPolicy {
    pub allow_private: bool,
}

impl NetPolicy {
    #[cfg(test)]
    pub const PUBLIC_ONLY: Self = Self {
        allow_private: false,
    };
    #[cfg(test)]
    pub const ALLOW_PRIVATE: Self = Self {
        allow_private: true,
    };

    /// `SDI_ALLOW_PRIVATE_DEPLOY_TARGETS=1` (or `true`) allows private targets.
    pub fn from_env() -> Self {
        let raw = sdi_common::config::env_or(ALLOW_PRIVATE_ENV, "0");
        Self {
            allow_private: matches!(raw.trim(), "1" | "true" | "TRUE" | "yes"),
        }
    }
}

/// Why an address is not a public one, or `None` when it is.
pub fn forbidden_ip(ip: IpAddr) -> Option<&'static str> {
    match ip {
        IpAddr::V4(v4) => forbidden_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return forbidden_v4(v4);
            }
            forbidden_v6(v6)
        }
    }
}

fn forbidden_v4(ip: Ipv4Addr) -> Option<&'static str> {
    let [a, b, c, _] = ip.octets();
    if ip.is_unspecified() || a == 0 {
        Some("unspecified")
    } else if ip.is_loopback() {
        Some("loopback")
    } else if ip.is_link_local() {
        // 169.254.0.0/16, which includes the 169.254.169.254 metadata endpoint.
        Some("link-local (cloud metadata)")
    } else if ip.is_private() {
        Some("private")
    } else if a == 100 && (64..128).contains(&b) {
        Some("carrier-grade NAT")
    } else if a == 192 && b == 0 && c == 0 {
        Some("IETF protocol assignment")
    } else if a == 198 && (b == 18 || b == 19) {
        Some("benchmarking")
    } else if ip.is_broadcast() || ip.is_multicast() || a >= 240 {
        Some("multicast or reserved")
    } else if ip.is_documentation() {
        Some("documentation")
    } else {
        None
    }
}

fn forbidden_v6(ip: Ipv6Addr) -> Option<&'static str> {
    let first = ip.segments()[0];
    if ip.is_unspecified() {
        Some("unspecified")
    } else if ip.is_loopback() {
        Some("loopback")
    } else if first & 0xffc0 == 0xfe80 {
        Some("link-local")
    } else if first & 0xfe00 == 0xfc00 {
        // fc00::/7, which includes the fd00:ec2::254 metadata endpoint.
        Some("unique local (private)")
    } else if ip.is_multicast() {
        Some("multicast")
    } else if first == 0x2001 && ip.segments()[1] == 0x0db8 {
        Some("documentation")
    } else if first == 0x0064 && ip.segments()[1] == 0xff9b {
        // NAT64: the embedded IPv4 address decides.
        let segments = ip.segments();
        let v4 = Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        );
        forbidden_v4(v4)
    } else {
        None
    }
}

/// Whether `host` (no port) is a loopback or private IP literal or
/// `localhost`: the hosts a registry may be reached over plain `http` on
/// when private targets are allowed, as the Docker CLI does.
pub fn is_local_host(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .is_ok_and(|ip| forbidden_ip(ip).is_some())
}

/// Applies the address rule when the client connects, so the answer the
/// pre-flight check saw cannot be swapped for another one.
struct GuardedResolver {
    policy: NetPolicy,
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let policy = self.policy;
        Box::pin(async move {
            let host = name.as_str().to_owned();
            let resolved: Vec<SocketAddr> =
                tokio::time::timeout(RESOLVE_TIMEOUT, tokio::net::lookup_host((host.as_str(), 0)))
                    .await
                    .map_err(|_| format!("resolving {host} timed out"))??
                    .collect();
            if resolved.is_empty() {
                return Err(format!("{host} does not resolve").into());
            }
            if !policy.allow_private
                && let Some((addr, reason)) = resolved
                    .iter()
                    .find_map(|addr| forbidden_ip(addr.ip()).map(|reason| (addr, reason)))
            {
                return Err(format!(
                    "{host} resolves to {}, a {reason} address; deploy targets must be public",
                    addr.ip()
                )
                .into());
            }
            Ok(Box::new(resolved.into_iter()) as Addrs)
        })
    }
}

/// The guarded HTTP client every deployer that talks to a workspace-typed
/// host uses. Requests can only be built through [`Net::request`], which
/// validates the URL first.
#[derive(Clone)]
pub struct Net {
    policy: NetPolicy,
    http: reqwest::Client,
}

impl std::fmt::Debug for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Net").field("policy", &self.policy).finish()
    }
}

impl Net {
    pub fn new(policy: NetPolicy) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .dns_resolver(Arc::new(GuardedResolver { policy }))
            .build()
            .expect("reqwest client");
        Self { policy, http }
    }

    pub fn policy(&self) -> NetPolicy {
        self.policy
    }

    /// Validates `raw` against the policy: scheme, no credentials in the
    /// URL, and where its host points. The error is a sentence for the
    /// person who typed the URL.
    pub async fn check(&self, raw: &str) -> Result<Url, String> {
        let url = Url::parse(raw.trim()).map_err(|e| format!("{raw} is not a valid URL: {e}"))?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(
                "URLs must not carry credentials; reference a workspace secret instead".to_owned(),
            );
        }
        let host = url
            .host_str()
            .filter(|h| !h.is_empty())
            .ok_or_else(|| format!("{raw} has no host"))?
            .to_owned();
        let port = url
            .port_or_known_default()
            .ok_or_else(|| format!("{raw} has no port"))?;
        let addresses = self.addresses(&host, port).await?;
        let blocked = addresses
            .iter()
            .find_map(|ip| forbidden_ip(*ip).map(|reason| (*ip, reason)));
        match url.scheme() {
            "https" => {}
            "http" => {
                // Plain http never leaves the private network, and only
                // when private targets are allowed at all.
                let all_private = addresses.iter().all(|ip| forbidden_ip(*ip).is_some());
                if !(self.policy.allow_private && all_private) {
                    return Err(format!(
                        "{} uses http; deploy targets must use https (http is only accepted for \
                         private addresses when {ALLOW_PRIVATE_ENV}=1)",
                        redact_url(&url)
                    ));
                }
            }
            other => {
                return Err(format!(
                    "{other} URLs are not supported; deploy targets must use https"
                ));
            }
        }
        if !self.policy.allow_private
            && let Some((ip, reason)) = blocked
        {
            return Err(format!(
                "{host} resolves to {ip}, a {reason} address; deploy targets must be public \
                 (set {ALLOW_PRIVATE_ENV}=1 on the execution service to allow private ones)"
            ));
        }
        Ok(url)
    }

    /// The addresses a host stands for: itself when it is an IP literal
    /// (the URL parser already normalised decimal and hex spellings to a
    /// dotted quad), else whatever it resolves to right now.
    async fn addresses(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
        let literal = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = literal.parse::<IpAddr>() {
            return Ok(vec![ip]);
        }
        let resolved: Vec<IpAddr> =
            tokio::time::timeout(RESOLVE_TIMEOUT, tokio::net::lookup_host((host, port)))
                .await
                .map_err(|_| format!("resolving {host} timed out"))?
                .map_err(|e| format!("{host} does not resolve: {e}"))?
                .map(|addr| addr.ip())
                .collect();
        if resolved.is_empty() {
            return Err(format!("{host} does not resolve"));
        }
        Ok(resolved)
    }

    /// A request to a validated URL, with the per-request `timeout`.
    pub async fn request(
        &self,
        method: Method,
        raw: &str,
        timeout: Duration,
    ) -> Result<RequestBuilder, String> {
        let url = self.check(raw).await?;
        Ok(self.http.request(method, url).timeout(timeout))
    }
}

/// The URL without its query string, for messages: queries may carry
/// tokens a deployer was configured with.
pub fn redact_url(url: &Url) -> String {
    let mut shown = url.clone();
    shown.set_query(None);
    shown.set_fragment(None);
    let _ = shown.set_password(None);
    let _ = shown.set_username("");
    shown.to_string()
}

/// Reads at most [`MAX_BODY_BYTES`] of a response body as text.
pub async fn body_text(response: reqwest::Response) -> String {
    let mut out = Vec::new();
    let mut response = response;
    while let Ok(Some(chunk)) = response.chunk().await {
        let room = MAX_BODY_BYTES.saturating_sub(out.len());
        if room == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The first lines of a body, for failure output.
pub fn excerpt(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_owned();
    }
    let cut: String = trimmed.chars().take(max_chars).collect();
    format!("{cut}...")
}

/// How a transport failure reads in a step's raw output, without the URL
/// (and so without the query string) reqwest would print.
pub fn transport_error(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut text = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(inner) = source {
        let inner_text = inner.to_string();
        if !text.contains(&inner_text) {
            text.push_str(": ");
            text.push_str(&inner_text);
        }
        source = inner.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn private_loopback_link_local_and_metadata_addresses_are_forbidden() {
        for (ip, reason) in [
            ("127.0.0.1", "loopback"),
            ("127.8.9.10", "loopback"),
            ("10.0.0.5", "private"),
            ("172.16.0.1", "private"),
            ("172.31.255.254", "private"),
            ("192.168.1.10", "private"),
            ("169.254.169.254", "link-local (cloud metadata)"),
            ("169.254.0.1", "link-local (cloud metadata)"),
            ("100.64.0.1", "carrier-grade NAT"),
            ("0.0.0.0", "unspecified"),
            ("255.255.255.255", "multicast or reserved"),
            ("224.0.0.1", "multicast or reserved"),
            ("::1", "loopback"),
            ("::", "unspecified"),
            ("fe80::1", "link-local"),
            ("fd00:ec2::254", "unique local (private)"),
            ("fc00::1", "unique local (private)"),
            ("::ffff:127.0.0.1", "loopback"),
            ("::ffff:10.1.2.3", "private"),
            ("::ffff:169.254.169.254", "link-local (cloud metadata)"),
            ("64:ff9b::a00:1", "private"),
        ] {
            assert_eq!(forbidden_ip(v4(ip)), Some(reason), "{ip}");
        }
        for ip in [
            "140.82.112.3",
            "8.8.8.8",
            "172.32.0.1",
            "192.169.0.1",
            "100.128.0.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert_eq!(forbidden_ip(v4(ip)), None, "{ip}");
        }
    }

    #[tokio::test]
    async fn the_default_policy_refuses_everything_that_is_not_public_https() {
        let net = Net::new(NetPolicy::PUBLIC_ONLY);
        for url in [
            "https://127.0.0.1/hook",
            "https://127.0.0.1:8443/hook",
            "https://localhost/hook",
            "https://10.1.2.3/deploy",
            "https://192.168.0.7/deploy",
            "https://169.254.169.254/latest/meta-data/",
            "https://[::1]/hook",
            "https://[fd00:ec2::254]/latest/meta-data/",
            "https://[::ffff:10.0.0.1]/x",
            // Decimal and hex spellings of 127.0.0.1 parse to the same address.
            "https://2130706433/x",
            "https://0x7f000001/x",
        ] {
            let error = net.check(url).await.expect_err(url);
            assert!(
                error.contains("address") || error.contains("resolve"),
                "{url}: {error}"
            );
        }
        // Plain http is refused even for a public address ...
        let error = net.check("http://8.8.8.8/hook").await.unwrap_err();
        assert!(error.contains("must use https"), "{error}");
        // ... as are other schemes, credentials in the URL and garbage.
        assert!(net.check("ftp://8.8.8.8/x").await.is_err());
        assert!(net.check("file:///etc/passwd").await.is_err());
        let error = net.check("https://user:pw@8.8.8.8/x").await.unwrap_err();
        assert!(error.contains("credentials"), "{error}");
        assert!(net.check("not a url").await.is_err());
        // A public https address passes.
        assert!(net.check("https://8.8.8.8/hook").await.is_ok());
        // And building a request goes through the same check.
        assert!(
            net.request(Method::GET, "https://127.0.0.1/x", Duration::from_secs(1))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn private_targets_need_the_flag_and_http_stays_private_only() {
        let net = Net::new(NetPolicy::ALLOW_PRIVATE);
        for url in [
            "http://127.0.0.1:8080/health",
            "http://localhost:8080/health",
            "https://10.1.2.3/deploy",
            "http://192.168.0.7:9000/hook",
            "https://8.8.8.8/hook",
        ] {
            assert!(net.check(url).await.is_ok(), "{url}");
        }
        // http to a public address is still refused with the flag on.
        let error = net.check("http://8.8.8.8/hook").await.unwrap_err();
        assert!(error.contains("must use https"), "{error}");
        assert!(
            net.check("https://user:pw@127.0.0.1/x").await.is_err(),
            "credentials are never accepted"
        );
    }

    /// The connection-time half of the guard: a client built for the default
    /// policy cannot connect to a name that resolves to loopback, even when
    /// nothing validated the URL first.
    #[tokio::test]
    async fn the_resolver_refuses_names_that_point_inward() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                use tokio::io::AsyncWriteExt;
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")
                    .await;
            }
        });
        let guarded = Net::new(NetPolicy::PUBLIC_ONLY);
        let error = guarded
            .http
            .get(format!("http://localhost:{port}/"))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .expect_err("loopback must be refused at connection time");
        let text = transport_error(error);
        assert!(text.contains("loopback"), "{text}");

        let open = Net::new(NetPolicy::ALLOW_PRIVATE);
        let response = open
            .request(
                Method::GET,
                &format!("http://localhost:{port}/"),
                Duration::from_secs(5),
            )
            .await
            .unwrap()
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
    }

    #[test]
    fn local_hosts_are_recognised_for_plain_http_registries() {
        for host in [
            "localhost",
            "127.0.0.1",
            "10.0.0.4",
            "[::1]",
            "registry.localhost",
        ] {
            assert!(is_local_host(host), "{host}");
        }
        for host in ["ghcr.io", "8.8.8.8", "myorg.azurecr.io"] {
            assert!(!is_local_host(host), "{host}");
        }
    }

    #[test]
    fn messages_never_carry_a_query_string_or_credentials() {
        let url =
            Url::parse("https://user:pw@ci.example.com/job/x/build?token=s3cret#frag").unwrap();
        assert_eq!(redact_url(&url), "https://ci.example.com/job/x/build");
        assert_eq!(excerpt("  short  ", 10), "short");
        assert_eq!(excerpt("abcdefghij-more", 10), "abcdefghij...");
    }
}
