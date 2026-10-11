//! CI/CD pipelines. Workflows are parsed into jobs and steps; the `uses:`
//! actions and `run:` commands reveal what a pipeline builds, tests and
//! deploys, and where. Publishing an image, a package or a GitHub release
//! is a *release*, recorded with target `release`, so a repository that
//! ships artifacts but never deploys them is described truthfully.

use crate::detectors::iac::{add_environment, record_cloud};
use crate::detectors::{Ctx, attrs};
use crate::files::is_workflow;
use crate::model::*;
use crate::util::{basename, line_with, short_path};
use crate::yaml;

/// What a job is for, read off its steps.
#[derive(Default, Debug)]
struct Intent {
    builds: bool,
    builds_image: bool,
    tests: bool,
    deploys: bool,
    /// Publishes an artifact (image, package, GitHub release) without deploying it.
    releases: bool,
    /// `azure` | `aws` | `gcp` | `kubernetes` | `github_pages` | `fly` | `vercel` | `netlify` | `heroku` | `registry`
    targets: Vec<String>,
    evidence: Vec<String>,
}

/// Targets that are a place something runs, as opposed to a registry.
fn is_runtime_target(t: &str) -> bool {
    !matches!(t, "registry" | "iac")
}

fn classify_uses(uses: &str, intent: &mut Intent) {
    let u = uses.to_ascii_lowercase();
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
        if !u.contains("push: false") {
            intent.targets.push("registry".into());
            intent.releases = true;
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
        intent.targets.push("registry".into());
        intent.releases = true;
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
    for (job_key, job) in yaml::entries(yaml::get(&doc, "jobs")) {
        let mut intent = Intent::default();
        let job_name = yaml::get_str(job, "name").unwrap_or(&job_key).to_string();
        let runs_on = yaml::str_list(yaml::get(job, "runs-on")).join(", ");
        let needs = yaml::str_list(yaml::get(job, "needs"));
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
                    classify_uses(u, &mut intent);
                }
                if let Some(r) = run {
                    classify_run(r, &mut intent);
                }
            }
        }
        if let Some(w) = yaml::get_str(job, "uses") {
            steps.push(format!("reusable workflow {w}"));
        }
        let env_name = match yaml::get(job, "environment") {
            Some(serde_yaml_ng::Value::String(s)) => Some(s.clone()),
            Some(v) => yaml::get_str(v, "name").map(String::from),
            None => None,
        };
        let lower_job = format!("{job_key} {job_name}").to_ascii_lowercase();
        let named_release = lower_job.contains("release") || lower_job.contains("publish");
        if named_release && !intent.targets.iter().any(|t| is_runtime_target(t)) {
            // "publish" without a cloud step ships an artifact, it does not deploy.
            intent.releases = intent.releases || intent.builds || env_name.is_some();
        }
        if lower_job.contains("deploy") || named_release {
            intent.deploys = intent.deploys
                || !intent.targets.is_empty()
                || env_name.is_some()
                || intent.releases;
        }
        if intent.deploys || env_name.is_some() {
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
        ]),
    );
    let repo = ctx.repo_key.clone();
    ctx.edge(
        &repo,
        &pipeline_key,
        "CONTAINS",
        RelationCategory::Structure,
    );
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

fn merge(into: &mut Intent, from: &Intent) {
    into.builds |= from.builds;
    into.builds_image |= from.builds_image;
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
        attrs(&[("provider", "gitlab_ci"), ("jobs", &jobs.len().to_string())]),
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
