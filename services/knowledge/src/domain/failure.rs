//! Root-cause heuristics for failed CI runs and environment inference for
//! deploy workflows. Names are the only signal GitHub gives without logs.

/// Summary and suggested fix for a failed job step, from its name alone.
pub fn diagnose(job: &str, step: &str) -> (String, String) {
    let needle = format!("{job} {step}").to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| needle.contains(w));

    let (summary, fix) = if has(&["fmt", "format", "prettier", "lint", "clippy", "eslint"]) {
        (
            "Formatting or lint check failed",
            "Run the formatter and linter locally and commit the result.",
        )
    } else if has(&["test", "spec", "jest", "vitest", "pytest"]) {
        (
            "Test suite failed",
            "Run the failing tests locally against the same commit; check for flaky or environment-dependent tests.",
        )
    } else if has(&["typecheck", "tsc", "type check", "mypy"]) {
        (
            "Type check failed",
            "Run the type checker locally and fix the reported errors.",
        )
    } else if has(&[
        "install",
        "npm ci",
        "pnpm install",
        "yarn",
        "cargo fetch",
        "pip install",
        "dependencies",
    ]) {
        (
            "Dependency installation failed",
            "Check the lock file is committed and in sync with the manifest; retry if a registry was unreachable.",
        )
    } else if has(&["docker", "image", "buildx", "push"]) {
        (
            "Container image build or push failed",
            "Build the image locally with the same Dockerfile; verify registry credentials and tags.",
        )
    } else if has(&["deploy", "publish", "terraform", "apply", "helm", "kubectl"]) {
        (
            "Deployment step failed",
            "Inspect the deployment target's logs and credentials; verify the environment secrets are present.",
        )
    } else if has(&["build", "compile", "cargo", "make", "gradle", "mvn"]) {
        (
            "Build failed",
            "Reproduce the build locally on a clean checkout; the compiler output names the file.",
        )
    } else if has(&["checkout", "setup", "cache"]) {
        (
            "Workflow setup step failed",
            "Usually transient; re-run the workflow and check action versions if it persists.",
        )
    } else {
        (
            "Step failed",
            "Open the run logs for this step; the first error line is the cause.",
        )
    };
    (summary.to_owned(), fix.to_owned())
}

/// Whether a workflow run looks like a deployment, from its name, path and trigger.
pub fn is_deploy_workflow(name: &str, path: &str, event: &str) -> bool {
    let needle = format!("{name} {path}").to_lowercase();
    event == "deployment"
        || ["deploy", "release", "publish", "cd."]
            .iter()
            .any(|w| needle.contains(w))
}

/// Environment a deploy workflow targets, from its name.
pub fn environment_for_workflow(name: &str) -> String {
    let lower = name.to_lowercase();
    for (needle, environment) in [
        ("prod", "production"),
        ("staging", "staging"),
        ("stage", "staging"),
        ("preview", "preview"),
        ("dev", "development"),
        ("test", "test"),
        ("pages", "github-pages"),
    ] {
        if lower.contains(needle) {
            return environment.to_owned();
        }
    }
    "production".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_steps() {
        assert!(diagnose("ci", "Run cargo clippy").0.contains("lint"));
        assert!(diagnose("test", "pnpm test").0.contains("Test"));
        assert!(
            diagnose("docker", "Build and push image")
                .0
                .contains("image")
        );
        assert!(
            diagnose("build", "cargo build --release")
                .0
                .contains("Build")
        );
        assert_eq!(diagnose("x", "y").0, "Step failed");
    }

    #[test]
    fn deploy_detection_and_environment() {
        assert!(is_deploy_workflow(
            "Deploy to Azure",
            ".github/workflows/deploy.yml",
            "push"
        ));
        assert!(is_deploy_workflow("CI", "ci.yml", "deployment"));
        assert!(!is_deploy_workflow(
            "CI",
            ".github/workflows/ci.yml",
            "push"
        ));
        assert_eq!(environment_for_workflow("Deploy staging"), "staging");
        assert_eq!(environment_for_workflow("Deploy"), "production");
        assert_eq!(
            environment_for_workflow("pages build and deployment"),
            "github-pages"
        );
    }
}
