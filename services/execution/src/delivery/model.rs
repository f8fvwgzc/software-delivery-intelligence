//! The vocabularies of continuous delivery (CONTRACT.md §22) and the pure
//! validation of what an admin or a `.sdi.yml` may put into a registry or an
//! environment. Nothing here touches the database or the network: a value
//! that passes is normalised and ready to store, a value that does not is
//! refused with a sentence that names the field.

use std::collections::BTreeMap;

use sdi_proto::execution::v1::{
    DeploymentStatus, Environment, ExecutionStatus, Registry, StepStatus,
};
use uuid::Uuid;

use crate::adapters::{
    self,
    gitops::{MODE_PULL_REQUEST, MODES, STRATEGIES, STRATEGY_KUSTOMIZE, STRATEGY_YAML_PATH},
    http_health, webhook,
};

pub const REGISTRY_GHCR: &str = "ghcr";
pub const REGISTRY_DOCKERHUB: &str = "dockerhub";
pub const REGISTRY_KINDS: [&str; 6] = [
    REGISTRY_GHCR,
    REGISTRY_DOCKERHUB,
    "acr",
    "ecr",
    "gar",
    "generic",
];

pub const AUTH_GITHUB_TOKEN: &str = "github_token";
pub const AUTH_OIDC: &str = "oidc";
pub const AUTH_SECRET: &str = "secret";
pub const REGISTRY_AUTHS: [&str; 3] = [AUTH_GITHUB_TOKEN, AUTH_OIDC, AUTH_SECRET];

pub const ENV_GITHUB_ACTIONS: &str = "github_actions";
pub const ENV_GITOPS_FLUX: &str = "gitops_flux";
pub const ENV_GITOPS_ARGOCD: &str = "gitops_argocd";
pub const ENV_JENKINS: &str = "jenkins";
pub const ENV_AZURE_DEVOPS: &str = "azure_devops";
pub const ENV_WEBHOOK: &str = "webhook";
pub const ENV_DOCKER_LOCAL: &str = "docker_local";
pub const ENVIRONMENT_KINDS: [&str; 7] = [
    ENV_GITHUB_ACTIONS,
    ENV_GITOPS_FLUX,
    ENV_GITOPS_ARGOCD,
    ENV_JENKINS,
    ENV_AZURE_DEVOPS,
    ENV_WEBHOOK,
    ENV_DOCKER_LOCAL,
];

/// Created in the UI, editable there.
pub const SOURCE_UI: &str = "ui";
/// Declared in `.sdi.yml`; replaced by `SyncEnvironments`, read-only in the UI.
pub const SOURCE_CONFIG: &str = "config";

pub const RELEASE_PIPELINE: &str = "pipeline";
pub const RELEASE_PACKAGE: &str = "package";
pub const RELEASE_MANUAL: &str = "manual";
pub const RELEASE_SOURCES: [&str; 3] = [RELEASE_PIPELINE, RELEASE_PACKAGE, RELEASE_MANUAL];

pub const RUN_PROMOTE: &str = "promote";
pub const RUN_ROLLBACK: &str = "rollback";

/// Step keys of a deployment run's plan.
pub const STEP_VERIFY_IMAGE: &str = "verify_image";
pub const STEP_DEPLOY: &str = "deploy";
pub const STEP_VERIFY_HEALTH: &str = "verify_health";
pub const STEP_RECORD: &str = "record";

/// Config keys every deployer kind accepts on top of its own.
pub const COMMON_CONFIG_KEYS: [&str; 2] = ["timeout_seconds", "health_timeout_seconds"];

/// Longest a deployer may be given to finish (six hours).
pub const MAX_TIMEOUT_SECONDS: u64 = 6 * 60 * 60;

/// `Environment.position` of an `UpsertEnvironment` that does not say where
/// the stage goes: an edit keeps the stage where it is, a new environment
/// is appended after the last one. (A proto3 `uint32` cannot be absent, and
/// 0 is a real position: the first stage.)
pub const POSITION_UNSET: u32 = u32::MAX;
/// The last position an environment may be given.
pub const MAX_POSITION: u32 = 10_000;

/// The config keys of a deployer kind (CONTRACT.md §22 table).
pub fn config_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        ENV_GITHUB_ACTIONS => &["workflow", "ref"],
        // Flux alone can also own image automation; the generators read it.
        ENV_GITOPS_FLUX => &[
            "repo",
            "branch",
            "path",
            "strategy",
            "yaml_path",
            "mode",
            "image_automation",
        ],
        ENV_GITOPS_ARGOCD => &["repo", "branch", "path", "strategy", "yaml_path", "mode"],
        ENV_JENKINS => &["url", "job", "user_secret", "token_secret"],
        ENV_AZURE_DEVOPS => &["organization", "project", "pipeline_id", "pat_secret"],
        ENV_WEBHOOK => &["url", "secret", "timeout_seconds"],
        ENV_DOCKER_LOCAL => &["port", "env"],
        _ => &[],
    }
}

/// The config keys of a kind whose value is the *name* of a workspace secret.
pub fn secret_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        ENV_JENKINS => &["user_secret", "token_secret"],
        ENV_AZURE_DEVOPS => &["pat_secret"],
        ENV_WEBHOOK => &["secret"],
        _ => &[],
    }
}

/// `^[A-Z][A-Z0-9_]{1,63}$`, the identity service's rule for secret names.
pub fn valid_secret_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=64).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
}

/// `registry.example.com[:port]`: no scheme, no path, no credentials.
fn valid_host(host: &str) -> bool {
    let (name, port) = match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (name, Some(port))
        }
        _ => (host, None),
    };
    if port.is_some_and(|p| p.parse::<u16>().is_err()) {
        return false;
    }
    !name.is_empty()
        && name.len() <= 253
        && !name.starts_with(['.', '-'])
        && !name.ends_with(['.', '-'])
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

/// An http(s) URL with a host and no credentials. Where it may point is
/// decided when it is called (the SSRF guard resolves the host then).
pub fn valid_url(raw: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(raw).map_err(|e| format!("is not a valid URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("must be an http or https URL".to_owned());
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("has no host".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("must not carry credentials; reference a workspace secret instead".to_owned());
    }
    Ok(())
}

/// A registry as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryInput {
    pub name: String,
    pub kind: String,
    pub host: String,
    pub namespace: String,
    pub auth: String,
    pub username_secret: String,
    pub password_secret: String,
    pub is_default: bool,
}

/// Where a registry kind lives when the request names no host.
pub fn default_host(kind: &str) -> Option<&'static str> {
    match kind {
        REGISTRY_GHCR => Some("ghcr.io"),
        REGISTRY_DOCKERHUB => Some("docker.io"),
        _ => None,
    }
}

pub fn normalize_registry(registry: &Registry) -> Result<RegistryInput, String> {
    let kind = registry.kind.trim().to_ascii_lowercase();
    if !REGISTRY_KINDS.contains(&kind.as_str()) {
        return Err(format!(
            "kind must be one of {} (got {:?})",
            REGISTRY_KINDS.join(", "),
            registry.kind.trim()
        ));
    }
    let name = registry.name.trim();
    if name.is_empty()
        || name.chars().count() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-'))
    {
        return Err(
            "name is required: up to 64 letters, digits, spaces, dots, dashes or underscores"
                .to_owned(),
        );
    }
    let mut host = registry.host.trim().to_ascii_lowercase();
    for scheme in ["https://", "http://"] {
        if let Some(stripped) = host.strip_prefix(scheme) {
            host = stripped.to_owned();
        }
    }
    let mut host = host.trim_end_matches('/').to_owned();
    if host.is_empty() {
        host = default_host(&kind)
            .ok_or_else(|| format!("host is required for {kind} registries"))?
            .to_owned();
    }
    if !valid_host(&host) {
        return Err(format!(
            "host {host:?} must be a registry host such as ghcr.io or myorg.azurecr.io, without a path"
        ));
    }
    let namespace = registry
        .namespace
        .trim()
        .trim_matches('/')
        .to_ascii_lowercase();
    if namespace.len() > 255
        || !namespace.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-' | b'/')
        })
        || namespace.contains("//")
    {
        return Err(
            "namespace may hold lowercase letters, digits, dots, dashes, underscores and slashes"
                .to_owned(),
        );
    }
    let auth = match registry.auth.trim().to_ascii_lowercase().as_str() {
        "" if kind == REGISTRY_GHCR => AUTH_GITHUB_TOKEN.to_owned(),
        "" => {
            return Err(format!(
                "auth is required for {kind} registries: oidc (reachable from CI only) or secret"
            ));
        }
        auth if REGISTRY_AUTHS.contains(&auth) => auth.to_owned(),
        other => {
            return Err(format!(
                "auth must be one of {} (got {other:?})",
                REGISTRY_AUTHS.join(", ")
            ));
        }
    };
    if auth == AUTH_GITHUB_TOKEN && kind != REGISTRY_GHCR {
        return Err("auth github_token only works for ghcr registries".to_owned());
    }
    // The GitHub token is the workspace's key to GitHub: it is only ever
    // sent to GitHub's own registry (the adapter enforces the same at use
    // time, down to the GitHub Enterprise host the workspace lives on).
    if auth == AUTH_GITHUB_TOKEN && host != "ghcr.io" && !host.starts_with("containers.") {
        return Err(format!(
            "host {host:?} cannot use auth github_token: the GitHub token is only sent to ghcr.io (or containers.<your GitHub Enterprise host>); give this registry auth secret"
        ));
    }
    if auth == AUTH_OIDC && matches!(kind.as_str(), REGISTRY_GHCR | REGISTRY_DOCKERHUB) {
        return Err(format!(
            "auth oidc is for cloud registries (acr, ecr, gar, generic), not {kind}"
        ));
    }
    let username_secret = registry.username_secret.trim().to_owned();
    let password_secret = registry.password_secret.trim().to_owned();
    if auth == AUTH_SECRET {
        for (field, value) in [
            ("username_secret", &username_secret),
            ("password_secret", &password_secret),
        ] {
            if !valid_secret_name(value) {
                return Err(format!(
                    "{field} must name a workspace secret (upper snake case, e.g. REGISTRY_PASSWORD) when auth is secret"
                ));
            }
        }
    } else if !username_secret.is_empty() || !password_secret.is_empty() {
        return Err(format!(
            "username_secret and password_secret only apply to auth secret, not {auth}"
        ));
    }
    Ok(RegistryInput {
        name: name.to_owned(),
        kind,
        host,
        namespace,
        auth,
        username_secret,
        password_secret,
        is_default: registry.is_default,
    })
}

/// An environment as stored (everything but its identity and its project).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentInput {
    pub name: String,
    pub position: i32,
    pub kind: String,
    pub config: BTreeMap<String, String>,
    pub url: String,
    pub health_path: String,
    pub registry_id: Option<Uuid>,
    pub protected: bool,
    pub auto_promote: bool,
}

fn seconds(key: &str, value: &str, max: u64) -> Result<(), String> {
    match value.parse::<u64>() {
        Ok(seconds) if (1..=max).contains(&seconds) => Ok(()),
        _ => Err(format!(
            "config.{key} must be a number of seconds between 1 and {max}"
        )),
    }
}

fn require<'a>(
    config: &'a BTreeMap<String, String>,
    kind: &str,
    key: &str,
) -> Result<&'a str, String> {
    config
        .get(key)
        .map(String::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("config.{key} is required for {kind} environments"))
}

/// Checks the deployer settings of one kind. Unknown keys are refused by
/// name; values are checked for shape, never for existence (whether a
/// secret exists is the caller's question to the identity service). Every
/// message names the offending key as `config.<key>`: the web editor maps
/// the message back to the field.
fn check_config(kind: &str, config: &BTreeMap<String, String>) -> Result<(), String> {
    let allowed = config_keys(kind);
    for key in config.keys() {
        if !allowed.contains(&key.as_str()) && !COMMON_CONFIG_KEYS.contains(&key.as_str()) {
            return Err(format!(
                "config.{key} is not a setting of {kind} environments (allowed: {})",
                allowed
                    .iter()
                    .chain(COMMON_CONFIG_KEYS.iter())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    for key in secret_keys(kind) {
        if let Some(name) = config.get(*key)
            && !valid_secret_name(name)
        {
            return Err(format!(
                "config.{key} must name a workspace secret (upper snake case, e.g. DEPLOY_TOKEN), never hold the value itself"
            ));
        }
    }
    if let Some(value) = config.get("timeout_seconds") {
        let max = if kind == ENV_WEBHOOK {
            webhook::MAX_TIMEOUT_SECONDS
        } else {
            MAX_TIMEOUT_SECONDS
        };
        seconds("timeout_seconds", value, max)?;
    }
    if let Some(value) = config.get("health_timeout_seconds") {
        seconds(
            "health_timeout_seconds",
            value,
            http_health::MAX_TIMEOUT_SECONDS,
        )?;
    }
    let safe_ref = |value: &str| {
        !value.starts_with(['-', '/'])
            && !value.contains("..")
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
    };
    match kind {
        ENV_GITHUB_ACTIONS => {
            if let Some(workflow) = config.get("workflow") {
                let file = workflow.ends_with(".yml") || workflow.ends_with(".yaml");
                let id = workflow.bytes().all(|b| b.is_ascii_digit());
                let plain = workflow
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
                if !(plain && (file || id)) {
                    return Err(
                        "config.workflow must be a workflow file name such as deploy.yml (no path)"
                            .to_owned(),
                    );
                }
            }
            if let Some(git_ref) = config.get("ref")
                && !safe_ref(git_ref)
            {
                return Err("config.ref must be a branch or tag name".to_owned());
            }
        }
        ENV_GITOPS_FLUX | ENV_GITOPS_ARGOCD => {
            if let Some(repo) = config.get("repo") {
                let parts: Vec<&str> = repo.split('/').collect();
                if parts.len() != 2 || parts.iter().any(|p| p.is_empty() || !safe_ref(p)) {
                    return Err("config.repo must be owner/name".to_owned());
                }
            }
            if let Some(branch) = config.get("branch")
                && !safe_ref(branch)
            {
                return Err("config.branch must be a branch name".to_owned());
            }
            let path = require(config, kind, "path")?;
            if path.starts_with('/')
                || path
                    .split('/')
                    .any(|segment| segment == ".." || segment.is_empty())
            {
                return Err(
                    "config.path must be a file path inside the repository (no leading slash, no ..)"
                        .to_owned(),
                );
            }
            let strategy = config
                .get("strategy")
                .map_or(STRATEGY_KUSTOMIZE, String::as_str);
            if !STRATEGIES.contains(&strategy) {
                return Err(format!(
                    "config.strategy must be one of {}",
                    STRATEGIES.join(", ")
                ));
            }
            if strategy == STRATEGY_YAML_PATH {
                require(config, kind, "yaml_path")?;
            }
            let mode = config.get("mode").map_or(MODE_PULL_REQUEST, String::as_str);
            if !MODES.contains(&mode) {
                return Err(format!("config.mode must be one of {}", MODES.join(", ")));
            }
            if let Some(flag) = config.get("image_automation")
                && !matches!(flag.as_str(), "true" | "false")
            {
                return Err("config.image_automation must be true or false".to_string());
            }
        }
        ENV_JENKINS => {
            valid_url(require(config, kind, "url")?).map_err(|e| format!("config.url {e}"))?;
            require(config, kind, "job")?;
            require(config, kind, "user_secret")?;
            require(config, kind, "token_secret")?;
        }
        ENV_AZURE_DEVOPS => {
            require(config, kind, "organization")?;
            require(config, kind, "project")?;
            let pipeline = require(config, kind, "pipeline_id")?;
            if !pipeline.bytes().all(|b| b.is_ascii_digit()) {
                return Err("config.pipeline_id must be the pipeline's numeric id".to_owned());
            }
            require(config, kind, "pat_secret")?;
        }
        ENV_WEBHOOK => {
            valid_url(require(config, kind, "url")?).map_err(|e| format!("config.url {e}"))?;
            require(config, kind, "secret")?;
        }
        ENV_DOCKER_LOCAL => {
            if let Some(port) = config.get("port")
                && !port.parse::<u16>().is_ok_and(|p| p > 0)
            {
                return Err("config.port must be a port number between 1 and 65535".to_owned());
            }
            if let Some(env) = config.get("env") {
                let parsed: Result<BTreeMap<String, serde_json::Value>, _> =
                    serde_json::from_str(env);
                match parsed {
                    Ok(map) => {
                        if let Some(bad) = map.keys().find(|key| {
                            key.is_empty()
                                || key.starts_with(|c: char| c.is_ascii_digit())
                                || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        }) {
                            return Err(format!(
                                "config.env: {bad} is not a valid environment variable name"
                            ));
                        }
                    }
                    Err(_) => {
                        return Err(
                            "config.env must be a JSON object of environment variables".to_owned()
                        );
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Validates and normalises an environment. `position` is whatever the
/// request carries; `SyncEnvironments` overrides it with the list order.
pub fn normalize_environment(environment: &Environment) -> Result<EnvironmentInput, String> {
    let name = environment.name.trim();
    let name_ok = !name.is_empty()
        && name.len() <= 63
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if !name_ok {
        return Err(
            "name is required: up to 63 letters, digits, dots, dashes or underscores, starting with a letter or digit"
                .to_owned(),
        );
    }
    let kind = environment.kind.trim().to_ascii_lowercase();
    if !ENVIRONMENT_KINDS.contains(&kind.as_str()) {
        return Err(format!(
            "kind must be one of {} (got {:?})",
            ENVIRONMENT_KINDS.join(", "),
            environment.kind.trim()
        ));
    }
    let config: BTreeMap<String, String> = environment
        .config
        .iter()
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .filter(|(_, value)| !value.is_empty())
        .collect();
    check_config(&kind, &config)?;

    let url = environment.url.trim().trim_end_matches('/').to_owned();
    if !url.is_empty() {
        valid_url(&url).map_err(|e| format!("url {e}"))?;
    }
    let mut health_path = environment.health_path.trim().to_owned();
    if !health_path.is_empty() {
        if !health_path.starts_with('/') {
            health_path.insert(0, '/');
        }
        if health_path.len() > 200 || health_path.chars().any(char::is_whitespace) {
            return Err("health_path must be a URL path such as /health".to_owned());
        }
    }
    let registry_id = match environment.registry_id.trim() {
        "" => None,
        raw => Some(Uuid::parse_str(raw).map_err(|_| "registry_id is not a valid id".to_owned())?),
    };
    if environment.position != POSITION_UNSET && environment.position > MAX_POSITION {
        return Err(format!("position must be between 0 and {MAX_POSITION}"));
    }
    Ok(EnvironmentInput {
        name: name.to_owned(),
        // `POSITION_UNSET` is resolved by the caller, which knows the stage.
        position: i32::try_from(environment.position.min(MAX_POSITION)).unwrap_or(0),
        kind,
        config,
        url,
        health_path,
        registry_id,
        protected: environment.protected,
        auto_promote: environment.auto_promote,
    })
}

/// The policy key of the deploy step: production for a protected
/// environment, development for an unprotected first stage, staging for
/// every other one; a rollback has its own key (CONTRACT.md §22).
pub fn deploy_action(position: i32, protected: bool, run_kind: &str) -> &'static str {
    if run_kind == RUN_ROLLBACK {
        adapters::DEPLOY_ROLLBACK
    } else if protected {
        adapters::DEPLOY_PRODUCTION
    } else if position <= 0 {
        adapters::DEPLOY_DEVELOPMENT
    } else {
        adapters::DEPLOY_STAGING
    }
}

/// The permission the executing identity needs to change an environment,
/// whichever way (promote or rollback).
pub fn deploy_permission(position: i32, protected: bool) -> &'static str {
    if protected {
        "deploy:production"
    } else if position <= 0 {
        "deploy:development"
    } else {
        "deploy:staging"
    }
}

/// The tool family that runs the deploy step of an environment kind.
pub fn deploy_tool(kind: &str) -> &'static str {
    match kind {
        ENV_GITHUB_ACTIONS => adapters::TOOL_GITHUB,
        ENV_GITOPS_FLUX | ENV_GITOPS_ARGOCD => adapters::TOOL_GITOPS,
        ENV_JENKINS => adapters::TOOL_JENKINS,
        ENV_AZURE_DEVOPS => adapters::TOOL_AZURE_DEVOPS,
        ENV_WEBHOOK => adapters::TOOL_WEBHOOK,
        _ => adapters::TOOL_DOCKER,
    }
}

pub fn status_to_db(status: DeploymentStatus) -> &'static str {
    status
        .as_str_name()
        .strip_prefix("DEPLOYMENT_STATUS_")
        .unwrap_or("UNSPECIFIED")
}

pub fn status_from_db(text: &str) -> DeploymentStatus {
    DeploymentStatus::from_str_name(&format!("DEPLOYMENT_STATUS_{text}")).unwrap_or_default()
}

/// The lowercase contract spelling, for platform events.
pub fn status_text(status: DeploymentStatus) -> String {
    status_to_db(status).to_ascii_lowercase()
}

/// What a deployment run reads as, given its execution: waiting for a
/// human, deploying, verifying once the deploy step is through, or done. A
/// run paused on a failure is FAILED even though a remediation may still
/// revive it; there is no better word in the contract's vocabulary and the
/// execution carries the detail.
pub fn derive_status(
    execution: ExecutionStatus,
    deploy_step: Option<StepStatus>,
) -> DeploymentStatus {
    match execution {
        ExecutionStatus::Planning => DeploymentStatus::Pending,
        ExecutionStatus::WaitingForApproval => DeploymentStatus::AwaitingApproval,
        ExecutionStatus::Succeeded => DeploymentStatus::Succeeded,
        ExecutionStatus::Cancelled => DeploymentStatus::Cancelled,
        ExecutionStatus::Failed | ExecutionStatus::Paused | ExecutionStatus::Diagnosing => {
            DeploymentStatus::Failed
        }
        ExecutionStatus::Authorized
        | ExecutionStatus::Executing
        | ExecutionStatus::Verifying
        | ExecutionStatus::Replanning
        | ExecutionStatus::Unspecified => match deploy_step {
            Some(StepStatus::Succeeded | StepStatus::Skipped) => DeploymentStatus::Verifying,
            _ => DeploymentStatus::Deploying,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn registry(kind: &str, host: &str, auth: &str) -> Registry {
        Registry {
            name: "main".into(),
            kind: kind.into(),
            host: host.into(),
            namespace: "Acme/Platform/".into(),
            auth: auth.into(),
            ..Default::default()
        }
    }

    #[test]
    fn registries_are_normalised_and_their_auth_must_fit_the_kind() {
        let ghcr = normalize_registry(&registry("GHCR", "", "")).unwrap();
        assert_eq!(ghcr.host, "ghcr.io");
        assert_eq!(ghcr.auth, "github_token");
        assert_eq!(ghcr.namespace, "acme/platform");
        let hub = normalize_registry(&Registry {
            username_secret: "HUB_USER".into(),
            password_secret: "HUB_TOKEN".into(),
            ..registry("dockerhub", "", "secret")
        })
        .unwrap();
        assert_eq!(hub.host, "docker.io");
        let acr =
            normalize_registry(&registry("acr", "https://MyOrg.azurecr.io/", "oidc")).unwrap();
        assert_eq!(acr.host, "myorg.azurecr.io");
        assert!(normalize_registry(&registry("generic", "registry.internal:5000", "oidc")).is_ok());

        for (bad, why) in [
            (registry("quay", "quay.io", "secret"), "kind must be one of"),
            (registry("acr", "", "oidc"), "host is required for acr"),
            (
                registry("acr", "myorg.azurecr.io/team", "oidc"),
                "without a path",
            ),
            (
                registry("acr", "user:pw@myorg.azurecr.io", "oidc"),
                "without a path",
            ),
            (registry("acr", "my org.io", "oidc"), "without a path"),
            (
                registry("ecr", "1.dkr.ecr.eu-west-1.amazonaws.com", ""),
                "auth is required for ecr",
            ),
            (
                registry("ecr", "1.dkr.ecr.eu-west-1.amazonaws.com", "token"),
                "auth must be one of",
            ),
            (
                registry("acr", "myorg.azurecr.io", "github_token"),
                "only works for ghcr",
            ),
            (
                registry("ghcr", "registry.evil.example", "github_token"),
                "only sent to ghcr.io",
            ),
            (
                registry("ghcr", "registry.evil.example", ""),
                "only sent to ghcr.io",
            ),
            (registry("ghcr", "", "oidc"), "not ghcr"),
            (registry("dockerhub", "", "oidc"), "not dockerhub"),
            (
                registry("acr", "myorg.azurecr.io", "secret"),
                "username_secret must name a workspace secret",
            ),
        ] {
            let error = normalize_registry(&bad).unwrap_err();
            assert!(error.contains(why), "{bad:?}: {error}");
        }
        let error = normalize_registry(&Registry {
            username_secret: "ACR_USER".into(),
            password_secret: "p4ssw0rd-typed-in-by-mistake".into(),
            ..registry("acr", "myorg.azurecr.io", "secret")
        })
        .unwrap_err();
        assert!(
            error.contains("password_secret must name a workspace secret"),
            "{error}"
        );
        assert!(
            !error.contains("p4ssw0rd"),
            "a mistyped value is never echoed: {error}"
        );
        let error = normalize_registry(&Registry {
            username_secret: "ACR_USER".into(),
            ..registry("acr", "myorg.azurecr.io", "oidc")
        })
        .unwrap_err();
        assert!(error.contains("only apply to auth secret"), "{error}");
        let error = normalize_registry(&Registry {
            name: String::new(),
            ..registry("ghcr", "", "")
        })
        .unwrap_err();
        assert!(error.contains("name is required"), "{error}");
        let error = normalize_registry(&Registry {
            namespace: "acme//x".into(),
            ..registry("ghcr", "", "")
        })
        .unwrap_err();
        assert!(error.contains("namespace"), "{error}");
    }

    fn environment(kind: &str, config: &[(&str, &str)]) -> Environment {
        Environment {
            name: "staging".into(),
            kind: kind.into(),
            config: config
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<HashMap<_, _>>(),
            ..Default::default()
        }
    }

    #[test]
    fn every_kind_accepts_its_own_keys_and_names_an_unknown_one() {
        let valid: [(&str, Vec<(&str, &str)>); 7] = [
            (
                "github_actions",
                vec![("workflow", "deploy.yml"), ("ref", "main")],
            ),
            (
                "gitops_flux",
                vec![
                    ("repo", "acme/fleet"),
                    ("branch", "main"),
                    ("path", "apps/api/kustomization.yaml"),
                    ("strategy", "kustomize_image"),
                    ("mode", "pull_request"),
                ],
            ),
            (
                "gitops_argocd",
                vec![
                    ("path", "apps/api/values.yaml"),
                    ("strategy", "yaml_path"),
                    ("yaml_path", "image.tag"),
                    ("mode", "commit"),
                ],
            ),
            (
                "jenkins",
                vec![
                    ("url", "https://ci.example.com"),
                    ("job", "team/deploy-api"),
                    ("user_secret", "JENKINS_USER"),
                    ("token_secret", "JENKINS_TOKEN"),
                ],
            ),
            (
                "azure_devops",
                vec![
                    ("organization", "acme"),
                    ("project", "Shop Api"),
                    ("pipeline_id", "12"),
                    ("pat_secret", "AZDO_PAT"),
                ],
            ),
            (
                "webhook",
                vec![
                    ("url", "https://deploy.example.com/hook"),
                    ("secret", "DEPLOY_HOOK_SECRET"),
                    ("timeout_seconds", "45"),
                ],
            ),
            (
                "docker_local",
                vec![("port", "8080"), ("env", r#"{"LOG_LEVEL":"debug"}"#)],
            ),
        ];
        for (kind, config) in &valid {
            let input = normalize_environment(&environment(kind, config))
                .unwrap_or_else(|e| panic!("{kind}: {e}"));
            assert_eq!(input.kind, *kind);
            assert_eq!(input.config.len(), config.len(), "{kind}");
            // The common keys are accepted everywhere.
            let mut with_common = config.clone();
            with_common.push(("health_timeout_seconds", "300"));
            assert!(
                normalize_environment(&environment(kind, &with_common)).is_ok(),
                "{kind}"
            );
            // A key of another kind is refused by name.
            let mut with_unknown = config.clone();
            with_unknown.push(("replicas", "3"));
            let error = normalize_environment(&environment(kind, &with_unknown)).unwrap_err();
            assert!(
                error.contains(&format!(
                    "config.replicas is not a setting of {kind} environments"
                )),
                "{kind}: {error}"
            );
        }
        // Keys do not travel between kinds either.
        let error = normalize_environment(&environment(
            "github_actions",
            &[("workflow", "deploy.yml"), ("job", "x")],
        ))
        .unwrap_err();
        assert!(
            error.contains("config.job is not a setting of github_actions"),
            "{error}"
        );
        // Empty values are dropped, not stored.
        let input = normalize_environment(&environment(
            "github_actions",
            &[("workflow", "  "), ("ref", " release ")],
        ))
        .unwrap();
        assert_eq!(
            input.config,
            BTreeMap::from([("ref".to_owned(), "release".to_owned())])
        );
        assert_eq!(secret_keys("jenkins"), ["user_secret", "token_secret"]);
        assert_eq!(secret_keys("webhook"), ["secret"]);
        assert!(secret_keys("github_actions").is_empty());
    }

    #[test]
    fn required_and_malformed_settings_are_refused_with_the_key() {
        type Case = (
            &'static str,
            Vec<(&'static str, &'static str)>,
            &'static str,
        );
        let cases: Vec<Case> = vec![
            ("mystery", vec![], "kind must be one of"),
            (
                "github_actions",
                vec![("workflow", "../deploy.yml")],
                "config.workflow",
            ),
            (
                "github_actions",
                vec![("workflow", "deploy")],
                "config.workflow",
            ),
            ("github_actions", vec![("ref", "main..x")], "config.ref"),
            ("gitops_flux", vec![], "config.path is required"),
            (
                "gitops_flux",
                vec![("path", "/abs.yaml")],
                "config.path must be",
            ),
            (
                "gitops_flux",
                vec![("path", "a/../b.yaml")],
                "config.path must be",
            ),
            (
                "gitops_flux",
                vec![("path", "k.yaml"), ("repo", "fleet")],
                "config.repo must be owner/name",
            ),
            (
                "gitops_flux",
                vec![("path", "k.yaml"), ("strategy", "sed")],
                "config.strategy must be one of",
            ),
            (
                "gitops_flux",
                vec![("path", "k.yaml"), ("strategy", "yaml_path")],
                "config.yaml_path is required",
            ),
            (
                "gitops_flux",
                vec![("path", "k.yaml"), ("mode", "push")],
                "config.mode must be one of",
            ),
            (
                "jenkins",
                vec![
                    ("job", "x"),
                    ("user_secret", "A_B"),
                    ("token_secret", "C_D"),
                ],
                "config.url is required",
            ),
            (
                "jenkins",
                vec![
                    ("url", "ftp://ci"),
                    ("job", "x"),
                    ("user_secret", "A_B"),
                    ("token_secret", "C_D"),
                ],
                "config.url must be an http or https URL",
            ),
            (
                "jenkins",
                vec![
                    ("url", "https://u:p@ci.example.com"),
                    ("job", "x"),
                    ("user_secret", "A_B"),
                    ("token_secret", "C_D"),
                ],
                "must not carry credentials",
            ),
            (
                "jenkins",
                vec![
                    ("url", "https://ci.example.com"),
                    ("job", "x"),
                    ("user_secret", "A_B"),
                    ("token_secret", "11aa22bb33"),
                ],
                "config.token_secret must name a workspace secret",
            ),
            (
                "azure_devops",
                vec![
                    ("organization", "acme"),
                    ("project", "p"),
                    ("pipeline_id", "deploy"),
                    ("pat_secret", "AZDO_PAT"),
                ],
                "config.pipeline_id must be",
            ),
            (
                "azure_devops",
                vec![
                    ("organization", "acme"),
                    ("project", "p"),
                    ("pipeline_id", "1"),
                ],
                "config.pat_secret is required",
            ),
            (
                "webhook",
                vec![("url", "https://d.example.com")],
                "config.secret is required",
            ),
            (
                "webhook",
                vec![
                    ("url", "https://d.example.com"),
                    ("secret", "HOOK_KEY"),
                    ("timeout_seconds", "601"),
                ],
                "config.timeout_seconds must be a number of seconds between 1 and 600",
            ),
            ("docker_local", vec![("port", "0")], "config.port must be"),
            (
                "docker_local",
                vec![("env", "[1]")],
                "config.env must be a JSON object",
            ),
            (
                "docker_local",
                vec![("env", r#"{"1BAD":"x"}"#)],
                "1BAD is not a valid environment variable name",
            ),
            (
                "docker_local",
                vec![("health_timeout_seconds", "0")],
                "config.health_timeout_seconds",
            ),
        ];
        for (kind, config, why) in cases {
            let error = normalize_environment(&environment(kind, &config)).unwrap_err();
            assert!(error.contains(why), "{kind} {config:?}: {error}");
        }
        // A token typed where a secret name belongs is refused without echo.
        let error = normalize_environment(&environment(
            "webhook",
            &[
                ("url", "https://d.example.com"),
                ("secret", "whsec_live_9f8e7d"),
            ],
        ))
        .unwrap_err();
        assert!(!error.contains("whsec_live"), "{error}");
    }

    #[test]
    fn names_urls_and_health_paths_are_checked_and_tidied() {
        let mut env = environment("docker_local", &[]);
        env.url = " https://staging.example.com/ ".into();
        env.health_path = "health".into();
        env.registry_id = String::new();
        let input = normalize_environment(&env).unwrap();
        assert_eq!(input.url, "https://staging.example.com");
        assert_eq!(input.health_path, "/health");
        assert_eq!(input.registry_id, None);
        for bad in ["", " ", "-prod", "my env", "a/b", &"x".repeat(64)] {
            env.name = bad.into();
            assert!(normalize_environment(&env).is_err(), "{bad:?}");
        }
        env.name = "eu-west.prod_2".into();
        assert!(normalize_environment(&env).is_ok());
        env.url = "staging.example.com".into();
        assert!(normalize_environment(&env).unwrap_err().starts_with("url "));
        env.url = String::new();
        env.health_path = "/a b".into();
        assert!(normalize_environment(&env).is_err());
        env.health_path = String::new();
        env.registry_id = "nope".into();
        assert!(
            normalize_environment(&env)
                .unwrap_err()
                .contains("registry_id")
        );
    }

    #[test]
    fn deploy_actions_follow_stage_and_protection() {
        assert_eq!(deploy_action(0, false, RUN_PROMOTE), "deploy.development");
        assert_eq!(deploy_action(1, false, RUN_PROMOTE), "deploy.staging");
        assert_eq!(deploy_action(4, false, RUN_PROMOTE), "deploy.staging");
        assert_eq!(deploy_action(0, true, RUN_PROMOTE), "deploy.production");
        assert_eq!(deploy_action(2, true, RUN_PROMOTE), "deploy.production");
        assert_eq!(deploy_action(0, false, RUN_ROLLBACK), "deploy.rollback");
        assert_eq!(deploy_action(2, true, RUN_ROLLBACK), "deploy.rollback");
        assert_eq!(deploy_permission(0, false), "deploy:development");
        assert_eq!(deploy_permission(1, false), "deploy:staging");
        assert_eq!(deploy_permission(1, true), "deploy:production");
        for kind in ENVIRONMENT_KINDS {
            assert!(!deploy_tool(kind).is_empty());
            assert!(!config_keys(kind).is_empty(), "{kind}");
        }
        assert_eq!(deploy_tool("github_actions"), "github");
        assert_eq!(deploy_tool("gitops_argocd"), "gitops");
        assert_eq!(deploy_tool("docker_local"), "docker");
    }

    #[test]
    fn run_status_is_derived_from_the_execution_and_the_deploy_step() {
        use DeploymentStatus as D;
        use ExecutionStatus as E;
        use StepStatus as S;
        assert_eq!(derive_status(E::Planning, None), D::Pending);
        assert_eq!(
            derive_status(E::WaitingForApproval, Some(S::WaitingApproval)),
            D::AwaitingApproval
        );
        assert_eq!(derive_status(E::Authorized, Some(S::Pending)), D::Deploying);
        assert_eq!(derive_status(E::Executing, Some(S::Running)), D::Deploying);
        assert_eq!(
            derive_status(E::Verifying, Some(S::Verifying)),
            D::Deploying
        );
        // Once the deploy step is through, the rest is verification.
        assert_eq!(
            derive_status(E::Authorized, Some(S::Succeeded)),
            D::Verifying
        );
        assert_eq!(
            derive_status(E::Executing, Some(S::Succeeded)),
            D::Verifying
        );
        assert_eq!(derive_status(E::Executing, Some(S::Skipped)), D::Verifying);
        assert_eq!(derive_status(E::Paused, Some(S::Failed)), D::Failed);
        assert_eq!(derive_status(E::Diagnosing, Some(S::Succeeded)), D::Failed);
        assert_eq!(derive_status(E::Replanning, Some(S::Pending)), D::Deploying);
        assert_eq!(derive_status(E::Failed, Some(S::Failed)), D::Failed);
        assert_eq!(
            derive_status(E::Cancelled, Some(S::Cancelled)),
            D::Cancelled
        );
        assert_eq!(
            derive_status(E::Succeeded, Some(S::Succeeded)),
            D::Succeeded
        );
        for status in [
            D::Pending,
            D::AwaitingApproval,
            D::Deploying,
            D::Verifying,
            D::Succeeded,
            D::Failed,
            D::Cancelled,
        ] {
            assert_eq!(status_from_db(status_to_db(status)), status);
        }
        assert_eq!(status_to_db(D::AwaitingApproval), "AWAITING_APPROVAL");
        assert_eq!(status_text(D::AwaitingApproval), "awaiting_approval");
        assert_eq!(status_from_db("garbage"), D::Unspecified);
    }

    #[test]
    fn secret_names_follow_the_identity_rule() {
        for good in ["AB", "JENKINS_TOKEN", "A1_"] {
            assert!(valid_secret_name(good), "{good}");
        }
        for bad in ["", "A", "a_b", "1A", "A-B", "A B"] {
            assert!(!valid_secret_name(bad), "{bad}");
        }
    }
}
