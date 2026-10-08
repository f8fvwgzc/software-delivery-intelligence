//! The public pattern catalog from CONTRACT.md §5.6, seeded on every start.
//! Upserting by slug keeps `pattern_usage` (and so success rates) intact
//! while letting a release refresh plans and scores.

use sdi_common::embed::embed;
use sqlx::PgPool;

use crate::db::patterns::{PatternUpsert, PlanStepRow};

/// Seeds every catalog pattern and returns how many were written.
pub async fn seed(pool: &PgPool) -> anyhow::Result<usize> {
    let patterns = catalog();
    for pattern in &patterns {
        crate::db::patterns::upsert(pool, pattern).await?;
    }
    Ok(patterns.len())
}

struct Spec {
    id: &'static str,
    name: &'static str,
    summary: &'static str,
    category: &'static str,
    technologies: &'static [&'static str],
    cloud: &'static str,
    runtime: &'static str,
    iac: &'static str,
    delivery: &'static str,
    requires: &'static [&'static str],
    cost: f64,
    security: f64,
    complexity: i32,
    pros: &'static [&'static str],
    cons: &'static [&'static str],
    plan: Vec<PlanStepRow>,
}

/// One plan step. `depends_on` and `required_permissions` are comma lists.
#[allow(clippy::too_many_arguments)]
fn step(
    key: &str,
    title: &str,
    description: &str,
    action_type: &str,
    adapter: &str,
    depends_on: &str,
    permissions: &str,
    verification: &str,
) -> PlanStepRow {
    let list = |s: &str| {
        s.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    };
    PlanStepRow {
        key: key.to_owned(),
        title: title.to_owned(),
        description: description.to_owned(),
        action_type: action_type.to_owned(),
        adapter: adapter.to_owned(),
        depends_on: list(depends_on),
        required_permissions: list(permissions),
        params: Default::default(),
        verification: verification.to_owned(),
    }
}

fn analyze_step() -> PlanStepRow {
    step(
        "analyze",
        "Analyze repository",
        "Confirm the detected stack, entry points and existing delivery files before changing anything.",
        "repo.analyze",
        "github",
        "",
        "repository:read",
        "Analysis lists the build tool, runtime and existing workflows.",
    )
}

fn build_image_step(depends_on: &str) -> PlanStepRow {
    step(
        "build-image",
        "Build container image",
        "Build the image from the repository Dockerfile with a commit-sha tag.",
        "container.build",
        "docker",
        depends_on,
        "repository:read,container:build",
        "Image builds reproducibly and is tagged with the commit sha.",
    )
}

fn strs(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

impl Spec {
    fn into_upsert(self) -> PatternUpsert {
        let embedding_text = format!(
            "{} {} {}",
            self.name,
            self.summary,
            self.technologies.join(" ")
        );
        PatternUpsert {
            id: self.id.to_owned(),
            name: self.name.to_owned(),
            summary: self.summary.to_owned(),
            category: self.category.to_owned(),
            technologies: strs(self.technologies),
            cloud: self.cloud.to_owned(),
            runtime: self.runtime.to_owned(),
            iac: self.iac.to_owned(),
            delivery: self.delivery.to_owned(),
            requires: strs(self.requires),
            scope: "public_template".to_owned(),
            source: "catalog".to_owned(),
            estimated_monthly_cost_usd: self.cost,
            security_score: self.security,
            complexity: self.complexity,
            pros: strs(self.pros),
            cons: strs(self.cons),
            plan: self.plan,
            embedding: embed(&embedding_text),
        }
    }
}

pub fn catalog() -> Vec<PatternUpsert> {
    vec![
        Spec {
            id: "vm-docker-compose",
            name: "Single VM with Docker Compose",
            summary: "Run the whole stack on one virtual machine with Docker Compose behind nginx, deployed over SSH from GitHub Actions.",
            category: "deployment",
            technologies: &["Docker", "Docker Compose", "nginx", "GitHub Actions", "Linux VM"],
            cloud: "any",
            runtime: "vm_docker_compose",
            iac: "cloud-init",
            delivery: "GitHub Actions + SSH",
            requires: &["dockerfile"],
            cost: 25.0,
            security: 0.55,
            complexity: 2,
            pros: &["Cheapest way to run containers with a database", "Everything in one place, easy to debug", "Works on every cloud and on bare metal"],
            cons: &["Single point of failure", "Manual OS patching", "Vertical scaling only"],
            plan: vec![
                analyze_step(),
                build_image_step("analyze"),
                step("compose-local", "Verify the compose stack locally", "Bring up docker-compose.yml with the built image and a throwaway database.", "env.create_temporary", "docker", "build-image", "container:run,environment:write", "All services report healthy and the app answers on its port."),
                step("provision-vm", "Provision the virtual machine", "Create a small VM with Docker installed through cloud-init and a firewall allowing 80/443 only.", "cloud.create_dev_resource", "cloud", "analyze", "cloud:write", "SSH reachable, docker --version succeeds, only 22/80/443 open."),
                step("deploy", "Deploy with docker compose over SSH", "Copy the compose file, pull the image and run docker compose up -d on the VM.", "deploy.staging", "github", "compose-local,provision-vm", "deploy:staging", "docker compose ps shows every service running; the public URL returns 200."),
                step("smoke", "Smoke test the public endpoint", "Hit the health endpoint and one real page through nginx.", "tests.run_local", "local", "deploy", "", "Health endpoint returns 200 within 10 seconds."),
            ],
        },
        Spec {
            id: "github-pages-static",
            name: "Static site on GitHub Pages",
            summary: "Build the static site in GitHub Actions and publish it to GitHub Pages with the official deploy-pages action.",
            category: "deployment",
            technologies: &["GitHub Pages", "GitHub Actions"],
            cloud: "github",
            runtime: "static_hosting",
            iac: "none",
            delivery: "GitHub Actions + deploy-pages",
            requires: &["static_site"],
            cost: 0.0,
            security: 0.85,
            complexity: 1,
            pros: &["Free for public repositories", "Zero infrastructure", "HTTPS and CDN included"],
            cons: &["Static content only", "No server-side logic", "Custom domains need DNS work"],
            plan: vec![
                analyze_step(),
                step("build", "Build the site locally", "Run the detected build command and confirm the output directory.", "tests.run_local", "local", "analyze", "", "Build exits 0 and the output directory contains index.html."),
                step("workflow", "Add the Pages workflow", "Generate .github/workflows/pages.yml with configure-pages, upload-pages-artifact and deploy-pages.", "docs.generate", "github", "build", "repository:read", "Workflow file validates and references the right output directory."),
                step("publish", "Publish to GitHub Pages", "Enable Pages with the Actions source and run the workflow.", "deploy.staging", "github", "workflow", "deploy:staging", "The Pages URL serves the built site."),
            ],
        },
        Spec {
            id: "azure-container-apps",
            name: "Azure Container Apps + GitHub OIDC",
            summary: "Serverless containers on Azure Container Apps, images in Azure Container Registry, deployed by GitHub Actions through OIDC without stored secrets.",
            category: "deployment",
            technologies: &["Azure Container Apps", "Azure Container Registry", "GitHub Actions", "OIDC", "Bicep"],
            cloud: "azure",
            runtime: "serverless_container",
            iac: "bicep",
            delivery: "GitHub Actions + OIDC",
            requires: &["dockerfile", "ci:github_actions"],
            cost: 40.0,
            security: 0.85,
            complexity: 3,
            pros: &["Scales to zero", "No long-lived cloud credentials in GitHub", "Managed TLS and revisions"],
            cons: &["Azure-only", "Cold starts when scaled to zero", "Bicep learning curve"],
            plan: vec![
                analyze_step(),
                build_image_step("analyze"),
                step("provision", "Provision registry and environment", "Deploy Bicep for a resource group, Container Registry and Container Apps environment.", "cloud.create_dev_resource", "cloud", "analyze", "cloud:write", "az deployment group show reports Succeeded."),
                step("oidc", "Configure GitHub OIDC federation", "Create the federated credential and role assignment for the repository's workflow identity.", "cloud.modify_resource", "cloud", "provision", "cloud:write", "A workflow run can az login with the federated token."),
                step("deploy-staging", "Deploy to staging revision", "Push the image to ACR and update the Container App with a staging label.", "deploy.staging", "github", "build-image,oidc", "deploy:staging", "Revision is healthy and the staging URL answers 200."),
                step("deploy-production", "Promote to production", "Shift 100% traffic to the verified revision.", "deploy.production", "cloud", "deploy-staging", "deploy:production", "Production URL serves the new revision; previous revision kept for rollback."),
            ],
        },
        Spec {
            id: "aws-ecs-fargate",
            name: "AWS ECS Fargate + Terraform",
            summary: "Containers on ECS Fargate behind an Application Load Balancer, infrastructure in Terraform, images in ECR.",
            category: "deployment",
            technologies: &["AWS ECS", "Fargate", "ECR", "Terraform", "GitHub Actions"],
            cloud: "aws",
            runtime: "serverless_container",
            iac: "terraform",
            delivery: "GitHub Actions + OIDC + Terraform",
            requires: &["dockerfile"],
            cost: 75.0,
            security: 0.8,
            complexity: 4,
            pros: &["No servers to patch", "Mature ecosystem and IAM integration", "Infrastructure fully in code"],
            cons: &["Higher baseline cost (ALB + NAT)", "Terraform state and IAM take time to set up", "AWS-only"],
            plan: vec![
                analyze_step(),
                build_image_step("analyze"),
                step("terraform-plan", "Plan the Terraform stack", "Write and plan the VPC, ECR, ECS cluster, task definition and ALB modules.", "repo.analyze", "cloud", "analyze", "repository:read", "terraform plan succeeds with the expected resource count."),
                step("provision", "Apply the Terraform stack", "Apply the plan to create the network, registry and cluster.", "cloud.create_dev_resource", "cloud", "terraform-plan", "cloud:write", "terraform apply completes; ALB DNS resolves."),
                step("deploy-staging", "Deploy the service to staging", "Push the image to ECR and roll out a new task definition revision.", "deploy.staging", "cloud", "build-image,provision", "deploy:staging", "ECS service reaches steady state with healthy targets."),
                step("deploy-production", "Deploy to production", "Roll the verified task definition into the production service.", "deploy.production", "cloud", "deploy-staging", "deploy:production", "Production targets healthy; previous revision kept for rollback."),
            ],
        },
        Spec {
            id: "gcp-cloud-run",
            name: "Google Cloud Run + Artifact Registry",
            summary: "Fully managed containers on Cloud Run with images in Artifact Registry and Workload Identity Federation from GitHub Actions.",
            category: "deployment",
            technologies: &["Google Cloud Run", "Artifact Registry", "GitHub Actions", "Workload Identity"],
            cloud: "gcp",
            runtime: "serverless_container",
            iac: "terraform",
            delivery: "GitHub Actions + Workload Identity",
            requires: &["dockerfile"],
            cost: 20.0,
            security: 0.85,
            complexity: 3,
            pros: &["Pay per request, scales to zero", "Simple revision-based rollbacks", "Managed HTTPS"],
            cons: &["GCP-only", "Request timeout limits", "Stateful workloads need extra services"],
            plan: vec![
                analyze_step(),
                build_image_step("analyze"),
                step("provision", "Provision registry and identity", "Create the Artifact Registry repository and the Workload Identity pool for GitHub.", "cloud.create_dev_resource", "cloud", "analyze", "cloud:write", "gcloud artifacts repositories describe succeeds; the identity pool is active."),
                step("deploy-staging", "Deploy a staging revision", "Push the image and deploy a Cloud Run revision without traffic.", "deploy.staging", "cloud", "build-image,provision", "deploy:staging", "Revision ready; tagged URL answers 200."),
                step("deploy-production", "Shift traffic to production", "Move 100% of traffic to the verified revision.", "deploy.production", "cloud", "deploy-staging", "deploy:production", "Service URL serves the new revision."),
            ],
        },
        Spec {
            id: "kubernetes-helm",
            name: "Kubernetes with Helm",
            summary: "Package the existing manifests as a Helm chart and roll out with helm upgrade from GitHub Actions.",
            category: "deployment",
            technologies: &["Kubernetes", "Helm", "GitHub Actions"],
            cloud: "any",
            runtime: "kubernetes",
            iac: "helm",
            delivery: "GitHub Actions + helm upgrade",
            requires: &["dockerfile", "k8s:manifests"],
            cost: 150.0,
            security: 0.7,
            complexity: 5,
            pros: &["Portable across clouds", "Rolling updates and health checks built in", "Chart values per environment"],
            cons: &["Cluster cost and operations", "Steep learning curve", "Secrets management needs extra tooling"],
            plan: vec![
                analyze_step(),
                build_image_step("analyze"),
                step("chart", "Generate the Helm chart", "Convert the manifests into a chart with values for image tag, replicas and ingress.", "docs.generate", "kubernetes", "analyze", "repository:read", "helm lint passes and helm template renders the original resources."),
                step("kind", "Verify on a disposable cluster", "Install the chart on a local kind cluster.", "env.create_temporary", "kubernetes", "chart,build-image", "environment:write", "All pods Ready; service reachable through port-forward."),
                step("deploy-staging", "helm upgrade into staging", "Upgrade the release in the staging namespace with the new image tag.", "deploy.staging", "kubernetes", "kind", "deploy:staging", "helm status is deployed; rollout status complete."),
                step("smoke", "Smoke test staging", "Call the health endpoint through the ingress.", "tests.run_local", "local", "deploy-staging", "", "Health endpoint returns 200."),
            ],
        },
        Spec {
            id: "fly-io",
            name: "Fly.io machines",
            summary: "Run the container on Fly.io machines close to users, with flyctl deploy from GitHub Actions.",
            category: "deployment",
            technologies: &["Fly.io", "GitHub Actions", "Docker"],
            cloud: "self_hosted",
            runtime: "paas",
            iac: "fly.toml",
            delivery: "GitHub Actions + flyctl",
            requires: &["dockerfile"],
            cost: 10.0,
            security: 0.7,
            complexity: 2,
            pros: &["Minutes from Dockerfile to URL", "Cheap for small apps", "Managed Postgres and volumes available"],
            cons: &["Smaller ecosystem than the big clouds", "Deploy token stored as a secret", "Limited compliance options"],
            plan: vec![
                analyze_step(),
                build_image_step("analyze"),
                step("app", "Create the Fly app", "Generate fly.toml and create the app in the chosen region.", "cloud.create_dev_resource", "cloud", "analyze", "cloud:write", "fly apps list shows the app."),
                step("deploy-staging", "Deploy to the staging app", "flyctl deploy with the built image to the staging app.", "deploy.staging", "cloud", "build-image,app", "deploy:staging", "fly status shows machines started; URL answers 200."),
                step("deploy-production", "Deploy to production", "flyctl deploy to the production app.", "deploy.production", "cloud", "deploy-staging", "deploy:production", "Production machines healthy; checks passing."),
            ],
        },
        Spec {
            id: "gha-ci-build-test",
            name: "GitHub Actions: lint, test, build",
            summary: "A single CI workflow that lints, tests and builds on every pull request and push to the default branch, with dependency caching.",
            category: "ci_cd",
            technologies: &["GitHub Actions"],
            cloud: "github",
            runtime: "",
            iac: "none",
            delivery: "GitHub Actions",
            requires: &[],
            cost: 0.0,
            security: 0.8,
            complexity: 1,
            pros: &["Catches regressions before merge", "Free minutes for public repositories", "Cache keeps runs fast"],
            cons: &["Minutes cost money on private repositories", "Flaky tests block merges"],
            plan: vec![
                analyze_step(),
                step("install", "Install dependencies", "Install with the detected package manager to confirm the lock file is valid.", "deps.install", "local", "analyze", "repository:read", "Install exits 0 without lock file changes."),
                step("verify", "Run lint, tests and build locally", "Run the detected lint, test and build commands.", "tests.run_local", "local", "install", "", "All three commands exit 0."),
                step("workflow", "Add ci.yml", "Generate .github/workflows/ci.yml with cache, lint, test and build jobs.", "docs.generate", "github", "verify", "repository:read", "Workflow validates and matches the local commands."),
            ],
        },
        Spec {
            id: "gha-docker-publish",
            name: "GitHub Actions: build and publish image",
            summary: "Build the Docker image with Buildx and publish it to GitHub Container Registry tagged by commit and branch.",
            category: "ci_cd",
            technologies: &["GitHub Actions", "Docker Buildx", "GHCR"],
            cloud: "github",
            runtime: "",
            iac: "none",
            delivery: "GitHub Actions + GHCR",
            requires: &["dockerfile"],
            cost: 0.0,
            security: 0.75,
            complexity: 2,
            pros: &["Images ready for any deployment target", "Uses the built-in GITHUB_TOKEN", "Layer cache between runs"],
            cons: &["Registry storage counts toward quota", "Needs image signing for supply-chain guarantees"],
            plan: vec![
                analyze_step(),
                build_image_step("analyze"),
                step("workflow", "Add the publish workflow", "Generate .github/workflows/publish.yml with docker/metadata-action and build-push-action.", "docs.generate", "github", "build-image", "repository:read", "Workflow validates and uses the GITHUB_TOKEN for GHCR."),
                step("publish", "Publish the first image", "Run the workflow once and confirm the package appears.", "deploy.staging", "github", "workflow", "deploy:staging", "ghcr.io package exists with the commit tag."),
            ],
        },
        Spec {
            id: "dependabot-weekly",
            name: "Dependabot weekly grouped updates",
            summary: "A dependabot.yml that updates every detected ecosystem weekly, grouping minor and patch bumps into one pull request.",
            category: "dependency",
            technologies: &["Dependabot", "GitHub"],
            cloud: "github",
            runtime: "",
            iac: "none",
            delivery: "GitHub",
            requires: &["lockfile"],
            cost: 0.0,
            security: 0.9,
            complexity: 1,
            pros: &["Security patches arrive automatically", "Grouping keeps PR noise low", "No infrastructure"],
            cons: &["Needs CI to be trustworthy", "Major updates still need a human"],
            plan: vec![
                analyze_step(),
                step("config", "Generate dependabot.yml", "Write .github/dependabot.yml covering each detected ecosystem plus github-actions, weekly, grouped.", "docs.generate", "github", "analyze", "repository:read", "File validates against the Dependabot schema and lists every manifest directory."),
                step("verify", "Confirm CI covers dependency updates", "Run the test suite so update PRs have a signal.", "tests.run_local", "local", "config", "", "Tests exit 0."),
            ],
        },
        Spec {
            id: "docker-compose-test-env",
            name: "Disposable Docker Compose test environment",
            summary: "A compose file that spins up the app with its database for integration tests, in CI and locally, and tears itself down.",
            category: "testing",
            technologies: &["Docker Compose", "GitHub Actions"],
            cloud: "any",
            runtime: "vm_docker_compose",
            iac: "none",
            delivery: "GitHub Actions",
            requires: &["docker_compose"],
            cost: 0.0,
            security: 0.7,
            complexity: 2,
            pros: &["Same environment locally and in CI", "Real database instead of mocks", "Cheap to recreate"],
            cons: &["Slower than unit tests", "Port conflicts on shared machines"],
            plan: vec![
                analyze_step(),
                build_image_step("analyze"),
                step("database", "Start a temporary database", "Bring up the database service from the compose file with a throwaway volume.", "db.create_temporary", "docker", "analyze", "container:run,database:write", "Database accepts connections."),
                step("env", "Start the stack", "Bring up the remaining services against the temporary database.", "env.create_temporary", "docker", "build-image,database", "container:run,environment:write", "docker compose ps shows every service healthy."),
                step("tests", "Run the integration tests", "Run the test suite against the compose stack.", "tests.run_local", "local", "env", "", "Tests exit 0."),
                step("teardown", "Tear the environment down", "docker compose down -v to remove containers and volumes.", "resource.delete", "docker", "tests", "environment:write", "No containers or volumes from the stack remain."),
            ],
        },
        Spec {
            id: "uat-checklist",
            name: "Generated UAT checklist per change",
            summary: "Derive a user-acceptance checklist from the changed files and routes so every release gets the same manual verification.",
            category: "testing",
            technologies: &["Markdown", "GitHub"],
            cloud: "any",
            runtime: "",
            iac: "none",
            delivery: "GitHub pull request",
            requires: &[],
            cost: 0.0,
            security: 0.8,
            complexity: 1,
            pros: &["Manual testing becomes repeatable", "Checklist lives with the change", "No tooling to install"],
            cons: &["Still manual", "Checklist quality depends on the analysis"],
            plan: vec![
                analyze_step(),
                step("checklist", "Generate the UAT checklist", "Write docs/uat/<change>.md listing the affected pages, APIs and data flows with pass/fail boxes.", "docs.generate", "github", "analyze", "repository:read", "Checklist names every changed route and screen."),
                step("verify", "Run automated checks first", "Run the existing tests so UAT only covers what automation cannot.", "tests.run_local", "local", "checklist", "", "Tests exit 0."),
            ],
        },
        Spec {
            id: "terraform-remote-state",
            name: "Terraform remote state + plan on PR",
            summary: "Move Terraform state to a locked remote backend and post terraform plan output on every pull request before anyone applies.",
            category: "infrastructure",
            technologies: &["Terraform", "GitHub Actions"],
            cloud: "any",
            runtime: "",
            iac: "terraform",
            delivery: "GitHub Actions + terraform plan",
            requires: &["iac:terraform"],
            cost: 5.0,
            security: 0.85,
            complexity: 3,
            pros: &["No state on laptops", "Every change reviewed as a plan", "State locking prevents corruption"],
            cons: &["Backend bootstrap is a one-off manual step", "Plan output can leak values if not masked"],
            plan: vec![
                analyze_step(),
                step("state-store", "Create the state backend", "Create a versioned, encrypted bucket or storage account with a lock table.", "cloud.create_dev_resource", "cloud", "analyze", "cloud:write", "Backend exists with versioning and encryption enabled."),
                step("backend", "Migrate state to the backend", "Add the backend block and run terraform init -migrate-state.", "cloud.modify_resource", "cloud", "state-store", "cloud:write", "terraform state list matches the previous local state."),
                step("validate", "Validate the configuration", "Run terraform fmt -check and terraform validate.", "tests.run_local", "local", "backend", "", "Both commands exit 0."),
                step("workflow", "Add the plan-on-PR workflow", "Generate a workflow that runs terraform plan and comments the result on pull requests.", "docs.generate", "github", "validate", "repository:read", "Workflow validates; a test PR receives a plan comment."),
            ],
        },
        Spec {
            id: "secret-scanning-baseline",
            name: "Secret scanning + env example",
            summary: "Scan history and every push for leaked secrets, document required variables in .env.example and rotate anything found.",
            category: "security",
            technologies: &["gitleaks", "GitHub Actions"],
            cloud: "any",
            runtime: "",
            iac: "none",
            delivery: "GitHub Actions",
            requires: &[],
            cost: 0.0,
            security: 0.95,
            complexity: 1,
            pros: &["Stops credentials before they land", "Onboarding knows every variable", "Free"],
            cons: &["False positives need an allowlist", "Rotation of found secrets is manual"],
            plan: vec![
                analyze_step(),
                step("env-example", "Write .env.example", "List every environment variable the code reads with a placeholder and a comment.", "docs.generate", "github", "analyze", "repository:read", "Every variable read from the environment appears in the file."),
                step("scan", "Scan the repository history", "Run gitleaks over the full history.", "tests.run_local", "local", "analyze", "", "Scan completes; findings are listed."),
                step("workflow", "Add the scanning workflow", "Generate a workflow running gitleaks on every push and pull request.", "docs.generate", "github", "scan", "repository:read", "Workflow validates and fails on a test secret."),
                step("rotate", "Rotate any leaked secret", "Rotate every credential the scan found and update the consumers.", "secrets.rotate", "cloud", "scan", "secrets:write", "Old credentials are rejected; services run on the new ones."),
            ],
        },
        Spec {
            id: "readme-architecture-docs",
            name: "README + architecture document",
            summary: "Generate a README that gets a newcomer running and an architecture document with the request flow and component map from the knowledge graph.",
            category: "documentation",
            technologies: &["Markdown", "Mermaid"],
            cloud: "any",
            runtime: "",
            iac: "none",
            delivery: "GitHub pull request",
            requires: &[],
            cost: 0.0,
            security: 0.9,
            complexity: 1,
            pros: &["Documentation derived from facts, not memory", "Diagrams stay close to the code", "Zero cost"],
            cons: &["Needs a rescan to stay current", "Prose still benefits from a human pass"],
            plan: vec![
                analyze_step(),
                step("readme", "Generate README.md", "Write setup, run, test and deploy sections from the detected commands.", "docs.generate", "github", "analyze", "repository:read", "Every command in the README exists in the repository."),
                step("architecture", "Generate docs/architecture.md", "Write the component map and request flow with a Mermaid diagram from the graph.", "docs.generate", "github", "analyze", "repository:read", "Diagram nodes match the knowledge graph."),
                step("links", "Check the documents", "Run a link and markdown lint over the generated files.", "tests.run_local", "local", "readme,architecture", "", "No broken links or lint errors."),
            ],
        },
    ]
    .into_iter()
    .map(Spec::into_upsert)
    .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const ACTION_TYPES: [&str; 19] = [
        "repo.read",
        "repo.analyze",
        "tests.run_local",
        "docs.generate",
        "deps.install",
        "container.build",
        "container.run",
        "env.create_temporary",
        "db.create_temporary",
        "cloud.create_dev_resource",
        "cloud.modify_resource",
        "deploy.staging",
        "deploy.production",
        "db.migrate_production",
        "backup.restore_production",
        "resource.delete",
        "firewall.modify",
        "secrets.rotate",
        "db.destroy_production",
    ];

    #[test]
    fn fifteen_patterns_with_valid_plans() {
        let patterns = catalog();
        assert_eq!(patterns.len(), 15);
        let ids: HashSet<&str> = patterns.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids.len(), 15);

        for pattern in &patterns {
            assert!(!pattern.plan.is_empty(), "{} has no plan", pattern.id);
            assert!(pattern.plan.len() <= 8, "{} plan too long", pattern.id);
            assert_eq!(pattern.embedding.len(), sdi_common::embed::EMBEDDING_DIM);
            assert!((1..=5).contains(&pattern.complexity));
            let keys: HashSet<&str> = pattern.plan.iter().map(|s| s.key.as_str()).collect();
            assert_eq!(
                keys.len(),
                pattern.plan.len(),
                "{} has duplicate step keys",
                pattern.id
            );
            for step in &pattern.plan {
                assert!(
                    ACTION_TYPES.contains(&step.action_type.as_str()),
                    "{}: bad action {}",
                    pattern.id,
                    step.action_type
                );
                assert!(!step.verification.is_empty());
                for dep in &step.depends_on {
                    assert!(
                        keys.contains(dep.as_str()),
                        "{}: {} depends on unknown {dep}",
                        pattern.id,
                        step.key
                    );
                }
                for permission in &step.required_permissions {
                    assert!(
                        permission.contains(':'),
                        "{}: bad permission {permission}",
                        pattern.id
                    );
                }
            }
        }
    }
}
