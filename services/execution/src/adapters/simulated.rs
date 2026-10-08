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
            let code = diagnosis::normalize_code(&fault.code).to_owned();
            let raw_output = failure_output(step, &code);
            return StepOutcome::Failed { code, raw_output };
        }
        StepOutcome::Succeeded {
            output: success_output(step, delay),
        }
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
            StepOutcome::Succeeded { output } => assert!(output.contains("Dockerfile")),
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
