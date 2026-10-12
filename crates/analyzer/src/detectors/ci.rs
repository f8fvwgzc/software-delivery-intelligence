//! CI/CD pipelines. Workflows are parsed into jobs and steps; the `uses:`
//! actions and `run:` commands reveal what a pipeline builds, tests and
//! deploys, and where. Publishing an image, a package or a GitHub release
//! is a *release*, recorded with target `release`, so a repository that
//! ships artifacts but never deploys them is described truthfully.
//!
//! Workflows also say where images go (CONTRACT.md §22): the registry a
//! job logs in to or tags for becomes `registry:<kind>`, a workflow that
//! builds an image is marked `ci:build_image:<path>`, and every GitHub
//! environment a job names becomes `cd:github_environment:<name>`.

use std::collections::BTreeMap;

use regex::Regex;
use serde_yaml_ng::Value;

use crate::detectors::iac::{add_environment, record_cloud};
use crate::detectors::{Ctx, attrs};
use crate::files::is_workflow;
use crate::model::*;
use crate::util::{basename, display, line_with, short_path};
use crate::yaml;

/// What a job is for, read off its steps.
#[derive(Default, Debug)]
struct Intent {
    builds: bool,
    builds_image: bool,
    /// The image leaves the runner: `push: true` or a `docker push`.
    pushes_image: bool,
    tests: bool,
    deploys: bool,
    /// Publishes an artifact (image, package, GitHub release) without deploying it.
    releases: bool,
    /// `azure` | `aws` | `gcp` | `kubernetes` | `github_pages` | `fly` | `vercel` | `netlify` | `heroku` | `registry`
    targets: Vec<String>,
    evidence: Vec<String>,
    /// Registries the steps log in to or tag for.
    registries: Vec<RegistryUse>,
}

/// One registry seen in a step: its kind, the host when one was named, and
/// the line that shows it.
#[derive(Debug, Clone)]
struct RegistryUse {
    kind: &'static str,
    host: String,
    evidence: String,
}

impl Intent {
    /// One entry per kind; a later step may supply the host the first one
    /// (an `az acr login`, say) did not name.
    fn registry(&mut self, kind: &'static str, host: &str, evidence: String) {
        match self.registries.iter_mut().find(|r| r.kind == kind) {
            Some(known) => {
                if known.host.is_empty() {
                    known.host = host.to_string();
                }
            }
            None => self.registries.push(RegistryUse {
                kind,
                host: host.to_string(),
                evidence,
            }),
        }
    }
}

/// Host part of a registry address or image reference, lowercased:
/// `ghcr.io/acme/api:1` → `ghcr.io`.
fn registry_host(reference: &str) -> String {
    reference
        .trim()
        .trim_matches(['"', '\''])
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Registry kind (CONTRACT.md §22) of a registry host or image reference.
/// `None` for a bare image name (`acme/api`) and for a host that is still
/// an unresolved expression: a guess here would name the wrong registry.
pub(crate) fn registry_kind(reference: &str) -> Option<&'static str> {
    let host = registry_host(reference);
    if host.is_empty() || host.contains('$') || host.contains(char::is_whitespace) {
        return None;
    }
    let bare = host.split(':').next().unwrap_or(&host);
    Some(if bare == "ghcr.io" {
        "ghcr"
    } else if bare.ends_with(".azurecr.io") {
        "acr"
    } else if (bare.ends_with(".amazonaws.com") && bare.contains(".ecr."))
        || bare == "public.ecr.aws"
    {
        "ecr"
    } else if bare.ends_with("-docker.pkg.dev") || bare == "gcr.io" || bare.ends_with(".gcr.io") {
        "gar"
    } else if matches!(
        bare,
        "docker.io" | "index.docker.io" | "registry-1.docker.io" | "registry.hub.docker.com"
    ) {
        "dockerhub"
    } else if bare.contains('.') || host.contains(':') {
        "generic"
    } else {
        return None;
    })
}

/// `with.push` of `docker/build-push-action`: the action only pushes when
/// asked to, and an expression means it pushes on some runs.
fn pushes(with: Option<&Value>) -> bool {
    let Some(with) = with else {
        return false;
    };
    let push = match yaml::get(with, "push") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => {
            let s = s.trim();
            !s.is_empty() && !s.eq_ignore_ascii_case("false")
        }
        _ => false,
    };
    push || yaml::get_str(with, "outputs")
        .is_some_and(|o| o.contains("type=registry") || o.contains("push=true"))
}

/// Image references of a `tags:` / `images:` input: one per line or comma.
fn image_refs(value: Option<&Value>) -> Vec<String> {
    yaml::str_list(value)
        .iter()
        .flat_map(|v| v.split(['\n', ',']))
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .collect()
}

/// Targets that are a place something runs, as opposed to a registry.
fn is_runtime_target(t: &str) -> bool {
    !matches!(t, "registry" | "iac")
}

/// `with` is the step's inputs; `resolve` substitutes the `${{ env.X }}`
/// values the workflow defines, so `registry: ${{ env.REGISTRY }}` reads as
/// the host it stands for.
fn classify_uses(
    uses: &str,
    with: Option<&Value>,
    resolve: &dyn Fn(&str) -> String,
    intent: &mut Intent,
) {
    let u = uses.to_ascii_lowercase();
    if u.starts_with("docker/login-action") {
        let registry = with
            .and_then(|w| yaml::get_str(w, "registry"))
            .map(resolve)
            .unwrap_or_default();
        if registry.trim().is_empty() {
            // The action's default registry.
            intent.registry("dockerhub", "docker.io", format!("uses: {uses}"));
        } else if let Some(kind) = registry_kind(&registry) {
            intent.registry(
                kind,
                &registry_host(&registry),
                format!("registry: {}", registry.trim()),
            );
        }
    } else if u.starts_with("azure/docker-login") {
        let server = with
            .and_then(|w| yaml::get_str(w, "login-server"))
            .map(resolve)
            .unwrap_or_default();
        let host = match registry_kind(&server) {
            Some("acr") => registry_host(&server),
            _ => String::new(),
        };
        intent.registry("acr", &host, format!("uses: {uses}"));
    } else if u.starts_with("aws-actions/amazon-ecr-login") {
        intent.registry("ecr", "", format!("uses: {uses}"));
    } else if u.starts_with("docker/metadata-action") {
        for image in image_refs(with.and_then(|w| yaml::get(w, "images"))) {
            let image = resolve(&image);
            if let Some(kind) = registry_kind(&image) {
                intent.registry(kind, &registry_host(&image), format!("images: {image}"));
            }
        }
    }
    let mut target = |t: &str| {
        intent.deploys = true;
        if !intent.targets.iter().any(|x| x == t) {
            intent.targets.push(t.to_string());
        }
        intent.evidence.push(format!("uses: {uses}"));
    };
    if u.starts_with("azure/webapps-deploy")
        || u.starts_with("azure/container-apps-deploy")
        || u.starts_with("azure/functions-action")
        || u.starts_with("azure/aks-set-context")
        || u.starts_with("azure/k8s-deploy")
        || u.starts_with("azure/static-web-apps-deploy")
        || u.starts_with("azure/arm-deploy")
        || u.starts_with("azure/cli")
    {
        target("azure");
    } else if u.starts_with("azure/login") {
        intent.targets.push("azure".into());
        intent.evidence.push(format!("uses: {uses}"));
    } else if u.starts_with("aws-actions/amazon-ecs-deploy")
        || u.starts_with("aws-actions/aws-cloudformation")
        || u.starts_with("aws-actions/amazon-ecr-login")
    {
        target("aws");
    } else if u.starts_with("aws-actions/configure-aws-credentials") {
        intent.targets.push("aws".into());
        intent.evidence.push(format!("uses: {uses}"));
    } else if u.starts_with("google-github-actions/deploy-cloudrun")
        || u.starts_with("google-github-actions/deploy-appengine")
        || u.starts_with("google-github-actions/get-gke-credentials")
    {
        target("gcp");
    } else if u.starts_with("google-github-actions/auth") {
        intent.targets.push("gcp".into());
        intent.evidence.push(format!("uses: {uses}"));
    } else if u.starts_with("docker/build-push-action") {
        intent.builds = true;
        intent.builds_image = true;
        intent.evidence.push(format!("uses: {uses}"));
        if pushes(with) {
            intent.pushes_image = true;
            intent.targets.push("registry".into());
            intent.releases = true;
        }
        for tag in image_refs(with.and_then(|w| yaml::get(w, "tags"))) {
            let tag = resolve(&tag);
            if let Some(kind) = registry_kind(&tag) {
                intent.registry(kind, &registry_host(&tag), format!("tags: {tag}"));
            }
        }
    } else if u.starts_with("softprops/action-gh-release")
        || u.starts_with("actions/create-release")
        || u.starts_with("ncipollo/release-action")
        || u.starts_with("googleapis/release-please-action")
        || u.starts_with("changesets/action")
        || u.starts_with("pypa/gh-action-pypi-publish")
        || u.starts_with("goreleaser/goreleaser-action")
        || u.starts_with("cycjimmy/semantic-release-action")
        || u.starts_with("marvinpinto/action-automatic-releases")
    {
        intent.releases = true;
        intent.evidence.push(format!("uses: {uses}"));
    } else if u.starts_with("peaceiris/actions-gh-pages")
        || u.starts_with("actions/deploy-pages")
        || u.starts_with("jamesives/github-pages-deploy-action")
    {
        target("github_pages");
    } else if u.starts_with("superfly/flyctl-actions") {
        target("fly");
    } else if u.starts_with("amondnet/vercel-action") || u.starts_with("vercel/") {
        target("vercel");
    } else if u.starts_with("nwtgck/actions-netlify") || u.starts_with("netlify/") {
        target("netlify");
    } else if u.starts_with("akhileshns/heroku-deploy") {
        target("heroku");
    } else if u.starts_with("azure/k8s-") || u.starts_with("helm/") {
        target("kubernetes");
    }
}

fn classify_run(run: &str, intent: &mut Intent) {
    let r = run.to_ascii_lowercase();
    let first = run.lines().next().unwrap_or(run).trim().to_string();
    let test_cmds = [
        "cargo test",
        "npm test",
        "npm run test",
        "pnpm test",
        "pnpm run test",
        "yarn test",
        "pytest",
        "go test",
        "vitest",
        "jest",
        "mvn test",
        "gradle test",
        "./gradlew test",
        "bundle exec rspec",
        "mix test",
        "phpunit",
        "playwright test",
        "cypress run",
        "cargo nextest",
    ];
    if test_cmds.iter().any(|c| r.contains(c)) {
        intent.tests = true;
        intent.evidence.push(format!("run: {first}"));
    }
    let build_cmds = [
        "cargo build",
        "npm run build",
        "pnpm build",
        "pnpm run build",
        "yarn build",
        "go build",
        "mvn package",
        "gradle build",
        "./gradlew build",
        "vite build",
        "next build",
        "docker build",
        "docker buildx",
    ];
    if build_cmds.iter().any(|c| r.contains(c)) {
        intent.builds = true;
        intent.evidence.push(format!("run: {first}"));
    }
    if r.contains("docker build") || r.contains("docker buildx") {
        intent.builds_image = true;
    }
    if r.contains("docker push") {
        intent.pushes_image = true;
        intent.targets.push("registry".into());
        intent.releases = true;
    }
    for (needle, kind) in [
        ("az acr login", "acr"),
        ("aws ecr get-login-password", "ecr"),
        ("aws ecr-public get-login-password", "ecr"),
        ("gcloud auth configure-docker", "gar"),
    ] {
        if r.contains(needle) {
            intent.registry(kind, "", format!("run: {first}"));
        }
    }
    if r.contains("docker login") || r.contains("docker push") {
        // Only hosts we know: a shell line is full of dotted words.
        for word in run.split_whitespace() {
            if let Some(kind) = registry_kind(word).filter(|k| *k != "generic") {
                intent.registry(kind, &registry_host(word), format!("run: {first}"));
            }
        }
    }
    let release_cmds = [
        "npm publish",
        "pnpm publish",
        "yarn publish",
        "cargo publish",
        "twine upload",
        "poetry publish",
        "uv publish",
        "gh release create",
        "goreleaser release",
        "mvn deploy",
        "gradle publish",
        "./gradlew publish",
        "dotnet nuget push",
        "gem push",
        "semantic-release",
        "npx changeset publish",
    ];
    if release_cmds.iter().any(|c| r.contains(c)) {
        intent.releases = true;
        intent.evidence.push(format!("run: {first}"));
    }
    let deploy_cmds: &[(&str, &str)] = &[
        ("kubectl apply", "kubernetes"),
        ("kubectl set image", "kubernetes"),
        ("kubectl rollout", "kubernetes"),
        ("helm upgrade", "kubernetes"),
        ("helm install", "kubernetes"),
        ("terraform apply", "iac"),
        ("pulumi up", "iac"),
        ("az webapp", "azure"),
        ("az containerapp", "azure"),
        ("az functionapp", "azure"),
        ("az deployment", "azure"),
        ("az aks", "azure"),
        ("aws ecs", "aws"),
        ("aws lambda", "aws"),
        ("aws s3 sync", "aws"),
        ("aws cloudformation deploy", "aws"),
        ("sam deploy", "aws"),
        ("gcloud run deploy", "gcp"),
        ("gcloud app deploy", "gcp"),
        ("gcloud functions deploy", "gcp"),
        ("flyctl deploy", "fly"),
        ("fly deploy", "fly"),
        ("vercel --prod", "vercel"),
        ("vercel deploy", "vercel"),
        ("netlify deploy", "netlify"),
        ("ssh ", "vm"),
        ("rsync ", "vm"),
        ("docker compose up", "vm"),
        ("docker-compose up", "vm"),
    ];
    for (cmd, target) in deploy_cmds {
        if r.contains(cmd) {
            intent.deploys = true;
            if !intent.targets.iter().any(|t| t == target) {
                intent.targets.push(target.to_string());
            }
            intent.evidence.push(format!("run: {first}"));
        }
    }
}

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    let workflows = ctx.files_where(is_workflow);
    if !workflows.is_empty() {
        let (p, c) = workflows[0];
        ctx.detected(
            FactCategory::CiCd,
            "ci:github_actions",
            "GitHub Actions",
            &format!("{} workflows", workflows.len()),
            p,
            line_with(c, "name:").unwrap_or(".github/workflows present"),
        );
    } else if ctx.tree_has(is_workflow) {
        let p = ctx
            .tree_paths()
            .find(|p| is_workflow(p))
            .unwrap_or("")
            .to_string();
        ctx.detected(
            FactCategory::CiCd,
            "ci:github_actions",
            "GitHub Actions",
            "workflow present",
            &p,
            "workflow file in tree",
        );
    }
    for (path, content) in workflows {
        github_workflow(ctx, path, content);
    }
    if let Some(content) = ctx.file(".gitlab-ci.yml") {
        gitlab(ctx, ".gitlab-ci.yml", content);
    }
}

fn github_workflow(ctx: &mut Ctx<'_>, path: &str, content: &str) {
    let Some(doc) = yaml::first(content) else {
        return;
    };
    let name = yaml::get_str(&doc, "name")
        .map(String::from)
        .unwrap_or_else(|| {
            basename(path)
                .trim_end_matches(".yml")
                .trim_end_matches(".yaml")
                .to_string()
        });
    let on = yaml::get(&doc, "on").or_else(|| {
        // YAML 1.1 parsers turn the bare `on` key into `true`; be tolerant.
        match &doc {
            serde_yaml_ng::Value::Mapping(m) => m.get(serde_yaml_ng::Value::Bool(true)),
            _ => None,
        }
    });
    let triggers = yaml::str_list(on);
    let mut jobs = Vec::new();
    let mut workflow_intent = Intent::default();
    let mut job_envs: Vec<(String, String)> = Vec::new();
    let mut github_environments: Vec<String> = Vec::new();
    let env_ref =
        Regex::new(r"\$\{\{\s*env\.([A-Za-z_][A-Za-z0-9_]*)\s*\}\}").expect("static regex");
    let workflow_env = env_map(yaml::get(&doc, "env"));
    let mut packages_write = grants_packages_write(yaml::get(&doc, "permissions"));
    for (job_key, job) in yaml::entries(yaml::get(&doc, "jobs")) {
        let mut intent = Intent::default();
        let job_name = yaml::get_str(job, "name").unwrap_or(&job_key).to_string();
        let runs_on = yaml::str_list(yaml::get(job, "runs-on")).join(", ");
        let needs = yaml::str_list(yaml::get(job, "needs"));
        packages_write |= grants_packages_write(yaml::get(job, "permissions"));
        let job_env = env_map(yaml::get(job, "env"));
        let resolve = |raw: &str| -> String {
            env_ref
                .replace_all(raw, |c: &regex::Captures<'_>| {
                    job_env
                        .get(&c[1])
                        .or_else(|| workflow_env.get(&c[1]))
                        .cloned()
                        .unwrap_or_else(|| c[0].to_string())
                })
                .into_owned()
        };
        let mut steps = Vec::new();
        if let Some(serde_yaml_ng::Value::Sequence(seq)) = yaml::get(job, "steps") {
            for step in seq {
                let uses = yaml::get_str(step, "uses");
                let run = yaml::get_str(step, "run");
                let label = yaml::get_str(step, "name")
                    .map(String::from)
                    .or_else(|| uses.map(String::from))
                    .or_else(|| run.map(|r| r.lines().next().unwrap_or("").trim().to_string()))
                    .unwrap_or_default();
                if !label.is_empty() {
                    steps.push(label);
                }
                if let Some(u) = uses {
                    classify_uses(u, yaml::get(step, "with"), &resolve, &mut intent);
                }
                if let Some(r) = run {
                    classify_run(&resolve(r), &mut intent);
                }
            }
        }
        if let Some(w) = yaml::get_str(job, "uses") {
            steps.push(format!("reusable workflow {w}"));
        }
        let declared_env = match yaml::get(job, "environment") {
            Some(serde_yaml_ng::Value::String(s)) => Some(s.trim().to_string()),
            Some(v) => yaml::get_str(v, "name").map(|n| n.trim().to_string()),
            None => None,
        }
        .filter(|e| !e.is_empty());
        // `environment: ${{ inputs.environment }}` is chosen per run: the job
        // deploys, but it names no environment of its own.
        let dynamic_env = declared_env.as_deref().is_some_and(|e| e.contains("${{"));
        let env_name = declared_env.filter(|_| !dynamic_env);
        if let Some(env) = &env_name
            && !github_environments.contains(env)
        {
            github_environments.push(env.clone());
        }
        let has_environment = env_name.is_some() || dynamic_env;
        let lower_job = format!("{job_key} {job_name}").to_ascii_lowercase();
        let named_release = lower_job.contains("release") || lower_job.contains("publish");
        if named_release && !intent.targets.iter().any(|t| is_runtime_target(t)) {
            // "publish" without a cloud step ships an artifact, it does not deploy.
            intent.releases = intent.releases || intent.builds || has_environment;
        }
        if lower_job.contains("deploy") || named_release {
            intent.deploys =
                intent.deploys || !intent.targets.is_empty() || has_environment || intent.releases;
        }
        if (intent.deploys || env_name.is_some()) && !dynamic_env {
            let env = env_name
                .clone()
                .unwrap_or_else(|| guess_env(&lower_job, &triggers));
            job_envs.push((job_key.clone(), env));
        }
        merge(&mut workflow_intent, &intent);
        jobs.push(PipelineJobDraft {
            key: job_key,
            name: job_name,
            runs_on,
            needs,
            steps,
        });
    }

    let pipeline_key = format!("pipeline:{path}");
    ctx.detected(
        FactCategory::CiCd,
        &format!("ci:workflow:{path}"),
        "Workflow",
        &format!("{name} on {}", triggers.join(", ")),
        path,
        line_with(content, "on:").unwrap_or("on:"),
    );
    ctx.node(
        NodeKind::Pipeline,
        &pipeline_key,
        &name,
        &format!("GitHub Actions workflow with {} jobs", jobs.len()),
        path,
        attrs(&[
            ("provider", "github_actions"),
            ("triggers", &triggers.join(",")),
            ("jobs", &jobs.len().to_string()),
            ("image", image_mode(&workflow_intent)),
        ]),
    );
    let repo = ctx.repo_key.clone();
    ctx.edge(
        &repo,
        &pipeline_key,
        "CONTAINS",
        RelationCategory::Structure,
    );
    if packages_write {
        // `packages: write` is what a push to GHCR needs. Next to an image
        // build it is the registry; on its own it may be any GitHub package.
        let evidence = line_with(content, "packages:").unwrap_or("packages: write");
        if workflow_intent.builds_image {
            workflow_intent.registry("ghcr", "ghcr.io", evidence.to_string());
        } else if !ctx.has_fact("registry:ghcr") {
            ctx.inferred(
                FactCategory::Container,
                "registry:ghcr",
                &display("ghcr"),
                "ghcr.io",
                0.6,
                &format!(
                    "{} grants `packages: write` but builds no image",
                    short_path(path, 1)
                ),
            );
        }
    }
    for env in &github_environments {
        let key = format!("cd:github_environment:{env}");
        if !ctx.has_fact(&key) {
            ctx.detected(
                FactCategory::Deployment,
                &key,
                "GitHub environment",
                env,
                path,
                &format!("environment: {env}"),
            );
        }
    }
    wire_intent(ctx, &pipeline_key, path, &workflow_intent, &job_envs);
    ctx.out.pipelines.push(PipelineDraft {
        provider: "github_actions".into(),
        name,
        path: path.to_string(),
        triggers,
        jobs,
    });
    ctx.chunk(
        ChunkKind::CiCd,
        path,
        &format!("Workflow {}", short_path(path, 1)),
        content,
    );
}

/// `env:` of a workflow or job as plain strings.
fn env_map(v: Option<&Value>) -> BTreeMap<String, String> {
    yaml::entries(v)
        .into_iter()
        .filter_map(|(k, v)| match v {
            Value::String(s) => Some((k, s.clone())),
            Value::Number(n) => Some((k, n.to_string())),
            _ => None,
        })
        .collect()
}

/// `permissions:` of a workflow or job grants `packages: write`.
fn grants_packages_write(v: Option<&Value>) -> bool {
    match v {
        Some(Value::String(s)) => s.trim() == "write-all",
        Some(m @ Value::Mapping(_)) => yaml::get_str(m, "packages") == Some("write"),
        _ => false,
    }
}

/// `push` when the pipeline publishes the image it builds, `build` when it
/// only builds it, empty when it builds none.
fn image_mode(intent: &Intent) -> &'static str {
    match (intent.builds_image, intent.pushes_image) {
        (true, true) => "push",
        (true, false) => "build",
        _ => "",
    }
}

fn merge(into: &mut Intent, from: &Intent) {
    into.builds |= from.builds;
    into.builds_image |= from.builds_image;
    into.pushes_image |= from.pushes_image;
    for r in &from.registries {
        into.registry(r.kind, &r.host, r.evidence.clone());
    }
    into.tests |= from.tests;
    into.deploys |= from.deploys;
    into.releases |= from.releases;
    for t in &from.targets {
        if !into.targets.contains(t) {
            into.targets.push(t.clone());
        }
    }
    into.evidence.extend(from.evidence.iter().cloned());
}

fn guess_env(job: &str, triggers: &[String]) -> String {
    if job.contains("staging") || job.contains("stage") {
        "staging".into()
    } else if job.contains("preview")
        || (job.contains("pr") && triggers.iter().any(|t| t == "pull_request"))
    {
        "preview".into()
    } else if job.contains("dev") {
        "development".into()
    } else {
        "production".into()
    }
}

/// Edges from what the pipeline does: BUILDS images/services, TESTS
/// services, DEPLOYS a deployment that is DEPLOYED_TO an environment.
fn wire_intent(
    ctx: &mut Ctx<'_>,
    pipeline_key: &str,
    path: &str,
    intent: &Intent,
    job_envs: &[(String, String)],
) {
    let services = ctx.service_keys();
    let images: Vec<String> = ctx
        .out
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::ContainerImage)
        .map(|n| n.key.clone())
        .collect();
    if intent.builds {
        if intent.builds_image && !images.is_empty() {
            for img in &images {
                ctx.edge(pipeline_key, img, "BUILDS", RelationCategory::Execution);
            }
        } else {
            for s in &services {
                ctx.edge(pipeline_key, s, "BUILDS", RelationCategory::Execution);
            }
        }
        ctx.detected(
            FactCategory::CiCd,
            &format!("ci:build:{path}"),
            "CI builds the project",
            &short_path(path, 1),
            path,
            intent
                .evidence
                .iter()
                .find(|e| e.contains("build") || e.contains("docker/build-push-action"))
                .map(String::as_str)
                .unwrap_or("build step"),
        );
    }
    if intent.builds_image {
        // What a release is cut from: the workflow whose success means a
        // new image exists (`push`) or could be built (`build`).
        ctx.detected(
            FactCategory::CiCd,
            &format!("ci:build_image:{path}"),
            "CI builds a container image",
            image_mode(intent),
            path,
            intent
                .evidence
                .iter()
                .find(|e| e.contains("docker"))
                .map(String::as_str)
                .unwrap_or("image build step"),
        );
    }
    for registry in &intent.registries {
        let key = format!("registry:{}", registry.kind);
        if ctx.fact(&key).is_some_and(|f| f.kind == FactKind::Detected) {
            continue;
        }
        let value = if registry.host.is_empty() {
            format!("used by {}", short_path(path, 1))
        } else {
            registry.host.clone()
        };
        ctx.detected(
            FactCategory::Container,
            &key,
            &display(registry.kind),
            &value,
            path,
            &registry.evidence,
        );
    }
    if intent.tests {
        for s in &services {
            ctx.edge(pipeline_key, s, "TESTS", RelationCategory::Validation);
        }
        ctx.detected(
            FactCategory::Testing,
            "testing:ci",
            "Tests run in CI",
            short_path(path, 1).as_str(),
            path,
            intent
                .evidence
                .iter()
                .find(|e| e.starts_with("run:"))
                .map(String::as_str)
                .unwrap_or("test step"),
        );
    }
    for t in &intent.targets {
        match t.as_str() {
            "azure" | "aws" | "gcp" => {
                let ev = intent
                    .evidence
                    .iter()
                    .find(|e| e.contains(t.as_str()) || e.contains("uses:"))
                    .cloned()
                    .unwrap_or_default();
                record_cloud(ctx, t, path, &ev, "GitHub Actions deploy step");
            }
            "iac" => {}
            other => {
                ctx.deploy_targets.insert(other.to_string());
            }
        }
    }
    if !intent.deploys && !intent.releases {
        return;
    }
    // A runtime target wins; a registry push or release action without one
    // is a release, and `unknown` is reserved for a deploy we could not read.
    let target = intent
        .targets
        .iter()
        .find(|t| is_runtime_target(t))
        .cloned()
        .unwrap_or_else(|| {
            if intent.releases || intent.targets.iter().any(|t| t == "registry") {
                "release".into()
            } else {
                "unknown".into()
            }
        });
    ctx.detected(
        FactCategory::Deployment,
        &format!("deploy:pipeline:{path}"),
        if target == "release" {
            "Releases from CI"
        } else {
            "Deploys from CI"
        },
        &target,
        path,
        intent
            .evidence
            .last()
            .map(String::as_str)
            .unwrap_or("deploy job"),
    );
    for (job, env) in job_envs {
        add_environment(ctx, env, path, "workflow job");
        let dep_key = format!("deployment:{}:{job}", short_path(path, 1));
        ctx.node(
            NodeKind::Deployment,
            &dep_key,
            &format!("{job} → {env}"),
            &format!("Deployment job {job} in {}", short_path(path, 1)),
            path,
            attrs(&[("target", &target), ("environment", env), ("job", job)]),
        );
        ctx.edge(
            pipeline_key,
            &dep_key,
            "DEPLOYS",
            RelationCategory::Execution,
        );
        ctx.edge(
            &dep_key,
            &format!("environment:{env}"),
            "DEPLOYED_TO",
            RelationCategory::Infrastructure,
        );
        let subject = images
            .first()
            .cloned()
            .or_else(|| services.first().cloned());
        if let Some(subject) = subject {
            ctx.edge(&dep_key, &subject, "RELEASES", RelationCategory::Execution);
        }
    }
}

fn gitlab(ctx: &mut Ctx<'_>, path: &str, content: &str) {
    let Some(doc) = yaml::first(content) else {
        return;
    };
    ctx.detected(
        FactCategory::CiCd,
        "ci:gitlab_ci",
        "GitLab CI",
        &short_path(path, 1),
        path,
        line_with(content, "stages:").unwrap_or(".gitlab-ci.yml present"),
    );
    const RESERVED: &[&str] = &[
        "stages",
        "variables",
        "include",
        "default",
        "image",
        "workflow",
        "services",
        "cache",
        "before_script",
        "after_script",
        "pages",
    ];
    let mut jobs = Vec::new();
    let mut intent = Intent::default();
    let mut job_envs = Vec::new();
    for (key, job) in yaml::entries(Some(&doc)) {
        if key.starts_with('.') || RESERVED.contains(&key.as_str()) || !job.is_mapping() {
            continue;
        }
        let mut job_intent = Intent::default();
        let steps: Vec<String> = yaml::str_list(yaml::get(job, "script"));
        for s in &steps {
            classify_run(s, &mut job_intent);
        }
        let env = match yaml::get(job, "environment") {
            Some(serde_yaml_ng::Value::String(s)) => Some(s.clone()),
            Some(v) => yaml::get_str(v, "name").map(String::from),
            None => None,
        };
        let stage = yaml::get_str(job, "stage").unwrap_or("").to_string();
        if stage.contains("deploy") || env.is_some() {
            job_intent.deploys = true;
        }
        if job_intent.deploys {
            job_envs.push((key.clone(), env.unwrap_or_else(|| guess_env(&key, &[]))));
        }
        merge(&mut intent, &job_intent);
        jobs.push(PipelineJobDraft {
            key: key.clone(),
            name: key,
            runs_on: yaml::get_str(job, "image").unwrap_or("").to_string(),
            needs: yaml::str_list(yaml::get(job, "needs")),
            steps,
        });
    }
    let pipeline_key = format!("pipeline:{path}");
    ctx.node(
        NodeKind::Pipeline,
        &pipeline_key,
        "GitLab CI",
        &format!("GitLab CI pipeline with {} jobs", jobs.len()),
        path,
        attrs(&[
            ("provider", "gitlab_ci"),
            ("jobs", &jobs.len().to_string()),
            ("image", image_mode(&intent)),
        ]),
    );
    let repo = ctx.repo_key.clone();
    ctx.edge(
        &repo,
        &pipeline_key,
        "CONTAINS",
        RelationCategory::Structure,
    );
    wire_intent(ctx, &pipeline_key, path, &intent, &job_envs);
    ctx.out.pipelines.push(PipelineDraft {
        provider: "gitlab_ci".into(),
        name: "GitLab CI".into(),
        path: path.to_string(),
        triggers: yaml::str_list(yaml::get(&doc, "stages")),
        jobs,
    });
    ctx.chunk(ChunkKind::CiCd, path, "GitLab CI pipeline", content);
}
