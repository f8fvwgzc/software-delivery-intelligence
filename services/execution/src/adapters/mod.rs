//! Tool adapters (CONTRACT.md §5.4). `simulated` is a timed dry run; `docker`
//! and `github` do the real thing. A run picks a *runtime* (`Execution.adapter`)
//! and the runner routes every step to the *family* its action type needs:
//! `repo.open_pull_request` is GitHub's, everything else the docker family's.
//! The reserved runtimes (`cloud`, `kubernetes`) answer UNIMPLEMENTED so a
//! plan naming them fails loudly instead of silently dry-running.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use tokio::sync::mpsc;
use tonic::Status;
use uuid::Uuid;

pub mod docker;
pub mod github;
pub mod simulated;

pub use docker::DockerAdapter;
pub use github::GithubAdapter;
pub use simulated::SimulatedAdapter;

pub const SIMULATED: &str = "simulated";
pub const DOCKER: &str = "docker";
pub const GITHUB: &str = "github";
pub const RESERVED: [&str; 2] = ["cloud", "kubernetes"];

/// Action types the docker family runs (CONTRACT.md §5.4).
pub const DOCKER_ACTIONS: [&str; 7] = [
    "repo.checkout",
    "container.build",
    "container.run",
    "tests.run_local",
    "env.create_temporary",
    "env.destroy_temporary",
    "deps.install",
];

/// Action types the github family runs. `repo.checkout` is listed because a
/// github run may download the tree first; it is still executed by the
/// docker family (a tarball download needs no daemon).
pub const GITHUB_ACTIONS: [&str; 2] = ["repo.checkout", "repo.open_pull_request"];

/// Longest log line the runner stores; longer lines are cut, not dropped.
pub const MAX_LOG_LINE_BYTES: usize = 2048;

/// Which family a step's action type belongs to on a non-simulated runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Docker,
    Github,
}

pub fn family_of(action_type: &str) -> Family {
    if action_type == "repo.open_pull_request" {
        Family::Github
    } else {
        Family::Docker
    }
}

/// Normalises the runtime named on a request: empty means `simulated`,
/// reserved names are refused with UNIMPLEMENTED, unknown ones with
/// INVALID_ARGUMENT.
pub fn runtime_name(raw: &str) -> Result<&'static str, Status> {
    match raw.trim() {
        "" | SIMULATED => Ok(SIMULATED),
        DOCKER => Ok(DOCKER),
        GITHUB => Ok(GITHUB),
        reserved if RESERVED.contains(&reserved) => Err(Status::unimplemented(format!(
            "adapter {reserved} is reserved and not implemented yet; use {SIMULATED}, {DOCKER} or {GITHUB}"
        ))),
        other => Err(Status::invalid_argument(format!(
            "unknown adapter: {other}"
        ))),
    }
}

/// Whether `runtime` can run a step of `action_type`. The simulated runtime
/// runs everything; `docker` runs its own family plus the github family so a
/// plan can build locally and then open a pull request (§5.4 "a plan may mix
/// families"); `github` only writes to the repository.
pub fn runnable(runtime: &str, action_type: &str) -> bool {
    match runtime {
        SIMULATED => true,
        DOCKER => DOCKER_ACTIONS.contains(&action_type) || GITHUB_ACTIONS.contains(&action_type),
        GITHUB => GITHUB_ACTIONS.contains(&action_type),
        _ => false,
    }
}

/// Name of the seeded identity a runtime executes as.
pub fn identity_for(runtime: &str) -> &'static str {
    match runtime {
        DOCKER => "local-docker",
        GITHUB => "github-token",
        _ => SIMULATED,
    }
}

/// What the adapter needs to know about the step it runs.
#[derive(Debug, Clone)]
pub struct StepInput {
    pub key: String,
    pub title: String,
    pub action_type: String,
    /// Adapter the plan named for this step ("docker", "cloud", ...). The
    /// simulated adapter uses it to pick a believable output shape.
    pub adapter: String,
    pub params: BTreeMap<String, String>,
    pub attempt: u32,
}

/// Where an adapter sends its stdout/stderr lines. The runner turns them
/// into LOG events; a sink without a channel (tests) swallows them.
#[derive(Debug, Clone, Default)]
pub struct LogSink(Option<mpsc::Sender<String>>);

impl LogSink {
    pub fn new(sender: mpsc::Sender<String>) -> Self {
        Self(Some(sender))
    }

    /// Never blocks the adapter on a slow consumer: a full channel drops the
    /// line (the raw-output tail still has it).
    pub fn line(&self, line: impl Into<String>) {
        if let Some(sender) = &self.0 {
            let _ = sender.try_send(truncate_line(&line.into()));
        }
    }
}

/// Cuts a line to `MAX_LOG_LINE_BYTES` on a char boundary, marking the cut.
pub fn truncate_line(line: &str) -> String {
    if line.len() <= MAX_LOG_LINE_BYTES {
        return line.to_owned();
    }
    let mut end = MAX_LOG_LINE_BYTES - 3;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &line[..end])
}

/// Per-run context: who the run is, what it already produced, what a pull
/// request must commit, and the dry-run fault to honour.
#[derive(Debug, Clone, Default)]
pub struct RunContext {
    pub execution_id: Uuid,
    pub execution_number: i64,
    pub proposal_id: String,
    pub fault: Option<Fault>,
    /// Outputs recorded by earlier steps ("image", "container", ...).
    pub outputs: BTreeMap<String, String>,
    /// Files a `repo.open_pull_request` step commits, keyed by repository path.
    pub artifacts: BTreeMap<String, String>,
    pub logs: LogSink,
}

impl RunContext {
    pub fn short_id(&self) -> String {
        short_id(self.execution_id)
    }
}

/// Last eight hex digits of the execution id: what container, image and
/// branch names carry so a human can match them to the run. The id is a
/// v7 UUID whose first digits are the timestamp, identical for every run
/// created in the same minute; the tail is the random part, so two runs
/// racing on the same daemon do not share a container name.
pub fn short_id(execution_id: Uuid) -> String {
    let simple = execution_id.simple().to_string();
    simple[simple.len() - 8..].to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fault {
    pub step_key: String,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    Succeeded {
        output: String,
        /// Merged into `Execution.outputs` once the step is checkpointed.
        outputs: BTreeMap<String, String>,
    },
    Failed {
        code: String,
        raw_output: String,
        /// For PERMISSION_DENIED raised by the tool itself (a 403 from
        /// GitHub), so the remediation knows what to grant.
        missing_permissions: Vec<String>,
    },
}

impl StepOutcome {
    pub fn ok(output: impl Into<String>) -> Self {
        Self::Succeeded {
            output: output.into(),
            outputs: BTreeMap::new(),
        }
    }

    pub fn failed(code: &str, raw_output: impl Into<String>) -> Self {
        Self::Failed {
            code: code.to_owned(),
            raw_output: raw_output.into(),
            missing_permissions: Vec::new(),
        }
    }
}

#[async_trait]
pub trait Adapter: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome;
    /// Stops whatever a run left running (containers) and keeps its files,
    /// so an interrupted step can be retried. Called on startup recovery.
    /// Best effort.
    async fn stop(&self, _execution_id: Uuid) {}
    /// Releases everything a run left behind (containers, workdirs). Called
    /// on cancel, give-up and success. Best effort.
    async fn cleanup(&self, _execution_id: Uuid) {}
}

/// Resolves the adapter a step runs on.
#[derive(Clone)]
pub struct Registry {
    simulated: Arc<dyn Adapter>,
    docker: Arc<dyn Adapter>,
    github: Arc<dyn Adapter>,
}

impl Registry {
    pub fn new(
        simulated: Arc<dyn Adapter>,
        docker: Arc<dyn Adapter>,
        github: Arc<dyn Adapter>,
    ) -> Self {
        Self {
            simulated,
            docker,
            github,
        }
    }

    /// The adapter for a step of `action_type` in a run on `runtime`.
    pub fn for_step(&self, runtime: &str, action_type: &str) -> Result<Arc<dyn Adapter>, Status> {
        match runtime_name(runtime)? {
            SIMULATED => Ok(self.simulated.clone()),
            _ => Ok(match family_of(action_type) {
                Family::Docker => self.docker.clone(),
                Family::Github => self.github.clone(),
            }),
        }
    }

    /// Every real adapter, for cleanup on cancel and recovery.
    pub fn real(&self) -> [Arc<dyn Adapter>; 2] {
        [self.docker.clone(), self.github.clone()]
    }
}

#[cfg(test)]
pub(crate) fn test_registry() -> Registry {
    let dir = std::env::temp_dir().join("sdi-execution-tests");
    Registry::new(
        Arc::new(SimulatedAdapter::new(1)),
        Arc::new(DockerAdapter::new(dir, None, github::DEFAULT_API)),
        Arc::new(GithubAdapter::new(None, github::DEFAULT_API)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_names_are_normalised_and_reserved_ones_refused() {
        assert_eq!(runtime_name("").unwrap(), SIMULATED);
        assert_eq!(runtime_name(" docker ").unwrap(), DOCKER);
        assert_eq!(runtime_name("github").unwrap(), GITHUB);
        for name in RESERVED {
            assert_eq!(
                runtime_name(name).unwrap_err().code(),
                tonic::Code::Unimplemented
            );
        }
        assert_eq!(
            runtime_name("mystery").unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn routing_table_follows_the_contract() {
        let registry = test_registry();
        // simulated runs everything on the simulated adapter.
        for action in DOCKER_ACTIONS.iter().chain(&GITHUB_ACTIONS) {
            assert!(runnable(SIMULATED, action));
            assert_eq!(
                registry.for_step(SIMULATED, action).unwrap().name(),
                SIMULATED
            );
        }
        assert!(runnable(SIMULATED, "deploy.production"));

        // docker: its own family plus a trailing pull request.
        for action in DOCKER_ACTIONS {
            assert!(runnable(DOCKER, action), "{action}");
            assert_eq!(family_of(action), Family::Docker);
            assert_eq!(registry.for_step(DOCKER, action).unwrap().name(), DOCKER);
        }
        assert!(runnable(DOCKER, "repo.open_pull_request"));
        assert_eq!(family_of("repo.open_pull_request"), Family::Github);
        assert_eq!(
            registry
                .for_step(DOCKER, "repo.open_pull_request")
                .unwrap()
                .name(),
            GITHUB
        );
        assert!(!runnable(DOCKER, "deploy.production"));
        assert!(!runnable(DOCKER, "cloud.create_dev_resource"));

        // github: repository writes only, checkout still goes to docker.
        assert!(runnable(GITHUB, "repo.open_pull_request"));
        assert!(runnable(GITHUB, "repo.checkout"));
        assert!(!runnable(GITHUB, "container.build"));
        assert_eq!(
            registry.for_step(GITHUB, "repo.checkout").unwrap().name(),
            DOCKER
        );
        assert_eq!(
            registry
                .for_step("kubernetes", "repo.read")
                .err()
                .expect("reserved")
                .code(),
            tonic::Code::Unimplemented
        );
        assert!(!runnable("cloud", "repo.read"));
    }

    #[test]
    fn identities_follow_the_runtime() {
        assert_eq!(identity_for(SIMULATED), "simulated");
        assert_eq!(identity_for(DOCKER), "local-docker");
        assert_eq!(identity_for(GITHUB), "github-token");
    }

    #[test]
    fn long_lines_are_cut_on_a_char_boundary() {
        let short = "x".repeat(MAX_LOG_LINE_BYTES);
        assert_eq!(truncate_line(&short), short);
        let long = format!("{}é{}", "a".repeat(MAX_LOG_LINE_BYTES - 4), "b".repeat(50));
        let cut = truncate_line(&long);
        assert!(cut.len() <= MAX_LOG_LINE_BYTES);
        assert!(cut.ends_with("..."));
        assert!(cut.starts_with("aaa"));
    }

    #[test]
    fn short_ids_come_from_the_random_tail_not_the_timestamp() {
        let ctx = RunContext {
            execution_id: Uuid::parse_str("0192f6a0-1234-7000-8000-0000abcdef01").unwrap(),
            ..Default::default()
        };
        assert_eq!(ctx.short_id(), "abcdef01");
        // Two ids minted in the same millisecond share their prefix, never
        // their short id.
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        assert_ne!(short_id(a), short_id(b));
    }
}
