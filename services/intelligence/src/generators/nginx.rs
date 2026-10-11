//! `deploy/nginx/default.conf`: either a reverse proxy in front of the app
//! (VM deployments) or the static-site server the unprivileged nginx image
//! loads. Listens on 8080 so it never needs root.

use super::{GenInput, Stack, artifact};
use crate::domain::model::ArtifactData;

pub fn generate(input: &GenInput) -> Vec<ArtifactData> {
    let (purpose, content) = if input.stack == Stack::Static {
        (
            "Serves the built static site with long-cache hashed assets and an SPA fallback.",
            static_conf(input),
        )
    } else {
        (
            "Reverse proxy in front of the application container with gzip, security headers and a health route.",
            proxy_conf(input),
        )
    };
    vec![artifact(
        input,
        "deploy/nginx/default.conf",
        "nginx",
        purpose,
        content,
    )]
}

const SECURITY_HEADERS: &str = "    add_header X-Content-Type-Options nosniff always;
    add_header X-Frame-Options DENY always;
    add_header Referrer-Policy strict-origin-when-cross-origin always;
";

fn proxy_conf(input: &GenInput) -> String {
    format!(
        "upstream app {{
    server app:{port} max_fails=3 fail_timeout=10s;
    keepalive 32;
}}

server {{
    listen 8080;
    server_name _;
    server_tokens off;

    gzip on;
    gzip_types text/plain text/css application/json application/javascript image/svg+xml;
{headers}
    location = /nginx-health {{
        access_log off;
        return 200 'ok';
    }}

    location / {{
        proxy_pass http://app;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Connection \"\";
        proxy_read_timeout 60s;
    }}
}}
",
        port = input.port,
        headers = SECURITY_HEADERS,
    )
}

fn static_conf(_input: &GenInput) -> String {
    format!(
        "server {{
    listen 8080;
    server_name _;
    server_tokens off;
    root /usr/share/nginx/html;
    index index.html;

    gzip on;
    gzip_types text/plain text/css application/json application/javascript image/svg+xml;
{headers}
    location ~* \\.(?:js|css|woff2?|png|jpg|jpeg|gif|svg|ico)$ {{
        expires 1y;
        add_header Cache-Control \"public, immutable\";
        try_files $uri =404;
    }}

    location / {{
        try_files $uri $uri/ /index.html;
    }}
}}
",
        headers = SECURITY_HEADERS,
    )
}

#[cfg(test)]
mod tests {
    use super::super::{input::fixtures, snapshot::assert_snapshot};
    use super::*;

    #[test]
    fn proxy_and_static_snapshots() {
        let proxy = generate(&fixtures::input(&fixtures::node_context(), ""));
        assert_snapshot("nginx_proxy", &proxy[0]);
        assert!(proxy[0].content.contains("server app:3000"));
        let site = generate(&fixtures::input(&fixtures::static_context(), ""));
        assert!(site[0].content.contains("try_files $uri $uri/ /index.html"));
        assert!(!site[0].content.contains("proxy_pass"));
    }
}
