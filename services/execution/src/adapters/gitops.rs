//! The `gitops_flux` / `gitops_argocd` deployer (CONTRACT.md §22). SDI does
//! not talk to the cluster: it changes the image reference in the GitOps
//! repository and lets Flux or Argo CD reconcile. One file is read through
//! the GitHub contents API, edited as text (see `yaml_edit`: comments and
//! formatting survive), and written back either as a pull request that the
//! step waits to see merged (`mode: pull_request`, the default) or as a
//! direct commit (`mode: commit`).
//!
//! The step is idempotent: a file that already names the release changes
//! nothing and succeeds; a retry finds the branch and the pull request the
//! first attempt made and keeps waiting on them. A write GitHub refuses
//! because the file moved underneath it is `GITOPS_CONFLICT`: retrying
//! reads the new state and applies the change on top.

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use super::{
    Adapter, RunContext, StepInput, StepOutcome, TOOL_GITOPS,
    github::{ApiError, GithubAdapter, repo_param, str_field},
    is_deploy_action,
    jenkins::step_limit,
    yaml_edit::{self, Edit, EditError},
};
use crate::domain::diagnosis;

pub const STRATEGY_KUSTOMIZE: &str = "kustomize_image";
pub const STRATEGY_HELM: &str = "helm_values";
pub const STRATEGY_YAML_PATH: &str = "yaml_path";
pub const STRATEGIES: [&str; 3] = [STRATEGY_KUSTOMIZE, STRATEGY_HELM, STRATEGY_YAML_PATH];
pub const MODE_PULL_REQUEST: &str = "pull_request";
pub const MODE_COMMIT: &str = "commit";
pub const MODES: [&str; 2] = [MODE_PULL_REQUEST, MODE_COMMIT];
/// Where a Helm chart conventionally keeps the image tag.
pub const DEFAULT_HELM_PATH: &str = "image.tag";
/// How long a pull request may wait for its merge (30 minutes).
pub const MERGE_TIMEOUT_SECONDS: u64 = 30 * 60;
pub const MAX_MERGE_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;
const MERGE_POLL: Duration = Duration::from_secs(10);
const WRITE_HINT: &str = "the GitHub token needs Contents and Pull requests \"Read and write\" on the GitOps repository (the `repo` scope on a classic token)";

pub struct GitopsAdapter {
    github: GithubAdapter,
    poll: Duration,
}

type Fail = StepOutcome;

fn failed(detail: impl Into<String>) -> Fail {
    StepOutcome::failed(diagnosis::DEPLOYER_FAILED, detail)
}

/// `sdi/deploy-<env>-<tag>`, with anything a ref name cannot hold replaced.
pub fn deploy_branch(environment: &str, tag: &str) -> String {
    let clean = |text: &str| -> String {
        let cleaned: String = text
            .trim()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        cleaned.trim_matches(['-', '.']).to_owned()
    };
    format!("sdi/deploy-{}-{}", clean(environment), clean(tag))
}

/// A repository path with each segment percent-encoded.
pub fn encode_path(path: &str) -> String {
    path.trim()
        .trim_matches('/')
        .split('/')
        .map(|segment| {
            segment
                .bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                        (b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Applies the environment's strategy to the file.
pub fn edit(
    text: &str,
    strategy: &str,
    yaml_path: &str,
    image: &str,
    tag: &str,
    digest: &str,
) -> Result<Edit, EditError> {
    match strategy {
        STRATEGY_KUSTOMIZE | "" => yaml_edit::set_kustomize_image(text, image, tag, digest),
        STRATEGY_HELM | STRATEGY_YAML_PATH => {
            let path = match (yaml_path.trim(), strategy) {
                ("", STRATEGY_HELM) => DEFAULT_HELM_PATH,
                ("", _) => {
                    return Err(EditError::NotFound(
                        "the yaml_path strategy needs the environment's yaml_path".to_owned(),
                    ));
                }
                (path, _) => path,
            };
            let current = yaml_edit::get_scalar(text, path)?;
            let value = yaml_edit::target_value(&current, image, tag, digest);
            yaml_edit::set_scalar(text, path, &value)
        }
        other => Err(EditError::Unsupported(format!(
            "unknown GitOps strategy {other}; use kustomize_image, helm_values or yaml_path"
        ))),
    }
}

/// What a write to the contents API left behind.
struct Written {
    commit: String,
    html_url: String,
}

impl GitopsAdapter {
    pub fn new(github_token: Option<String>, github_api: &str) -> Self {
        Self {
            github: GithubAdapter::new(github_token, github_api),
            poll: MERGE_POLL,
        }
    }

    #[cfg(test)]
    pub fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    fn api_failure(error: ApiError) -> Fail {
        match error.status() {
            Some(StatusCode::CONFLICT) => {
                StepOutcome::failed(diagnosis::GITOPS_CONFLICT, error.describe())
            }
            _ => error.into_deploy_outcome("repository:write", WRITE_HINT),
        }
    }

    /// The file at `path` on `branch`: its text and blob sha.
    async fn read(
        &self,
        ctx: &RunContext,
        repo: &str,
        path: &str,
        branch: &str,
    ) -> Result<(String, String), Fail> {
        let api_path = format!("/repos/{repo}/contents/{}?ref={branch}", encode_path(path));
        let (_, file) = self
            .github
            .call(ctx, Method::GET, &api_path, None, &[])
            .await
            .map_err(|error| match error.status() {
                Some(StatusCode::NOT_FOUND) => failed(format!(
                    "{}\n{path} does not exist on {repo}@{branch} (or the token cannot read that repository)",
                    error.describe()
                )),
                _ => Self::api_failure(error),
            })?;
        if file.is_array() || str_field(&file, "type") != "file" {
            return Err(failed(format!("{path} on {repo}@{branch} is not a file")));
        }
        if str_field(&file, "encoding") != "base64" {
            return Err(failed(format!(
                "{path} on {repo}@{branch} is too large to edit through the contents API"
            )));
        }
        let encoded: String = str_field(&file, "content")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| failed(format!("{path} on {repo}@{branch} did not decode")))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| failed(format!("{path} on {repo}@{branch} is not UTF-8 text")))?;
        Ok((text, str_field(&file, "sha")))
    }

    /// Replaces the file on `branch`, given the blob sha it had when read.
    #[allow(clippy::too_many_arguments)]
    async fn write(
        &self,
        ctx: &RunContext,
        repo: &str,
        path: &str,
        branch: &str,
        sha: &str,
        content: &str,
        message: &str,
    ) -> Result<Written, Fail> {
        let (_, written) = self
            .github
            .call(
                ctx,
                Method::PUT,
                &format!("/repos/{repo}/contents/{}", encode_path(path)),
                Some(json!({
                    "message": message,
                    "content": STANDARD.encode(content),
                    "sha": sha,
                    "branch": branch,
                })),
                &[],
            )
            .await
            .map_err(|error| match error.status() {
                Some(StatusCode::CONFLICT) if error.describe().contains("pull request") => {
                    failed(format!(
                        "{}\n{branch} only takes changes through pull requests: set the environment's mode to pull_request",
                        error.describe()
                    ))
                }
                _ => Self::api_failure(error),
            })?;
        let commit = written.get("commit").unwrap_or(&Value::Null);
        Ok(Written {
            commit: str_field(commit, "sha"),
            html_url: str_field(commit, "html_url"),
        })
    }

    async fn deploy(&self, step: &StepInput, ctx: &RunContext) -> Result<StepOutcome, Fail> {
        if self.github.github_for(ctx).0.is_none() {
            return Err(StepOutcome::Failed {
                code: diagnosis::PERMISSION_DENIED.to_owned(),
                raw_output: format!(
                    "no GitHub token is available for this workspace; {WRITE_HINT}"
                ),
                missing_permissions: vec!["repository:write".to_owned()],
            });
        }
        let param = |key: &str| step.params.get(key).map(|v| v.trim()).unwrap_or_default();
        let Some(repo) = repo_param(step) else {
            return Err(failed(
                "the GitOps environment needs repo (owner/name) of the repository to edit",
            ));
        };
        let path = param("path");
        if path.is_empty() {
            return Err(failed(
                "the GitOps environment needs path: the file that names the image",
            ));
        }
        let (image, tag, environment) = (param("image"), param("tag"), param("environment"));
        let digest = ctx
            .outputs
            .get("digest")
            .map(String::as_str)
            .filter(|d| !d.is_empty())
            .unwrap_or(param("digest"));
        let mode = match param("mode") {
            "" => MODE_PULL_REQUEST,
            mode => mode,
        };
        if !MODES.contains(&mode) {
            return Err(failed(format!(
                "unknown GitOps mode {mode}; use pull_request or commit"
            )));
        }
        let branch = match param("branch") {
            "" => self
                .github
                .default_branch(ctx, &repo)
                .await
                .map_err(Self::api_failure)?,
            branch => branch.to_owned(),
        };

        let (text, sha) = self.read(ctx, &repo, path, &branch).await?;
        let edited = edit(
            &text,
            param("strategy"),
            param("yaml_path"),
            image,
            tag,
            digest,
        )
        .map_err(|error| failed(format!("{path} on {repo}@{branch}: {error}")))?;
        ctx.logs.line(format!("{path}: {}", edited.summary));
        let mut outputs = BTreeMap::from([("gitops_file".to_owned(), format!("{repo}/{path}"))]);
        if !edited.changed {
            return Ok(StepOutcome::Succeeded {
                output: format!(
                    "{repo}@{branch} already names {image}:{tag} in {path}; nothing to commit"
                ),
                outputs,
            });
        }
        let message = format!(
            "Deploy {image}:{tag} to {environment}\n\n{}\n\nSoftware Delivery Intelligence execution #{}",
            edited.summary, ctx.execution_number
        );

        if mode == MODE_COMMIT {
            let written = self
                .write(ctx, &repo, path, &branch, &sha, &edited.content, &message)
                .await?;
            ctx.logs
                .line(format!("committed {} to {repo}@{branch}", written.commit));
            outputs.insert("gitops_commit".to_owned(), written.commit.clone());
            if !written.html_url.is_empty() {
                outputs.insert("external_url".to_owned(), written.html_url.clone());
            }
            return Ok(StepOutcome::Succeeded {
                output: format!(
                    "committed {} to {repo}@{branch}: {}",
                    written.commit, edited.summary
                ),
                outputs,
            });
        }

        // Pull request mode: the change lives on its own branch.
        let work = deploy_branch(environment, tag);
        let base_path = format!("/repos/{repo}");
        let (status, existing) = self
            .github
            .call(
                ctx,
                Method::GET,
                &format!("{base_path}/git/ref/heads/{work}"),
                None,
                &[StatusCode::NOT_FOUND],
            )
            .await
            .map_err(Self::api_failure)?;
        let _ = existing;
        let (work_text, work_sha) = if status == StatusCode::NOT_FOUND {
            let (_, head) = self
                .github
                .call(
                    ctx,
                    Method::GET,
                    &format!("{base_path}/git/ref/heads/{branch}"),
                    None,
                    &[],
                )
                .await
                .map_err(Self::api_failure)?;
            let base_sha = str_field(head.get("object").unwrap_or(&Value::Null), "sha");
            self.github
                .call(
                    ctx,
                    Method::POST,
                    &format!("{base_path}/git/refs"),
                    Some(json!({ "ref": format!("refs/heads/{work}"), "sha": base_sha })),
                    &[],
                )
                .await
                .map_err(Self::api_failure)?;
            ctx.logs.line(format!("created {work} from {branch}"));
            (text.clone(), sha.clone())
        } else {
            // A previous attempt made the branch: write on top of what it holds.
            ctx.logs.line(format!("reusing {work}"));
            self.read(ctx, &repo, path, &work).await?
        };
        if work_text != edited.content {
            let written = self
                .write(
                    ctx,
                    &repo,
                    path,
                    &work,
                    &work_sha,
                    &edited.content,
                    &message,
                )
                .await?;
            ctx.logs
                .line(format!("committed {} to {work}", written.commit));
            outputs.insert("gitops_commit".to_owned(), written.commit);
        }

        let owner = repo.split('/').next().unwrap_or_default();
        let (_, open) = self
            .github
            .call(
                ctx,
                Method::GET,
                &format!("{base_path}/pulls?state=open&head={owner}:{work}"),
                None,
                &[],
            )
            .await
            .map_err(Self::api_failure)?;
        let pull = match open.as_array().and_then(|pulls| pulls.first()).cloned() {
            Some(pull) => pull,
            None => {
                let (_, pull) = self
                    .github
                    .call(
                        ctx,
                        Method::POST,
                        &format!("{base_path}/pulls"),
                        Some(json!({
                            "title": format!("Deploy {tag} to {environment}"),
                            "head": work,
                            "base": branch,
                            "body": format!(
                                "Points `{path}` at `{image}:{tag}` for **{environment}**.\n\n- {}\n\nMerging this lets the GitOps controller roll the release out. Opened by Software Delivery Intelligence execution #{}, which is waiting for the merge.",
                                edited.summary, ctx.execution_number
                            ),
                        })),
                        &[],
                    )
                    .await
                    .map_err(Self::api_failure)?;
                pull
            }
        };
        let number = pull.get("number").and_then(Value::as_u64).unwrap_or(0);
        let html_url = str_field(&pull, "html_url");
        ctx.logs
            .line(format!("pull request #{number} is open: {html_url}"));
        ctx.progress.emit("external_url", html_url.clone());
        outputs.insert("external_url".to_owned(), html_url.clone());
        outputs.insert("pull_request_url".to_owned(), html_url.clone());

        // Wait for a human (or an auto-merge rule) to merge it.
        let limit = step_limit(step, MERGE_TIMEOUT_SECONDS, MAX_MERGE_TIMEOUT_SECONDS);
        let deadline = tokio::time::Instant::now() + limit;
        let pull_path = format!("{base_path}/pulls/{number}");
        loop {
            let (_, current) = self
                .github
                .call(ctx, Method::GET, &pull_path, None, &[])
                .await
                .map_err(Self::api_failure)?;
            if current.get("merged").and_then(Value::as_bool) == Some(true) {
                let merge_commit = str_field(&current, "merge_commit_sha");
                ctx.logs
                    .line(format!("pull request #{number} merged as {merge_commit}"));
                if !merge_commit.is_empty() {
                    outputs.insert("gitops_commit".to_owned(), merge_commit);
                }
                return Ok(StepOutcome::Succeeded {
                    output: format!(
                        "pull request {html_url} merged into {repo}@{branch}: {}",
                        edited.summary
                    ),
                    outputs,
                });
            }
            if str_field(&current, "state") == "closed" {
                return Err(failed(format!(
                    "pull request {html_url} was closed without being merged"
                )));
            }
            if tokio::time::Instant::now() + self.poll >= deadline {
                return Ok(StepOutcome::failed(
                    diagnosis::DEPLOYER_TIMEOUT,
                    format!(
                        "pull request {html_url} was still open after {}s; merge it and retry",
                        limit.as_secs()
                    ),
                ));
            }
            tokio::time::sleep(self.poll).await;
        }
    }
}

#[async_trait]
impl Adapter for GitopsAdapter {
    fn name(&self) -> &'static str {
        TOOL_GITOPS
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        if !is_deploy_action(&step.action_type) {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("the gitops adapter cannot run {}", step.action_type),
            );
        }
        match self.deploy(step, ctx).await {
            Ok(outcome) | Err(outcome) => outcome,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;
    use crate::{
        adapters::{GithubAccess, OutputSink},
        tests::fakes::github::{self as fake, FakeGithub},
    };

    const KUSTOMIZATION: &str = "\
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
resources:
  - ../../base
images:
  - name: ghcr.io/acme/api   # bumped by SDI
    newTag: 9f2c1ab
";

    fn step(mode: &str, strategy: &str, extra: &[(&str, &str)]) -> StepInput {
        let mut params: BTreeMap<String, String> = [
            ("deployer", "gitops_flux"),
            ("repo", "acme/fleet"),
            ("branch", ""),
            ("path", "apps/api/kustomization.yaml"),
            ("strategy", strategy),
            ("mode", mode),
            ("environment", "production"),
            ("image", "ghcr.io/acme/api"),
            ("tag", "a1b2c3d"),
            ("digest", ""),
            ("version", "a1b2c3d"),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
        for (key, value) in extra {
            params.insert((*key).to_owned(), (*value).to_owned());
        }
        StepInput {
            key: "deploy".into(),
            title: "Deploy to production".into(),
            action_type: "deploy.production".into(),
            adapter: "gitops".into(),
            params,
            attempt: 1,
        }
    }

    fn context(api: &str) -> (RunContext, tokio::sync::mpsc::Receiver<(String, String)>) {
        let (ptx, progress) = tokio::sync::mpsc::channel(16);
        let ctx = RunContext {
            execution_number: 7,
            github: Some(GithubAccess {
                token: fake::TOKEN.into(),
                api_base_url: api.to_owned(),
            }),
            progress: OutputSink::new(ptx),
            ..Default::default()
        };
        (ctx, progress)
    }

    fn adapter() -> GitopsAdapter {
        GitopsAdapter::new(None, "http://unused.invalid").with_poll(Duration::from_millis(10))
    }

    #[tokio::test]
    async fn commit_mode_writes_the_edited_file_straight_to_the_branch() {
        let (github, api) = FakeGithub::default()
            .with_file(
                "acme/fleet",
                "main",
                "apps/api/kustomization.yaml",
                KUSTOMIZATION,
            )
            .serve()
            .await;
        let (ctx, _progress) = context(&api);
        let outcome = adapter()
            .run(&step("commit", "kustomize_image", &[]), &ctx)
            .await;
        let StepOutcome::Succeeded { output, outputs } = outcome else {
            panic!("expected success: {outcome:?}");
        };
        assert!(output.contains("newTag 9f2c1ab -> a1b2c3d"), "{output}");
        assert_eq!(
            outputs["gitops_file"],
            "acme/fleet/apps/api/kustomization.yaml"
        );
        assert!(outputs["gitops_commit"].starts_with("commit-"));
        // The file on main differs from the original in the one tag only.
        let written = github
            .file("acme/fleet", "main", "apps/api/kustomization.yaml")
            .unwrap();
        assert_eq!(written, KUSTOMIZATION.replace("9f2c1ab", "a1b2c3d"));
        assert!(written.contains("# bumped by SDI"));
        let commits = github.commits();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].branch, "main");
        assert!(
            commits[0]
                .message
                .starts_with("Deploy ghcr.io/acme/api:a1b2c3d to production\n"),
            "{}",
            commits[0].message
        );
        assert!(
            github.pulls().is_empty(),
            "commit mode opens no pull request"
        );

        // Running the step again finds nothing to do.
        let outcome = adapter()
            .run(&step("commit", "kustomize_image", &[]), &ctx)
            .await;
        let StepOutcome::Succeeded { output, .. } = outcome else {
            panic!("expected success");
        };
        assert!(output.contains("nothing to commit"), "{output}");
        assert_eq!(github.commits().len(), 1);
    }

    #[tokio::test]
    async fn pull_request_mode_opens_a_pull_request_and_waits_for_the_merge() {
        let (github, api) = FakeGithub::default()
            .with_file(
                "acme/fleet",
                "main",
                "apps/api/kustomization.yaml",
                KUSTOMIZATION,
            )
            .merge_pulls_after(3)
            .serve()
            .await;
        let (ctx, mut progress) = context(&api);
        let outcome = adapter()
            .run(&step("pull_request", "kustomize_image", &[]), &ctx)
            .await;
        let StepOutcome::Succeeded { output, outputs } = outcome else {
            panic!("expected success: {outcome:?}");
        };
        let pulls = github.pulls();
        assert_eq!(pulls.len(), 1);
        assert_eq!(pulls[0].head, "sdi/deploy-production-a1b2c3d");
        assert_eq!(pulls[0].base, "main");
        assert_eq!(pulls[0].title, "Deploy a1b2c3d to production");
        assert!(pulls[0].body.contains("execution #7"));
        assert_eq!(outputs["pull_request_url"], pulls[0].html_url);
        assert_eq!(outputs["external_url"], pulls[0].html_url);
        assert!(output.contains("merged into acme/fleet@main"), "{output}");
        // The link was reported while the step was still waiting.
        assert_eq!(
            progress.try_recv().unwrap(),
            ("external_url".to_owned(), pulls[0].html_url.clone())
        );
        // The commit went to the work branch; main was left for the merge.
        let commits = github.commits();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].branch, "sdi/deploy-production-a1b2c3d");
        assert_eq!(
            github
                .file(
                    "acme/fleet",
                    "sdi/deploy-production-a1b2c3d",
                    "apps/api/kustomization.yaml"
                )
                .unwrap(),
            KUSTOMIZATION.replace("9f2c1ab", "a1b2c3d")
        );
        assert_eq!(
            github
                .file("acme/fleet", "main", "apps/api/kustomization.yaml")
                .unwrap(),
            KUSTOMIZATION
        );
        assert!(github.pull_polls.load(Ordering::SeqCst) >= 3);
    }

    #[tokio::test]
    async fn an_unmerged_pull_request_times_out_and_a_retry_reuses_it() {
        let (github, api) = FakeGithub::default()
            .with_file(
                "acme/fleet",
                "main",
                "apps/api/kustomization.yaml",
                KUSTOMIZATION,
            )
            .serve()
            .await;
        let (ctx, _progress) = context(&api);
        let input = step(
            "pull_request",
            "kustomize_image",
            &[("timeout_seconds", "1")],
        );
        let outcome = adapter().run(&input, &ctx).await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a timeout");
        };
        assert_eq!(code, diagnosis::DEPLOYER_TIMEOUT);
        assert!(raw_output.contains("still open after 1s"), "{raw_output}");

        // The retry neither commits again nor opens a second pull request.
        let outcome = adapter().run(&input, &ctx).await;
        assert!(matches!(outcome, StepOutcome::Failed { .. }));
        assert_eq!(github.commits().len(), 1);
        assert_eq!(github.pulls().len(), 1);

        // Closed without a merge is a failure, not a wait.
        let (github, api) = FakeGithub::default()
            .with_file(
                "acme/fleet",
                "main",
                "apps/api/kustomization.yaml",
                KUSTOMIZATION,
            )
            .close_pulls_after(2)
            .serve()
            .await;
        let (ctx, _progress) = context(&api);
        let outcome = adapter()
            .run(&step("pull_request", "kustomize_image", &[]), &ctx)
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(
            raw_output.contains("was closed without being merged"),
            "{raw_output}"
        );
        assert_eq!(github.pulls().len(), 1);
    }

    #[tokio::test]
    async fn conflicts_refusals_and_bad_files_map_to_their_codes() {
        // GitHub answers 409 when the blob moved underneath the write.
        let (github, api) = FakeGithub::default()
            .with_file(
                "acme/fleet",
                "main",
                "apps/api/kustomization.yaml",
                KUSTOMIZATION,
            )
            .conflict_on_write()
            .serve()
            .await;
        let (ctx, _progress) = context(&api);
        let outcome = adapter()
            .run(&step("commit", "kustomize_image", &[]), &ctx)
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a conflict");
        };
        assert_eq!(code, diagnosis::GITOPS_CONFLICT);
        assert!(raw_output.contains("409 Conflict"), "{raw_output}");
        assert!(github.commits().is_empty());

        // A token that may not write.
        let (_, api) = FakeGithub::default()
            .with_file(
                "acme/fleet",
                "main",
                "apps/api/kustomization.yaml",
                KUSTOMIZATION,
            )
            .forbid_writes()
            .serve()
            .await;
        let (ctx, _progress) = context(&api);
        let outcome = adapter()
            .run(&step("commit", "kustomize_image", &[]), &ctx)
            .await;
        let StepOutcome::Failed {
            code,
            missing_permissions,
            raw_output,
        } = outcome
        else {
            panic!("expected a refusal");
        };
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
        assert_eq!(missing_permissions, ["repository:write"]);
        assert!(raw_output.contains("Read and write"), "{raw_output}");

        // A file that is not there, and one without the image.
        let (github, api) = FakeGithub::default()
            .with_file(
                "acme/fleet",
                "main",
                "apps/api/kustomization.yaml",
                "resources:\n  - a.yaml\n",
            )
            .serve()
            .await;
        let (ctx, _progress) = context(&api);
        let outcome = adapter()
            .run(
                &step(
                    "commit",
                    "kustomize_image",
                    &[("path", "apps/missing.yaml")],
                ),
                &ctx,
            )
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(
            raw_output.contains("apps/missing.yaml does not exist on acme/fleet@main"),
            "{raw_output}"
        );
        let outcome = adapter()
            .run(&step("commit", "kustomize_image", &[]), &ctx)
            .await;
        let StepOutcome::Failed {
            code, raw_output, ..
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(code, diagnosis::DEPLOYER_FAILED);
        assert!(raw_output.contains("no images: section"), "{raw_output}");
        assert!(github.commits().is_empty());

        // Without any token nothing is attempted.
        let outcome = adapter()
            .run(
                &step("commit", "kustomize_image", &[]),
                &RunContext::default(),
            )
            .await;
        let StepOutcome::Failed { code, .. } = outcome else {
            panic!("expected a refusal");
        };
        assert_eq!(code, diagnosis::PERMISSION_DENIED);
    }

    #[tokio::test]
    async fn helm_values_and_yaml_paths_are_edited_on_the_configured_branch() {
        let values = "image:\n  repository: ghcr.io/acme/api\n  tag: \"9f2c1ab\"  # current\n";
        let manifest = "spec:\n  template:\n    spec:\n      containers:\n        - name: api\n          image: ghcr.io/acme/api:9f2c1ab\n";
        let (github, api) = FakeGithub::default()
            .with_file(
                "acme/fleet",
                "release",
                "charts/api/values prod.yaml",
                values,
            )
            .with_file(
                "acme/fleet",
                "release",
                "apps/api/deployment.yaml",
                manifest,
            )
            .serve()
            .await;
        let (ctx, _progress) = context(&api);
        let outcome = adapter()
            .run(
                &step(
                    "commit",
                    "helm_values",
                    &[
                        ("branch", "release"),
                        ("path", "charts/api/values prod.yaml"),
                    ],
                ),
                &ctx,
            )
            .await;
        assert!(
            matches!(outcome, StepOutcome::Succeeded { .. }),
            "{outcome:?}"
        );
        assert_eq!(
            github
                .file("acme/fleet", "release", "charts/api/values prod.yaml")
                .unwrap(),
            values.replace("9f2c1ab", "a1b2c3d")
        );

        let outcome = adapter()
            .run(
                &step(
                    "commit",
                    "yaml_path",
                    &[
                        ("branch", "release"),
                        ("path", "apps/api/deployment.yaml"),
                        ("yaml_path", "spec.template.spec.containers[0].image"),
                    ],
                ),
                &ctx,
            )
            .await;
        assert!(
            matches!(outcome, StepOutcome::Succeeded { .. }),
            "{outcome:?}"
        );
        assert_eq!(
            github
                .file("acme/fleet", "release", "apps/api/deployment.yaml")
                .unwrap(),
            manifest.replace("api:9f2c1ab", "api:a1b2c3d")
        );
        // yaml_path without a path is refused before anything is written.
        let outcome = adapter()
            .run(
                &step(
                    "commit",
                    "yaml_path",
                    &[("branch", "release"), ("path", "apps/api/deployment.yaml")],
                ),
                &ctx,
            )
            .await;
        let StepOutcome::Failed { raw_output, .. } = outcome else {
            panic!("expected a failure");
        };
        assert!(raw_output.contains("needs the environment's yaml_path"));
        assert_eq!(github.commits().len(), 2);
    }

    #[test]
    fn branch_names_and_paths_are_made_safe() {
        assert_eq!(
            deploy_branch("production", "a1b2c3d"),
            "sdi/deploy-production-a1b2c3d"
        );
        assert_eq!(
            deploy_branch("EU West", "v1.2.3+build"),
            "sdi/deploy-EU-West-v1.2.3-build"
        );
        assert_eq!(deploy_branch("..prod..", "~x^y:"), "sdi/deploy-prod-x-y");
        assert_eq!(
            encode_path("/apps/api/kustomization.yaml"),
            "apps/api/kustomization.yaml"
        );
        assert_eq!(
            encode_path("charts/my app/values#1.yaml"),
            "charts/my%20app/values%231.yaml"
        );
    }
}
