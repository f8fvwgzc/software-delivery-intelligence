//! The delivery generators end to end: snapshots per registry kind and per
//! deployer kind, every generated YAML parsed, no placeholder anywhere, and
//! the choices (registry, images, target, file names) checked one by one.

use std::collections::{BTreeMap, BTreeSet};

use sdi_proto::{
    execution::v1::{Environment, Registry},
    knowledge::v1::{Graph, GraphNode, NodeKind, ProjectConfig, ProjectContext},
};
use serde::Deserialize;

use super::{
    DeliveryFiles, for_project, generate, input,
    target::{Target, TargetChoice},
};
use crate::{
    domain::model::ArtifactData,
    generators::{
        Stack,
        input::fixtures::{self, fact},
        snapshot::assert_snapshot,
    },
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn registry(id: &str, kind: &str, host: &str, namespace: &str) -> Registry {
    Registry {
        id: id.into(),
        name: id.into(),
        kind: kind.into(),
        host: host.into(),
        namespace: namespace.into(),
        auth: match kind {
            "ghcr" => "github_token",
            "acr" | "ecr" | "gar" => "oidc",
            _ => "secret",
        }
        .into(),
        ..Default::default()
    }
}

fn ghcr() -> Registry {
    Registry {
        is_default: true,
        ..registry("reg-ghcr", "ghcr", "ghcr.io", "f8fvwgzc")
    }
}

fn environment(name: &str, position: u32, kind: &str, config: &[(&str, &str)]) -> Environment {
    Environment {
        id: format!("env-{name}"),
        project_id: "proj-1".into(),
        name: name.into(),
        position,
        kind: kind.into(),
        config: config
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        url: format!("https://{name}.view-engine.dev"),
        health_path: "/healthz".into(),
        protected: name == "production",
        auto_promote: name == "staging",
        source: "ui".into(),
        ..Default::default()
    }
}

/// The pnpm service of the generator fixtures, deployed to Kubernetes
/// with its own manifests under `deploy/k8s`.
fn kubernetes_context() -> ProjectContext {
    let mut ctx = fixtures::node_context();
    ctx.facts.push(fact(
        "k1",
        "k8s:manifests",
        "deploy/k8s/deployment.yaml",
        "deploy/k8s/deployment.yaml",
    ));
    ctx.capabilities.push("k8s:manifests".into());
    ctx.graph = Some(Graph {
        nodes: ["Deployment.view-engine", "Service.view-engine"]
            .iter()
            .map(|object| GraphNode {
                id: (*object).to_owned(),
                kind: NodeKind::Infrastructure as i32,
                key: format!("infrastructure:k8s/{object}"),
                source_path: format!(
                    "deploy/k8s/{}.yaml",
                    object.split('.').next().unwrap().to_lowercase()
                ),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    });
    ctx
}

fn find<'a>(files: &'a DeliveryFiles, path: &str) -> &'a ArtifactData {
    files
        .artifacts
        .iter()
        .find(|a| a.path == path)
        .unwrap_or_else(|| panic!("no {path} among {:?}", paths(files)))
}

fn paths(files: &DeliveryFiles) -> Vec<&str> {
    files.artifacts.iter().map(|a| a.path.as_str()).collect()
}

/// Every document of a YAML file, or a panic naming the file.
fn parse_yaml(artifact: &ArtifactData) -> Vec<serde_yaml_ng::Value> {
    serde_yaml_ng::Deserializer::from_str(&artifact.content)
        .map(|doc| {
            serde_yaml_ng::Value::deserialize(doc).unwrap_or_else(|error| {
                panic!(
                    "{} is not valid YAML: {error}\n{}",
                    artifact.path, artifact.content
                )
            })
        })
        .collect()
}

/// Helm templates are Go templates around YAML, not YAML.
fn is_plain_yaml(artifact: &ArtifactData) -> bool {
    artifact.language == "yaml" && !artifact.path.contains("/templates/")
}

/// What would make a file something a person has to finish.
const PLACEHOLDERS: &[&str] = &[
    "TODO",
    "FIXME",
    "XXX",
    "placeholder",
    "PLACEHOLDER",
    "changeme",
    "CHANGEME",
    "CHANGE_ME",
    "REPLACE_ME",
    "REPLACE-ME",
    "<your",
    "<YOUR",
    "your-",
    "YOUR_",
    "example.com",
    "example.org",
    "my-app",
    "myapp",
    "lorem",
];

/// The invariants of every generated set: unique paths, a purpose, a final
/// newline, no placeholder, every YAML parses, every action pinned to a
/// major tag and one version per action across the files.
fn assert_complete(files: &DeliveryFiles) {
    let mut seen = BTreeSet::new();
    let mut versions: BTreeMap<String, String> = BTreeMap::new();
    for artifact in &files.artifacts {
        assert!(seen.insert(&artifact.path), "{} twice", artifact.path);
        assert!(
            !artifact.purpose.is_empty(),
            "{} has no purpose",
            artifact.path
        );
        assert!(
            artifact.content.ends_with('\n') && !artifact.content.ends_with("\n\n"),
            "{} must end with exactly one newline",
            artifact.path
        );
        for placeholder in PLACEHOLDERS {
            assert!(
                !artifact.content.contains(placeholder),
                "{} contains `{placeholder}`:\n{}",
                artifact.path,
                artifact.content
            );
        }
        if is_plain_yaml(artifact) {
            let docs = parse_yaml(artifact);
            assert!(!docs.is_empty(), "{} is empty", artifact.path);
            assert!(
                docs.iter().all(serde_yaml_ng::Value::is_mapping),
                "{} has a document that is not a mapping",
                artifact.path
            );
        }
        for line in artifact.content.lines() {
            let Some(action) = line.trim().trim_start_matches("- ").strip_prefix("uses: ") else {
                continue;
            };
            let (name, version) = action
                .rsplit_once('@')
                .unwrap_or_else(|| panic!("{}: `{action}` is not pinned", artifact.path));
            assert!(
                version.len() >= 2
                    && version.starts_with('v')
                    && version[1..].chars().all(|c| c.is_ascii_digit()),
                "{}: `{action}` must be pinned to a major tag",
                artifact.path
            );
            let known = versions
                .entry(name.to_owned())
                .or_insert(version.to_owned());
            assert_eq!(known, version, "{name} is pinned to two versions");
        }
    }
    for note in &files.notes {
        assert!(note.ends_with('.'), "a note is a sentence: {note}");
    }
    let mut secrets = BTreeSet::new();
    for secret in &files.required_secrets {
        assert!(secrets.insert(secret), "{secret} listed twice");
        // A secret that is listed is referenced by a file.
        let base = secret
            .rsplit_once('-')
            .map(|(head, _)| head)
            .filter(|_| secret.chars().any(|c| c.is_ascii_lowercase()))
            .unwrap_or(secret);
        assert!(
            files.artifacts.iter().any(|a| a.content.contains(base)),
            "{secret} is required but no file references it"
        );
    }
}

fn workflow_secrets(artifact: &ArtifactData) -> BTreeSet<String> {
    regex::Regex::new(r"secrets\.([A-Z0-9_]+)")
        .unwrap()
        .captures_iter(&artifact.content)
        .map(|c| c[1].to_owned())
        .filter(|s| s != "GITHUB_TOKEN")
        .collect()
}

// ---------------------------------------------------------------------------
// build.yml
// ---------------------------------------------------------------------------

#[test]
fn build_workflow_snapshot_per_registry_kind() {
    let ctx = fixtures::node_context();
    let cases: Vec<(&str, Registry, Vec<&str>, Vec<&str>)> = vec![
        (
            "ghcr",
            ghcr(),
            vec![
                "registry: ghcr.io",
                "username: ${{ github.actor }}",
                "password: ${{ secrets.GITHUB_TOKEN }}",
                "            ghcr.io/f8fvwgzc/view-engine\n",
            ],
            vec![],
        ),
        (
            "acr",
            registry("reg-acr", "acr", "acme.azurecr.io", ""),
            vec![
                "uses: azure/login@v2",
                "client-id: ${{ secrets.AZURE_CLIENT_ID }}",
                "run: az acr login --name acme",
                "            acme.azurecr.io/view-engine\n",
            ],
            vec![
                "AZURE_CLIENT_ID",
                "AZURE_TENANT_ID",
                "AZURE_SUBSCRIPTION_ID",
            ],
        ),
        (
            "ecr",
            registry(
                "reg-ecr",
                "ecr",
                "123456789012.dkr.ecr.eu-west-1.amazonaws.com",
                "",
            ),
            vec![
                "uses: aws-actions/configure-aws-credentials@v4",
                "role-to-assume: ${{ secrets.AWS_BUILD_ROLE_ARN }}",
                "aws-region: eu-west-1",
                "uses: aws-actions/amazon-ecr-login@v2",
                "            123456789012.dkr.ecr.eu-west-1.amazonaws.com/view-engine\n",
            ],
            vec!["AWS_BUILD_ROLE_ARN"],
        ),
        (
            "gar",
            registry(
                "reg-gar",
                "gar",
                "europe-west1-docker.pkg.dev",
                "acme-prod/containers",
            ),
            vec![
                "uses: google-github-actions/auth@v2",
                "token_format: access_token",
                "workload_identity_provider: ${{ secrets.GCP_WORKLOAD_IDENTITY_PROVIDER }}",
                "registry: europe-west1-docker.pkg.dev",
                "username: oauth2accesstoken",
                "password: ${{ steps.gcp-auth.outputs.access_token }}",
                "            europe-west1-docker.pkg.dev/acme-prod/containers/view-engine\n",
            ],
            vec![
                "GCP_WORKLOAD_IDENTITY_PROVIDER",
                "GCP_BUILD_SERVICE_ACCOUNT",
            ],
        ),
        (
            "dockerhub",
            Registry {
                username_secret: "dockerhub-username".into(),
                password_secret: "dockerhub-token".into(),
                ..registry("reg-hub", "dockerhub", "docker.io", "acme")
            },
            vec![
                "name: Log in to Docker Hub",
                "username: ${{ secrets.DOCKERHUB_USERNAME }}",
                "password: ${{ secrets.DOCKERHUB_TOKEN }}",
                "            docker.io/acme/view-engine\n",
            ],
            vec!["DOCKERHUB_USERNAME", "DOCKERHUB_TOKEN"],
        ),
        (
            "generic",
            Registry {
                username_secret: "registry.user".into(),
                password_secret: "registry-password".into(),
                ..registry("reg-own", "generic", "registry.acme.dev:5000", "team")
            },
            vec![
                "name: Log in to registry.acme.dev:5000",
                "registry: registry.acme.dev:5000",
                "username: ${{ secrets.REGISTRY_USER }}",
                "password: ${{ secrets.REGISTRY_PASSWORD }}",
                "            registry.acme.dev:5000/team/view-engine\n",
            ],
            vec!["REGISTRY_USER", "REGISTRY_PASSWORD"],
        ),
    ];
    for (kind, registry, expected, secrets) in cases {
        let registry = Registry {
            is_default: true,
            ..registry
        };
        let files = for_project(&ctx, &[registry], &[]);
        assert_complete(&files);
        // Without environments only the build is set up.
        assert_eq!(paths(&files), [".github/workflows/build.yml"], "{kind}");
        let build = &files.artifacts[0];
        assert_snapshot(&format!("delivery_build_{kind}"), build);
        for needle in expected {
            assert!(
                build.content.contains(needle),
                "{kind}: missing `{needle}` in\n{}",
                build.content
            );
        }
        assert_eq!(files.required_secrets, secrets, "{kind}");
        // Every secret a workflow reads is listed, and nothing else is.
        assert_eq!(
            workflow_secrets(build),
            secrets.iter().map(|s| (*s).to_owned()).collect(),
            "{kind}"
        );
        assert!(
            !build.content.contains("AWS_ACCESS_KEY_ID")
                && !build.content.contains("client-secret"),
            "{kind}: a long-lived cloud credential"
        );
    }
}

#[test]
fn build_workflow_has_the_triggers_permissions_tags_and_outputs_a_release_needs() {
    let files = for_project(&fixtures::node_context(), &[ghcr()], &[]);
    let build = find(&files, ".github/workflows/build.yml");
    assert!(!build.replaces_existing);
    let doc = &parse_yaml(build)[0];
    // `on:` is the boolean `true` to a YAML 1.1 parser and a string to 1.2.
    let on = doc
        .get("on")
        .or_else(|| doc.get(serde_yaml_ng::Value::Bool(true)))
        .expect("triggers");
    assert_eq!(on["push"]["branches"][0], "main");
    assert_eq!(on["push"]["tags"][0], "v*");
    assert!(on.get("workflow_dispatch").is_some());
    assert!(
        on.get("pull_request").is_none(),
        "a pull request must not push"
    );
    let permissions = &doc["permissions"];
    assert_eq!(permissions["contents"], "read");
    assert_eq!(permissions["packages"], "write");
    assert_eq!(permissions["id-token"], "write");
    assert_eq!(permissions["attestations"], "write");
    assert_eq!(permissions.as_mapping().unwrap().len(), 4);

    let job = &doc["jobs"]["image"];
    assert_eq!(
        job["outputs"]["digest"],
        "${{ steps.build.outputs.digest }}"
    );
    assert_eq!(job["outputs"]["image"], "ghcr.io/f8fvwgzc/view-engine");
    let steps = job["steps"].as_sequence().unwrap();
    let step = |uses: &str| {
        steps
            .iter()
            .find(|s| s["uses"].as_str().is_some_and(|u| u.starts_with(uses)))
            .unwrap_or_else(|| panic!("no {uses} step"))
    };
    let tags = step("docker/metadata-action")["with"]["tags"]
        .as_str()
        .unwrap();
    assert_eq!(
        tags.lines().collect::<Vec<_>>(),
        [
            "type=sha,format=short,prefix=",
            "type=semver,pattern={{version}}",
            "type=raw,value=latest,enable={{is_default_branch}}"
        ]
    );
    let with = &step("docker/build-push-action")["with"];
    assert_eq!(with["push"], true);
    assert_eq!(with["context"], ".");
    assert_eq!(with["file"], "Dockerfile");
    assert_eq!(with["cache-from"], "type=gha");
    assert_eq!(with["cache-to"], "type=gha,mode=max");
    assert_eq!(with["provenance"], "mode=max");
    assert_eq!(with["sbom"], true);
    assert_eq!(with["tags"], "${{ steps.meta.outputs.tags }}");
    let attest = step("actions/attest-build-provenance");
    assert_eq!(
        attest["with"]["subject-digest"],
        "${{ steps.build.outputs.digest }}"
    );
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("delivery.build_workflow: build.yml"))
    );
}

#[test]
fn several_registries_get_a_login_each_and_one_image_list() {
    let registries = [
        ghcr(),
        registry("reg-acr", "acr", "acme.azurecr.io", ""),
        registry("reg-gar-a", "gar", "europe-west1-docker.pkg.dev", "acme/a"),
        registry("reg-gar-b", "gar", "us-central1-docker.pkg.dev", "acme/b"),
    ];
    let mut environments = vec![
        environment("staging", 0, "github_actions", &[]),
        environment("production", 1, "github_actions", &[]),
        environment("eu", 2, "github_actions", &[]),
        environment("us", 3, "github_actions", &[]),
    ];
    environments[1].registry_id = "reg-acr".into();
    environments[2].registry_id = "reg-gar-a".into();
    environments[3].registry_id = "reg-gar-b".into();
    let input = input::build(&kubernetes_context(), &registries, &environments);
    // The default first (staging relies on it), then in environment order.
    assert_eq!(
        input
            .registries
            .iter()
            .map(|r| r.host.as_str())
            .collect::<Vec<_>>(),
        [
            "ghcr.io",
            "acme.azurecr.io",
            "europe-west1-docker.pkg.dev",
            "us-central1-docker.pkg.dev"
        ]
    );
    assert_eq!(input.release_image(), "ghcr.io/f8fvwgzc/view-engine");
    assert_eq!(
        input.image_for(&input.environments[1]),
        "acme.azurecr.io/view-engine"
    );
    let files = generate(&input);
    assert_complete(&files);
    let build = find(&files, ".github/workflows/build.yml");
    for needle in [
        "Log in to GitHub Container Registry",
        "az acr login --name acme",
        "- id: gcp-auth\n",
        "- id: gcp-auth-2\n",
        "password: ${{ steps.gcp-auth-2.outputs.access_token }}",
        "            ghcr.io/f8fvwgzc/view-engine\n            acme.azurecr.io/view-engine\n            europe-west1-docker.pkg.dev/acme/a/view-engine\n            us-central1-docker.pkg.dev/acme/b/view-engine\n",
    ] {
        assert!(build.content.contains(needle), "missing `{needle}`");
    }
}

#[test]
fn the_registry_falls_back_from_environments_to_default_to_config_to_ghcr() {
    let ctx = fixtures::node_context();
    // Nothing known anywhere: GHCR under the repository's owner.
    let input = input::build(&ctx, &[], &[]);
    assert_eq!(input.registries.len(), 1);
    assert_eq!(input.release_image(), "ghcr.io/f8fvwgzc/view-engine");
    assert_eq!(input.registries[0].auth, input::RegistryAuth::GithubToken);

    // `.sdi.yml` declares one and the workspace has none.
    let mut declared = ctx.clone();
    declared.config = Some(ProjectConfig {
        present: true,
        delivery_json: r#"{"registry":{"kind":"acr","host":"acme.azurecr.io","namespace":"team"},"build_workflow":"release.yml","environments":[]}"#.into(),
        ..Default::default()
    });
    let input = input::build(&declared, &[], &[]);
    assert_eq!(input.release_image(), "acme.azurecr.io/team/view-engine");
    assert_eq!(input.build_workflow, "release.yml");
    // The declared name is used while no such workflow exists yet...
    let files = generate(&input);
    assert_eq!(paths(&files), [".github/workflows/release.yml"]);
    assert!(
        !files
            .notes
            .iter()
            .any(|n| n.contains("delivery.build_workflow"))
    );
    // ...and an existing workflow of that name is never overwritten.
    declared.config.as_mut().unwrap().delivery_json = r#"{"build_workflow":"ci.yml"}"#.into();
    let files = for_project(&declared, &[], &[]);
    assert_eq!(paths(&files), [".github/workflows/build.yml"]);
    assert!(
        files.notes.iter().any(|n| n
            .contains("names `ci.yml` as the build workflow, but that workflow pushes no image")),
        "{:#?}",
        files.notes
    );
    // A config with errors declares nothing.
    declared.config.as_mut().unwrap().errors = vec!["error: `delivery` must be a mapping".into()];
    declared.config.as_mut().unwrap().delivery_json =
        r#"{"registry":{"kind":"acr","host":"acme.azurecr.io"}}"#.into();
    assert_eq!(
        input::build(&declared, &[], &[]).release_image(),
        "ghcr.io/f8fvwgzc/view-engine"
    );

    // The workspace default beats the file; an environment's own beats both.
    let hub = Registry {
        is_default: true,
        ..registry("reg-hub", "dockerhub", "", "")
    };
    let input = input::build(&ctx, std::slice::from_ref(&hub), &[]);
    assert_eq!(input.release_image(), "docker.io/f8fvwgzc/view-engine");
    assert_eq!(input.registries[0].username_secret, "DOCKERHUB_USERNAME");
    assert_eq!(input.registries[0].password_secret, "DOCKERHUB_TOKEN");
    let mut production = environment("production", 0, "github_actions", &[]);
    production.registry_id = "reg-acr".into();
    let input = input::build(
        &ctx,
        &[hub, registry("reg-acr", "acr", "acme.azurecr.io", "")],
        &[production],
    );
    assert_eq!(
        input.registries.len(),
        1,
        "the default is not used by anyone"
    );
    assert_eq!(input.release_image(), "acme.azurecr.io/view-engine");
}

#[test]
fn registry_details_that_need_a_person_become_notes() {
    let ctx = fixtures::node_context();
    // GHCR under another owner cannot be pushed to with GITHUB_TOKEN.
    let other = Registry {
        is_default: true,
        ..registry("reg", "ghcr", "ghcr.io", "Acme-Platform")
    };
    let files = for_project(&ctx, &[other], &[]);
    assert!(
        files.artifacts[0]
            .content
            .contains("ghcr.io/acme-platform/view-engine")
    );
    assert!(
        files.notes[0].contains("`GITHUB_TOKEN` can only push under `ghcr.io/f8fvwgzc`"),
        "{:#?}",
        files.notes
    );
    // A GHCR registry with named secrets logs in with them instead.
    let pat = Registry {
        is_default: true,
        username_secret: "ghcr_user".into(),
        password_secret: "github_pat".into(),
        ..registry("reg", "ghcr", "ghcr.io", "acme-platform")
    };
    let files = for_project(&ctx, &[pat], &[]);
    assert!(
        files.artifacts[0]
            .content
            .contains("username: ${{ secrets.GHCR_USER }}")
    );
    // `GITHUB_` is reserved for GitHub's own secrets.
    assert!(
        files.artifacts[0]
            .content
            .contains("password: ${{ secrets.REGISTRY_GITHUB_PAT }}")
    );
    assert_eq!(files.required_secrets, ["GHCR_USER", "REGISTRY_GITHUB_PAT"]);

    // An ECR host whose region cannot be read asks for a variable.
    let odd = Registry {
        is_default: true,
        ..registry("reg", "ecr", "images.internal.acme.dev", "")
    };
    let files = for_project(&ctx, &[odd], &[]);
    assert!(
        files.artifacts[0]
            .content
            .contains("aws-region: ${{ vars.AWS_REGION }}")
    );
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("repository variable `AWS_REGION`"))
    );
    // The public registry authenticates in us-east-1 with its own type.
    let public = Registry {
        is_default: true,
        ..registry("reg", "ecr", "public.ecr.aws", "acme")
    };
    let content = &for_project(&ctx, &[public], &[]).artifacts[0].content;
    assert!(content.contains("aws-region: us-east-1"));
    assert!(content.contains("          registry-type: public\n"));
    assert!(content.contains("public.ecr.aws/acme/view-engine"));
    // An Artifact Registry without a namespace gets one, and says so.
    let gar = Registry {
        is_default: true,
        ..registry("reg", "gar", "europe-west1-docker.pkg.dev", "")
    };
    let files = for_project(&ctx, &[gar], &[]);
    assert!(
        files.artifacts[0]
            .content
            .contains("europe-west1-docker.pkg.dev/f8fvwgzc/view-engine/view-engine")
    );
    assert!(files.notes[0].contains("`<project id>/<repository>`"));
}

#[test]
fn a_monorepo_builds_one_job_per_image_building_package() {
    let mut ctx = fixtures::rust_context();
    ctx.capabilities.push("monorepo".into());
    // Not a Cargo workspace: the crate's manifest sits in `backend`, so each
    // package keeps its own directory.
    for fact in ctx
        .facts
        .iter_mut()
        .filter(|f| f.source_path.starts_with("Cargo."))
    {
        fact.source_path = format!("backend/{}", fact.source_path);
    }
    ctx.facts.extend([
        fact(
            "d1",
            "container:dockerfile:backend/Dockerfile",
            "",
            "backend/Dockerfile",
        ),
        fact(
            "d2",
            "container:dockerfile:web/Dockerfile",
            "",
            "web/Dockerfile",
        ),
        fact(
            "d3",
            "container:dockerfile:services/worker/Dockerfile",
            "",
            "services/worker/Dockerfile",
        ),
        // Not images of the application.
        fact(
            "d4",
            "container:dockerfile:web/Dockerfile.dev",
            "",
            "web/Dockerfile.dev",
        ),
        fact(
            "d5",
            "container:dockerfile:.devcontainer/Dockerfile",
            "",
            ".devcontainer/Dockerfile",
        ),
        fact(
            "d6",
            "container:dockerfile:tests/e2e/Dockerfile",
            "",
            "tests/e2e/Dockerfile",
        ),
    ]);
    let node = |kind: NodeKind, name: &str, dir: &str, pm: &str, framework: &str| GraphNode {
        id: name.into(),
        kind: kind as i32,
        key: format!("service:{name}"),
        label: name.into(),
        attributes: [
            ("directory", dir),
            ("package_manager", pm),
            ("frameworks", framework),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect(),
        ..Default::default()
    };
    ctx.graph = Some(Graph {
        nodes: vec![
            node(NodeKind::Service, "api", "backend", "cargo", "axum"),
            node(NodeKind::Service, "console", "web", "pnpm", "react"),
            node(NodeKind::Service, "mailer", "services/worker", "pip", ""),
            node(NodeKind::Library, "shared", "libs/shared", "cargo", ""),
        ],
        ..Default::default()
    });
    let input = input::build(&ctx, &[], &[]);
    assert_eq!(
        input
            .base
            .packages
            .iter()
            .map(|p| p.dir.as_str())
            .collect::<Vec<_>>(),
        ["backend", "libs/shared", "services/worker", "web"],
        "the Package logic of the CI generator"
    );
    assert_eq!(input.base.app_dir, "backend");
    let images: Vec<(&str, &str, &str, &str, bool)> = input
        .images
        .iter()
        .map(|i| {
            (
                i.job.as_str(),
                i.name.as_str(),
                i.context.as_str(),
                i.dockerfile.as_str(),
                i.primary,
            )
        })
        .collect();
    assert_eq!(
        images,
        [
            // The server package is the image a release is cut from, so it
            // carries the repository's name.
            (
                "image-texc-symphony",
                "texc-symphony",
                "./backend",
                "backend/Dockerfile",
                true
            ),
            (
                "image-mailer",
                "texc-symphony-mailer",
                "./services/worker",
                "services/worker/Dockerfile",
                false
            ),
            (
                "image-console",
                "texc-symphony-console",
                "./web",
                "web/Dockerfile",
                false
            ),
        ]
    );
    let files = generate(&input);
    assert_complete(&files);
    let build = find(&files, ".github/workflows/build.yml");
    let doc = &parse_yaml(build)[0];
    let jobs = doc["jobs"].as_mapping().unwrap();
    assert_eq!(
        jobs.keys().map(|k| k.as_str().unwrap()).collect::<Vec<_>>(),
        ["image-texc-symphony", "image-mailer", "image-console"]
    );
    for (job, context, dockerfile, image) in [
        (
            "image-texc-symphony",
            "./backend",
            "backend/Dockerfile",
            "ghcr.io/f8fvwgzc/texc-symphony",
        ),
        (
            "image-console",
            "./web",
            "web/Dockerfile",
            "ghcr.io/f8fvwgzc/texc-symphony-console",
        ),
    ] {
        let steps = doc["jobs"][job]["steps"].as_sequence().unwrap();
        let build_step = steps
            .iter()
            .find(|s| s["id"] == "build")
            .unwrap_or_else(|| panic!("{job} has no build step"));
        assert_eq!(build_step["with"]["context"], context);
        assert_eq!(build_step["with"]["file"], dockerfile);
        // One cache scope per image, or they evict each other.
        assert_eq!(
            build_step["with"]["cache-to"],
            format!("type=gha,mode=max,scope={job}")
        );
        assert_eq!(doc["jobs"][job]["outputs"]["image"], image);
    }
    // Nothing is generated for a Dockerfile that exists.
    assert!(!paths(&files).iter().any(|p| p.ends_with("Dockerfile")));
    assert_eq!(input.release_image(), "ghcr.io/f8fvwgzc/texc-symphony");
}

#[test]
fn a_missing_dockerfile_is_generated_and_a_tooling_directory_builds_the_root() {
    // No Dockerfile in the scan: the build would have nothing to build.
    let files = for_project(&fixtures::python_context(), &[], &[]);
    assert_complete(&files);
    assert_eq!(
        paths(&files),
        [".github/workflows/build.yml", "Dockerfile", ".dockerignore"]
    );
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("The build expects `Dockerfile`"))
    );
    // An unknown stack cannot be given one; that is said, not guessed.
    let mut unknown = fixtures::static_context();
    unknown.capabilities.clear();
    unknown.facts.clear();
    unknown.project.as_mut().unwrap().primary_language = String::new();
    let input = input::build(&unknown, &[], &[]);
    assert_eq!(input.base.stack, Stack::Unknown);
    let files = generate(&input);
    assert_eq!(paths(&files), [".github/workflows/build.yml"]);
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("No Dockerfile was found"))
    );

    // `docker/Dockerfile` builds the repository, not the `docker` directory.
    let mut ctx = fixtures::rust_context();
    ctx.facts.push(fact(
        "d1",
        "container:dockerfile:docker/Dockerfile",
        "",
        "docker/Dockerfile",
    ));
    let input = input::build(&ctx, &[], &[]);
    assert_eq!(input.images.len(), 1);
    assert_eq!(input.images[0].job, "image");
    assert_eq!(input.images[0].context, ".");
    assert_eq!(input.images[0].dockerfile, "docker/Dockerfile");
    let build = &generate(&input).artifacts[0];
    assert!(
        build
            .content
            .contains("          context: \".\"\n          file: \"docker/Dockerfile\"\n")
    );
    assert!(build.content.contains("cache-to: type=gha,mode=max\n"));
}

#[test]
fn an_existing_workflow_that_already_pushes_is_pointed_out() {
    let mut ctx = fixtures::node_context();
    ctx.facts.push(fact(
        "b1",
        "ci:build_image:.github/workflows/publish.yml",
        "push",
        ".github/workflows/publish.yml",
    ));
    ctx.facts.push(fact(
        "b2",
        "ci:build_image:.github/workflows/ci.yml",
        "build",
        ".github/workflows/ci.yml",
    ));
    let files = for_project(&ctx, &[], &[]);
    let about: Vec<&String> = files
        .notes
        .iter()
        .filter(|n| n.contains("already pushes an image"))
        .collect();
    assert_eq!(about.len(), 1, "{:#?}", files.notes);
    assert!(about[0].contains("`.github/workflows/publish.yml`"));
    // Regenerating over our own build workflow is not a duplicate.
    ctx.facts.push(fact(
        "b3",
        "ci:build_image:.github/workflows/build.yml",
        "push",
        ".github/workflows/build.yml",
    ));
    let files = for_project(&ctx, &[], &[]);
    assert!(find(&files, ".github/workflows/build.yml").replaces_existing);
    assert_eq!(
        files
            .notes
            .iter()
            .filter(|n| n.contains("already pushes an image"))
            .count(),
        1
    );
}

// ---------------------------------------------------------------------------
// Deployers
// ---------------------------------------------------------------------------

#[test]
fn deploy_workflow_snapshot_for_github_actions_environments() {
    let environments = [
        environment("staging", 0, "github_actions", &[]),
        environment(
            "production",
            1,
            "github_actions",
            &[("workflow", "deploy.yml")],
        ),
    ];
    let files = for_project(&kubernetes_context(), &[ghcr()], &environments);
    assert_complete(&files);
    assert_eq!(
        paths(&files),
        [
            ".github/workflows/build.yml",
            ".github/workflows/deploy.yml"
        ],
        "only the deployer kinds in use get files"
    );
    let deploy = find(&files, ".github/workflows/deploy.yml");
    assert_snapshot("delivery_deploy_github_actions", deploy);

    let doc = &parse_yaml(deploy)[0];
    let on = doc
        .get("on")
        .or_else(|| doc.get(serde_yaml_ng::Value::Bool(true)))
        .expect("triggers");
    assert_eq!(
        on.as_mapping().unwrap().len(),
        1,
        "dispatch only, never on push"
    );
    let inputs = on["workflow_dispatch"]["inputs"].as_mapping().unwrap();
    assert_eq!(
        inputs
            .keys()
            .map(|k| k.as_str().unwrap())
            .collect::<Vec<_>>(),
        ["environment", "image", "digest", "version"]
    );
    assert_eq!(
        on["workflow_dispatch"]["inputs"]["environment"]["type"],
        "environment"
    );
    assert_eq!(
        doc["concurrency"]["group"],
        "deploy-${{ inputs.environment }}"
    );
    assert_eq!(doc["concurrency"]["cancel-in-progress"], false);
    let jobs = doc["jobs"].as_mapping().unwrap();
    assert_eq!(jobs.len(), 1, "one job");
    let job = &doc["jobs"]["deploy"];
    assert_eq!(job["environment"], "${{ inputs.environment }}");
    // Kubernetes with the repository's own Deployment, from the facts.
    assert_eq!(
        job["env"]["KUBE_DEPLOYMENT"],
        "${{ vars.KUBE_DEPLOYMENT || 'view-engine' }}"
    );
    assert_eq!(
        job["env"]["KUBE_NAMESPACE"],
        "${{ vars.KUBE_NAMESPACE || inputs.environment }}"
    );
    assert!(deploy.content.contains(
        "kubectl set image \"deployment/$KUBE_DEPLOYMENT\" \"$KUBE_CONTAINER=$IMAGE_REF\" --namespace \"$KUBE_NAMESPACE\""
    ));
    // Inputs reach the shell as environment variables, never interpolated
    // into a script.
    for line in deploy
        .content
        .lines()
        .filter(|l| l.starts_with("          "))
    {
        assert!(
            !line.contains("${{ inputs."),
            "input interpolated into a script: {line}"
        );
    }
    assert_eq!(files.required_secrets, ["KUBECONFIG_B64"]);
    assert_eq!(
        workflow_secrets(deploy),
        ["KUBECONFIG_B64".to_owned()].into()
    );
    assert!(
        files.notes.iter().any(|n| n.contains(
            "because the repository has Kubernetes manifests at `deploy/k8s/deployment.yaml`"
        )),
        "{:#?}",
        files.notes
    );
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("GitHub environments `staging`, `production`"))
    );
}

#[test]
fn every_target_renders_one_complete_branch_in_every_script_deployer() {
    let environments = [
        environment("staging", 0, "github_actions", &[]),
        environment(
            "qa",
            1,
            "jenkins",
            &[("url", "https://ci.view-engine.dev"), ("job", "deploy")],
        ),
        environment(
            "production",
            2,
            "azure_devops",
            &[
                ("organization", "f8fvwgzc"),
                ("project", "view-engine"),
                ("pipeline_id", "12"),
            ],
        ),
    ];
    let base = input::build(&kubernetes_context(), &[ghcr()], &environments);
    let commands: [(Target, &str, &[&str]); 10] = [
        (Target::Kubectl, "kubectl set image", &["KUBECONFIG_B64"]),
        (Target::Helm, "helm upgrade --install", &["KUBECONFIG_B64"]),
        (
            Target::AzureContainerApps,
            "az containerapp update",
            &[
                "AZURE_CLIENT_ID",
                "AZURE_TENANT_ID",
                "AZURE_SUBSCRIPTION_ID",
            ],
        ),
        (
            Target::CloudRun,
            "gcloud run deploy",
            &[
                "GCP_WORKLOAD_IDENTITY_PROVIDER",
                "GCP_DEPLOY_SERVICE_ACCOUNT",
            ],
        ),
        (
            Target::AzureAppService,
            "az webapp config container set",
            &[
                "AZURE_CLIENT_ID",
                "AZURE_TENANT_ID",
                "AZURE_SUBSCRIPTION_ID",
            ],
        ),
        (
            Target::AwsLambda,
            "aws lambda update-function-code",
            &["AWS_DEPLOY_ROLE_ARN"],
        ),
        (Target::Fly, "flyctl deploy", &["FLY_API_TOKEN"]),
        (
            Target::AwsEcs,
            "aws ecs update-service",
            &["AWS_DEPLOY_ROLE_ARN"],
        ),
        (
            Target::Render,
            "imgURL=$IMAGE_REF",
            &["RENDER_DEPLOY_HOOK_URL"],
        ),
        (
            Target::SshCompose,
            "docker compose -f docker-compose.yml -f docker-compose.release.yml up -d",
            &["DEPLOY_HOST", "DEPLOY_USER", "DEPLOY_SSH_KEY"],
        ),
    ];
    assert_eq!(commands.len(), Target::ALL.len());
    for (target, command, github_secrets) in commands {
        let mut input = base.clone();
        input.target = TargetChoice {
            target,
            evidence: "the test says so".into(),
            detected: true,
        };
        let files = generate(&input);
        assert_complete(&files);
        let deploy = find(&files, ".github/workflows/deploy.yml");
        let jenkinsfile = find(&files, "Jenkinsfile");
        let pipeline = find(&files, "azure-pipelines.yml");
        for file in [deploy, jenkinsfile, pipeline] {
            assert!(
                file.content.contains(command),
                "{target:?}: {} lacks `{command}`",
                file.path
            );
            // Only the branch that applies: no other target's command.
            for (other, other_command, _) in commands.iter().filter(|(t, _, _)| *t != target) {
                assert!(
                    !file.content.contains(other_command),
                    "{target:?}: {} also carries the {other:?} branch",
                    file.path
                );
            }
        }
        assert_eq!(
            workflow_secrets(deploy),
            github_secrets.iter().map(|s| (*s).to_owned()).collect(),
            "{target:?}"
        );
        // Cloud targets federate from GitHub: no static key in the workflow.
        let oidc = matches!(
            target,
            Target::AzureContainerApps
                | Target::AzureAppService
                | Target::CloudRun
                | Target::AwsEcs
                | Target::AwsLambda
        );
        assert_eq!(
            deploy.content.contains("id-token: write"),
            oidc,
            "{target:?}"
        );
        assert!(
            !deploy.content.contains("AWS_SECRET_ACCESS_KEY"),
            "{target:?}"
        );
        assert!(
            !deploy.content.contains("AZURE_CLIENT_SECRET"),
            "{target:?}"
        );
        // A chart is read from the checkout; nothing else needs one.
        assert_eq!(
            deploy.content.contains("actions/checkout"),
            target == Target::Helm,
            "{target:?}"
        );
        assert_eq!(
            pipeline.content.contains("- checkout: self"),
            target == Target::Helm,
            "{target:?}"
        );
        // Groovy keeps the shell's backslashes.
        if target == Target::SshCompose {
            assert!(
                jenkinsfile
                    .content
                    .contains("printf 'services:\\\\n  %s:\\\\n    image: %s\\\\n'")
            );
            assert!(
                deploy
                    .content
                    .contains("printf 'services:\\n  %s:\\n    image: %s\\n'")
            );
        }
        // Every secret a script reads is bound in Jenkins and mapped in Azure.
        let plan = target.plan(&input.base, &input.workload);
        for credential in &plan.credentials {
            assert!(
                jenkinsfile
                    .content
                    .contains(&format!("variable: '{}'", credential.name)),
                "{target:?}: Jenkins does not bind {}",
                credential.name
            );
            assert!(
                pipeline
                    .content
                    .contains(&format!("{name}: $({name})", name = credential.name)),
                "{target:?}: Azure Pipelines does not map {}",
                credential.name
            );
        }
    }
}

#[test]
fn flux_snapshots_for_gitops_flux_environments() {
    let environments = [
        environment("staging", 0, "gitops_flux", &[]),
        environment(
            "production",
            1,
            "gitops_flux",
            &[
                ("path", "apps/view-engine/production/kustomization.yaml"),
                ("strategy", "kustomize_image"),
                ("image_automation", "true"),
            ],
        ),
    ];
    // No manifests of its own: the workload is generated per environment.
    let files = for_project(&fixtures::node_context(), &[ghcr()], &environments);
    assert_complete(&files);
    assert_eq!(
        paths(&files),
        [
            ".github/workflows/build.yml",
            "deploy/staging/deployment.yaml",
            "deploy/staging/service.yaml",
            "deploy/staging/namespace.yaml",
            "deploy/staging/kustomization.yaml",
            "clusters/staging/view-engine-staging.yaml",
            "apps/view-engine/production/deployment.yaml",
            "apps/view-engine/production/service.yaml",
            "apps/view-engine/production/namespace.yaml",
            "apps/view-engine/production/kustomization.yaml",
            "clusters/production/view-engine-production.yaml",
            "clusters/production/view-engine-production-images.yaml",
        ]
    );
    assert!(!paths(&files).contains(&".github/workflows/deploy.yml"));
    assert_snapshot(
        "delivery_flux_objects",
        find(&files, "clusters/staging/view-engine-staging.yaml"),
    );
    assert_snapshot(
        "delivery_flux_kustomization",
        find(&files, "deploy/staging/kustomization.yaml"),
    );
    assert_snapshot(
        "delivery_flux_image_automation",
        find(
            &files,
            "clusters/production/view-engine-production-images.yaml",
        ),
    );

    let objects = parse_yaml(find(&files, "clusters/staging/view-engine-staging.yaml"));
    assert_eq!(objects.len(), 2);
    let (source, kustomization) = (&objects[0], &objects[1]);
    assert_eq!(source["apiVersion"], "source.toolkit.fluxcd.io/v1");
    assert_eq!(source["kind"], "GitRepository");
    assert_eq!(
        source["spec"]["url"],
        "https://github.com/f8fvwgzc/view-engine.git"
    );
    assert_eq!(source["spec"]["ref"]["branch"], "main");
    assert!(
        source["spec"].get("secretRef").is_none(),
        "a public repository"
    );
    assert_eq!(
        kustomization["apiVersion"],
        "kustomize.toolkit.fluxcd.io/v1"
    );
    assert_eq!(kustomization["kind"], "Kustomization");
    assert_eq!(kustomization["spec"]["path"], "./deploy/staging");
    assert_eq!(kustomization["spec"]["prune"], true);
    assert_eq!(
        kustomization["spec"]["sourceRef"]["name"],
        "view-engine-staging"
    );
    let check = &kustomization["spec"]["healthChecks"][0];
    assert_eq!(check["kind"], "Deployment");
    assert_eq!(check["name"], "view-engine");
    assert_eq!(check["namespace"], "view-engine-staging");

    let overlay = &parse_yaml(find(&files, "deploy/staging/kustomization.yaml"))[0];
    assert_eq!(overlay["apiVersion"], "kustomize.config.k8s.io/v1beta1");
    assert_eq!(overlay["namespace"], "view-engine-staging");
    assert_eq!(
        overlay["resources"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap())
            .collect::<Vec<_>>(),
        ["namespace.yaml", "deployment.yaml", "service.yaml"]
    );
    let image = &overlay["images"][0];
    // `name` is what the generated Deployment says; `newName` where the
    // environment pulls from.
    assert_eq!(image["name"], "ghcr.io/f8fvwgzc/view-engine");
    assert_eq!(image["newName"], "ghcr.io/f8fvwgzc/view-engine");
    assert!(image.get("newTag").is_some());
    assert!(
        find(&files, "deploy/staging/deployment.yaml")
            .content
            .contains("image: ghcr.io/f8fvwgzc/view-engine:latest")
    );
    // The setter marker only where Flux itself writes the tag.
    assert!(
        !find(&files, "deploy/staging/kustomization.yaml")
            .content
            .contains("$imagepolicy")
    );
    assert!(
        find(&files, "apps/view-engine/production/kustomization.yaml")
            .content
            .contains("    newTag: latest # {\"$imagepolicy\": \"flux-system:view-engine-production:tag\"}\n")
    );
    let automation = parse_yaml(find(
        &files,
        "clusters/production/view-engine-production-images.yaml",
    ));
    assert_eq!(
        automation
            .iter()
            .map(|d| d["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["ImageRepository", "ImagePolicy", "ImageUpdateAutomation"]
    );
    assert_eq!(
        automation[0]["spec"]["image"],
        "ghcr.io/f8fvwgzc/view-engine"
    );
    assert_eq!(
        automation[2]["spec"]["update"]["path"],
        "./apps/view-engine/production"
    );
    // A pull-request environment gets Flux's commits on a branch of their own.
    assert_eq!(
        automation[2]["spec"]["git"]["push"]["branch"],
        "flux/view-engine-production"
    );
    // Image automation writes to the repository, so even a public one needs
    // a credential there.
    let production = parse_yaml(find(
        &files,
        "clusters/production/view-engine-production.yaml",
    ));
    assert_eq!(
        production[0]["spec"]["secretRef"]["name"],
        "view-engine-production-git"
    );
    assert!(files.notes.iter().any(|n| {
        n.contains("A promotion to `staging` edits `deploy/staging/kustomization.yaml` in `f8fvwgzc/view-engine` on `main`")
            && n.contains("opens a pull request")
    }));
}

#[test]
fn gitops_layouts_follow_the_environment_config() {
    // The project's own manifests are a base; another repository gets a
    // self-contained tree to carry over; Helm values get a HelmRelease.
    let mut ctx = kubernetes_context();
    ctx.project.as_mut().unwrap().is_private = true;
    ctx.project.as_mut().unwrap().repo_url = "https://ghe.acme.dev/f8fvwgzc/view-engine".into();
    ctx.facts.push(fact(
        "h1",
        "k8s:helm",
        "view-engine",
        "charts/view-engine/Chart.yaml",
    ));
    let acr = registry("reg-acr", "acr", "acme.azurecr.io", "");
    let mut fleet = environment(
        "production",
        2,
        "gitops_flux",
        &[
            ("repo", "acme/fleet"),
            ("branch", "release"),
            ("path", "apps/view-engine/kustomization.yaml"),
            ("mode", "commit"),
        ],
    );
    fleet.registry_id = "reg-acr".into();
    let environments = [
        environment("staging", 0, "gitops_flux", &[]),
        environment(
            "canary",
            1,
            "gitops_flux",
            &[
                ("strategy", "helm_values"),
                ("path", "deploy/canary/values.yaml"),
            ],
        ),
        fleet,
        environment(
            "dr",
            3,
            "gitops_flux",
            &[
                ("strategy", "yaml_path"),
                ("path", "deploy/k8s/deployment.yaml"),
                ("yaml_path", "spec.template.spec.containers[0].image"),
            ],
        ),
    ];
    let files = for_project(&ctx, &[ghcr(), acr], &environments);
    assert_complete(&files);
    let all = paths(&files);

    // staging: an overlay over the repository's own manifests, which become
    // a kustomize base.
    let base = &parse_yaml(find(&files, "deploy/k8s/kustomization.yaml"))[0];
    assert_eq!(
        base["resources"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap())
            .collect::<Vec<_>>(),
        ["deployment.yaml", "service.yaml"]
    );
    let overlay = &parse_yaml(find(&files, "deploy/staging/kustomization.yaml"))[0];
    assert_eq!(overlay["resources"][0], "namespace.yaml");
    assert_eq!(overlay["resources"][1], "../k8s");
    assert!(!all.contains(&"deploy/staging/deployment.yaml"));
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("if the manifests in `deploy/k8s` name the image differently"))
    );
    // A private repository on GitHub Enterprise: its own host, a credential.
    let staging = parse_yaml(find(&files, "clusters/staging/view-engine-staging.yaml"));
    assert_eq!(
        staging[0]["spec"]["url"],
        "https://ghe.acme.dev/f8fvwgzc/view-engine.git"
    );
    assert_eq!(
        staging[0]["spec"]["secretRef"]["name"],
        "view-engine-staging-git"
    );

    // canary: a HelmRelease over the repository's chart and the values file.
    let canary = parse_yaml(find(&files, "clusters/canary/view-engine-canary.yaml"));
    assert_eq!(canary[1]["apiVersion"], "helm.toolkit.fluxcd.io/v2");
    assert_eq!(canary[1]["kind"], "HelmRelease");
    let chart = &canary[1]["spec"]["chart"]["spec"];
    assert_eq!(chart["chart"], "./charts/view-engine");
    assert_eq!(chart["valuesFiles"][1], "./deploy/canary/values.yaml");
    let values = &parse_yaml(find(&files, "deploy/canary/values.yaml"))[0];
    assert_eq!(
        values["image"]["repository"],
        "ghcr.io/f8fvwgzc/view-engine"
    );
    assert!(
        !all.iter().any(|p| p.starts_with("charts/")),
        "the chart exists already"
    );

    // production: another repository, so a tree to copy there, complete.
    for path in [
        "gitops/fleet/apps/view-engine/kustomization.yaml",
        "gitops/fleet/apps/view-engine/namespace.yaml",
        "gitops/fleet/apps/view-engine/deployment.yaml",
        "gitops/fleet/apps/view-engine/service.yaml",
        "gitops/fleet/clusters/production/view-engine-production.yaml",
    ] {
        assert!(all.contains(&path), "missing {path} in {all:?}");
    }
    let production = parse_yaml(find(
        &files,
        "gitops/fleet/clusters/production/view-engine-production.yaml",
    ));
    assert_eq!(
        production[0]["spec"]["url"],
        "https://ghe.acme.dev/acme/fleet.git"
    );
    assert_eq!(production[0]["spec"]["ref"]["branch"], "release");
    assert_eq!(production[1]["spec"]["path"], "./apps/view-engine");
    let overlay = &parse_yaml(find(
        &files,
        "gitops/fleet/apps/view-engine/kustomization.yaml",
    ))[0];
    // The environment pulls from its own registry.
    assert_eq!(
        overlay["images"][0]["newName"],
        "acme.azurecr.io/view-engine"
    );
    assert!(files.notes.iter().any(|n| {
        n.contains("copy the contents of `gitops/fleet` to the root of that repository")
    }));
    assert!(files.notes.iter().any(|n| {
        n.contains("A promotion to `production` edits `apps/view-engine/kustomization.yaml` in `acme/fleet` on `release`")
            && n.contains("commits it")
    }));

    // dr: the repository's own manifest is edited in place; Flux waits for
    // whatever that directory holds.
    let dr = parse_yaml(find(&files, "clusters/dr/view-engine-dr.yaml"));
    assert_eq!(dr[1]["spec"]["path"], "./deploy/k8s");
    assert_eq!(dr[1]["spec"]["wait"], true);
    assert!(dr[1]["spec"].get("healthChecks").is_none());
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("the value at `spec.template.spec.containers[0].image`"))
    );
}

#[test]
fn argocd_snapshot_for_gitops_argocd_environments() {
    let environments = [
        environment("staging", 0, "gitops_argocd", &[]),
        environment(
            "production",
            1,
            "gitops_argocd",
            &[
                ("strategy", "helm_values"),
                ("path", "deploy/production/values.yaml"),
            ],
        ),
    ];
    let files = for_project(&fixtures::node_context(), &[ghcr()], &environments);
    assert_complete(&files);
    let all = paths(&files);
    for path in [
        "argocd/view-engine-staging.yaml",
        "deploy/staging/kustomization.yaml",
        "deploy/staging/deployment.yaml",
        "argocd/view-engine-production.yaml",
        "deploy/production/values.yaml",
        // No chart in the repository: the one the values configure comes along.
        "charts/view-engine/Chart.yaml",
        "charts/view-engine/templates/deployment.yaml",
    ] {
        assert!(all.contains(&path), "missing {path} in {all:?}");
    }
    assert!(
        !all.iter().any(|p| p.starts_with("clusters/")),
        "no Flux objects"
    );
    let application = find(&files, "argocd/view-engine-staging.yaml");
    assert_snapshot("delivery_argocd_application", application);
    let doc = &parse_yaml(application)[0];
    assert_eq!(doc["apiVersion"], "argoproj.io/v1alpha1");
    assert_eq!(doc["kind"], "Application");
    assert_eq!(
        doc["spec"]["source"]["repoURL"],
        "https://github.com/f8fvwgzc/view-engine.git"
    );
    assert_eq!(doc["spec"]["source"]["path"], "deploy/staging");
    assert_eq!(doc["spec"]["source"]["targetRevision"], "main");
    assert_eq!(
        doc["spec"]["destination"]["namespace"],
        "view-engine-staging"
    );
    assert_eq!(doc["spec"]["syncPolicy"]["automated"]["prune"], true);
    let helm = &parse_yaml(find(&files, "argocd/view-engine-production.yaml"))[0];
    assert_eq!(helm["spec"]["source"]["path"], "charts/view-engine");
    // Value files are resolved from the chart's directory.
    assert_eq!(
        helm["spec"]["source"]["helm"]["valueFiles"][0],
        "../../deploy/production/values.yaml"
    );
    // One chart serves every environment: its own ingress is off and has
    // no host, and the environment's values turn it on with the host of
    // the environment's URL.
    let chart = &parse_yaml(find(&files, "charts/view-engine/values.yaml"))[0];
    assert_eq!(chart["ingress"]["enabled"], false);
    assert_eq!(chart["ingress"]["host"], "");
    assert_eq!(chart["image"]["repository"], "ghcr.io/f8fvwgzc/view-engine");
    let values = &parse_yaml(find(&files, "deploy/production/values.yaml"))[0];
    assert_eq!(
        values["image"]["repository"],
        "ghcr.io/f8fvwgzc/view-engine"
    );
    assert_eq!(values["ingress"]["enabled"], true);
    assert_eq!(values["ingress"]["host"], "production.view-engine.dev");
}

#[test]
fn jenkinsfile_snapshot_for_jenkins_environments() {
    let environments = [
        environment(
            "staging",
            0,
            "jenkins",
            &[
                ("url", "https://ci.view-engine.dev"),
                ("job", "deploy-view-engine"),
                ("user_secret", "jenkins-user"),
                ("token_secret", "jenkins-token"),
            ],
        ),
        Environment {
            url: String::new(),
            ..environment(
                "production",
                1,
                "jenkins",
                &[
                    ("url", "https://ci.view-engine.dev"),
                    ("job", "deploy-view-engine"),
                ],
            )
        },
    ];
    let files = for_project(&kubernetes_context(), &[ghcr()], &environments);
    assert_complete(&files);
    assert_eq!(
        paths(&files),
        [".github/workflows/build.yml", "Jenkinsfile"]
    );
    let jenkinsfile = find(&files, "Jenkinsfile");
    assert_snapshot("delivery_jenkinsfile", jenkinsfile);
    assert_eq!(jenkinsfile.language, "text");
    let text = &jenkinsfile.content;
    for needle in [
        "string(name: 'IMAGE', defaultValue: 'ghcr.io/f8fvwgzc/view-engine'",
        "string(name: 'DIGEST'",
        "choice(name: 'ENVIRONMENT', choices: ['staging', 'production']",
        "stage('Verify')",
        "stage('Deploy')",
        "stage('Health')",
        "string(credentialsId: \"kubeconfig-b64-${params.ENVIRONMENT}\", variable: 'KUBECONFIG_B64')",
        "KUBE_NAMESPACE = \"${params.ENVIRONMENT}\"",
        // Only the environment with a URL has a health check.
        "def HEALTH_URLS = [\n  'staging': 'https://staging.view-engine.dev/healthz'\n]",
    ] {
        assert!(text.contains(needle), "missing `{needle}` in\n{text}");
    }
    // Braces balance: the cheapest proof the Groovy is whole.
    for (open, close) in [('{', '}'), ('(', ')'), ('[', ']')] {
        assert_eq!(
            text.matches(open).count(),
            text.matches(close).count(),
            "unbalanced {open}{close}"
        );
    }
    let stages: Vec<usize> = ["stage('Verify')", "stage('Deploy')", "stage('Health')"]
        .iter()
        .map(|s| text.find(s).unwrap())
        .collect();
    assert!(
        stages.windows(2).all(|w| w[0] < w[1]),
        "verify, deploy, health in order"
    );
    assert_eq!(
        files.required_secrets,
        ["kubeconfig-b64-staging", "kubeconfig-b64-production"]
    );
    assert!(files.notes.iter().any(|n| {
        n.contains("create a Pipeline job `deploy-view-engine` on https://ci.view-engine.dev")
            && n.contains("workspace secrets `jenkins-user` and `jenkins-token`")
    }));
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("Set `user_secret` and `token_secret`"))
    );

    // The repository's own Jenkinsfile is not overwritten.
    let mut ctx = kubernetes_context();
    ctx.facts
        .push(fact("j1", "cd:jenkins", "Jenkinsfile", "Jenkinsfile"));
    let files = for_project(&ctx, &[ghcr()], &environments);
    assert_eq!(
        paths(&files),
        [".github/workflows/build.yml", "Jenkinsfile.deploy"]
    );
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("script path is `Jenkinsfile.deploy`"))
    );
}

#[test]
fn azure_pipelines_snapshot_for_azure_devops_environments() {
    let config = [
        ("organization", "f8fvwgzc"),
        ("project", "view-engine"),
        ("pipeline_id", "42"),
        ("pat_secret", "azure-devops-pat"),
    ];
    let environments = [
        environment("staging", 0, "azure_devops", &config),
        environment("production", 1, "azure_devops", &config),
    ];
    let files = for_project(&kubernetes_context(), &[ghcr()], &environments);
    assert_complete(&files);
    assert_eq!(
        paths(&files),
        [".github/workflows/build.yml", "azure-pipelines.yml"]
    );
    let pipeline = find(&files, "azure-pipelines.yml");
    assert_snapshot("delivery_azure_pipelines", pipeline);
    let doc = &parse_yaml(pipeline)[0];
    assert_eq!(doc["trigger"], "none");
    assert_eq!(doc["pr"], "none");
    let parameters = doc["parameters"].as_sequence().unwrap();
    assert_eq!(
        parameters
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["environment", "image", "digest", "version"]
    );
    assert_eq!(parameters[0]["values"][0], "staging");
    assert_eq!(parameters[0]["values"][1], "production");
    assert_eq!(parameters[1]["default"], "ghcr.io/f8fvwgzc/view-engine");
    // A deployment job bound to the environment: its approvals gate the run.
    let job = &doc["stages"][0]["jobs"][0];
    assert_eq!(job["deployment"], "deploy");
    assert_eq!(job["environment"], "${{ parameters.environment }}");
    let steps = job["strategy"]["runOnce"]["deploy"]["steps"]
        .as_sequence()
        .unwrap();
    assert_eq!(steps[0]["download"], "none");
    assert_eq!(steps[1]["checkout"], "none");
    assert_eq!(steps.len(), 5, "download, checkout, verify, deploy, health");
    assert_eq!(steps[3]["env"]["KUBECONFIG_B64"], "$(KUBECONFIG_B64)");
    assert!(
        steps[4]["bash"]
            .as_str()
            .unwrap()
            .contains("\"staging\") url=\"https://staging.view-engine.dev/healthz\" ;;")
    );
    // Secrets come from one variable group per environment, after the
    // defaults so a group may override them.
    let variables = doc["variables"].as_sequence().unwrap();
    assert_eq!(
        variables.last().unwrap()["group"],
        "view-engine-${{ parameters.environment }}"
    );
    assert_eq!(variables[0]["name"], "KUBE_DEPLOYMENT");
    assert_eq!(files.required_secrets, ["KUBECONFIG_B64"]);
    assert!(files.notes.iter().any(|n| {
        n.contains("project `view-engine` of the organization `f8fvwgzc`")
            && n.contains("`pipeline_id` (42)")
            && n.contains("workspace secret `azure-devops-pat`")
    }));
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("add the approvals and checks each stage needs"))
    );

    let mut ctx = kubernetes_context();
    ctx.facts.push(fact(
        "a1",
        "cd:azure_pipelines",
        "azure-pipelines.yml",
        "azure-pipelines.yml",
    ));
    assert_eq!(
        paths(&for_project(&ctx, &[ghcr()], &environments)),
        [".github/workflows/build.yml", "azure-pipelines.deploy.yml"]
    );
}

#[test]
fn every_deployer_at_once_is_complete_and_only_used_kinds_get_files() {
    let mut odd = environment("edge", 6, "teleport", &[]);
    odd.url = String::new();
    let environments = [
        // Out of order on purpose: `position` decides.
        environment(
            "production",
            3,
            "azure_devops",
            &[
                ("organization", "o"),
                ("project", "p"),
                ("pipeline_id", "7"),
            ],
        ),
        environment("dev", 0, "docker_local", &[("port", "3000")]),
        environment(
            "staging",
            1,
            "github_actions",
            &[("workflow", "release.yaml")],
        ),
        environment("qa", 2, "gitops_argocd", &[]),
        environment("eu", 4, "gitops_flux", &[]),
        environment(
            "hook",
            5,
            "webhook",
            &[
                ("url", "https://hooks.view-engine.dev/deploy"),
                ("secret", "hook-secret"),
            ],
        ),
        environment(
            "legacy",
            7,
            "jenkins",
            &[("url", "https://ci.view-engine.dev"), ("job", "deploy")],
        ),
        odd,
    ];
    let files = for_project(&kubernetes_context(), &[ghcr()], &environments);
    assert_complete(&files);
    let all = paths(&files);
    for path in [
        ".github/workflows/build.yml",
        ".github/workflows/release.yaml",
        "argocd/view-engine-qa.yaml",
        "clusters/eu/view-engine-eu.yaml",
        "Jenkinsfile",
        "azure-pipelines.yml",
    ] {
        assert!(all.contains(&path), "missing {path} in {all:?}");
    }
    // The workflow name comes from the environment's config.
    assert!(!all.contains(&".github/workflows/deploy.yml"));
    // One environment per script deployer: only its own name is offered.
    assert!(
        find(&files, "Jenkinsfile")
            .content
            .contains("choices: ['legacy']")
    );
    assert!(
        find(&files, "azure-pipelines.yml")
            .content
            .contains("    values:\n      - \"production\"\n  - name: image")
    );
    for (needle, what) in [
        (
            "`hook` is deployed by a signed webhook to `https://hooks.view-engine.dev/deploy`",
            "webhook",
        ),
        ("with the workspace secret `hook-secret`", "webhook secret"),
        (
            "`dev` runs on the Docker daemon of the SDI host",
            "docker_local",
        ),
        (
            "Environment `edge` uses the deployer `teleport`",
            "unknown kind",
        ),
    ] {
        assert!(
            files.notes.iter().any(|n| n.contains(needle)),
            "no note about the {what}: {:#?}",
            files.notes
        );
    }
    // Nothing but the build without environments, and that is said.
    let alone = for_project(&kubernetes_context(), &[ghcr()], &[]);
    assert_eq!(paths(&alone), [".github/workflows/build.yml"]);
    assert!(
        alone
            .notes
            .iter()
            .any(|n| n.contains("No environments are configured"))
    );
    // The same input always yields the same files.
    assert_eq!(
        files,
        for_project(&kubernetes_context(), &[ghcr()], &environments)
    );
}

#[test]
fn an_undetected_target_is_a_documented_default_not_a_placeholder() {
    let environments = [environment("production", 0, "github_actions", &[])];
    let files = for_project(&fixtures::rust_context(), &[], &environments);
    assert_complete(&files);
    let deploy = find(&files, ".github/workflows/deploy.yml");
    assert!(
        deploy
            .content
            .contains("docker compose -f docker-compose.yml -f docker-compose.release.yml pull")
    );
    assert!(
        deploy
            .content
            .contains("DEPLOY_DIR: ${{ vars.DEPLOY_DIR || '/opt/texc-symphony' }}")
    );
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("No deployment target was detected"))
    );
    assert!(
        files
            .notes
            .iter()
            .any(|n| n.contains("one-time `docker login ghcr.io`"))
    );
}

#[test]
fn merging_into_an_option_replaces_what_the_setup_supersedes() {
    let pattern_file = |path: &str, content: &str| ArtifactData {
        path: path.into(),
        content: content.into(),
        language: "yaml".into(),
        purpose: "from the pattern".into(),
        replaces_existing: false,
    };
    let environments = [environment("staging", 0, "gitops_flux", &[])];
    let delivery = for_project(&fixtures::node_context(), &[ghcr()], &environments);
    let mut option = vec![
        pattern_file("Dockerfile", "FROM node:22\n"),
        pattern_file(".github/workflows/ci.yml", "name: CI\n"),
        // Publishes on every push: the build workflow does that now.
        pattern_file(
            ".github/workflows/deploy.yml",
            "uses: docker/build-push-action@v6\nwith:\n  push: true\n",
        ),
        // Same path as a delivery file: the real setup wins.
        pattern_file("deploy/staging/kustomization.yaml", "resources: []\n"),
        pattern_file("docs/deployment.md", "# Deploy\n"),
    ];
    delivery.merge_into(&mut option);
    let merged: Vec<&str> = option.iter().map(|a| a.path.as_str()).collect();
    assert_eq!(
        &merged[..3],
        [
            "Dockerfile",
            ".github/workflows/ci.yml",
            "docs/deployment.md"
        ]
    );
    assert!(merged.contains(&".github/workflows/build.yml"));
    assert!(!merged.contains(&".github/workflows/deploy.yml"));
    assert_eq!(
        merged
            .iter()
            .filter(|p| **p == "deploy/staging/kustomization.yaml")
            .count(),
        1
    );
    assert!(
        option
            .iter()
            .find(|a| a.path == "deploy/staging/kustomization.yaml")
            .unwrap()
            .content
            .contains("kustomize.config.k8s.io")
    );
    // A deploy workflow that publishes nothing (GitHub Pages) stays.
    let mut pages = vec![pattern_file(
        ".github/workflows/deploy.yml",
        "uses: actions/deploy-pages@v4\n",
    )];
    delivery.merge_into(&mut pages);
    assert_eq!(pages[0].purpose, "from the pattern");
    // Merging twice changes nothing.
    let once = option.clone();
    delivery.merge_into(&mut option);
    assert_eq!(option, once);
}
