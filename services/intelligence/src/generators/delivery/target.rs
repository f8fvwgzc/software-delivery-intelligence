//! Where a release runs, and the commands that put an image there.
//!
//! The target is chosen from evidence, strongest first: what `.sdi.yml`
//! declares, what an existing pipeline already deploys to, the manifests
//! and charts in the repository, the cloud its infrastructure code names.
//! Every deployer that runs a script (the GitHub deploy workflow, the
//! Jenkinsfile, the Azure pipeline) renders the same [`Plan`], so a project
//! is deployed the same way whichever tool starts it.

use sdi_proto::knowledge::v1::ProjectContext;

use super::input::Workload;
use crate::generators::{DeployTarget, GenInput, input as gen_input};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// `kubectl set image` on the project's Deployment.
    Kubectl,
    /// `helm upgrade --install` of the chart in the repository.
    Helm,
    /// `az containerapp update`.
    AzureContainerApps,
    /// `az webapp config container set` on an App Service.
    AzureAppService,
    /// `gcloud run deploy`.
    CloudRun,
    /// `flyctl deploy --image`.
    Fly,
    /// A new ECS task definition revision and `aws ecs update-service`.
    AwsEcs,
    /// `aws lambda update-function-code` with the image.
    AwsLambda,
    /// Render's deploy hook with the image to run.
    Render,
    /// `docker compose pull` and `up` on a host over SSH.
    SshCompose,
}

impl Target {
    #[cfg(test)]
    pub const ALL: [Target; 10] = [
        Target::Kubectl,
        Target::Helm,
        Target::AzureContainerApps,
        Target::AzureAppService,
        Target::CloudRun,
        Target::Fly,
        Target::AwsEcs,
        Target::AwsLambda,
        Target::Render,
        Target::SshCompose,
    ];

    /// The branch that deploys what a catalog pattern sets up, so an
    /// option's deploy step matches the option. A pattern that only
    /// publishes (GitHub Pages, a registry) names no branch.
    pub fn of_pattern(target: DeployTarget) -> Option<Target> {
        Some(match target {
            DeployTarget::AzureContainerApps => Target::AzureContainerApps,
            DeployTarget::AzureAppService => Target::AzureAppService,
            DeployTarget::AwsEcs => Target::AwsEcs,
            DeployTarget::AwsLambda => Target::AwsLambda,
            DeployTarget::GcpCloudRun => Target::CloudRun,
            DeployTarget::Fly => Target::Fly,
            DeployTarget::Render => Target::Render,
            DeployTarget::Vm => Target::SshCompose,
            // The pattern ships a chart under `charts/<name>`.
            DeployTarget::Kubernetes => Target::Helm,
            DeployTarget::GithubPages | DeployTarget::Registry => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Target::Kubectl => "Kubernetes (kubectl)",
            Target::Helm => "Kubernetes (Helm)",
            Target::AzureContainerApps => "Azure Container Apps",
            Target::AzureAppService => "Azure App Service",
            Target::CloudRun => "Google Cloud Run",
            Target::Fly => "Fly.io",
            Target::AwsEcs => "AWS ECS",
            Target::AwsLambda => "AWS Lambda",
            Target::Render => "Render",
            Target::SshCompose => "a host over SSH with Docker Compose",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetChoice {
    pub target: Target,
    /// What it was chosen from, as the end of a sentence.
    pub evidence: String,
    /// False when nothing in the repository named a target and the
    /// default was taken.
    pub detected: bool,
}

fn kubernetes(workload: &Workload) -> Target {
    if workload.chart_dir.is_empty() {
        Target::Kubectl
    } else {
        Target::Helm
    }
}

fn by_name(name: &str, workload: &Workload) -> Option<Target> {
    Some(match name {
        "kubernetes" => kubernetes(workload),
        "azure" => Target::AzureContainerApps,
        "aws" => Target::AwsEcs,
        "gcp" => Target::CloudRun,
        "fly" => Target::Fly,
        "render" => Target::Render,
        "vm" => Target::SshCompose,
        _ => return None,
    })
}

/// Chooses the deploy target from the project's facts.
pub fn detect(ctx: &ProjectContext, base: &GenInput, workload: &Workload) -> TargetChoice {
    let found = |target: Target, evidence: String| TargetChoice {
        target,
        evidence,
        detected: true,
    };
    // 1. The repository said so.
    if let Some(config) = gen_input::usable_config(ctx)
        && let Some(target) = by_name(&config.deploy_target, workload)
    {
        return found(
            target,
            format!("`.sdi.yml` sets `deploy.target: {}`", config.deploy_target),
        );
    }
    // 2. A pipeline already deploys somewhere.
    for fact in &ctx.facts {
        if let Some(path) = fact.key.strip_prefix("deploy:pipeline:")
            && let Some(target) = by_name(&fact.value, workload)
        {
            return found(
                target,
                format!("`{path}` already deploys to {}", fact.value),
            );
        }
    }
    // 3. What the repository holds.
    let fact = |key: &str| {
        ctx.facts
            .iter()
            .find(|f| f.key == key && f.confidence >= 0.8)
    };
    if let Some(chart) = fact("k8s:helm") {
        return found(
            kubernetes(workload),
            format!("the repository has a Helm chart at `{}`", chart.source_path),
        );
    }
    if let Some(manifests) = fact("k8s:manifests") {
        return found(
            Target::Kubectl,
            format!(
                "the repository has Kubernetes manifests at `{}`",
                manifests.source_path
            ),
        );
    }
    for (key, tool) in [("cd:flux", "Flux"), ("cd:argocd", "Argo CD")] {
        if fact(key).is_some() {
            return found(
                Target::Kubectl,
                format!("{tool} reconciles this repository"),
            );
        }
    }
    if base.exists("fly.toml") {
        return found(Target::Fly, "the repository has a `fly.toml`".to_owned());
    }
    // 4. The cloud the infrastructure code names (capabilities only hold
    //    facts strong enough to act on).
    for capability in &ctx.capabilities {
        if let Some(cloud) = capability.strip_prefix("cloud:")
            && let Some(target) = by_name(cloud, workload)
        {
            return found(
                target,
                format!("the repository's infrastructure targets {cloud}"),
            );
        }
    }
    if let Some(compose) = ctx
        .facts
        .iter()
        .find(|f| f.key.starts_with("container:compose:"))
    {
        return found(
            Target::SshCompose,
            format!(
                "the repository has the Compose file `{}`",
                compose.source_path
            ),
        );
    }
    TargetChoice {
        target: Target::SshCompose,
        evidence: "nothing in the repository names a deployment target".to_owned(),
        detected: false,
    }
}

/// How a setting defaults when nobody overrides it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingDefault {
    Text(String),
    /// The name of the environment being deployed to.
    EnvironmentName,
}

/// A non-secret value the script reads from its environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub name: &'static str,
    pub default: SettingDefault,
}

/// A secret the script reads from its environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credential {
    pub name: &'static str,
    /// What it holds, as the end of "`NAME`: …".
    pub what: &'static str,
}

/// The shell a deployer runs for one target. Scripts read `IMAGE`,
/// `IMAGE_REF` (the image with its digest, or its tag) and, for Helm,
/// `IMAGE_TAG`, plus the settings and credentials listed here. Every line
/// is one command, so the same lines sit in a YAML block or a Groovy string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub settings: Vec<Setting>,
    pub credentials: Vec<Credential>,
    /// Authenticates with `credentials` where the CI system has no
    /// federation of its own for this target.
    pub login: Vec<String>,
    pub deploy: Vec<String>,
    /// The script reads files of the repository (a chart).
    pub needs_checkout: bool,
}

fn text(name: &'static str, default: impl Into<String>) -> Setting {
    Setting {
        name,
        default: SettingDefault::Text(default.into()),
    }
}

fn lines(raw: &[&str]) -> Vec<String> {
    raw.iter().map(|l| (*l).to_owned()).collect()
}

const KUBECONFIG: Credential = Credential {
    name: "KUBECONFIG_B64",
    what: "a base64-encoded kubeconfig for the cluster, as `base64 -w0 kubeconfig` prints it",
};

const AZURE_CREDENTIALS: [Credential; 4] = [
    Credential {
        name: "AZURE_CLIENT_ID",
        what: "the application (client) id of the deploying service principal",
    },
    Credential {
        name: "AZURE_CLIENT_SECRET",
        what: "that service principal's client secret",
    },
    Credential {
        name: "AZURE_TENANT_ID",
        what: "the Microsoft Entra tenant id",
    },
    Credential {
        name: "AZURE_SUBSCRIPTION_ID",
        what: "the subscription that holds the application",
    },
];

const AZURE_LOGIN: &[&str] = &[
    "az login --service-principal --username \"$AZURE_CLIENT_ID\" --password \"$AZURE_CLIENT_SECRET\" --tenant \"$AZURE_TENANT_ID\" --output none",
    "az account set --subscription \"$AZURE_SUBSCRIPTION_ID\"",
];

const AWS_CREDENTIALS: [Credential; 2] = [
    Credential {
        name: "AWS_ACCESS_KEY_ID",
        what: "the access key id of the deploying IAM user",
    },
    Credential {
        name: "AWS_SECRET_ACCESS_KEY",
        what: "that user's secret access key",
    },
];

const AWS_LOGIN: &[&str] = &["export AWS_DEFAULT_REGION=\"$AWS_REGION\""];

const KUBE_LOGIN: &[&str] = &[
    "mkdir -p \"$HOME/.kube\"",
    "echo \"$KUBECONFIG_B64\" | base64 -d > \"$HOME/.kube/config\"",
    "chmod 600 \"$HOME/.kube/config\"",
];

impl Target {
    pub fn plan(self, base: &GenInput, workload: &Workload) -> Plan {
        let name = base.image.as_str();
        let region = |cloud: &str| {
            if base.cloud == cloud {
                base.region.clone()
            } else {
                gen_input::default_region(cloud).to_owned()
            }
        };
        let namespace = Setting {
            name: "KUBE_NAMESPACE",
            default: SettingDefault::EnvironmentName,
        };
        match self {
            Target::Kubectl => Plan {
                settings: vec![
                    text("KUBE_DEPLOYMENT", &workload.deployment),
                    text("KUBE_CONTAINER", &workload.deployment),
                    namespace,
                ],
                credentials: vec![KUBECONFIG],
                login: lines(KUBE_LOGIN),
                deploy: lines(&[
                    "kubectl set image \"deployment/$KUBE_DEPLOYMENT\" \"$KUBE_CONTAINER=$IMAGE_REF\" --namespace \"$KUBE_NAMESPACE\"",
                    "kubectl rollout status \"deployment/$KUBE_DEPLOYMENT\" --namespace \"$KUBE_NAMESPACE\" --timeout=300s",
                ]),
                needs_checkout: false,
            },
            Target::Helm => {
                let chart = if workload.chart_dir.is_empty() {
                    format!("charts/{name}")
                } else {
                    workload.chart_dir.clone()
                };
                Plan {
                    settings: vec![text("HELM_RELEASE", name), namespace],
                    credentials: vec![KUBECONFIG],
                    login: lines(KUBE_LOGIN),
                    deploy: vec![format!(
                        "helm upgrade --install \"$HELM_RELEASE\" \"./{chart}\" --namespace \"$KUBE_NAMESPACE\" --create-namespace --set-string image.repository=\"$IMAGE\" --set-string image.tag=\"$IMAGE_TAG\" --wait --timeout 5m"
                    )],
                    needs_checkout: true,
                }
            }
            Target::AzureContainerApps => Plan {
                settings: vec![
                    text("CONTAINER_APP", name),
                    text("AZURE_RESOURCE_GROUP", format!("rg-{name}")),
                ],
                credentials: AZURE_CREDENTIALS.to_vec(),
                login: lines(AZURE_LOGIN),
                deploy: lines(&[
                    "az containerapp update --name \"$CONTAINER_APP\" --resource-group \"$AZURE_RESOURCE_GROUP\" --image \"$IMAGE_REF\" --output none",
                ]),
                needs_checkout: false,
            },
            Target::AzureAppService => Plan {
                settings: vec![
                    text("AZURE_WEBAPP_NAME", name),
                    text("AZURE_RESOURCE_GROUP", format!("rg-{name}")),
                ],
                credentials: AZURE_CREDENTIALS.to_vec(),
                login: lines(AZURE_LOGIN),
                deploy: lines(&[
                    "az webapp config container set --name \"$AZURE_WEBAPP_NAME\" --resource-group \"$AZURE_RESOURCE_GROUP\" --container-image-name \"$IMAGE_REF\" --output none",
                    "az webapp restart --name \"$AZURE_WEBAPP_NAME\" --resource-group \"$AZURE_RESOURCE_GROUP\"",
                ]),
                needs_checkout: false,
            },
            Target::CloudRun => Plan {
                settings: vec![
                    text("CLOUD_RUN_SERVICE", name),
                    text("GCP_REGION", region("gcp")),
                ],
                credentials: vec![
                    Credential {
                        name: "GCP_SERVICE_ACCOUNT_KEY",
                        what: "the JSON key of the deploying service account",
                    },
                    Credential {
                        name: "GCP_PROJECT_ID",
                        what: "the Google Cloud project id",
                    },
                ],
                login: lines(&[
                    "printf '%s' \"$GCP_SERVICE_ACCOUNT_KEY\" > \"$HOME/gcp-key.json\"",
                    "gcloud auth activate-service-account --key-file \"$HOME/gcp-key.json\" --quiet",
                    "gcloud config set project \"$GCP_PROJECT_ID\" --quiet",
                ]),
                deploy: lines(&[
                    "gcloud run deploy \"$CLOUD_RUN_SERVICE\" --image \"$IMAGE_REF\" --region \"$GCP_REGION\" --quiet",
                ]),
                needs_checkout: false,
            },
            Target::Fly => Plan {
                settings: vec![text("FLY_APP", name)],
                credentials: vec![Credential {
                    name: "FLY_API_TOKEN",
                    what: "a Fly.io deploy token (`fly tokens create deploy`)",
                }],
                login: lines(&[
                    "curl -fsSL https://fly.io/install.sh | sh",
                    "export PATH=\"$HOME/.fly/bin:$PATH\"",
                ]),
                deploy: lines(&["flyctl deploy --app \"$FLY_APP\" --image \"$IMAGE_REF\""]),
                needs_checkout: false,
            },
            Target::AwsEcs => Plan {
                settings: vec![
                    text("ECS_CLUSTER", name),
                    text("ECS_SERVICE", name),
                    text("AWS_REGION", region("aws")),
                ],
                credentials: AWS_CREDENTIALS.to_vec(),
                login: lines(AWS_LOGIN),
                deploy: lines(&[
                    "CURRENT=$(aws ecs describe-services --cluster \"$ECS_CLUSTER\" --services \"$ECS_SERVICE\" --query 'services[0].taskDefinition' --output text)",
                    "aws ecs describe-task-definition --task-definition \"$CURRENT\" --query taskDefinition > task-definition.json",
                    "jq --arg image \"$IMAGE_REF\" '.containerDefinitions[0].image = $image | del(.taskDefinitionArn, .revision, .status, .requiresAttributes, .compatibilities, .registeredAt, .registeredBy)' task-definition.json > next-task-definition.json",
                    "NEXT=$(aws ecs register-task-definition --cli-input-json file://next-task-definition.json --query taskDefinition.taskDefinitionArn --output text)",
                    "aws ecs update-service --cluster \"$ECS_CLUSTER\" --service \"$ECS_SERVICE\" --task-definition \"$NEXT\" --query service.serviceArn --output text",
                    "aws ecs wait services-stable --cluster \"$ECS_CLUSTER\" --services \"$ECS_SERVICE\"",
                ]),
                needs_checkout: false,
            },
            Target::AwsLambda => Plan {
                settings: vec![
                    text("LAMBDA_FUNCTION", name),
                    text("AWS_REGION", region("aws")),
                ],
                credentials: AWS_CREDENTIALS.to_vec(),
                login: lines(AWS_LOGIN),
                deploy: lines(&[
                    "aws lambda update-function-code --function-name \"$LAMBDA_FUNCTION\" --image-uri \"$IMAGE_REF\" --query FunctionArn --output text",
                    "aws lambda wait function-updated --function-name \"$LAMBDA_FUNCTION\"",
                ]),
                needs_checkout: false,
            },
            Target::Render => Plan {
                settings: Vec::new(),
                credentials: vec![Credential {
                    name: "RENDER_DEPLOY_HOOK_URL",
                    what: "the deploy hook URL of the Render service (Settings, Deploy Hook)",
                }],
                login: Vec::new(),
                deploy: lines(&[
                    "curl -fsS -G \"$RENDER_DEPLOY_HOOK_URL\" --data-urlencode \"imgURL=$IMAGE_REF\"",
                ]),
                needs_checkout: false,
            },
            Target::SshCompose => Plan {
                settings: vec![
                    text("DEPLOY_DIR", format!("/opt/{name}")),
                    text("COMPOSE_SERVICE", "app"),
                ],
                credentials: vec![
                    Credential {
                        name: "DEPLOY_HOST",
                        what: "the host name or address of the server",
                    },
                    Credential {
                        name: "DEPLOY_USER",
                        what: "the user to connect as (must be allowed to run Docker)",
                    },
                    Credential {
                        name: "DEPLOY_SSH_KEY",
                        what: "the private key of that user",
                    },
                ],
                login: lines(&[
                    "mkdir -p \"$HOME/.ssh\"",
                    "printf '%s\\n' \"$DEPLOY_SSH_KEY\" > \"$HOME/.ssh/deploy_key\"",
                    "chmod 600 \"$HOME/.ssh/deploy_key\"",
                    "ssh-keyscan -H \"$DEPLOY_HOST\" >> \"$HOME/.ssh/known_hosts\"",
                ]),
                // The release image is written as a Compose override next to
                // the host's own file, so the base file needs no variable.
                deploy: lines(&[
                    "ssh -i \"$HOME/.ssh/deploy_key\" \"$DEPLOY_USER@$DEPLOY_HOST\" \"cd '$DEPLOY_DIR' && printf 'services:\\n  %s:\\n    image: %s\\n' '$COMPOSE_SERVICE' '$IMAGE_REF' > docker-compose.release.yml && docker compose -f docker-compose.yml -f docker-compose.release.yml pull '$COMPOSE_SERVICE' && docker compose -f docker-compose.yml -f docker-compose.release.yml up -d --remove-orphans\"",
                ]),
                needs_checkout: false,
            },
        }
    }

    /// Helm sets repository and tag apart; everything else takes one reference.
    pub fn needs_image_tag(self) -> bool {
        self == Target::Helm
    }

    /// Command-line tools the script calls, for runners that do not come
    /// with them (a Jenkins agent, a self-hosted pool).
    pub fn tools(self) -> &'static [&'static str] {
        match self {
            Target::Kubectl => &["kubectl"],
            Target::Helm => &["kubectl", "helm"],
            Target::AzureContainerApps | Target::AzureAppService => &["az"],
            Target::CloudRun => &["gcloud"],
            Target::Fly | Target::Render => &["curl"],
            Target::AwsEcs => &["aws", "jq"],
            Target::AwsLambda => &["aws"],
            Target::SshCompose => &["ssh"],
        }
    }
}

impl Target {
    /// Shell that turns `IMAGE`, `DIGEST` and `VERSION` into `ref`: the
    /// image by digest when there is one, by tag otherwise. A target that
    /// sets the tag apart also gets `tag`, with the digest appended so a
    /// chart that renders `repository:tag` still pins the exact image.
    pub fn resolve_reference(self) -> Vec<String> {
        let mut script = lines(&[
            "if [ -z \"$IMAGE\" ]; then echo \"An image is required\" >&2; exit 1; fi",
            "if [ -z \"$VERSION\" ] && [ -z \"$DIGEST\" ]; then echo \"A version or a digest is required\" >&2; exit 1; fi",
            "if [ -n \"$DIGEST\" ]; then ref=\"$IMAGE@$DIGEST\"; else ref=\"$IMAGE:$VERSION\"; fi",
        ]);
        if self.needs_image_tag() {
            script.extend(lines(&[
                "if [ -z \"$VERSION\" ]; then echo \"A chart is deployed by version: a version is required\" >&2; exit 1; fi",
                "if [ -n \"$DIGEST\" ]; then tag=\"$VERSION@$DIGEST\"; else tag=\"$VERSION\"; fi",
            ]));
        }
        script
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::input::fixtures;

    fn detect_for(ctx: &ProjectContext) -> TargetChoice {
        let base = fixtures::input(ctx, "");
        let workload = Workload {
            deployment: base.image.clone(),
            chart_dir: ctx
                .facts
                .iter()
                .find(|f| f.key == "k8s:helm")
                .map(|f| f.source_path.trim_end_matches("/Chart.yaml").to_owned())
                .unwrap_or_default(),
            ..Default::default()
        };
        detect(ctx, &base, &workload)
    }

    #[test]
    fn the_target_follows_the_strongest_evidence() {
        // Nothing known: the documented default, flagged as a default.
        let mut ctx = fixtures::rust_context();
        let choice = detect_for(&ctx);
        assert_eq!(choice.target, Target::SshCompose);
        assert!(!choice.detected);

        // A Compose file is evidence for the same target.
        ctx.facts.push(fixtures::fact(
            "c1",
            "container:compose:docker-compose.yml",
            "",
            "docker-compose.yml",
        ));
        let choice = detect_for(&ctx);
        assert_eq!(choice.target, Target::SshCompose);
        assert!(choice.detected);

        // Infrastructure code that names a cloud beats it.
        ctx.capabilities.push("cloud:gcp".into());
        assert_eq!(detect_for(&ctx).target, Target::CloudRun);

        // Manifests beat a cloud, a chart beats plain manifests.
        ctx.facts.push(fixtures::fact(
            "k1",
            "k8s:manifests",
            "",
            "deploy/k8s/deployment.yaml",
        ));
        assert_eq!(detect_for(&ctx).target, Target::Kubectl);
        ctx.facts.push(fixtures::fact(
            "k2",
            "k8s:helm",
            "api",
            "charts/api/Chart.yaml",
        ));
        let choice = detect_for(&ctx);
        assert_eq!(choice.target, Target::Helm);
        assert!(choice.evidence.contains("charts/api/Chart.yaml"));

        // What a pipeline already deploys to beats what merely exists.
        ctx.facts.push(fixtures::fact(
            "d1",
            "deploy:pipeline:.github/workflows/deploy.yml",
            "azure",
            ".github/workflows/deploy.yml",
        ));
        assert_eq!(detect_for(&ctx).target, Target::AzureContainerApps);

        // And the repository's own declaration beats everything.
        ctx.config = Some(sdi_proto::knowledge::v1::ProjectConfig {
            present: true,
            deploy_target: "fly".into(),
            ..Default::default()
        });
        let choice = detect_for(&ctx);
        assert_eq!(choice.target, Target::Fly);
        assert!(choice.evidence.contains("deploy.target: fly"));
        // `kubernetes` means the chart when there is one.
        ctx.config.as_mut().unwrap().deploy_target = "kubernetes".into();
        assert_eq!(detect_for(&ctx).target, Target::Helm);
        // `none` declares nothing: the facts decide again.
        ctx.config.as_mut().unwrap().deploy_target = "none".into();
        assert_eq!(detect_for(&ctx).target, Target::AzureContainerApps);
        // A release-only pipeline says nothing about where things run.
        ctx.facts.retain(|f| f.id != "d1");
        ctx.facts.push(fixtures::fact(
            "d2",
            "deploy:pipeline:.github/workflows/publish.yml",
            "release",
            ".github/workflows/publish.yml",
        ));
        assert_eq!(detect_for(&ctx).target, Target::Helm);
    }

    #[test]
    fn every_plan_is_one_command_per_line_and_names_what_it_reads() {
        let base = fixtures::input(&fixtures::node_context(), "");
        let workload = Workload {
            deployment: "view-engine".into(),
            chart_dir: "charts/view-engine".into(),
            ..Default::default()
        };
        let variable = regex::Regex::new(r"\$\{?([A-Za-z_][A-Za-z0-9_]*)").unwrap();
        for target in Target::ALL {
            let plan = target.plan(&base, &workload);
            assert!(!plan.deploy.is_empty(), "{target:?}");
            let script = plan
                .login
                .iter()
                .chain(&plan.deploy)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            for line in script.lines() {
                assert!(!line.ends_with('\\'), "{target:?}: continued line {line}");
                assert!(!line.trim().is_empty(), "{target:?}");
            }
            // Every variable a script reads is a setting, a credential or
            // one of the release variables.
            let known: Vec<&str> = plan
                .settings
                .iter()
                .map(|s| s.name)
                .chain(plan.credentials.iter().map(|c| c.name))
                .chain([
                    "IMAGE",
                    "IMAGE_REF",
                    "IMAGE_TAG",
                    "HOME",
                    "PATH",
                    "CURRENT",
                    "NEXT",
                    "image",
                ])
                .collect();
            for capture in variable.captures_iter(&script) {
                assert!(
                    known.contains(&&capture[1]),
                    "{target:?} reads ${} which nothing provides",
                    &capture[1]
                );
            }
            assert!(
                script.contains("$IMAGE_REF") || script.contains("$IMAGE_TAG"),
                "{target:?} deploys no image"
            );
            assert_eq!(target.needs_image_tag(), script.contains("$IMAGE_TAG"));
        }
    }
}
