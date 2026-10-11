//! `.github/dependabot.yml`: weekly grouped updates for every ecosystem the
//! scan found, so dependency updates arrive as a few reviewable pull
//! requests instead of one per package.

use super::{GenInput, Stack, artifact};
use crate::domain::model::ArtifactData;

pub fn generate(input: &GenInput) -> Vec<ArtifactData> {
    vec![artifact(
        input,
        ".github/dependabot.yml",
        "yaml",
        format!(
            "Weekly grouped Dependabot updates for {}.",
            ecosystems(input).join(", ")
        ),
        dependabot(input),
    )]
}

/// Ecosystems in Dependabot's own vocabulary; GitHub Actions is always
/// included because the generated workflows pin action versions.
fn ecosystems(input: &GenInput) -> Vec<String> {
    let mut out: Vec<String> = input.ecosystems.clone();
    if !out.iter().any(|e| e == "github-actions") {
        out.push("github-actions".to_owned());
    }
    out
}

/// Directories Dependabot must watch for one ecosystem: every workspace
/// package of that stack in a monorepo, the image directory for `docker`,
/// the root otherwise. Dependabot ignores a manifest it is not pointed at.
fn directories(input: &GenInput, ecosystem: &str) -> Vec<String> {
    let stacks: &[Stack] = match ecosystem {
        "npm" => &[Stack::Node, Stack::Static],
        "cargo" => &[Stack::Rust],
        "pip" | "uv" | "poetry" => &[Stack::Python],
        "gomod" => &[Stack::Go],
        "maven" | "gradle" => &[Stack::Java],
        "nuget" => &[Stack::DotNet],
        "composer" => &[Stack::Php],
        "bundler" => &[Stack::Ruby],
        "docker" => return vec![format!("/{}", input.app_dir)],
        _ => return vec!["/".to_owned()],
    };
    let dirs: Vec<String> = input
        .packages
        .iter()
        .filter(|p| stacks.contains(&p.stack))
        .map(|p| format!("/{}", p.dir))
        .collect();
    if dirs.is_empty() {
        vec!["/".to_owned()]
    } else {
        dirs
    }
}

fn dependabot(input: &GenInput) -> String {
    let mut out = String::from("version: 2\nupdates:\n");
    for ecosystem in ecosystems(input) {
        for directory in directories(input, &ecosystem) {
            let groups = match ecosystem.as_str() {
                "npm" => {
                    "      production:\n        dependency-type: \"production\"\n      development:\n        dependency-type: \"development\"\n"
                }
                "github-actions" => "      actions:\n        patterns: [\"*\"]\n",
                _ => "      all:\n        patterns: [\"*\"]\n",
            };
            out.push_str(&format!(
            "  - package-ecosystem: \"{ecosystem}\"\n    directory: \"{directory}\"\n    schedule:\n      interval: \"weekly\"\n      day: \"monday\"\n    open-pull-requests-limit: 5\n    groups:\n{groups}"
        ));
            if ecosystem != "github-actions" {
                out.push_str("    ignore:\n      - dependency-name: \"*\"\n        update-types: [\"version-update:semver-major\"]\n");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{input::fixtures, snapshot::assert_snapshot};
    use super::*;

    #[test]
    fn dependabot_snapshot() {
        let files = generate(&fixtures::input(&fixtures::node_context(), ""));
        assert_snapshot("dependabot_node", &files[0]);
        assert!(files[0].content.contains("package-ecosystem: \"npm\""));
        assert!(files[0].content.contains("package-ecosystem: \"docker\""));
        assert!(
            files[0]
                .content
                .contains("package-ecosystem: \"github-actions\"")
        );
        let rust = generate(&fixtures::input(&fixtures::rust_context(), ""));
        assert!(rust[0].content.contains("package-ecosystem: \"cargo\""));
        assert_eq!(rust[0].content.matches("github-actions").count(), 1);
    }

    #[test]
    fn monorepo_points_each_ecosystem_at_its_packages() {
        let mut input = fixtures::input(&fixtures::node_context(), "");
        input.app_dir = "backend".into();
        input.ecosystems = vec!["npm".into(), "cargo".into(), "docker".into()];
        input.packages = vec![
            super::super::Package {
                dir: "backend".into(),
                name: "api".into(),
                is_library: false,
                stack: Stack::Rust,
                package_manager: "cargo".into(),
                framework: "axum".into(),
            },
            super::super::Package {
                dir: "frontend".into(),
                name: "ui".into(),
                is_library: false,
                stack: Stack::Node,
                package_manager: "npm".into(),
                framework: "react".into(),
            },
        ];
        let yaml = &generate(&input)[0].content;
        assert!(yaml.contains("package-ecosystem: \"npm\"\n    directory: \"/frontend\""));
        assert!(yaml.contains("package-ecosystem: \"cargo\"\n    directory: \"/backend\""));
        assert!(yaml.contains("package-ecosystem: \"docker\"\n    directory: \"/backend\""));
        assert!(yaml.contains("package-ecosystem: \"github-actions\"\n    directory: \"/\""));
    }
}
