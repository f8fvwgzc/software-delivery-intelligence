//! Tool adapters (CONTRACT.md §5.4). `simulated` is a timed dry run; `docker`
//! and `github` do the real thing. A run picks a *runtime* (`Execution.adapter`)
//! and the runner routes every step to the *family* its action type needs:
//! `repo.open_pull_request` is GitHub's, everything else the docker family's.
//! The reserved runtimes (`cloud`, `kubernetes`) answer UNIMPLEMENTED so a
//! plan naming them fails loudly instead of silently dry-running.
//!
//! The `delivery` runtime (CONTRACT.md §22) carries deployment runs. Its
//! steps name their tool on the step itself (`PlanStep.adapter`): the image
//! registry, the environment's deployer (GitHub Actions, a GitOps
//! repository, Jenkins, Azure Pipelines, a signed webhook, local Docker),
//! the HTTP health verifier and the GitHub deployment record. Only
//! `Promote` and `Rollback` create such runs.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use tokio::sync::mpsc;
use tonic::Status;
use uuid::Uuid;

pub mod azure_devops;
pub mod docker;
pub mod github;
pub mod gitops;
pub mod http_health;
pub mod jenkins;
pub mod net;
pub mod registry;
pub mod simulated;
pub mod webhook;
pub mod yaml_edit;

pub use docker::DockerAdapter;
pub use github::GithubAdapter;
pub use simulated::SimulatedAdapter;

pub const SIMULATED: &str = "simulated";
pub const DOCKER: &str = "docker";
pub const GITHUB: &str = "github";
/// The runtime of deployment runs (CONTRACT.md §22).
pub const DELIVERY: &str = "delivery";
pub const RESERVED: [&str; 2] = ["cloud", "kubernetes"];

/// Action types of a deployment run's plan (CONTRACT.md §22).
pub const VERIFY_IMAGE: &str = "registry.verify_image";
pub const DEPLOY_DEVELOPMENT: &str = "deploy.development";
pub const DEPLOY_STAGING: &str = "deploy.staging";
pub const DEPLOY_PRODUCTION: &str = "deploy.production";
pub const DEPLOY_ROLLBACK: &str = "deploy.rollback";
pub const VERIFY_HEALTH: &str = "deploy.verify_health";
pub const DEPLOY_RECORD: &str = "deploy.record";
pub const DELIVERY_ACTIONS: [&str; 7] = [
    VERIFY_IMAGE,
    DEPLOY_DEVELOPMENT,
    DEPLOY_STAGING,
    DEPLOY_PRODUCTION,
    DEPLOY_ROLLBACK,
    VERIFY_HEALTH,
    DEPLOY_RECORD,
];

/// Tool families a delivery step names in `PlanStep.adapter`.
pub const TOOL_REGISTRY: &str = "registry";
pub const TOOL_GITHUB: &str = "github";
pub const TOOL_GITOPS: &str = "gitops";
pub const TOOL_JENKINS: &str = "jenkins";
pub const TOOL_AZURE_DEVOPS: &str = "azure_devops";
pub const TOOL_WEBHOOK: &str = "webhook";
pub const TOOL_DOCKER: &str = "docker";
pub const TOOL_HTTP: &str = "http";

/// The step that changes what runs in an environment.
pub fn is_deploy_action(action_type: &str) -> bool {
    matches!(
        action_type,
        DEPLOY_DEVELOPMENT | DEPLOY_STAGING | DEPLOY_PRODUCTION | DEPLOY_ROLLBACK
    )
}

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
/// INVALID_ARGUMENT. `delivery` is a runtime too, but only the delivery
/// module creates runs on it (`CreateExecution` refuses it).
pub fn runtime_name(raw: &str) -> Result<&'static str, Status> {
    match raw.trim() {
        "" | SIMULATED => Ok(SIMULATED),
        DOCKER => Ok(DOCKER),
        GITHUB => Ok(GITHUB),
        DELIVERY => Ok(DELIVERY),
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
        DELIVERY => DELIVERY_ACTIONS.contains(&action_type),
        _ => false,
    }
}

/// Name of the seeded identity a runtime executes as.
pub fn identity_for(runtime: &str) -> &'static str {
    match runtime {
        DOCKER => "local-docker",
        GITHUB => "github-token",
        DELIVERY => DELIVERY,
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

/// Values that must never appear in an event, a step output or a failure:
/// every secret a run resolved and the tokens it authenticates with. The
/// log sink applies it to each line and the runner applies it once more to
/// whatever an adapter returns, so a deployer that echoes a credential back
/// in an error body cannot leak it into the event log.
#[derive(Clone, Default)]
pub struct Redactor(Arc<Mutex<Vec<String>>>);

/// Shorter values are not secrets worth masking, and masking them would
/// shred ordinary text.
const MIN_REDACTED_LEN: usize = 4;
pub const REDACTED: &str = "[redacted]";

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Redactor({} values)", self.lock().len())
    }
}

impl Redactor {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Remembers `value` (and, for a multi-line value such as a PEM key,
    /// each of its lines) as something to mask from now on.
    pub fn protect(&self, value: &str) {
        let mut values = self.lock();
        let mut add = |candidate: &str| {
            let candidate = candidate.trim();
            if candidate.len() >= MIN_REDACTED_LEN && !values.iter().any(|v| v == candidate) {
                values.push(candidate.to_owned());
            }
        };
        add(value);
        if value.contains('\n') {
            for line in value.lines().filter(|line| line.trim().len() >= 16) {
                add(line);
            }
        }
        // Longest first, so a value that contains another is masked whole.
        values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    }

    pub fn redact(&self, text: &str) -> String {
        let values = self.lock();
        if values.is_empty() || text.is_empty() {
            return text.to_owned();
        }
        let mut out = text.to_owned();
        for value in values.iter() {
            if out.contains(value.as_str()) {
                out = out.replace(value.as_str(), REDACTED);
            }
        }
        out
    }
}

/// Where an adapter sends its stdout/stderr lines. The runner turns them
/// into LOG events; a sink without a channel (tests) swallows them. Every
/// line passes the run's [`Redactor`] first.
#[derive(Debug, Clone, Default)]
pub struct LogSink {
    sender: Option<mpsc::Sender<String>>,
    redactor: Redactor,
}

impl LogSink {
    pub fn new(sender: mpsc::Sender<String>) -> Self {
        Self {
            sender: Some(sender),
            redactor: Redactor::default(),
        }
    }

    pub fn with_redactor(mut self, redactor: Redactor) -> Self {
        self.redactor = redactor;
        self
    }

    /// Never blocks the adapter on a slow consumer: a full channel drops the
    /// line (the raw-output tail still has it).
    pub fn line(&self, line: impl Into<String>) {
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(truncate_line(&self.redactor.redact(&line.into())));
        }
    }
}

/// Outputs an adapter reports while its step is still running (the URL of
/// the workflow run it dispatched, the pull request it opened), so a human
/// can follow the deployer before the step ends. The runner merges them
/// into `Execution.outputs` as they arrive; a sink without a channel
/// (tests) swallows them.
#[derive(Debug, Clone, Default)]
pub struct OutputSink(Option<mpsc::Sender<(String, String)>>);

impl OutputSink {
    pub fn new(sender: mpsc::Sender<(String, String)>) -> Self {
        Self(Some(sender))
    }

    pub fn emit(&self, key: &str, value: impl Into<String>) {
        if let Some(sender) = &self.0 {
            let _ = sender.try_send((key.to_owned(), value.into()));
        }
    }
}

/// Why a workspace secret could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    #[error("secret {0} is not set in this workspace")]
    NotFound(String),
    #[error("workspace secrets are unavailable: {0}")]
    Unavailable(String),
}

/// Reads a workspace secret by name at the moment a step needs it
/// (`Identity.ResolveSecret`, CONTRACT.md §22). Values are never stored by
/// this service.
#[async_trait]
pub trait SecretSource: Send + Sync {
    async fn resolve(&self, workspace_id: Uuid, name: &str) -> Result<String, SecretError>;
}

/// The secrets of the workspace a run belongs to. Every value handed out is
/// registered with the run's [`Redactor`] before the adapter sees it.
#[derive(Clone, Default)]
pub struct Secrets {
    source: Option<Arc<dyn SecretSource>>,
    workspace_id: Uuid,
    redactor: Redactor,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secrets")
            .field("workspace_id", &self.workspace_id)
            .field("available", &self.source.is_some())
            .finish_non_exhaustive()
    }
}

impl Secrets {
    pub fn new(source: Arc<dyn SecretSource>, workspace_id: Uuid, redactor: Redactor) -> Self {
        Self {
            source: Some(source),
            workspace_id,
            redactor,
        }
    }

    pub fn redactor(&self) -> &Redactor {
        &self.redactor
    }

    /// The value of the secret `name`. A failure is already the step's
    /// outcome: it names the secret and what to do, never a value.
    pub async fn get(&self, name: &str) -> Result<String, StepOutcome> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StepOutcome::failed(
                crate::domain::diagnosis::UNKNOWN,
                "the deployer needs a secret but its configuration names none",
            ));
        }
        let Some(source) = &self.source else {
            return Err(StepOutcome::failed(
                crate::domain::diagnosis::UNKNOWN,
                format!(
                    "secret {name} cannot be read: workspace secrets are not configured on the execution service"
                ),
            ));
        };
        match source.resolve(self.workspace_id, name).await {
            Ok(value) => {
                self.redactor.protect(&value);
                Ok(value)
            }
            Err(SecretError::NotFound(_)) => Err(StepOutcome::failed(
                crate::domain::diagnosis::UNKNOWN,
                format!(
                    "secret {name} is not set in this workspace: add it under Workspace, Secrets, then retry"
                ),
            )),
            Err(SecretError::Unavailable(reason)) => Err(StepOutcome::failed(
                crate::domain::diagnosis::UNKNOWN,
                format!("secret {name} could not be read: {reason}"),
            )),
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

/// The GitHub credentials a run reads and writes with: the workspace's
/// token and the API base of its host (CONTRACT.md §18), minted by
/// identity per workspace. `None` on the context means the adapter falls
/// back to the process-wide `GITHUB_TOKEN` / `GITHUB_API_URL` (dev mode).
#[derive(Clone, PartialEq, Eq)]
pub struct GithubAccess {
    pub token: String,
    pub api_base_url: String,
}

impl std::fmt::Debug for GithubAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The token never reaches a log line.
        f.debug_struct("GithubAccess")
            .field("api_base_url", &self.api_base_url)
            .finish_non_exhaustive()
    }
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
    /// The workspace's GitHub access, when identity could mint it.
    pub github: Option<GithubAccess>,
    /// Workspace secrets, resolved by name when a step needs them.
    pub secrets: Secrets,
    /// Outputs reported while the step runs.
    pub progress: OutputSink,
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

    /// The same outcome with every protected value masked.
    pub fn redacted(self, redactor: &Redactor) -> Self {
        match self {
            Self::Succeeded { output, outputs } => Self::Succeeded {
                output: redactor.redact(&output),
                outputs: outputs
                    .into_iter()
                    .map(|(key, value)| (key, redactor.redact(&value)))
                    .collect(),
            },
            Self::Failed {
                code,
                raw_output,
                missing_permissions,
            } => Self::Failed {
                code,
                raw_output: redactor.redact(&raw_output),
                missing_permissions,
            },
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

/// The tools only deployment runs use (CONTRACT.md §22).
#[derive(Clone)]
pub struct DeliveryTools {
    pub registry: Arc<dyn Adapter>,
    pub gitops: Arc<dyn Adapter>,
    pub jenkins: Arc<dyn Adapter>,
    pub azure_devops: Arc<dyn Adapter>,
    pub webhook: Arc<dyn Adapter>,
    pub http: Arc<dyn Adapter>,
}

impl DeliveryTools {
    /// The real tools. `github_token` / `github_api` are the process-wide
    /// dev fallback the github adapter uses too; `azure_base` is
    /// `https://dev.azure.com` outside tests.
    pub fn new(
        policy: net::NetPolicy,
        github_token: Option<String>,
        github_api: &str,
        azure_base: &str,
    ) -> Self {
        let net = net::Net::new(policy);
        Self {
            registry: Arc::new(registry::RegistryAdapter::new(
                net.clone(),
                github_token.clone(),
            )),
            gitops: Arc::new(gitops::GitopsAdapter::new(github_token, github_api)),
            jenkins: Arc::new(jenkins::JenkinsAdapter::new(net.clone())),
            azure_devops: Arc::new(azure_devops::AzureDevopsAdapter::new(
                net.clone(),
                azure_base,
            )),
            webhook: Arc::new(webhook::WebhookAdapter::new(net.clone())),
            http: Arc::new(http_health::HttpHealthAdapter::new(net)),
        }
    }
}

/// Resolves the adapter a step runs on.
#[derive(Clone)]
pub struct Registry {
    simulated: Arc<dyn Adapter>,
    docker: Arc<dyn Adapter>,
    github: Arc<dyn Adapter>,
    delivery: Option<DeliveryTools>,
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
            delivery: None,
        }
    }

    pub fn with_delivery(mut self, tools: DeliveryTools) -> Self {
        self.delivery = Some(tools);
        self
    }

    /// The adapter for a step of `action_type` in a run on `runtime`. A
    /// delivery run routes by the `tool` its step names; the others by the
    /// family the action type belongs to.
    pub fn for_step(
        &self,
        runtime: &str,
        action_type: &str,
        tool: &str,
    ) -> Result<Arc<dyn Adapter>, Status> {
        match runtime_name(runtime)? {
            SIMULATED => Ok(self.simulated.clone()),
            DELIVERY => {
                let Some(tools) = &self.delivery else {
                    return Err(Status::failed_precondition(
                        "the delivery adapters are not configured on this execution service",
                    ));
                };
                Ok(match tool.trim() {
                    TOOL_REGISTRY => tools.registry.clone(),
                    TOOL_GITHUB => self.github.clone(),
                    TOOL_GITOPS => tools.gitops.clone(),
                    TOOL_JENKINS => tools.jenkins.clone(),
                    TOOL_AZURE_DEVOPS => tools.azure_devops.clone(),
                    TOOL_WEBHOOK => tools.webhook.clone(),
                    TOOL_DOCKER => self.docker.clone(),
                    TOOL_HTTP => tools.http.clone(),
                    other => {
                        return Err(Status::invalid_argument(format!(
                            "no delivery tool named {other} can run {action_type}"
                        )));
                    }
                })
            }
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
    .with_delivery(DeliveryTools::new(
        net::NetPolicy::ALLOW_PRIVATE,
        None,
        github::DEFAULT_API,
        azure_devops::DEFAULT_BASE,
    ))
}

/// A secret source backed by a map (tests).
#[cfg(test)]
#[derive(Default, Clone)]
pub struct StaticSecrets(pub Arc<Mutex<BTreeMap<String, String>>>);

#[cfg(test)]
impl StaticSecrets {
    pub fn with(pairs: &[(&str, &str)]) -> Self {
        Self(Arc::new(Mutex::new(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        )))
    }

    /// Secrets for a run context, sharing `redactor` with its log sink.
    pub fn secrets(&self, redactor: &Redactor) -> Secrets {
        Secrets::new(Arc::new(self.clone()), Uuid::nil(), redactor.clone())
    }
}

#[cfg(test)]
#[async_trait]
impl SecretSource for StaticSecrets {
    async fn resolve(&self, _: Uuid, name: &str) -> Result<String, SecretError> {
        self.0
            .lock()
            .expect("secrets lock")
            .get(name)
            .cloned()
            .ok_or_else(|| SecretError::NotFound(name.to_owned()))
    }
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
                registry.for_step(SIMULATED, action, "").unwrap().name(),
                SIMULATED
            );
        }
        assert!(runnable(SIMULATED, "deploy.production"));

        // docker: its own family plus a trailing pull request.
        for action in DOCKER_ACTIONS {
            assert!(runnable(DOCKER, action), "{action}");
            assert_eq!(family_of(action), Family::Docker);
            assert_eq!(
                registry.for_step(DOCKER, action, "").unwrap().name(),
                DOCKER
            );
        }
        assert!(runnable(DOCKER, "repo.open_pull_request"));
        assert_eq!(family_of("repo.open_pull_request"), Family::Github);
        assert_eq!(
            registry
                .for_step(DOCKER, "repo.open_pull_request", "")
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
            registry
                .for_step(GITHUB, "repo.checkout", "")
                .unwrap()
                .name(),
            DOCKER
        );
        assert_eq!(
            registry
                .for_step("kubernetes", "repo.read", "")
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
