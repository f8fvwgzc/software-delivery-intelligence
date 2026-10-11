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
    /// Cost model: `(item, monthly_usd, note)`; sums to `cost`. The
    /// intelligence generator rescales it per project.
    cost_lines: &'static [(&'static str, f64, &'static str)],
    /// Maturity axes a modernization pattern raises (CONTRACT.md §13); empty otherwise.
    targets_axes: &'static [&'static str],
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

fn checkout_step(depends_on: &str) -> PlanStepRow {
    step(
        "checkout",
        "Download the repository",
        "Fetch the repository archive at the scanned commit into a working directory.",
        "repo.checkout",
        "docker",
        depends_on,
        "repository:read",
        "The working directory contains the repository tree at the expected commit.",
    )
}

fn pull_request_step(depends_on: &str, files: &str) -> PlanStepRow {
    step(
        "pull-request",
        "Open a pull request",
        &format!(
            "Commit {files} on a branch and open a pull request describing the change and how it was verified."
        ),
        "repo.open_pull_request",
        "github",
        depends_on,
        "repository:write",
        "The pull request exists, contains only the generated files and CI starts on it.",
    )
}

fn strs(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// `{cost_lines: [{item, monthly_usd, note}], targets_axes: [..]}` — the
/// shape other services read from `Pattern` attributes.
pub fn attributes(cost_lines: &[(&str, f64, &str)], targets_axes: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "cost_lines": cost_lines
            .iter()
            .map(|(item, monthly_usd, note)| serde_json::json!({
                "item": item,
                "monthly_usd": monthly_usd,
                "note": note,
            }))
            .collect::<Vec<_>>(),
        "targets_axes": targets_axes,
    })
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
            workspace_id: None,
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
            attributes: attributes(self.cost_lines, self.targets_axes),
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
            cost_lines: &[("Small VM (2 vCPU, 4 GB)", 20.0, "compute"), ("Static IP and egress", 3.0, "networking"), ("GitHub Actions minutes", 2.0, "ci")],
            targets_axes: &[],
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
            cost_lines: &[("GitHub Pages", 0.0, "compute: free for public repositories"), ("GitHub Actions minutes", 0.0, "ci: free tier")],
            targets_axes: &[],
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
            cost_lines: &[("Container Apps 0.5 vCPU / 1 GiB, scale to zero", 25.0, "compute"), ("Azure Container Registry Basic", 5.0, "registry"), ("Ingress and egress", 5.0, "networking"), ("GitHub Actions minutes", 5.0, "ci")],
            targets_axes: &[],
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
            cost_lines: &[("Fargate task 0.5 vCPU / 1 GB", 18.0, "compute"), ("Application Load Balancer", 22.0, "networking"), ("NAT gateway", 32.0, "networking"), ("ECR storage", 1.0, "registry"), ("GitHub Actions minutes", 2.0, "ci")],
            targets_axes: &[],
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
            cost_lines: &[("Cloud Run requests and CPU time", 12.0, "compute"), ("Artifact Registry", 3.0, "registry"), ("Egress", 3.0, "networking"), ("GitHub Actions minutes", 2.0, "ci")],
            targets_axes: &[],
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
            cost_lines: &[("Managed control plane", 73.0, "compute"), ("Two worker nodes", 60.0, "compute"), ("Load balancer", 15.0, "networking"), ("GitHub Actions minutes", 2.0, "ci")],
            targets_axes: &[],
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
            cost_lines: &[("shared-cpu-1x machine", 5.0, "compute"), ("3 GB volume", 1.0, "database"), ("Dedicated IPv4", 2.0, "networking"), ("GitHub Actions minutes", 2.0, "ci")],
            targets_axes: &[],
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
            cost_lines: &[("GitHub Actions minutes", 0.0, "ci: free tier for public repositories")],
            targets_axes: &[],
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
            cost_lines: &[("GHCR storage", 0.0, "registry: free tier"), ("GitHub Actions minutes", 0.0, "ci")],
            targets_axes: &[],
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
            cost_lines: &[("Dependabot", 0.0, "ci: included with GitHub")],
            targets_axes: &[],
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
            cost_lines: &[("GitHub Actions minutes", 0.0, "ci: free tier")],
            targets_axes: &[],
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
            cost_lines: &[("Markdown in the repository", 0.0, "ci: no infrastructure")],
            targets_axes: &[],
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
            cost_lines: &[("State bucket with versioning", 3.0, "database"), ("Plan-on-PR minutes", 2.0, "ci")],
            targets_axes: &[],
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
            cost_lines: &[("gitleaks in GitHub Actions", 0.0, "ci: free")],
            targets_axes: &[],
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
            cost_lines: &[("Markdown in the repository", 0.0, "ci: no infrastructure")],
            targets_axes: &[],
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
        // --- modernization: each raises one or more maturity axes ------------
        Spec {
            id: "containerize-legacy-app",
            name: "Containerize an application that has no Dockerfile",
            summary: "Generate a multi-stage Dockerfile and .dockerignore from the detected runtime, build and run the image locally, then open a pull request with the files.",
            category: "modernization",
            technologies: &["Docker", "GitHub"],
            cloud: "any",
            runtime: "",
            iac: "none",
            delivery: "GitHub pull request",
            requires: &[],
            cost: 0.0,
            cost_lines: &[("Local Docker build", 0.0, "compute: runs on the operator's machine"), ("GitHub Actions minutes", 0.0, "ci: free tier")],
            targets_axes: &["containerization"],
            security: 0.8,
            complexity: 2,
            pros: &["Same artifact in every environment", "Unlocks every container deployment pattern", "Non-root user and pinned base image from day one"],
            cons: &["Build context needs a .dockerignore to stay small", "Runtime version must be chosen explicitly"],
            plan: vec![
                analyze_step(),
                checkout_step("analyze"),
                step("dockerfile", "Generate Dockerfile and .dockerignore", "Write a multi-stage Dockerfile for the detected language, package manager and entry point, running as a non-root user.", "docs.generate", "github", "checkout", "repository:read", "docker build parses the file; the image exposes the detected port."),
                step("build", "Build the image", "docker build the generated Dockerfile in the working directory.", "container.build", "docker", "dockerfile", "container:build", "Build exits 0 and the image is tagged with the commit sha."),
                step("run", "Run the container", "Start the image with the configured port and poll the health path.", "container.run", "docker", "build", "container:run", "The health path answers 200 within 30 seconds."),
                step("teardown", "Remove the temporary container", "Stop and remove the container and the working directory.", "env.destroy_temporary", "docker", "run", "environment:write", "No container from this run is left on the host."),
                pull_request_step("teardown", "Dockerfile and .dockerignore"),
            ],
        },
        Spec {
            id: "add-ci-baseline",
            name: "Add a CI baseline where none exists",
            summary: "Generate a GitHub Actions workflow that installs, tests and builds on every push and pull request, verified locally first, delivered as a pull request.",
            category: "modernization",
            technologies: &["GitHub Actions"],
            cloud: "github",
            runtime: "",
            iac: "none",
            delivery: "GitHub pull request",
            requires: &[],
            cost: 0.0,
            cost_lines: &[("GitHub Actions minutes", 0.0, "ci: free tier for public repositories")],
            targets_axes: &["ci", "tests"],
            security: 0.8,
            complexity: 1,
            pros: &["Every change gets a test signal", "Cache keeps runs under a few minutes", "Prerequisite for any automated deploy"],
            cons: &["Private repositories pay for minutes", "A missing test command yields a build-only workflow"],
            plan: vec![
                analyze_step(),
                checkout_step("analyze"),
                step("install", "Install dependencies", "Install with the detected package manager inside a clean container.", "deps.install", "docker", "checkout", "repository:read,container:run", "Install exits 0 without lock file changes."),
                step("verify", "Run the test command", "Run the detected or configured test command once.", "tests.run_local", "docker", "install", "container:run", "Tests exit 0."),
                step("workflow", "Generate ci.yml", "Write .github/workflows/ci.yml with cache, install, test and build jobs matching what just ran.", "docs.generate", "github", "verify", "repository:read", "Workflow validates and names the same commands."),
                pull_request_step("workflow", ".github/workflows/ci.yml"),
            ],
        },
        Spec {
            id: "upgrade-runtime",
            name: "Move an end-of-life runtime to a supported version",
            summary: "Bump the runtime pin (version file, manifest engines, Dockerfile base image) to the current LTS, reinstall, run the tests and build, then open a pull request.",
            category: "modernization",
            technologies: &["Docker", "GitHub"],
            cloud: "any",
            runtime: "",
            iac: "none",
            delivery: "GitHub pull request",
            requires: &[],
            cost: 0.0,
            cost_lines: &[("Local build and test", 0.0, "compute: runs on the operator's machine")],
            targets_axes: &["runtime", "dependencies"],
            security: 0.9,
            complexity: 3,
            pros: &["Security patches resume", "Newer runtimes are faster", "Removes the biggest audit finding"],
            cons: &["Breaking changes may need code fixes", "Dependencies may need bumps too"],
            plan: vec![
                analyze_step(),
                checkout_step("analyze"),
                step("bump", "Update the runtime pins", "Rewrite .nvmrc, engines, runtime.txt, pyproject, go.mod, Cargo.toml edition or the Dockerfile base image to the supported version.", "docs.generate", "github", "checkout", "repository:read", "Every runtime pin names the target version."),
                step("install", "Reinstall dependencies", "Install with the new runtime to surface incompatible packages.", "deps.install", "docker", "bump", "repository:read,container:run", "Install exits 0."),
                step("tests", "Run the tests", "Run the test command on the new runtime.", "tests.run_local", "docker", "install", "container:run", "Tests exit 0."),
                step("build", "Build the image", "Build the Dockerfile with the new base image when one exists.", "container.build", "docker", "tests", "container:build", "Build exits 0 and the image reports the new runtime version."),
                pull_request_step("build", "the runtime pins"),
            ],
        },
        Spec {
            id: "config-to-environment",
            name: "Externalise configuration into environment variables",
            summary: "Find hard-coded hosts, ports, credentials and feature flags, read them from the environment instead, document them in .env.example and open a pull request.",
            category: "modernization",
            technologies: &["dotenv", "GitHub"],
            cloud: "any",
            runtime: "",
            iac: "none",
            delivery: "GitHub pull request",
            requires: &[],
            cost: 0.0,
            cost_lines: &[("Local test run", 0.0, "compute: runs on the operator's machine")],
            targets_axes: &["secrets", "containerization"],
            security: 0.95,
            complexity: 2,
            pros: &["Same image runs in every environment", "Secrets leave the repository", "Onboarding knows every variable"],
            cons: &["Every deployment target must now provide the variables", "Defaults need care to keep local runs easy"],
            plan: vec![
                analyze_step(),
                checkout_step("analyze"),
                step("inventory", "Inventory configuration values", "List literal hosts, ports, keys and flags in code and compose files with their source lines.", "repo.analyze", "docker", "checkout", "repository:read", "The inventory names every value with a file and line."),
                step("externalise", "Read values from the environment", "Replace each literal with an environment lookup and a safe default; write .env.example with one line per variable.", "docs.generate", "github", "inventory", "repository:read", ".env.example lists every variable the code now reads."),
                step("tests", "Run the tests with .env.example", "Run the test command with the example values loaded.", "tests.run_local", "docker", "externalise", "container:run", "Tests exit 0."),
                pull_request_step("tests", "the configuration changes and .env.example"),
            ],
        },
        Spec {
            id: "add-healthcheck",
            name: "Add a health endpoint and container healthcheck",
            summary: "Add a GET health route that checks the datastore, a HEALTHCHECK in the Dockerfile and a compose healthcheck, verified by running the image.",
            category: "modernization",
            technologies: &["Docker", "GitHub"],
            cloud: "any",
            runtime: "",
            iac: "none",
            delivery: "GitHub pull request",
            requires: &["dockerfile"],
            cost: 0.0,
            cost_lines: &[("Local Docker run", 0.0, "compute: runs on the operator's machine")],
            targets_axes: &["containerization"],
            security: 0.8,
            complexity: 2,
            pros: &["Orchestrators restart unhealthy containers", "Deploys can wait for readiness", "Load balancers stop routing to broken instances"],
            cons: &["A health check that touches the database adds load", "Route must stay unauthenticated"],
            plan: vec![
                analyze_step(),
                checkout_step("analyze"),
                step("endpoint", "Add the health route and HEALTHCHECK", "Add GET /health returning the version and datastore status; add HEALTHCHECK to the Dockerfile and healthcheck to compose.", "docs.generate", "github", "checkout", "repository:read", "The route exists in the framework's router and the Dockerfile has a HEALTHCHECK."),
                step("build", "Build the image", "docker build with the new HEALTHCHECK.", "container.build", "docker", "endpoint", "container:build", "Build exits 0."),
                step("run", "Run and watch the health status", "Start the container and wait for docker inspect to report healthy.", "container.run", "docker", "build", "container:run", "docker inspect reports healthy within 60 seconds."),
                step("teardown", "Remove the temporary container", "Stop and remove the container.", "env.destroy_temporary", "docker", "run", "environment:write", "No container from this run remains."),
                pull_request_step("teardown", "the health route, Dockerfile and compose changes"),
            ],
        },
        Spec {
            id: "managed-database",
            name: "Move a self-hosted database to a managed service",
            summary: "Provision a managed instance of the detected engine, rehearse the migrations against a temporary copy, point the configuration at the new host and cut production over with a backup in hand.",
            category: "modernization",
            technologies: &["Managed PostgreSQL / MySQL", "Terraform"],
            cloud: "any",
            runtime: "",
            iac: "terraform",
            delivery: "Terraform + GitHub Actions",
            requires: &["database:*"],
            cost: 15.0,
            cost_lines: &[("Smallest managed instance", 12.0, "database"), ("Automated backups and storage", 3.0, "database")],
            targets_axes: &["iac", "containerization"],
            security: 0.85,
            complexity: 4,
            pros: &["Backups, patching and failover become someone else's job", "Compose no longer carries a stateful service", "Point-in-time recovery"],
            cons: &["Monthly cost where there was none", "Cut-over needs a maintenance window", "Network path to the instance must be private"],
            plan: vec![
                analyze_step(),
                step("provision", "Provision the managed instance", "Apply a Terraform module for the smallest managed instance of the detected engine with private networking.", "cloud.create_dev_resource", "cloud", "analyze", "cloud:write", "The instance accepts connections from the application network only."),
                step("rehearse", "Rehearse migrations on a temporary database", "Create a temporary database on the instance and run every migration against it.", "db.create_temporary", "cloud", "provision", "database:write", "All migrations apply cleanly; the schema matches the current one."),
                step("configure", "Point configuration at the new host", "Replace the compose database service with the managed connection string in the environment example and deployment config.", "docs.generate", "github", "rehearse", "repository:read", "No service in compose runs the database; the connection string is an environment variable."),
                step("deploy-staging", "Deploy staging against the managed instance", "Roll staging out with the new connection string.", "deploy.staging", "cloud", "configure", "deploy:staging", "Staging health passes and reads/writes land on the managed instance."),
                step("cutover", "Migrate production data", "Back up the self-hosted database, restore it to the managed instance and run the migrations.", "db.migrate_production", "cloud", "deploy-staging", "database:write", "Row counts match on both sides and the application answers on the new instance."),
            ],
        },
        // --- additional delivery and CI patterns ---------------------------
        Spec {
            id: "monorepo-ci-matrix",
            name: "Per-package CI matrix for a monorepo",
            summary: "Detect the workspace packages and generate one workflow whose matrix runs lint, test and build per package, only for packages touched by the change.",
            category: "ci_cd",
            technologies: &["GitHub Actions", "Monorepo"],
            cloud: "github",
            runtime: "",
            iac: "none",
            delivery: "GitHub Actions",
            requires: &["monorepo"],
            cost: 0.0,
            cost_lines: &[("GitHub Actions minutes", 0.0, "ci: free tier; paths filtering keeps private minutes low")],
            targets_axes: &[],
            security: 0.8,
            complexity: 2,
            pros: &["Failures point at one package", "Untouched packages do not run", "Parallel jobs keep wall time flat"],
            cons: &["Path filters need maintenance as packages move", "Shared libraries fan out to many jobs"],
            plan: vec![
                analyze_step(),
                checkout_step("analyze"),
                step("packages", "Detect workspace packages", "Read the workspace manifest (pnpm-workspace, Cargo workspace, go.work, lerna) and list each package with its test command.", "repo.analyze", "docker", "checkout", "repository:read", "Every package directory appears with a command."),
                step("verify", "Run one package's tests", "Run the test command of the first package to confirm the commands are right.", "tests.run_local", "docker", "packages", "container:run", "Tests exit 0."),
                step("workflow", "Generate the matrix workflow", "Write .github/workflows/ci.yml with a package matrix and dorny/paths-filter so only changed packages run.", "docs.generate", "github", "verify", "repository:read", "Workflow validates and the matrix lists every package."),
                pull_request_step("workflow", ".github/workflows/ci.yml"),
            ],
        },
        Spec {
            id: "azure-app-service",
            name: "Azure App Service (Linux) + GitHub OIDC",
            summary: "Run the application on an App Service Linux plan with deployment slots, deployed from GitHub Actions through OIDC without stored secrets.",
            category: "deployment",
            technologies: &["Azure App Service", "GitHub Actions", "OIDC", "Bicep"],
            cloud: "azure",
            runtime: "paas",
            iac: "bicep",
            delivery: "GitHub Actions + OIDC",
            requires: &[],
            cost: 13.0,
            cost_lines: &[("App Service B1 Linux plan", 13.0, "compute"), ("GitHub Actions minutes", 0.0, "ci: free tier")],
            targets_axes: &[],
            security: 0.85,
            complexity: 2,
            pros: &["No Dockerfile needed for common runtimes", "Slots give zero-downtime swaps", "Managed TLS and custom domains"],
            cons: &["Azure-only", "B1 has no autoscale", "Container support needs a registry"],
            plan: vec![
                analyze_step(),
                step("provision", "Provision the plan and web app", "Deploy Bicep for a resource group, a B1 Linux plan, the web app and a staging slot.", "cloud.create_dev_resource", "cloud", "analyze", "cloud:write", "az deployment group show reports Succeeded and both slots exist."),
                step("oidc", "Configure GitHub OIDC federation", "Create the federated credential and the Website Contributor role assignment for the repository's workflow identity.", "cloud.modify_resource", "cloud", "provision", "cloud:write", "A workflow run can az login with the federated token."),
                step("workflow", "Generate the deploy workflow", "Write .github/workflows/deploy.yml that builds, logs in with OIDC and deploys to the staging slot.", "docs.generate", "github", "oidc", "repository:read", "Workflow validates and references the web app and slot names."),
                step("deploy-staging", "Deploy to the staging slot", "Run the workflow against the staging slot.", "deploy.staging", "github", "workflow", "deploy:staging", "The staging slot URL answers 200 on the health path."),
                step("deploy-production", "Swap staging into production", "Swap the slots so the verified build serves production.", "deploy.production", "cloud", "deploy-staging", "deploy:production", "Production URL serves the new build; the previous one sits in staging for rollback."),
            ],
        },
        Spec {
            id: "aws-lambda-container",
            name: "AWS Lambda container image + API Gateway",
            summary: "Package the service as a Lambda container image in ECR behind an HTTP API, with aliases for staging and production.",
            category: "deployment",
            technologies: &["AWS Lambda", "API Gateway", "ECR", "Terraform", "GitHub Actions"],
            cloud: "aws",
            runtime: "serverless_container",
            iac: "terraform",
            delivery: "GitHub Actions + OIDC + Terraform",
            requires: &["dockerfile"],
            cost: 8.0,
            cost_lines: &[("Lambda, 1M requests at 512 MB", 3.0, "compute"), ("API Gateway HTTP API", 3.0, "networking"), ("ECR storage", 1.0, "registry"), ("GitHub Actions minutes", 1.0, "ci")],
            targets_axes: &[],
            security: 0.8,
            complexity: 3,
            pros: &["Pay per request, nothing idle", "Scales without configuration", "IAM-native"],
            cons: &["Cold starts on container images", "15-minute execution limit", "Needs a Lambda web adapter for ordinary HTTP servers"],
            plan: vec![
                analyze_step(),
                build_image_step("analyze"),
                step("provision", "Provision registry, function and API", "Apply Terraform for the ECR repository, the function with the web adapter layer, and an HTTP API route.", "cloud.create_dev_resource", "cloud", "analyze", "cloud:write", "terraform apply completes; the API endpoint resolves."),
                step("deploy-staging", "Publish a version to the staging alias", "Push the image to ECR, update the function and point the staging alias at the new version.", "deploy.staging", "cloud", "build-image,provision", "deploy:staging", "The staging stage answers 200 on the health path."),
                step("smoke", "Smoke test the staging stage", "Hit the health path and one real route through API Gateway.", "tests.run_local", "local", "deploy-staging", "", "Both requests return 200 under 3 seconds."),
                step("deploy-production", "Shift the production alias", "Move the production alias to the verified version.", "deploy.production", "cloud", "smoke", "deploy:production", "The production stage serves the new version; the previous version stays for rollback."),
            ],
        },
        Spec {
            id: "hetzner-vm-compose",
            name: "Hetzner VM + Docker Compose (lowest cost)",
            summary: "The cheapest way to run a containerised stack with a database: one Hetzner Cloud VM with Docker Compose, a cloud firewall and SSH deploys from GitHub Actions.",
            category: "deployment",
            technologies: &["Hetzner Cloud", "Docker Compose", "nginx", "GitHub Actions"],
            cloud: "self_hosted",
            runtime: "vm_docker_compose",
            iac: "cloud-init",
            delivery: "GitHub Actions + SSH",
            requires: &["dockerfile"],
            cost: 5.0,
            cost_lines: &[("CX22 VM, 2 vCPU / 4 GB", 4.5, "compute"), ("Primary IPv4", 0.5, "networking"), ("GitHub Actions minutes", 0.0, "ci: free tier")],
            targets_axes: &[],
            security: 0.6,
            complexity: 2,
            pros: &["A few euros a month for the whole stack", "EU data residency", "Simple to reason about"],
            cons: &["Single point of failure", "You patch the OS", "SSH key is a stored secret"],
            plan: vec![
                analyze_step(),
                build_image_step("analyze"),
                step("compose-local", "Verify the compose stack locally", "Bring up docker-compose.yml with the built image and a throwaway database.", "env.create_temporary", "docker", "build-image", "container:run,environment:write", "All services report healthy and the app answers on its port."),
                step("provision-vm", "Provision the VM", "Create a CX22 with Docker installed through cloud-init and the deploy key authorised.", "cloud.create_dev_resource", "cloud", "analyze", "cloud:write", "SSH reachable and docker --version succeeds."),
                step("firewall", "Apply the cloud firewall", "Attach a Hetzner firewall allowing 22, 80 and 443 only.", "firewall.modify", "cloud", "provision-vm", "cloud:write", "A port scan shows only 22/80/443 open."),
                step("deploy", "Deploy with docker compose over SSH", "Copy the compose file, pull the image and run docker compose up -d.", "deploy.staging", "github", "compose-local,firewall", "deploy:staging", "docker compose ps shows every service running; the public URL returns 200."),
                step("smoke", "Smoke test the public endpoint", "Hit the health endpoint and one real page through nginx.", "tests.run_local", "local", "deploy", "", "Health endpoint returns 200 within 10 seconds."),
            ],
        },
        Spec {
            id: "render-web-service",
            name: "Render web service",
            summary: "Deploy from the repository with a render.yaml blueprint: Render builds on every push to the default branch, with managed TLS and a free static tier for the front end.",
            category: "deployment",
            technologies: &["Render", "render.yaml"],
            cloud: "self_hosted",
            runtime: "paas",
            iac: "render.yaml",
            delivery: "Render Git deploys",
            requires: &[],
            cost: 7.0,
            cost_lines: &[("Starter web service, 512 MB", 7.0, "compute"), ("Managed TLS and bandwidth", 0.0, "networking: 100 GB included")],
            targets_axes: &[],
            security: 0.7,
            complexity: 1,
            pros: &["Minutes from repository to URL", "No CI workflow needed", "Preview environments per pull request"],
            cons: &["Smaller ecosystem than the big clouds", "Free tier sleeps", "Limited region choice"],
            plan: vec![
                analyze_step(),
                step("blueprint", "Generate render.yaml", "Write a blueprint with the web service, build and start commands, health path and environment variables.", "docs.generate", "github", "analyze", "repository:read", "render.yaml validates and names the detected commands."),
                step("build", "Run the build locally", "Run the build and start commands from the blueprint once.", "tests.run_local", "local", "blueprint", "", "The build exits 0 and the app answers on its port."),
                step("service", "Create the service from the blueprint", "Create the Render service connected to the repository.", "cloud.create_dev_resource", "cloud", "build", "cloud:write", "The service exists and its first deploy starts."),
                step("deploy-staging", "Deploy the default branch", "Trigger a deploy and wait for it to go live.", "deploy.staging", "cloud", "service", "deploy:staging", "The onrender.com URL answers 200 on the health path."),
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

    const ACTION_TYPES: [&str; 22] = [
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
        "repo.checkout",
        "env.destroy_temporary",
        "repo.open_pull_request",
    ];

    const AXES: [&str; 8] = [
        "containerization",
        "ci",
        "tests",
        "iac",
        "dependencies",
        "docs",
        "secrets",
        "runtime",
    ];

    const CONTRACT_IDS: [&str; 26] = [
        "vm-docker-compose",
        "github-pages-static",
        "azure-container-apps",
        "aws-ecs-fargate",
        "gcp-cloud-run",
        "kubernetes-helm",
        "fly-io",
        "gha-ci-build-test",
        "gha-docker-publish",
        "dependabot-weekly",
        "docker-compose-test-env",
        "uat-checklist",
        "terraform-remote-state",
        "secret-scanning-baseline",
        "readme-architecture-docs",
        "containerize-legacy-app",
        "add-ci-baseline",
        "upgrade-runtime",
        "config-to-environment",
        "add-healthcheck",
        "managed-database",
        "monorepo-ci-matrix",
        "azure-app-service",
        "aws-lambda-container",
        "hetzner-vm-compose",
        "render-web-service",
    ];

    #[test]
    fn every_contract_pattern_with_a_valid_plan() {
        let patterns = catalog();
        assert_eq!(patterns.len(), CONTRACT_IDS.len());
        let ids: HashSet<&str> = patterns.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids.len(), CONTRACT_IDS.len());
        for id in CONTRACT_IDS {
            assert!(ids.contains(id), "missing catalog pattern {id}");
        }

        for pattern in &patterns {
            assert!(pattern.plan.len() >= 3, "{} plan too short", pattern.id);
            assert!(pattern.plan.len() <= 8, "{} plan too long", pattern.id);
            assert!(!pattern.pros.is_empty() && !pattern.cons.is_empty());
            assert!((0.0..=1.0).contains(&pattern.security_score));
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
                if step.action_type == "repo.open_pull_request" {
                    assert!(
                        step.required_permissions
                            .iter()
                            .any(|p| p == "repository:write"),
                        "{}: pull request step must require repository:write",
                        pattern.id
                    );
                }
            }
        }
    }

    #[test]
    fn cost_model_sums_to_the_estimate_and_modernization_targets_axes() {
        for pattern in catalog() {
            let lines = pattern.attributes["cost_lines"]
                .as_array()
                .unwrap_or_else(|| panic!("{} has no cost_lines", pattern.id));
            assert!(!lines.is_empty(), "{} has an empty cost model", pattern.id);
            let total: f64 = lines
                .iter()
                .map(|l| l["monthly_usd"].as_f64().unwrap())
                .sum();
            assert!(
                (total - pattern.estimated_monthly_cost_usd).abs() < 0.01,
                "{}: cost lines sum to {total}, estimate is {}",
                pattern.id,
                pattern.estimated_monthly_cost_usd
            );
            for line in lines {
                assert!(line["item"].as_str().is_some_and(|s| !s.is_empty()));
                assert!(line["note"].as_str().is_some());
            }

            let axes: Vec<&str> = pattern.attributes["targets_axes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap())
                .collect();
            if pattern.category == "modernization" {
                assert!(!axes.is_empty(), "{} targets no axis", pattern.id);
                assert!(
                    pattern
                        .plan
                        .iter()
                        .any(|s| s.action_type == "repo.open_pull_request"
                            || s.action_type == "db.migrate_production"),
                    "{} never lands its change",
                    pattern.id
                );
            }
            for axis in axes {
                assert!(AXES.contains(&axis), "{}: unknown axis {axis}", pattern.id);
            }
        }
        assert_eq!(
            catalog()
                .iter()
                .filter(|p| p.category == "modernization")
                .count(),
            6
        );
    }
}
