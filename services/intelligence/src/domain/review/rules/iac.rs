//! Infrastructure as code: ingress open to the world, public buckets,
//! resources without tags, and Terraform without a remote state backend.

use std::sync::LazyLock;

use regex::Regex;

use super::{Finding, Severity, evidence};
use crate::domain::review::input::ReviewInput;

static OPEN_CIDR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:"|')?(?:0\.0\.0\.0/0|::/0|\*)(?:"|')?"#).expect("cidr"));
static INGRESS_CONTEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)cidr|source|ingress|from_port|sourceAddressPrefix|SourceAddressPrefix|CidrIp|allow|inbound")
        .expect("ingress context")
});
static PUBLIC_BUCKET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)acl\s*=\s*["']public-read(?:-write)?["']|block_public_acls\s*=\s*false|block_public_policy\s*=\s*false|restrict_public_buckets\s*=\s*false|ignore_public_acls\s*=\s*false|allowBlobPublicAccess\s*[:=]\s*true|publicAccess\s*[:=]\s*['"]?(?:blob|container)|uniform_bucket_level_access\s*=\s*false|"Principal"\s*:\s*"\*"|allUsers|public_network_access_enabled\s*=\s*true|publicNetworkAccess\s*[:=]\s*['"]?Enabled"#,
    )
    .expect("public bucket")
});
static RESOURCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*resource\s+"(aws_|azurerm_|google_)([\w]+)"\s+"([\w-]+)""#).expect("resource")
});

/// Resource types that cannot carry tags and must not be flagged for it.
fn untaggable(kind: &str) -> bool {
    kind.contains("_policy")
        || kind.contains("_attachment")
        || kind.contains("_association")
        || kind.contains("_rule")
        || kind.contains("_version")
        || kind.contains("_membership")
        || kind.contains("_role_policy")
        || kind.ends_with("_object")
        || kind.contains("_notification")
        || kind.contains("_target")
        || kind.contains("_permission")
        || kind.contains("_route_table_")
        || kind == "aws_route"
        || kind.contains("resource_group_template")
}

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in input.files.iter().filter(|f| f.is_iac() && !f.is_removed()) {
        let path = &file.path;
        let added = file.added_lines();
        let text = file.added_text();
        let terraform = file.extension() == "tf";

        for (line_no, line) in &added {
            if OPEN_CIDR.is_match(line)
                && INGRESS_CONTEXT.is_match(line)
                && !line.contains("egress")
            {
                findings.push(Finding::new(
                    "iac",
                    "open_ingress",
                    Severity::High,
                    path,
                    *line_no,
                    "Ingress open to the internet",
                    format!(
                        "Line {line_no} allows traffic from anywhere: {}. Restrict the source to the networks that \
                         need access (a load balancer's security group, an office range, a VPN).",
                        evidence(line)
                    ),
                ));
            }
            if PUBLIC_BUCKET.is_match(line) {
                findings.push(Finding::new(
                    "iac",
                    "public_bucket",
                    Severity::High,
                    path,
                    *line_no,
                    "Storage reachable by the public",
                    format!(
                        "Line {line_no} makes storage publicly accessible: {}. Keep public access blocked and serve \
                         content through a CDN or signed URLs unless the data is meant to be public.",
                        evidence(line)
                    ),
                ));
            }
        }

        if terraform {
            // Tags: every resource block added in this file, as a whole, must mention tags.
            let mentions_tags = text.contains("tags") || text.contains("default_tags");
            let resources: Vec<(u32, String)> = added
                .iter()
                .filter_map(|(n, l)| {
                    RESOURCE
                        .captures(l)
                        .map(|c| (*n, format!("{}{}", &c[1], &c[2])))
                })
                .filter(|(_, kind)| !untaggable(kind))
                .collect();
            if !resources.is_empty() && !mentions_tags {
                let (line_no, kind) = &resources[0];
                findings.push(Finding::new(
                    "iac",
                    "missing_tags",
                    Severity::Low,
                    path,
                    *line_no,
                    "Resources without tags",
                    format!(
                        "`{path}` adds {} resource{} (first: `{kind}` at line {line_no}) and never sets `tags`. Tags are how \
                         cost, ownership and environment are tracked; add them per resource or through provider `default_tags`.",
                        resources.len(),
                        if resources.len() == 1 { "" } else { "s" }
                    ),
                ));
            }

            // State: a new terraform block without a backend keeps state on the author's disk.
            let terraform_block = added
                .iter()
                .find(|(_, l)| l.trim_start().starts_with("terraform {"));
            let has_backend = text.contains("backend \"")
                || text.contains("backend '")
                || text.contains("cloud {");
            if let Some((line_no, _)) = terraform_block
                && !has_backend
                && !input.has_file(|f| {
                    f.extension() == "tf"
                        && f.path != *path
                        && (f.added_text().contains("backend \"")
                            || f.added_text().contains("cloud {"))
                })
            {
                findings.push(
                    Finding::new(
                        "iac",
                        "no_remote_state",
                        Severity::Medium,
                        path,
                        *line_no,
                        "Terraform without a remote state backend",
                        format!(
                            "`{path}` declares a `terraform {{}}` block with no `backend` (or `cloud`) block, so state stays \
                             local and unlocked. Configure a remote backend with locking before anyone else applies this."
                        ),
                    )
                    .with_suggestion("terraform {\n  backend \"s3\" {\n    bucket         = \"<state-bucket>\"\n    key            = \"<project>/terraform.tfstate\"\n    region         = \"<region>\"\n    dynamodb_table = \"<lock-table>\"\n    encrypt        = true\n  }\n}"),
                );
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;

    #[test]
    fn open_ingress_in_terraform_and_bicep() {
        let findings = analyze(&input(vec![
            added(
                "infra/sg.tf",
                &[
                    "resource \"aws_security_group_rule\" \"ssh\" {",
                    "  type        = \"ingress\"",
                    "  cidr_blocks = [\"0.0.0.0/0\"]",
                    "  tags = {}",
                    "}",
                ],
            ),
            added(
                "infra/nsg.bicep",
                &[
                    "        sourceAddressPrefix: '*'",
                    "        destinationAddressPrefix: '*'",
                ],
            ),
        ]));
        assert_eq!(
            rules(&findings),
            vec!["iac.open_ingress", "iac.open_ingress"]
        );
        assert_eq!(findings[0].line, 3);
        assert_eq!(findings[1].line, 1);
    }

    #[test]
    fn public_buckets() {
        let findings = analyze(&input(vec![
            added(
                "infra/s3.tf",
                &["  acl = \"public-read\"", "  tags = { env = \"dev\" }"],
            ),
            added("infra/storage.bicep", &["    allowBlobPublicAccess: true"]),
            added(
                "infra/bucket.tf",
                &["  block_public_acls = false", "  tags = {}"],
            ),
        ]));
        assert_eq!(
            rules(&findings),
            vec![
                "iac.public_bucket",
                "iac.public_bucket",
                "iac.public_bucket"
            ]
        );
    }

    #[test]
    fn missing_tags_and_local_state() {
        let findings = analyze(&input(vec![added(
            "infra/main.tf",
            &[
                "terraform {",
                "  required_version = \">= 1.5\"",
                "}",
                "resource \"aws_instance\" \"web\" {",
                "  ami = \"ami-123\"",
                "}",
                "resource \"aws_iam_role_policy\" \"p\" {}",
            ],
        )]));
        assert_eq!(
            rules(&findings),
            vec!["iac.missing_tags", "iac.no_remote_state"]
        );
        assert_eq!(findings[0].line, 4);
        assert!(findings[0].detail.contains("1 resource "));
        assert!(
            findings[1]
                .suggestion
                .as_deref()
                .unwrap()
                .contains("backend")
        );
    }

    #[test]
    fn scoped_tagged_remote_state_is_clean() {
        let findings = analyze(&input(vec![
            added(
                "infra/main.tf",
                &[
                    "terraform {",
                    "  backend \"s3\" { bucket = \"x\" }",
                    "}",
                    "resource \"aws_security_group_rule\" \"ssh\" {",
                    "  type = \"ingress\"",
                    "  cidr_blocks = [\"10.0.0.0/8\"]",
                    "  egress { cidr_blocks = [\"0.0.0.0/0\"] }",
                    "}",
                    "resource \"aws_s3_bucket\" \"logs\" {",
                    "  tags = { owner = \"platform\" }",
                    "}",
                ],
            ),
            added("src/main.rs", &["let any = \"0.0.0.0/0\"; // ingress"]),
        ]));
        assert!(findings.is_empty(), "{findings:?}");
    }
}
