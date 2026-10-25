//! Timed dry run. Sleeps per step, prints an output line that looks like the
//! tool the step names, and fails on purpose when a fault targets the step.

use std::time::Duration;

use async_trait::async_trait;

use super::{Adapter, RunContext, SIMULATED, StepInput, StepOutcome};
use crate::domain::diagnosis;

pub const DEFAULT_DELAY_MS: u64 = 300;
const DELAY_SPREAD_MS: u64 = 600;

pub struct SimulatedAdapter {
    base_delay_ms: u64,
}

impl SimulatedAdapter {
    /// `base_delay_ms` is the floor; each step adds a deterministic jitter of
    /// up to twice that so runs look organic without a random source.
    pub fn new(base_delay_ms: u64) -> Self {
        Self { base_delay_ms }
    }

    fn delay_for(&self, key: &str) -> Duration {
        let spread = if self.base_delay_ms >= DEFAULT_DELAY_MS {
            DELAY_SPREAD_MS
        } else {
            self.base_delay_ms * 2
        };
        let jitter = if spread == 0 {
            0
        } else {
            key.bytes()
                .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)))
                % (spread + 1)
        };
        Duration::from_millis(self.base_delay_ms + jitter)
    }
}

/// A believable success line per tool family.
pub fn success_output(step: &StepInput, elapsed: Duration) -> String {
    let p = |key: &str, default: &str| {
        step.params
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_owned())
    };
    let secs = elapsed.as_secs_f64();
    // The pull request operations of CONTRACT.md §23 change a repository
    // when they are real: the dry run says what it would do and that it
    // did not, and reports no commit, merge or setting as an output.
    let pull_request = || format!("pull request #{}", p("pull_request", "?"));
    match step.action_type.as_str() {
        super::COMMIT_FIXES => {
            return format!(
                "Would commit the review fixes to {} of {} on top of {}\nNothing was committed (dry run, {secs:.1}s)",
                p("branch", "the head branch"),
                pull_request(),
                p("expected_head_sha", "its head")
            );
        }
        super::VERIFY_MERGEABLE => {
            return format!(
                "Would read {} and the checks of {} on GitHub\nNothing was read or verified (dry run, {secs:.1}s)",
                pull_request(),
                p("expected_head_sha", "its head")
            );
        }
        super::MERGE_PULL_REQUEST => {
            return format!(
                "Would merge {} at {} with {}\nNothing was merged (dry run, {secs:.1}s)",
                pull_request(),
                p("expected_head_sha", "its head"),
                p("merge_method", "the repository's merge method")
            );
        }
        super::DELETE_BRANCH => {
            return format!(
                "Would delete {} once {} is merged\nNothing was deleted (dry run, {secs:.1}s)",
                p("branch", "the head branch"),
                pull_request()
            );
        }
        super::ENABLE_SECURITY_UPDATES => {
            return format!(
                "Would turn on Dependabot (alerts: {}, security updates: {})\nNo repository setting was changed (dry run, {secs:.1}s)",
                p("alerts", "false"),
                p("security_updates", "false")
            );
        }
        // CONTRACT.md §24.7: the GitOps engine is not asked anything.
        super::GITOPS_PREVIEW => {
            return format!(
                "Would preview a sync of application {}\nNothing was rendered or compared (dry run, {secs:.1}s)",
                p("application", "?")
            );
        }
        super::GITOPS_SYNC => {
            return format!(
                "Would sync application {} to {}\nNothing was synced (dry run, {secs:.1}s)",
                p("application", "?"),
                p("revision", "its source's revision")
            );
        }
        super::GITOPS_ROLLBACK => {
            return format!(
                "Would roll application {} back to history entry {}\nNothing was rolled back (dry run, {secs:.1}s)",
                p("application", "?"),
                p("history_id", "?")
            );
        }
        super::GITOPS_VERIFY_HEALTH => {
            return format!(
                "Would wait up to {}s for application {} to be healthy\nNothing was read (dry run, {secs:.1}s)",
                p("timeout_seconds", "300"),
                p("application", "?")
            );
        }
        _ => {}
    }
    // The action type says what the step does; the adapter only decides the
    // shape when the action type is not one we know.
    let kind = match step.action_type.as_str() {
        "container.build" => "build",
        "cloud.create_dev_resource" | "cloud.modify_resource" | "db.create_temporary" => "cloud",
        "deploy.staging" | "deploy.production" | "container.run" => "deploy",
        "tests.run_local" => "tests",
        "deps.install" => "deps",
        "repo.read" | "repo.analyze" => "repo",
        "docs.generate" => "docs",
        _ => match step.adapter.as_str() {
            "docker" => "build",
            "cloud" => "cloud",
            "kubernetes" => "deploy",
            "github" => "repo",
            _ => "generic",
        },
    };
    match kind {
        "build" => format!(
            "#1 [internal] load build definition from Dockerfile\n#2 [builder 1/4] FROM {}\n#3 [builder 2/4] COPY . .\n#4 [builder 3/4] RUN build\n#5 [builder 4/4] exporting layers\n#6 naming to {}:{} done\nBuild finished in {secs:.1}s (dry run)",
            p("base_image", "docker.io/library/node:22-alpine"),
            p("image", "registry.local/app"),
            p("tag", "sha-dryrun")
        ),
        "cloud" => {
            format!(
                "Terraform will perform the following actions:\n  + {}.{} will be created\n      region = \"{}\"\n      sku    = \"{}\"\n      size   = \"{}\"\nPlan: 1 to add, 0 to change, 0 to destroy.\nApply complete after {secs:.1}s (dry run)",
                p("resource_type", "azurerm_container_app"),
                step.key,
                p("region", "westeurope"),
                p("sku", "Standard_B2s"),
                p("size", "small")
            )
        }
        "deploy" => {
            format!(
                "deployment.apps/{} configured\nWaiting for rollout... 1/1 replicas available\nGET {} -> 200 OK in 42ms\nHealth check passed after {secs:.1}s (dry run)",
                p("service", &step.key),
                p("health_url", "https://app.staging.local/health")
            )
        }
        "tests" => format!(
            "running {} tests\ntest result: ok. {} passed; 0 failed; 0 ignored in {secs:.1}s (dry run)",
            p("test_count", "128"),
            p("test_count", "128")
        ),
        "deps" => format!(
            "Resolving {} packages...\nLockfile is up to date\nInstalled in {secs:.1}s (dry run)",
            p("package_count", "412")
        ),
        "repo" => format!(
            "GET /repos/{} -> 200\nFetched tree ({} files) in {secs:.1}s (dry run)",
            p("repo", "owner/repo"),
            p("file_count", "238")
        ),
        "docs" => format!(
            "Wrote {} in {secs:.1}s (dry run)",
            p("output", "docs/ARCHITECTURE.md")
        ),
        _ => format!(
            "{} ({}) completed in {secs:.1}s on attempt {} (dry run)",
            step.title, step.action_type, step.attempt
        ),
    }
}

/// Raw tool output for an injected failure, so the diagnosis has something
/// realistic to show.
pub fn failure_output(step: &StepInput, code: &str) -> String {
    let p = |key: &str, default: &str| {
        step.params
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_owned())
    };
    match code {
        diagnosis::SKU_UNAVAILABLE => format!(
            "Error: creating {}: SkuNotAvailable: The requested SKU '{}' is currently not available in location '{}'.",
            step.key,
            p("sku", "Standard_D4s_v5"),
            p("region", "westeurope")
        ),
        diagnosis::QUOTA_EXCEEDED => format!(
            "Error: QuotaExceeded: Operation could not be completed as it results in exceeding approved quota in region '{}'. Current limit: 10, required: 14.",
            p("region", "westeurope")
        ),
        diagnosis::TIMEOUT => format!(
            "Error: timed out after {}s waiting for {} to reach the desired state.",
            p("timeout_seconds", "300"),
            step.key
        ),
        diagnosis::HEALTH_CHECK_FAILED => format!(
            "GET {} -> 503 Service Unavailable (x12)\nError: health check failed after 60s",
            p("health_url", "https://app.staging.local/health")
        ),
        diagnosis::BUILD_FAILED => "#4 [builder 3/4] RUN build\n#4 12.3 error TS2322: Type 'string' is not assignable to type 'number'.\n#4 ERROR: process \"/bin/sh -c build\" did not complete successfully: exit code 2".to_owned(),
        diagnosis::TESTS_FAILED => "test result: FAILED. 126 passed; 2 failed; 0 ignored\n---- api::health_returns_ok stdout ----\nassertion failed: status == 200 (got 500)".to_owned(),
        diagnosis::DEPENDENCY_CONFLICT => "error: failed to select a version for `serde` (conflicting requirements ^1.0.200 and =1.0.188)".to_owned(),
        diagnosis::HEAD_MOVED => format!(
            "pull request #{} is at another commit now; this run was planned for {}. Nothing was committed or merged (injected fault).",
            p("pull_request", "?"),
            p("expected_head_sha", "its earlier head")
        ),
        diagnosis::MERGE_CONFLICT => format!(
            "pull request #{} conflicts with its base branch. Nothing was merged (injected fault).",
            p("pull_request", "?")
        ),
        diagnosis::CHECKS_FAILING => format!(
            "pull request #{} is not ready to merge: 1 check failed (ci) and 1 check is still running (build). Nothing was merged (injected fault).",
            p("pull_request", "?")
        ),
        diagnosis::MERGE_BLOCKED => format!(
            "GitHub refused to merge pull request #{}: at least 1 approving review is required. Nothing was merged (injected fault).",
            p("pull_request", "?")
        ),
        diagnosis::RENDER_FAILED => format!(
            "the sync of application {} could not start: its source did not render: kustomize build: accumulating resources: missing file (injected fault)",
            p("application", "?")
        ),
        diagnosis::CLUSTER_UNREACHABLE => format!(
            "the sync of application {} could not start: the cluster is out of reach: dial tcp 10.0.0.5:6443: i/o timeout (injected fault)",
            p("application", "?")
        ),
        diagnosis::SYNC_FAILED => format!(
            "the sync of application {} failed; 1 resource failed: Deployment api (ImagePullBackOff) (injected fault)",
            p("application", "?")
        ),
        other => format!("{} failed: {other} (injected fault)", step.key),
    }
}

#[async_trait]
impl Adapter for SimulatedAdapter {
    fn name(&self) -> &'static str {
        SIMULATED
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        let delay = self.delay_for(&step.key);
        tokio::time::sleep(delay).await;

        if let Some(fault) = &ctx.fault
            && fault.step_key == step.key
        {
            let code = diagnosis::normalize_code(&fault.code);
            return StepOutcome::failed(code, failure_output(step, code));
        }
        StepOutcome::ok(success_output(step, delay))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::adapters::Fault;

    fn step(key: &str, adapter: &str, action_type: &str) -> StepInput {
        StepInput {
            key: key.into(),
            title: key.into(),
            action_type: action_type.into(),
            adapter: adapter.into(),
            params: BTreeMap::new(),
            attempt: 1,
        }
    }

    #[tokio::test]
    async fn succeeds_with_tool_shaped_output() {
        let adapter = SimulatedAdapter::new(1);
        let out = adapter
            .run(
                &step("build", "docker", "container.build"),
                &RunContext::default(),
            )
            .await;
        match out {
            StepOutcome::Succeeded { output, .. } => assert!(output.contains("Dockerfile")),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn honours_the_fault_for_its_step_only() {
        let adapter = SimulatedAdapter::new(1);
        let ctx = RunContext {
            fault: Some(Fault {
                step_key: "db".into(),
                code: "sku_unavailable".into(),
            }),
            ..Default::default()
        };
        let failed = adapter
            .run(&step("db", "cloud", "cloud.create_dev_resource"), &ctx)
            .await;
        assert!(
            matches!(failed, StepOutcome::Failed { ref code, .. } if code == "SKU_UNAVAILABLE")
        );
        let ok = adapter
            .run(&step("build", "docker", "container.build"), &ctx)
            .await;
        assert!(matches!(ok, StepOutcome::Succeeded { .. }));
    }

    /// The pull request steps of CONTRACT.md §23 are dry-run like every
    /// other: the output says what would happen and that it did not, no
    /// output claims a commit or a merge, and their failure codes can be
    /// rehearsed.
    #[tokio::test]
    async fn pull_request_steps_are_dry_run_and_their_faults_can_be_injected() {
        let adapter = SimulatedAdapter::new(1);
        let with_params = |action_type: &str| StepInput {
            params: [
                ("pull_request", "12"),
                ("branch", "fix/lint"),
                ("expected_head_sha", "a1b2c3d"),
                ("merge_method", "squash"),
                ("alerts", "true"),
            ]
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
            ..step("pr", "github", action_type)
        };
        for (action_type, says) in [
            (
                crate::adapters::COMMIT_FIXES,
                "Nothing was committed (dry run",
            ),
            (
                crate::adapters::VERIFY_MERGEABLE,
                "Nothing was read or verified (dry run",
            ),
            (
                crate::adapters::MERGE_PULL_REQUEST,
                "Nothing was merged (dry run",
            ),
            (
                crate::adapters::DELETE_BRANCH,
                "Nothing was deleted (dry run",
            ),
            (
                crate::adapters::ENABLE_SECURITY_UPDATES,
                "No repository setting was changed (dry run",
            ),
        ] {
            match adapter
                .run(&with_params(action_type), &RunContext::default())
                .await
            {
                StepOutcome::Succeeded { output, outputs } => {
                    assert!(output.contains(says), "{action_type}: {output}");
                    assert!(output.starts_with("Would "), "{action_type}: {output}");
                    assert!(outputs.is_empty(), "{action_type}: {outputs:?}");
                }
                other => panic!("{action_type}: {other:?}"),
            }
        }
        for code in [
            diagnosis::HEAD_MOVED,
            diagnosis::MERGE_CONFLICT,
            diagnosis::CHECKS_FAILING,
            diagnosis::MERGE_BLOCKED,
        ] {
            let ctx = RunContext {
                fault: Some(Fault {
                    step_key: "pr".into(),
                    code: code.to_ascii_lowercase(),
                }),
                ..Default::default()
            };
            match adapter
                .run(&with_params(crate::adapters::MERGE_PULL_REQUEST), &ctx)
                .await
            {
                StepOutcome::Failed {
                    code: failed,
                    raw_output,
                    ..
                } => {
                    assert_eq!(failed, code);
                    assert!(raw_output.contains("#12"), "{raw_output}");
                    assert!(raw_output.contains("injected fault"), "{raw_output}");
                }
                other => panic!("{code}: {other:?}"),
            }
        }
    }

    /// The GitOps steps of CONTRACT.md §24.7 are dry-run too, and their
    /// failure codes can be rehearsed.
    #[tokio::test]
    async fn gitops_steps_are_dry_run_and_their_faults_can_be_injected() {
        let adapter = SimulatedAdapter::new(1);
        let with_params = |action_type: &str| StepInput {
            params: [("application", "app-1"), ("history_id", "h-1")]
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..step("sync", "gitops", action_type)
        };
        for action_type in crate::adapters::GITOPS_ACTIONS {
            match adapter
                .run(&with_params(action_type), &RunContext::default())
                .await
            {
                StepOutcome::Succeeded { output, outputs } => {
                    assert!(output.starts_with("Would "), "{action_type}: {output}");
                    assert!(output.contains("app-1"), "{action_type}: {output}");
                    assert!(output.contains("(dry run"), "{action_type}: {output}");
                    assert!(outputs.is_empty(), "{action_type}: {outputs:?}");
                }
                other => panic!("{action_type}: {other:?}"),
            }
        }
        for code in [
            diagnosis::RENDER_FAILED,
            diagnosis::CLUSTER_UNREACHABLE,
            diagnosis::SYNC_FAILED,
        ] {
            let ctx = RunContext {
                fault: Some(Fault {
                    step_key: "sync".into(),
                    code: code.to_ascii_lowercase(),
                }),
                ..Default::default()
            };
            match adapter
                .run(&with_params(crate::adapters::GITOPS_SYNC), &ctx)
                .await
            {
                StepOutcome::Failed {
                    code: failed,
                    raw_output,
                    ..
                } => {
                    assert_eq!(failed, code);
                    assert!(raw_output.contains("app-1"), "{raw_output}");
                    assert!(raw_output.contains("injected fault"), "{raw_output}");
                }
                other => panic!("{code}: {other:?}"),
            }
        }
    }

    #[test]
    fn delay_stays_inside_the_configured_window() {
        let adapter = SimulatedAdapter::new(DEFAULT_DELAY_MS);
        for key in ["a", "build", "deploy-production", "x"] {
            let d = adapter.delay_for(key).as_millis() as u64;
            assert!(
                (DEFAULT_DELAY_MS..=DEFAULT_DELAY_MS + DELAY_SPREAD_MS).contains(&d),
                "{key}: {d}"
            );
        }
        assert_eq!(SimulatedAdapter::new(0).delay_for("a"), Duration::ZERO);
    }
}
