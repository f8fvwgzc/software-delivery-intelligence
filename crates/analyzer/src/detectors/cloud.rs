//! Clouds that IaC and CI did not already prove are inferred from SDKs and
//! documentation with lower confidence. Third-party APIs become
//! `external_api` nodes the service USES.

use crate::detectors::{Ctx, attrs};
use crate::model::*;
use crate::util::{display, line_with};

/// (ecosystem, dependency prefix, cloud)
const CLOUD_SDKS: &[(&str, &str, &str)] = &[
    ("npm", "@azure/", "azure"),
    ("npm", "@aws-sdk/", "aws"),
    ("npm", "aws-sdk", "aws"),
    ("npm", "@google-cloud/", "gcp"),
    ("cargo", "azure_", "azure"),
    ("cargo", "aws-sdk-", "aws"),
    ("cargo", "google-cloud-", "gcp"),
    ("pip", "azure-", "azure"),
    ("pip", "boto3", "aws"),
    ("pip", "google-cloud-", "gcp"),
    ("go", "github.com/azure/", "azure"),
    ("go", "github.com/aws/aws-sdk-go", "aws"),
    ("go", "cloud.google.com/go", "gcp"),
    ("maven", "com.azure:", "azure"),
    ("maven", "software.amazon.awssdk:", "aws"),
    ("maven", "com.google.cloud:", "gcp"),
];

/// (ecosystem, dependency prefix, api key, display name)
const EXTERNAL_APIS: &[(&str, &str, &str, &str)] = &[
    ("npm", "stripe", "stripe", "Stripe"),
    ("npm", "@sendgrid/mail", "sendgrid", "SendGrid"),
    ("npm", "twilio", "twilio", "Twilio"),
    ("npm", "openai", "openai", "OpenAI"),
    ("npm", "@anthropic-ai/sdk", "anthropic", "Anthropic"),
    ("npm", "@octokit/", "github", "GitHub API"),
    ("npm", "@slack/", "slack", "Slack"),
    ("npm", "@sentry/", "sentry", "Sentry"),
    ("npm", "firebase", "firebase", "Firebase"),
    ("npm", "@auth0/", "auth0", "Auth0"),
    ("npm", "@clerk/", "clerk", "Clerk"),
    ("npm", "algoliasearch", "algolia", "Algolia"),
    ("cargo", "octocrab", "github", "GitHub API"),
    ("cargo", "async-stripe", "stripe", "Stripe"),
    ("cargo", "sentry", "sentry", "Sentry"),
    ("pip", "stripe", "stripe", "Stripe"),
    ("pip", "openai", "openai", "OpenAI"),
    ("pip", "anthropic", "anthropic", "Anthropic"),
    ("pip", "sendgrid", "sendgrid", "SendGrid"),
    ("pip", "twilio", "twilio", "Twilio"),
    ("pip", "pygithub", "github", "GitHub API"),
    ("pip", "sentry-sdk", "sentry", "Sentry"),
    ("pip", "slack-sdk", "slack", "Slack"),
    ("go", "github.com/stripe/stripe-go", "stripe", "Stripe"),
    ("go", "github.com/google/go-github", "github", "GitHub API"),
    ("maven", "com.stripe:", "stripe", "Stripe"),
    ("bundler", "stripe", "stripe", "Stripe"),
    ("composer", "stripe/stripe-php", "stripe", "Stripe"),
];

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    for m in ctx.manifests.clone() {
        let content = ctx.file(&m.path).unwrap_or("");
        let app_key = ctx
            .app_for_manifest(&m.path)
            .map(|a| a.key.clone())
            .or_else(|| ctx.primary_service_key());
        for d in &m.deps {
            for (eco, prefix, cloud) in CLOUD_SDKS {
                if m.ecosystem == *eco && d.name.starts_with(prefix) {
                    let key = format!("cloud:{cloud}");
                    if !ctx.has_fact(&key) {
                        ctx.inferred(
                            FactCategory::Cloud,
                            &key,
                            &display(cloud),
                            &format!("SDK {} in {}", d.name, m.path),
                            0.6,
                            line_with(content, &d.name).unwrap_or(&d.name),
                        );
                    }
                }
            }
            for (eco, prefix, api, name) in EXTERNAL_APIS {
                if m.ecosystem != *eco || !d.name.starts_with(prefix) {
                    continue;
                }
                let key = format!("external_api:{api}");
                if !ctx.has_node(&key) {
                    ctx.node(
                        NodeKind::ExternalApi,
                        &key,
                        name,
                        &format!("{name} via {}", d.name),
                        &m.path,
                        attrs(&[("dependency", &d.name)]),
                    );
                    ctx.detected(
                        FactCategory::Dependency,
                        &key,
                        "External API",
                        name,
                        &m.path,
                        line_with(content, &d.name).unwrap_or(&d.name),
                    );
                }
                if let Some(app) = &app_key {
                    ctx.edge(app, &key, "USES", RelationCategory::Source);
                }
            }
        }
    }

    // A README that names a host is weak but real evidence.
    if let Some((path, readme)) = ctx
        .files_where(|p| p.to_ascii_lowercase().starts_with("readme"))
        .into_iter()
        .next()
    {
        let lower = readme.to_ascii_lowercase();
        for (needle, cloud) in [
            ("azure", "azure"),
            ("aws", "aws"),
            ("google cloud", "gcp"),
            ("vercel", "vercel"),
            ("netlify", "netlify"),
            ("fly.io", "fly"),
            ("heroku", "heroku"),
        ] {
            if !lower.contains(needle) {
                continue;
            }
            let hosted =
                lower.contains("deploy") || lower.contains("hosted") || lower.contains("runs on");
            let key = format!("deploy:mentioned:{cloud}");
            if hosted && !ctx.has_fact(&key) {
                ctx.inferred(
                    FactCategory::Deployment,
                    &key,
                    "Deployment target mentioned",
                    &display(cloud),
                    0.4,
                    &format!("{path} mentions {needle}"),
                );
            }
        }
    }
}
