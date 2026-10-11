//! GitHub Actions workflows: `ci.yml` (lint, test, build, image build) per
//! stack and `deploy.yml` per target. Every cloud login uses OIDC federated
//! credentials, so no long-lived cloud secret is ever stored in GitHub.

use super::{DeployTarget, GenInput, Stack, artifact};
use crate::domain::model::ArtifactData;

const CI_PATH: &str = ".github/workflows/ci.yml";
const DEPLOY_PATH: &str = ".github/workflows/deploy.yml";

pub fn ci(input: &GenInput) -> Vec<ArtifactData> {
    vec![artifact(
        input,
        CI_PATH,
        "yaml",
        format!(
            "Continuous integration for {}: install, lint, `{}`, build{}.",
            input.stack.label(),
            input.test_command,
            if input.stack == Stack::Unknown {
                ""
            } else {
                " and a container image build"
            }
        ),
        ci_yaml(input),
    )]
}

pub fn deploy(input: &GenInput, target: DeployTarget) -> Vec<ArtifactData> {
    let mut files = vec![artifact(
        input,
        DEPLOY_PATH,
        "yaml",
        format!(
            "Deploys {} to {} from `{}` after CI passes.",
            input.repo_name,
            target_label(target),
            input.default_branch
        ),
        deploy_yaml(input, target),
    )];
    match target {
        DeployTarget::Fly => files.push(artifact(
            input,
            "fly.toml",
            "text",
            "Fly.io machine configuration: region, port, health check and autostop.",
            fly_toml(input),
        )),
        DeployTarget::Render => files.push(artifact(
            input,
            "render.yaml",
            "yaml",
            "Render blueprint: a Docker web service with a health check and the datastore it needs.",
            render_yaml(input),
        )),
        _ => {}
    }
    files
}

pub fn terraform_plan(input: &GenInput) -> Vec<ArtifactData> {
    vec![artifact(
        input,
        ".github/workflows/terraform.yml",
        "yaml",
        "Runs `terraform fmt`, `validate` and `plan` on pull requests and posts the plan as a comment.",
        terraform_yaml(input),
    )]
}

pub fn secret_scan(input: &GenInput) -> Vec<ArtifactData> {
    vec![artifact(
        input,
        ".github/workflows/secret-scan.yml",
        "yaml",
        "Scans every push and pull request for committed secrets with gitleaks.",
        secret_scan_yaml(input),
    )]
}

fn target_label(target: DeployTarget) -> &'static str {
    match target {
        DeployTarget::AzureContainerApps => "Azure Container Apps",
        DeployTarget::AzureAppService => "Azure App Service",
        DeployTarget::AwsEcs => "AWS ECS Fargate",
        DeployTarget::AwsLambda => "AWS Lambda",
        DeployTarget::GcpCloudRun => "Google Cloud Run",
        DeployTarget::Fly => "Fly.io",
        DeployTarget::Render => "Render",
        DeployTarget::Vm => "a VM over SSH with Docker Compose",
        DeployTarget::GithubPages => "GitHub Pages",
        DeployTarget::Kubernetes => "Kubernetes with Helm",
        DeployTarget::Registry => "GitHub Container Registry",
    }
}

const CHECKOUT: &str = "      - uses: actions/checkout@v4\n";

/// What the test job of one package needs to know. A single-package
/// repository has one of these (the repository itself); a monorepo has one
/// per workspace package, because a Rust API and a React bundle in the same
/// repository must not share `npm ci`.
struct StackSteps<'a> {
    stack: Stack,
    package_manager: &'a str,
    runtime_version: &'a str,
    test_command: String,
}

impl<'a> StackSteps<'a> {
    fn of_repo(input: &'a GenInput) -> Self {
        Self {
            stack: input.stack,
            package_manager: &input.package_manager,
            runtime_version: &input.runtime_version,
            test_command: input.test_command.clone(),
        }
    }

    /// A package that shares the repository's stack inherits the resolved
    /// runtime version and test command; any other stack gets its LTS line
    /// and the stack's conventional test command.
    fn of_package(input: &'a GenInput, package: &'a super::Package) -> Self {
        if package.stack == input.stack && package.package_manager == input.package_manager {
            return Self::of_repo(input);
        }
        Self {
            stack: package.stack,
            package_manager: &package.package_manager,
            runtime_version: package.stack.default_runtime_version(),
            test_command: super::input::default_test_command(
                package.stack,
                &package.package_manager,
            ),
        }
    }
}

fn node_setup(input: &StackSteps<'_>) -> String {
    let mut out = String::new();
    if input.package_manager == "pnpm" {
        out.push_str(&format!(
            "      - uses: pnpm/action-setup@v4\n        with:\n          version: {}\n",
            super::PNPM_MAJOR
        ));
    }
    if input.package_manager == "bun" {
        out.push_str(
            "      - uses: oven-sh/setup-bun@v2\n        with:\n          bun-version: latest\n",
        );
        out.push_str("      - run: bun install --frozen-lockfile\n");
        return out;
    }
    let cache = match input.package_manager {
        "pnpm" => "pnpm",
        "yarn" => "yarn",
        _ => "npm",
    };
    out.push_str(&format!(
        "      - uses: actions/setup-node@v4\n        with:\n          node-version: \"{}\"\n          cache: {cache}\n",
        input.runtime_version
    ));
    out.push_str(&match input.package_manager {
        "pnpm" => "      - run: pnpm install --frozen-lockfile\n".to_owned(),
        "yarn" => "      - run: yarn install --frozen-lockfile\n".to_owned(),
        _ => "      - run: npm ci\n".to_owned(),
    });
    out
}

fn run_script(input: &StackSteps<'_>, script: &str) -> String {
    match input.package_manager {
        "pnpm" => format!("pnpm run {script} --if-present"),
        "yarn" => format!("yarn run {script} --if-present"),
        "bun" => format!("bun run {script} --if-present"),
        _ => format!("npm run {script} --if-present"),
    }
}

/// Steps of the `test` job after checkout, per stack.
fn test_steps(input: &StackSteps<'_>) -> String {
    let test = input.test_command.as_str();
    match input.stack {
        Stack::Node | Stack::Static => {
            if input.package_manager.is_empty() {
                return "      - run: test -f index.html\n".to_owned();
            }
            format!(
                "{setup}      - run: {lint}\n      - run: {test}\n      - run: {build}\n",
                setup = node_setup(input),
                lint = run_script(input, "lint"),
                build = run_script(input, "build"),
            )
        }
        Stack::Rust => format!(
            "      - uses: dtolnay/rust-toolchain@stable\n        with:\n          components: rustfmt, clippy\n      - uses: Swatinem/rust-cache@v2\n      - run: cargo fmt --all -- --check\n      - run: cargo clippy --all-targets -- -D warnings\n      - run: {test}\n"
        ),
        Stack::Python => {
            let install = match input.package_manager {
                "uv" => {
                    "      - uses: astral-sh/setup-uv@v3\n      - run: uv sync --frozen\n      - run: uv run ruff check . || true\n"
                }
                "poetry" => {
                    "      - run: pipx install poetry\n      - run: poetry install --no-interaction\n"
                }
                _ => "      - run: pip install -r requirements.txt\n",
            };
            let run = match input.package_manager {
                "uv" => format!("uv run {test}"),
                "poetry" => format!("poetry run {test}"),
                _ => test.to_owned(),
            };
            format!(
                "      - uses: actions/setup-python@v5\n        with:\n          python-version: \"{}\"\n{install}      - run: {run}\n",
                input.runtime_version
            )
        }
        Stack::Go => format!(
            "      - uses: actions/setup-go@v5\n        with:\n          go-version: \"{}\"\n      - run: go vet ./...\n      - run: {test}\n      - run: go build ./...\n",
            input.runtime_version
        ),
        Stack::Java => {
            let cache = if input.package_manager == "gradle" {
                "gradle"
            } else {
                "maven"
            };
            format!(
                "      - uses: actions/setup-java@v4\n        with:\n          distribution: temurin\n          java-version: \"{}\"\n          cache: {cache}\n      - run: {test}\n",
                input.runtime_version
            )
        }
        Stack::DotNet => format!(
            "      - uses: actions/setup-dotnet@v4\n        with:\n          dotnet-version: \"{}\"\n      - run: dotnet restore\n      - run: dotnet build --no-restore -c Release\n      - run: {test} --no-build -c Release\n",
            input.runtime_version
        ),
        Stack::Php => format!(
            "      - uses: shivammathur/setup-php@v2\n        with:\n          php-version: \"{}\"\n          tools: composer\n      - run: composer install --prefer-dist --no-progress --no-interaction\n      - run: {test}\n",
            input.runtime_version
        ),
        Stack::Ruby => format!(
            "      - uses: ruby/setup-ruby@v1\n        with:\n          ruby-version: \"{}\"\n          bundler-cache: true\n      - run: {test}\n",
            input.runtime_version
        ),
        Stack::Unknown => format!("      - run: {test}\n"),
    }
}

fn ci_yaml(input: &GenInput) -> String {
    let branch = &input.default_branch;
    let mut out = format!(
        "name: CI

on:
  push:
    branches: [{branch}]
  pull_request:
    branches: [{branch}]

concurrency:
  group: ci-${{{{ github.ref }}}}
  cancel-in-progress: true

permissions:
  contents: read

jobs:
"
    );
    // One job per workspace package, each with its own toolchain, instead
    // of a matrix: a matrix would run the same steps in every directory and
    // a Rust crate cannot be tested with `npm test`.
    let mut job_ids: Vec<String> = Vec::new();
    if input.packages.is_empty() {
        job_ids.push("test".to_owned());
        out.push_str(&format!(
            "  test:\n    name: Lint, test, build\n    runs-on: ubuntu-latest\n    timeout-minutes: 20\n    steps:\n{CHECKOUT}{}",
            test_steps(&StackSteps::of_repo(input))
        ));
    } else {
        for package in &input.packages {
            let id = package.job_id();
            let steps = StackSteps::of_package(input, package);
            let (name, workdir) = if package.dir.is_empty() {
                (
                    format!("Test the {} workspace", steps.stack.label()),
                    String::new(),
                )
            } else {
                (
                    format!("Test {} ({})", package.dir, steps.stack.label()),
                    format!(
                        "    defaults:\n      run:\n        working-directory: {}\n",
                        package.dir
                    ),
                )
            };
            out.push_str(&format!(
                "  {id}:\n    name: {name}\n    runs-on: ubuntu-latest\n    timeout-minutes: 20\n{workdir}    steps:\n{CHECKOUT}{}\n",
                test_steps(&steps),
            ));
            job_ids.push(id);
        }
        if let Some(last) = out.strip_suffix('\n') {
            // Keep exactly one blank line before the image job below.
            out = last.to_owned();
        }
    }

    if input.stack != Stack::Unknown {
        out.push_str(&format!(
            "
  image:
    name: Build container image
    runs-on: ubuntu-latest
    needs: [{needs}]
    timeout-minutes: 20
    steps:
{CHECKOUT}      - uses: docker/setup-buildx-action@v3
      - uses: docker/build-push-action@v6
        with:
          context: {context}
          file: {dockerfile}
          push: false
          tags: {image}:${{{{ github.sha }}}}
          cache-from: type=gha
          cache-to: type=gha,mode=max
",
            needs = job_ids.join(", "),
            context = input.context_dir(),
            dockerfile = input.in_app("Dockerfile"),
            image = input.image
        ));
    }
    out
}

fn header(name: &str, branch: &str, extra_permissions: &str) -> String {
    format!(
        "name: {name}

on:
  push:
    branches: [{branch}]
  workflow_dispatch:

concurrency:
  group: deploy-${{{{ github.ref }}}}
  cancel-in-progress: false

permissions:
  contents: read
  id-token: write
{extra_permissions}
env:
  IMAGE_TAG: ${{{{ github.sha }}}}
"
    )
}

fn ghcr_push(input: &GenInput) -> String {
    format!(
        "      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{{{ github.actor }}}}
          password: ${{{{ secrets.GITHUB_TOKEN }}}}
      - uses: docker/build-push-action@v6
        with:
          context: {context}
          file: {dockerfile}
          push: true
          tags: |
            {image}:${{{{ env.IMAGE_TAG }}}}
            {image}:latest
          cache-from: type=gha
          cache-to: type=gha,mode=max
",
        image = input.image_ref(),
        context = input.context_dir(),
        dockerfile = input.in_app("Dockerfile"),
    )
}

fn deploy_yaml(input: &GenInput, target: DeployTarget) -> String {
    let branch = &input.default_branch;
    let name = &input.image;
    match target {
        DeployTarget::Registry => format!(
            "{head}
jobs:
  publish:
    name: Publish image to GHCR
    runs-on: ubuntu-latest
    steps:
{CHECKOUT}{push}",
            head = header("Publish image", branch, "  packages: write\n"),
            push = ghcr_push(input),
        ),
        DeployTarget::AzureContainerApps => format!(
            "{head}  AZURE_RESOURCE_GROUP: rg-{name}
  ACR_NAME: ${{{{ vars.ACR_NAME }}}}
  CONTAINER_APP: {name}

jobs:
  deploy:
    name: Build, push and roll out
    runs-on: ubuntu-latest
    environment: production
    steps:
{CHECKOUT}      - uses: azure/login@v2
        with:
          client-id: ${{{{ secrets.AZURE_CLIENT_ID }}}}
          tenant-id: ${{{{ secrets.AZURE_TENANT_ID }}}}
          subscription-id: ${{{{ secrets.AZURE_SUBSCRIPTION_ID }}}}
      - run: az acr login --name \"$ACR_NAME\"
      - uses: docker/setup-buildx-action@v3
      - uses: docker/build-push-action@v6
        with:
          context: {context}
          file: {dockerfile}
          push: true
          tags: ${{{{ env.ACR_NAME }}}}.azurecr.io/{name}:${{{{ env.IMAGE_TAG }}}}
          cache-from: type=gha
          cache-to: type=gha,mode=max
      - name: Roll out the new revision
        run: |
          az containerapp update \\
            --name \"$CONTAINER_APP\" \\
            --resource-group \"$AZURE_RESOURCE_GROUP\" \\
            --image \"$ACR_NAME.azurecr.io/{name}:$IMAGE_TAG\"
      - name: Verify health
        run: |
          FQDN=$(az containerapp show --name \"$CONTAINER_APP\" --resource-group \"$AZURE_RESOURCE_GROUP\" --query properties.configuration.ingress.fqdn -o tsv)
          curl --retry 10 --retry-delay 6 --retry-all-errors -fsS \"https://$FQDN{health}\"
",
            head = header("Deploy to Azure Container Apps", branch, ""),
            context = input.context_dir(),
            dockerfile = input.in_app("Dockerfile"),
            health = input.health_path,
        ),
        DeployTarget::AzureAppService => format!(
            "{head}  AZURE_WEBAPP_NAME: {name}

jobs:
  deploy:
    name: Build, publish and deploy
    runs-on: ubuntu-latest
    environment: production
    steps:
{CHECKOUT}{push}      - uses: azure/login@v2
        with:
          client-id: ${{{{ secrets.AZURE_CLIENT_ID }}}}
          tenant-id: ${{{{ secrets.AZURE_TENANT_ID }}}}
          subscription-id: ${{{{ secrets.AZURE_SUBSCRIPTION_ID }}}}
      - uses: azure/webapps-deploy@v3
        with:
          app-name: ${{{{ env.AZURE_WEBAPP_NAME }}}}
          images: {image}:${{{{ env.IMAGE_TAG }}}}
      - run: curl --retry 10 --retry-delay 6 --retry-all-errors -fsS \"https://$AZURE_WEBAPP_NAME.azurewebsites.net{health}\"
",
            head = header("Deploy to Azure App Service", branch, "  packages: write\n"),
            push = ghcr_push(input),
            image = input.image_ref(),
            health = input.health_path,
        ),
        DeployTarget::AwsEcs => format!(
            "{head}  AWS_REGION: {region}
  ECR_REPOSITORY: {name}
  ECS_CLUSTER: {name}
  ECS_SERVICE: {name}

jobs:
  deploy:
    name: Build, push and update the service
    runs-on: ubuntu-latest
    environment: production
    steps:
{CHECKOUT}      - uses: aws-actions/configure-aws-credentials@v4
        with:
          role-to-assume: ${{{{ secrets.AWS_DEPLOY_ROLE_ARN }}}}
          aws-region: ${{{{ env.AWS_REGION }}}}
      - id: ecr
        uses: aws-actions/amazon-ecr-login@v2
      - uses: docker/setup-buildx-action@v3
      - uses: docker/build-push-action@v6
        with:
          context: {context}
          file: {dockerfile}
          push: true
          tags: ${{{{ steps.ecr.outputs.registry }}}}/${{{{ env.ECR_REPOSITORY }}}}:${{{{ env.IMAGE_TAG }}}}
          cache-from: type=gha
          cache-to: type=gha,mode=max
      - name: Update the task definition image
        run: |
          TASK_DEF=$(aws ecs describe-task-definition --task-definition \"$ECS_SERVICE\" --query taskDefinition)
          NEW_DEF=$(echo \"$TASK_DEF\" | jq --arg IMAGE \"${{{{ steps.ecr.outputs.registry }}}}/$ECR_REPOSITORY:$IMAGE_TAG\" \\
            '.containerDefinitions[0].image = $IMAGE | del(.taskDefinitionArn, .revision, .status, .requiresAttributes, .compatibilities, .registeredAt, .registeredBy)')
          ARN=$(aws ecs register-task-definition --cli-input-json \"$NEW_DEF\" --query taskDefinition.taskDefinitionArn --output text)
          aws ecs update-service --cluster \"$ECS_CLUSTER\" --service \"$ECS_SERVICE\" --task-definition \"$ARN\"
      - run: aws ecs wait services-stable --cluster \"$ECS_CLUSTER\" --services \"$ECS_SERVICE\"
",
            head = header("Deploy to AWS ECS Fargate", branch, ""),
            context = input.context_dir(),
            dockerfile = input.in_app("Dockerfile"),
            region = input.region,
        ),
        DeployTarget::AwsLambda => format!(
            "{head}  AWS_REGION: {region}
  ECR_REPOSITORY: {name}
  FUNCTION_NAME: {name}

jobs:
  deploy:
    name: Build, push and update the function
    runs-on: ubuntu-latest
    environment: production
    steps:
{CHECKOUT}      - uses: aws-actions/configure-aws-credentials@v4
        with:
          role-to-assume: ${{{{ secrets.AWS_DEPLOY_ROLE_ARN }}}}
          aws-region: ${{{{ env.AWS_REGION }}}}
      - id: ecr
        uses: aws-actions/amazon-ecr-login@v2
      - uses: docker/setup-buildx-action@v3
      - uses: docker/build-push-action@v6
        with:
          context: {context}
          file: {dockerfile}
          push: true
          provenance: false
          tags: ${{{{ steps.ecr.outputs.registry }}}}/${{{{ env.ECR_REPOSITORY }}}}:${{{{ env.IMAGE_TAG }}}}
      - run: |
          aws lambda update-function-code --function-name \"$FUNCTION_NAME\" \\
            --image-uri \"${{{{ steps.ecr.outputs.registry }}}}/$ECR_REPOSITORY:$IMAGE_TAG\"
          aws lambda wait function-updated --function-name \"$FUNCTION_NAME\"
",
            head = header("Deploy to AWS Lambda", branch, ""),
            context = input.context_dir(),
            dockerfile = input.in_app("Dockerfile"),
            region = input.region,
        ),
        DeployTarget::GcpCloudRun => format!(
            "{head}  GCP_REGION: {region}
  SERVICE: {name}
  REPOSITORY: {name}

jobs:
  deploy:
    name: Build, push and deploy
    runs-on: ubuntu-latest
    environment: production
    steps:
{CHECKOUT}      - id: auth
        uses: google-github-actions/auth@v2
        with:
          workload_identity_provider: ${{{{ secrets.GCP_WORKLOAD_IDENTITY_PROVIDER }}}}
          service_account: ${{{{ secrets.GCP_DEPLOY_SERVICE_ACCOUNT }}}}
      - uses: google-github-actions/setup-gcloud@v2
      - run: gcloud auth configure-docker \"$GCP_REGION-docker.pkg.dev\" --quiet
      - uses: docker/setup-buildx-action@v3
      - uses: docker/build-push-action@v6
        with:
          context: {context}
          file: {dockerfile}
          push: true
          tags: ${{{{ env.GCP_REGION }}}}-docker.pkg.dev/${{{{ secrets.GCP_PROJECT_ID }}}}/${{{{ env.REPOSITORY }}}}/{name}:${{{{ env.IMAGE_TAG }}}}
          cache-from: type=gha
          cache-to: type=gha,mode=max
      - id: run
        uses: google-github-actions/deploy-cloudrun@v2
        with:
          service: ${{{{ env.SERVICE }}}}
          region: ${{{{ env.GCP_REGION }}}}
          image: ${{{{ env.GCP_REGION }}}}-docker.pkg.dev/${{{{ secrets.GCP_PROJECT_ID }}}}/${{{{ env.REPOSITORY }}}}/{name}:${{{{ env.IMAGE_TAG }}}}
      - run: curl --retry 10 --retry-delay 6 --retry-all-errors -fsS \"${{{{ steps.run.outputs.url }}}}{health}\"
",
            head = header("Deploy to Google Cloud Run", branch, ""),
            context = input.context_dir(),
            dockerfile = input.in_app("Dockerfile"),
            region = input.region,
            health = input.health_path,
        ),
        DeployTarget::Fly => format!(
            "{head}
jobs:
  deploy:
    name: Deploy to Fly.io
    runs-on: ubuntu-latest
    environment: production
    steps:
{CHECKOUT}      - uses: superfly/flyctl-actions/setup-flyctl@master
      - run: flyctl deploy --remote-only --app {name}
        env:
          FLY_API_TOKEN: ${{{{ secrets.FLY_API_TOKEN }}}}
      - run: curl --retry 10 --retry-delay 6 --retry-all-errors -fsS \"https://{name}.fly.dev{health}\"
",
            head = header("Deploy to Fly.io", branch, ""),
            health = input.health_path,
        ),
        DeployTarget::Render => format!(
            "{head}
jobs:
  deploy:
    name: Trigger the Render deploy hook
    runs-on: ubuntu-latest
    environment: production
    steps:
      - run: curl -fsS -X POST \"${{{{ secrets.RENDER_DEPLOY_HOOK_URL }}}}\"
      - run: sleep 60 && curl --retry 10 --retry-delay 6 --retry-all-errors -fsS \"${{{{ vars.RENDER_SERVICE_URL }}}}{health}\"
",
            head = header("Deploy to Render", branch, ""),
            health = input.health_path,
        ),
        DeployTarget::Vm => format!(
            "{head}  DEPLOY_DIR: /opt/{name}

jobs:
  deploy:
    name: Publish image and roll out with Compose
    runs-on: ubuntu-latest
    environment: production
    steps:
{CHECKOUT}{push}      - name: Install the SSH key
        run: |
          mkdir -p ~/.ssh
          echo \"${{{{ secrets.VM_SSH_KEY }}}}\" > ~/.ssh/deploy_key
          chmod 600 ~/.ssh/deploy_key
          ssh-keyscan -H \"${{{{ secrets.VM_HOST }}}}\" >> ~/.ssh/known_hosts
      - name: Ship the compose file and restart
        run: |
          scp -i ~/.ssh/deploy_key docker-compose.yml deploy/nginx/default.conf \"${{{{ secrets.VM_USER }}}}@${{{{ secrets.VM_HOST }}}}:$DEPLOY_DIR/\"
          ssh -i ~/.ssh/deploy_key \"${{{{ secrets.VM_USER }}}}@${{{{ secrets.VM_HOST }}}}\" \\
            \"cd $DEPLOY_DIR && echo '${{{{ secrets.GITHUB_TOKEN }}}}' | docker login ghcr.io -u '${{{{ github.actor }}}}' --password-stdin && \\
             IMAGE={image}:$IMAGE_TAG docker compose pull app && IMAGE={image}:$IMAGE_TAG docker compose up -d --remove-orphans\"
      - run: curl --retry 10 --retry-delay 6 --retry-all-errors -fsS \"http://${{{{ secrets.VM_HOST }}}}{health}\"
",
            head = header("Deploy to the VM", branch, "  packages: write\n"),
            push = ghcr_push(input),
            image = input.image_ref(),
            health = input.health_path,
        ),
        DeployTarget::GithubPages => {
            let build = if input.package_manager.is_empty() {
                "      - run: test -f index.html\n".to_owned()
            } else {
                {
                    let steps = StackSteps::of_repo(input);
                    format!(
                        "{}      - run: {}\n",
                        node_setup(&steps),
                        run_script(&steps, "build")
                    )
                }
            };
            format!(
                "{head}
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
{CHECKOUT}{build}      - uses: actions/configure-pages@v5
      - uses: actions/upload-pages-artifact@v3
        with:
          path: {dir}

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{{{ steps.deployment.outputs.page_url }}}}
    steps:
      - id: deployment
        uses: actions/deploy-pages@v4
",
                head = header("Deploy to GitHub Pages", branch, "  pages: write\n"),
                dir = input.static_dir,
            )
        }
        DeployTarget::Kubernetes => format!(
            "{head}  NAMESPACE: {name}

jobs:
  deploy:
    name: Publish image and upgrade the release
    runs-on: ubuntu-latest
    environment: production
    steps:
{CHECKOUT}{push}      - uses: azure/setup-helm@v4
      - uses: azure/setup-kubectl@v4
      - name: Configure cluster access
        run: |
          mkdir -p ~/.kube
          echo \"${{{{ secrets.KUBECONFIG }}}}\" | base64 -d > ~/.kube/config
          chmod 600 ~/.kube/config
      - run: |
          helm upgrade --install {name} ./charts/{name} \\
            --namespace \"$NAMESPACE\" --create-namespace \\
            --set image.repository={image} \\
            --set image.tag=$IMAGE_TAG \\
            --wait --timeout 5m
      - run: kubectl rollout status deployment/{name} -n \"$NAMESPACE\" --timeout=300s
",
            head = header("Deploy to Kubernetes", branch, "  packages: write\n"),
            push = ghcr_push(input),
            image = input.image_ref(),
        ),
    }
}

fn fly_toml(input: &GenInput) -> String {
    let region = match input.region.as_str() {
        "westeurope" | "eu-west-1" | "europe-west1" => "ams",
        "eastus" | "us-east-1" | "us-east1" => "iad",
        other => other,
    };
    format!(
        "app = \"{name}\"
primary_region = \"{region}\"

[build]
  dockerfile = \"Dockerfile\"

[env]
  PORT = \"{port}\"

[http_service]
  internal_port = {port}
  force_https = true
  auto_stop_machines = \"stop\"
  auto_start_machines = true
  min_machines_running = 0

  [[http_service.checks]]
    interval = \"30s\"
    timeout = \"5s\"
    grace_period = \"15s\"
    method = \"GET\"
    path = \"{health}\"

[[vm]]
  size = \"shared-cpu-1x\"
  memory = \"512mb\"
",
        name = input.image,
        port = input.port,
        health = input.health_path,
    )
}

fn render_yaml(input: &GenInput) -> String {
    let mut out = format!(
        "services:
  - type: web
    name: {name}
    runtime: docker
    plan: starter
    region: frankfurt
    branch: {branch}
    healthCheckPath: {health}
    envVars:
      - key: PORT
        value: \"{port}\"
",
        name = input.image,
        branch = input.default_branch,
        health = input.health_path,
        port = input.port,
    );
    if input.has_datastore("postgres") {
        out.push_str(&format!(
            "      - key: DATABASE_URL
        fromDatabase:
          name: {name}-db
          property: connectionString

databases:
  - name: {name}-db
    plan: basic-256mb
    region: frankfurt
",
            name = input.image
        ));
    }
    if input.has_datastore("redis") {
        out.push_str(&format!(
            "      - key: REDIS_URL
        fromService:
          type: keyvalue
          name: {name}-cache
          property: connectionString
  - type: keyvalue
    name: {name}-cache
    plan: free
    region: frankfurt
    ipAllowList: []
",
            name = input.image
        ));
    }
    out
}

fn terraform_yaml(input: &GenInput) -> String {
    let dir = if input.cloud.is_empty() {
        "infra".to_owned()
    } else {
        format!("infra/{}", input.cloud)
    };
    let auth = match input.cloud.as_str() {
        "aws" => format!(
            "      - uses: aws-actions/configure-aws-credentials@v4
        with:
          role-to-assume: ${{{{ secrets.AWS_PLAN_ROLE_ARN }}}}
          aws-region: {}
",
            input.region
        ),
        "gcp" => "      - uses: google-github-actions/auth@v2
        with:
          workload_identity_provider: ${{ secrets.GCP_WORKLOAD_IDENTITY_PROVIDER }}
          service_account: ${{ secrets.GCP_PLAN_SERVICE_ACCOUNT }}
"
        .to_owned(),
        _ => "      - uses: azure/login@v2
        with:
          client-id: ${{ secrets.AZURE_CLIENT_ID }}
          tenant-id: ${{ secrets.AZURE_TENANT_ID }}
          subscription-id: ${{ secrets.AZURE_SUBSCRIPTION_ID }}
"
        .to_owned(),
    };
    format!(
        "name: Terraform

on:
  pull_request:
    paths: [\"{dir}/**\"]
  push:
    branches: [{branch}]
    paths: [\"{dir}/**\"]

permissions:
  contents: read
  id-token: write
  pull-requests: write

jobs:
  plan:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: {dir}
    steps:
{CHECKOUT}{auth}      - uses: hashicorp/setup-terraform@v3
        with:
          terraform_version: \"1.9.8\"
      - run: terraform fmt -check -recursive
      - run: terraform init -input=false
      - run: terraform validate
      - id: plan
        run: terraform plan -input=false -no-color -out=tfplan
        continue-on-error: true
      - uses: actions/github-script@v7
        if: github.event_name == 'pull_request'
        with:
          script: |
            const plan = `${{{{ steps.plan.outputs.stdout }}}}`.slice(0, 60000);
            await github.rest.issues.createComment({{
              issue_number: context.issue.number,
              owner: context.repo.owner,
              repo: context.repo.repo,
              body: `#### Terraform plan (${{{{ steps.plan.outcome }}}})\\n\\n\\`\\`\\`\\n${{plan}}\\n\\`\\`\\``,
            }});
      - if: steps.plan.outcome == 'failure'
        run: exit 1
      - if: github.event_name == 'push'
        run: terraform apply -input=false tfplan
",
        branch = input.default_branch,
    )
}

fn secret_scan_yaml(input: &GenInput) -> String {
    format!(
        "name: Secret scan

on:
  push:
    branches: [{branch}]
  pull_request:

permissions:
  contents: read

jobs:
  gitleaks:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: gitleaks/gitleaks-action@v2
        env:
          GITHUB_TOKEN: ${{{{ secrets.GITHUB_TOKEN }}}}
",
        branch = input.default_branch,
    )
}

#[cfg(test)]
mod tests {
    use super::super::{input::fixtures, snapshot::assert_snapshot};
    use super::*;

    #[test]
    fn ci_snapshots_per_stack() {
        let node = ci(&fixtures::input(&fixtures::node_context(), ""));
        assert_snapshot("gha_ci_node_pnpm", &node[0]);
        assert!(node[0].replaces_existing);
        assert!(node[0].content.contains("pnpm/action-setup@v4"));
        assert!(node[0].content.contains("run: pnpm test"));
        let rust = ci(&fixtures::input(&fixtures::rust_context(), ""));
        assert!(
            rust[0]
                .content
                .contains("cargo clippy --all-targets -- -D warnings")
        );
        assert!(rust[0].content.contains("texc-symphony:${{ github.sha }}"));
        let py = ci(&fixtures::input(&fixtures::python_context(), ""));
        assert!(py[0].content.contains("astral-sh/setup-uv@v3"));
        assert!(py[0].content.contains("uv run pytest"));
    }

    #[test]
    fn monorepo_ci_uses_a_matrix() {
        let mut input = fixtures::input(&fixtures::node_context(), "");
        input.packages = vec![
            super::super::Package {
                dir: "apps/web".into(),
                name: "web".into(),
                is_library: false,
                stack: Stack::Node,
                package_manager: "pnpm".into(),
                framework: "react".into(),
            },
            super::super::Package {
                dir: "backend".into(),
                name: "api".into(),
                is_library: false,
                stack: Stack::Rust,
                package_manager: "cargo".into(),
                framework: "axum".into(),
            },
        ];
        let files = ci(&input);
        let yaml = &files[0].content;
        assert!(yaml.contains("  test-apps-web:\n    name: Test apps/web (Node.js)"));
        assert!(yaml.contains("working-directory: apps/web"));
        assert!(yaml.contains("  test-backend:\n    name: Test backend (Rust)"));
        assert!(yaml.contains("working-directory: backend\n"));
        assert!(yaml.contains("cargo clippy --all-targets -- -D warnings"));
        assert!(yaml.contains("needs: [test-apps-web, test-backend]"));
        assert!(!yaml.contains("matrix"));
    }

    #[test]
    fn deploy_snapshot_azure_and_every_target_is_complete() {
        let input = fixtures::input(&fixtures::node_context(), "azure");
        let aca = deploy(&input, DeployTarget::AzureContainerApps);
        assert_snapshot("gha_deploy_azure_container_apps", &aca[0]);
        assert!(aca[0].content.contains("azure/login@v2"));
        assert!(aca[0].content.contains("id-token: write"));
        assert!(aca[0].content.contains("/healthz"));
        for target in [
            DeployTarget::AzureAppService,
            DeployTarget::AwsEcs,
            DeployTarget::AwsLambda,
            DeployTarget::GcpCloudRun,
            DeployTarget::Fly,
            DeployTarget::Render,
            DeployTarget::Vm,
            DeployTarget::GithubPages,
            DeployTarget::Kubernetes,
            DeployTarget::Registry,
        ] {
            let files = deploy(&input, target);
            let yaml = &files[0].content;
            assert!(yaml.starts_with("name: "), "{target:?}");
            assert!(yaml.contains("jobs:"), "{target:?}");
            assert!(!yaml.contains("TODO"), "{target:?}");
            assert!(
                !yaml.contains("AWS_ACCESS_KEY_ID"),
                "{target:?} uses static cloud keys"
            );
        }
        let fly = deploy(&input, DeployTarget::Fly);
        assert_eq!(fly[1].path, "fly.toml");
        assert!(fly[1].content.contains("primary_region = \"ams\""));
        let render = deploy(&input, DeployTarget::Render);
        assert!(render[1].content.contains("fromDatabase"));
        let plan = terraform_plan(&input);
        assert!(plan[0].content.contains("working-directory: infra/azure"));
        assert!(secret_scan(&input)[0].content.contains("gitleaks"));
    }
}
