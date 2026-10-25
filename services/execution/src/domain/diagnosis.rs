//! Turns a failure code into a diagnosis a human can act on, and turns the
//! option they pick back into a plan change. Codes are CONTRACT.md §5.5.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PERMISSION_DENIED: &str = "PERMISSION_DENIED";
pub const POLICY_BLOCKED: &str = "POLICY_BLOCKED";
pub const SKU_UNAVAILABLE: &str = "SKU_UNAVAILABLE";
pub const QUOTA_EXCEEDED: &str = "QUOTA_EXCEEDED";
pub const TIMEOUT: &str = "TIMEOUT";
pub const HEALTH_CHECK_FAILED: &str = "HEALTH_CHECK_FAILED";
pub const BUILD_FAILED: &str = "BUILD_FAILED";
pub const TESTS_FAILED: &str = "TESTS_FAILED";
pub const DEPENDENCY_CONFLICT: &str = "DEPENDENCY_CONFLICT";
/// The registry has no manifest for the release's image (CONTRACT.md §22).
pub const IMAGE_NOT_FOUND: &str = "IMAGE_NOT_FOUND";
/// The environment's deployer ran and reported a failure.
pub const DEPLOYER_FAILED: &str = "DEPLOYER_FAILED";
/// The deployer did not reach a terminal state in time.
pub const DEPLOYER_TIMEOUT: &str = "DEPLOYER_TIMEOUT";
/// The GitOps repository changed underneath the commit.
pub const GITOPS_CONFLICT: &str = "GITOPS_CONFLICT";
/// A pull request's branch is no longer at the commit the plan was made
/// for (CONTRACT.md §23).
pub const HEAD_MOVED: &str = "HEAD_MOVED";
/// GitHub cannot merge the pull request: it conflicts with its base.
pub const MERGE_CONFLICT: &str = "MERGE_CONFLICT";
/// A check of the pull request's head failed or is still running.
pub const CHECKS_FAILING: &str = "CHECKS_FAILING";
/// GitHub will not merge the pull request for a reason other than the
/// three above (a branch protection rule, a draft, a closed pull request).
pub const MERGE_BLOCKED: &str = "MERGE_BLOCKED";
/// The GitOps engine could not render an application's desired state
/// (CONTRACT.md §24.7): nothing was applied.
pub const RENDER_FAILED: &str = "RENDER_FAILED";
/// The GitOps engine, or the cluster it syncs to, could not be reached.
pub const CLUSTER_UNREACHABLE: &str = "CLUSTER_UNREACHABLE";
/// A sync or rollback ran and failed (or could not run because another
/// operation holds the application).
pub const SYNC_FAILED: &str = "SYNC_FAILED";
pub const UNKNOWN: &str = "UNKNOWN";

pub const CODES: [&str; 21] = [
    PERMISSION_DENIED,
    POLICY_BLOCKED,
    SKU_UNAVAILABLE,
    QUOTA_EXCEEDED,
    TIMEOUT,
    HEALTH_CHECK_FAILED,
    BUILD_FAILED,
    TESTS_FAILED,
    DEPENDENCY_CONFLICT,
    IMAGE_NOT_FOUND,
    DEPLOYER_FAILED,
    DEPLOYER_TIMEOUT,
    GITOPS_CONFLICT,
    HEAD_MOVED,
    MERGE_CONFLICT,
    CHECKS_FAILING,
    MERGE_BLOCKED,
    RENDER_FAILED,
    CLUSTER_UNREACHABLE,
    SYNC_FAILED,
    UNKNOWN,
];

/// Remediation that ends a deployment run and starts a rollback run for
/// its environment (CONTRACT.md §22).
pub const ROLLBACK_TO_PREVIOUS: &str = "rollback_to_previous";

/// Normalises a user-supplied code; anything unknown becomes `UNKNOWN`.
pub fn normalize_code(code: &str) -> &'static str {
    let upper = code.trim().to_ascii_uppercase();
    CODES
        .iter()
        .copied()
        .find(|known| *known == upper)
        .unwrap_or(UNKNOWN)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemediationOption {
    pub id: String,
    pub label: String,
    pub description: String,
    pub effect: String,
    pub recommended: bool,
}

/// What is stored in `executions.failure` while paused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureRecord {
    pub step_key: String,
    pub code: String,
    pub message: String,
    pub raw_output: String,
    pub diagnosis: String,
    pub options: Vec<RemediationOption>,
    pub blocked_steps: Vec<String>,
    pub missing_permissions: Vec<String>,
    pub occurred_at: DateTime<Utc>,
}

/// Adapter attempts a step gets before retrying stops being offered
/// (CONTRACT.md §7 safety: never loop forever on the same failure).
pub const MAX_ATTEMPTS: u32 = 5;

/// The failed step as the diagnosis sees it.
pub struct FailedStep<'a> {
    pub title: &'a str,
    pub action_type: &'a str,
    pub params: &'a BTreeMap<String, String>,
    /// What the adapter (or the service) said went wrong, for UNKNOWN.
    pub message: &'a str,
    /// Adapter attempts so far; at `MAX_ATTEMPTS` only give_up/cancel remain.
    pub attempts: u32,
    /// PERMISSION_DENIED raised by the tool itself (GitHub answering 403)
    /// rather than by the pre-flight grant check, which never runs a thing.
    pub refused_by_tool: bool,
    /// First line of what the tool said, when it said anything: the
    /// sentence a diagnosis can quote instead of pointing at the raw output.
    pub detail: &'a str,
    /// The step belongs to a deployment run whose environment has an
    /// earlier successful release to go back to.
    pub can_rollback: bool,
}

/// Whether a step is part of a deployment run's plan (CONTRACT.md §22).
fn is_delivery_action(action_type: &str) -> bool {
    action_type.starts_with("deploy.") || action_type.starts_with("registry.")
}

/// Whether a step drives SDI's GitOps engine (CONTRACT.md §24.7). Its
/// adapter words every failure as a sentence too.
fn is_gitops_action(action_type: &str) -> bool {
    action_type.starts_with("gitops.")
}

/// Whether a step is one of the pull request operations of CONTRACT.md
/// §23. Their adapter words every failure as a sentence, so the diagnosis
/// quotes it instead of pointing at the raw output.
fn is_pull_request_action(action_type: &str) -> bool {
    matches!(
        action_type,
        "repo.commit_fixes"
            | "repo.verify_mergeable"
            | "repo.merge_pull_request"
            | "repo.delete_branch"
            | "repo.enable_security_updates"
    )
}

/// A sentence ending without a period, for embedding in another one.
fn clause(text: &str) -> &str {
    text.trim().trim_end_matches('.')
}

/// `text` as a sentence of its own: capitalised, with a closing period.
fn sentence(text: &str) -> String {
    let mut chars = text.trim().chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut sentence: String = first.to_uppercase().chain(chars).collect();
    if !sentence.ends_with(['.', '!', '?']) {
        sentence.push('.');
    }
    sentence
}

fn option(
    id: &str,
    label: &str,
    description: &str,
    effect: &str,
    recommended: bool,
) -> RemediationOption {
    RemediationOption {
        id: id.to_owned(),
        label: label.to_owned(),
        description: description.to_owned(),
        effect: effect.to_owned(),
        recommended,
    }
}

/// Region the simulated adapter reports when the plan names none.
const DEFAULT_REGION: &str = "westeurope";
const REGIONS: [&str; 4] = [DEFAULT_REGION, "northeurope", "eastus", "westus2"];
const SUPPORTED_SKU: &str = "Standard_B2s";

fn next_region(current: &str) -> &'static str {
    REGIONS
        .iter()
        .copied()
        .find(|r| *r != current)
        .unwrap_or(REGIONS[0])
}

/// The diagnosis sentence and the options for one failure. Exactly one
/// option is recommended; `skip_step` only appears when nothing depends on
/// the step.
pub fn diagnose(
    code: &str,
    step: &FailedStep<'_>,
    blocked_steps: &[String],
    missing_permissions: &[String],
) -> (String, Vec<RemediationOption>) {
    let code = normalize_code(code);
    let region = step
        .params
        .get("region")
        .map(String::as_str)
        .unwrap_or(DEFAULT_REGION);
    let sku = step
        .params
        .get("sku")
        .map(String::as_str)
        .unwrap_or("the requested SKU");
    let can_skip = blocked_steps.is_empty();

    let cancel = option(
        "cancel",
        "Cancel the execution",
        "Stop here. Checkpointed steps stay as they are; nothing else runs.",
        "The execution ends as CANCELLED.",
        false,
    );
    let give_up = |recommended: bool| {
        option(
            "give_up",
            "Give up",
            "Record the run as failed so the pattern's success rate reflects it.",
            "The execution ends as FAILED and the outcome is recorded against the pattern.",
            recommended,
        )
    };
    if step.attempts >= MAX_ATTEMPTS {
        return (
            format!(
                "'{}' failed {} times with {code}; retrying is no longer offered. See the raw output of the last attempt.",
                step.title, step.attempts
            ),
            vec![give_up(true), cancel],
        );
    }
    let skip = option(
        "skip_step",
        "Skip this step",
        "Nothing depends on this step, so the plan can continue without it.",
        "The step is marked SKIPPED and the next ready step runs.",
        false,
    );
    let retry = |recommended: bool| {
        option(
            "retry",
            "Retry the step",
            "Run the same step again with the same parameters.",
            "The step returns to PENDING and runs from the last checkpoint.",
            recommended,
        )
    };

    let refused_by_tool = step.refused_by_tool;
    let delivery = is_delivery_action(step.action_type);
    let needs_admin = missing_permissions
        .iter()
        .any(|p| p == super::policy::REPOSITORY_ADMIN);
    // The pull request codes of §23: the adapter's first line already is
    // a sentence a person can act on (which pull request, which commit,
    // which checks), so it is the diagnosis.
    let told = |fallback: String| {
        if step.detail.is_empty() {
            fallback
        } else {
            sentence(step.detail)
        }
    };
    let rollback = |recommended: bool| {
        option(
            ROLLBACK_TO_PREVIOUS,
            "Roll back to the previous release",
            "Stop this deployment and redeploy the last release that ran successfully in this environment.",
            "This run ends as CANCELLED and a rollback run starts for the environment (it passes the same gates).",
            recommended,
        )
    };
    // The GitOps engine's own steps, and the deploy step of an sdi_gitops
    // environment it runs (`params.operation = gitops.sync`).
    let gitops = is_gitops_action(step.action_type)
        || step.params.get("operation").map(|op| op.trim()) == Some("gitops.sync");
    let detail_clause = |prefix: &str| {
        if step.detail.is_empty() {
            String::new()
        } else {
            format!("{prefix}{}", clause(step.detail))
        }
    };
    let (diagnosis, mut options) = match code {
        // The engine checks roles itself (CONTRACT.md §24.1): the grants
        // in front of the step were in order, so granting more is no fix.
        PERMISSION_DENIED if refused_by_tool && gitops => (
            format!(
                "The GitOps engine refused '{}'{}. The execution's own grants were in order; the engine checks the role of the person the run acts for, who must be an approver of this workspace.",
                step.title,
                detail_clause(": ")
            ),
            vec![option(
                "retry",
                "Retry once the role is fixed",
                "Make sure whoever approved or started the run is an approver of the workspace, then ask the GitOps engine again.",
                "The step returns to PENDING and asks the GitOps engine again.",
                true,
            )],
        ),
        PERMISSION_DENIED => (
            if refused_by_tool && delivery {
                format!(
                    "The tool behind '{}' refused the credentials it was given ({}). See the raw output for what it needs.",
                    step.title,
                    missing_permissions.join(", ")
                )
            } else if refused_by_tool {
                format!(
                    "The tool behind '{}' refused the connected credentials ({}). The configured token lacks the scope for this write; see the raw output for what it needs.",
                    step.title,
                    missing_permissions.join(", ")
                )
            } else {
                format!(
                    "The executing identity lacks {} needed by '{}'. Nothing ran; the check happens before the action.",
                    missing_permissions.join(", "),
                    step.title
                )
            },
            vec![
                option(
                    "request_access",
                    "Request access",
                    if refused_by_tool && delivery {
                        "Fix the credentials the raw output names (a token scope or a workspace secret), then retry."
                    } else if refused_by_tool && needs_admin {
                        "Give the connected GitHub access admin rights on the repository (a token of a repository admin; Administration read and write for the GitHub App), then retry."
                    } else if refused_by_tool {
                        "Re-issue GITHUB_TOKEN with write access to the repository (the `repo` scope, or Contents + Pull requests read and write on a fine-grained token), restart the service, then retry."
                    } else {
                        "Grant the missing permissions to the identity for 30 minutes and retry."
                    },
                    &format!(
                        "Grants {} for 30 minutes, then retries the step.",
                        missing_permissions.join(", ")
                    ),
                    true,
                ),
                option(
                    "use_another_identity",
                    "Use another identity",
                    "Switch to an identity that already holds the required permissions.",
                    "The execution runs as the chosen identity and retries the step.",
                    false,
                ),
            ],
        ),
        POLICY_BLOCKED => (
            format!(
                "Policy marks '{}' ({}) as blocked. It can never run automatically.",
                step.title, step.action_type
            ),
            if can_skip {
                vec![]
            } else {
                vec![option(
                    "skip_branch",
                    "Skip this step and everything that depends on it",
                    &format!(
                        "Leave out '{}' and its {} dependent steps; the rest of the plan continues.",
                        step.title,
                        blocked_steps.len()
                    ),
                    "The step and its dependents are marked SKIPPED.",
                    false,
                )]
            },
        ),
        SKU_UNAVAILABLE => (
            format!(
                "{sku} is not available in {region}. The provider rejected the request before creating anything."
            ),
            vec![
                option(
                    "change_region",
                    "Change region",
                    &format!("Create the resource in {} instead.", next_region(region)),
                    &format!(
                        "Sets params.region = {} and retries the step.",
                        next_region(region)
                    ),
                    true,
                ),
                option(
                    "use_supported_sku",
                    "Use a supported SKU",
                    &format!("Fall back to {SUPPORTED_SKU}, which is available in {region}."),
                    &format!("Sets params.sku = {SUPPORTED_SKU} and retries the step."),
                    false,
                ),
                option(
                    "investigate_alternative",
                    "Investigate an alternative",
                    "Retry after reviewing the provider's capacity notes; no parameters change.",
                    "Retries the step unchanged.",
                    false,
                ),
            ],
        ),
        QUOTA_EXCEEDED => (
            format!(
                "The subscription quota is exhausted for the resource '{}' needs in {region}.",
                step.title
            ),
            vec![
                option(
                    "request_quota",
                    "Request more quota",
                    "File a quota increase with the provider and retry once it is granted.",
                    "Retries the step unchanged (expects the quota to have been raised).",
                    false,
                ),
                option(
                    "reduce_size",
                    "Reduce the size",
                    "Use a smaller size that fits in the remaining quota.",
                    "Sets params.size = small and retries the step.",
                    true,
                ),
            ],
        ),
        TIMEOUT => (
            format!(
                "'{}' did not finish within its time limit. The action may still be in progress on the provider side.",
                step.title
            ),
            vec![
                retry(true),
                option(
                    "increase_timeout",
                    "Increase the timeout",
                    "Double the time limit and retry.",
                    "Doubles params.timeout_seconds and retries the step.",
                    false,
                ),
            ],
        ),
        HEALTH_CHECK_FAILED if gitops => (
            format!(
                "The application behind '{}' did not become healthy{}.",
                step.title,
                detail_clause(": ")
            ),
            if step.can_rollback {
                vec![rollback(true), retry(false)]
            } else {
                vec![retry(true)]
            },
        ),
        // CONTRACT.md §24.7. Nothing reached the cluster in the first two.
        RENDER_FAILED => (
            format!(
                "The GitOps engine could not render the desired state for '{}'{}. Nothing was applied to the cluster.",
                step.title,
                detail_clause(": ")
            ),
            {
                let mut options = vec![option(
                    "retry",
                    "Retry after fixing the source",
                    "Fix the manifests, the kustomization or the chart in the repository (or the image the step sets), then render and sync again.",
                    "The step returns to PENDING and the engine renders the source again.",
                    true,
                )];
                if step.can_rollback {
                    options.push(rollback(false));
                }
                options
            },
        ),
        CLUSTER_UNREACHABLE => (
            format!(
                "'{}' could not reach the GitOps engine or the cluster it deploys to{}. Retrying helps once both answer again; Workspace, Clusters shows the cluster's status.",
                step.title,
                detail_clause(": ")
            ),
            vec![option(
                "retry",
                "Retry when the cluster answers",
                "Run the step again once the GitOps engine and the cluster's API server are reachable.",
                "The step returns to PENDING and asks the GitOps engine again.",
                true,
            )],
        ),
        SYNC_FAILED => (
            format!(
                "The sync behind '{}' ran and failed{}. Resources that were applied before the failure stay applied.",
                step.title,
                detail_clause(": ")
            ),
            if step.can_rollback {
                vec![retry(true), rollback(false)]
            } else {
                vec![retry(true)]
            },
        ),
        HEALTH_CHECK_FAILED if delivery => (
            format!(
                "The release was deployed but '{}' never saw a healthy answer{}.",
                step.title,
                if step.detail.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", clause(step.detail))
                }
            ),
            if step.can_rollback {
                vec![rollback(true), retry(false)]
            } else {
                vec![retry(true)]
            },
        ),
        IMAGE_NOT_FOUND => (
            format!(
                "The registry has no image for this release{}. Nothing was deployed.",
                if step.detail.is_empty() {
                    String::new()
                } else {
                    format!(": {}", clause(step.detail))
                }
            ),
            vec![option(
                "retry",
                "Check the registry again",
                "Look the image up again once the build has pushed it under this name and tag.",
                "The step returns to PENDING and asks the registry again.",
                true,
            )],
        ),
        DEPLOYER_FAILED => (
            format!(
                "The deployer behind '{}' reported a failure{}.",
                step.title,
                if step.detail.is_empty() {
                    "; see the raw output for the failing job and step".to_owned()
                } else {
                    format!(": {}", clause(step.detail))
                }
            ),
            if step.can_rollback {
                vec![retry(true), rollback(false)]
            } else {
                vec![retry(true)]
            },
        ),
        DEPLOYER_TIMEOUT => (
            format!(
                "The deployer behind '{}' did not finish in time{}. It may still be running on its side.",
                step.title,
                if step.detail.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", clause(step.detail))
                }
            ),
            {
                let mut options = vec![
                    retry(true),
                    option(
                        "increase_timeout",
                        "Increase the timeout",
                        "Double the time limit and retry.",
                        "Doubles params.timeout_seconds and retries the step.",
                        false,
                    ),
                ];
                if step.can_rollback {
                    options.push(rollback(false));
                }
                options
            },
        ),
        GITOPS_CONFLICT => (
            format!(
                "The GitOps repository changed while '{}' was writing to it{}. Nothing was overwritten.",
                step.title,
                if step.detail.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", clause(step.detail))
                }
            ),
            {
                let mut options = vec![option(
                    "retry",
                    "Retry on the new state",
                    "Read the file again and apply the image change on top of what is there now.",
                    "The step returns to PENDING and starts from the repository's current state.",
                    true,
                )];
                if step.can_rollback {
                    options.push(rollback(false));
                }
                options
            },
        ),
        // CONTRACT.md §23.3. Nothing was committed or merged in any of
        // the four; none of them offers to skip the step, because a run
        // that skips its commit or its merge must not end as a success.
        HEAD_MOVED => (
            format!(
                "{} This run was planned for the old head, so it cannot simply run again.",
                told(format!(
                    "The pull request's branch is no longer at the commit '{}' was planned for.",
                    step.title
                ))
            ),
            // The plan names the old head, so running it again cannot
            // succeed: the new head has to be looked at first.
            vec![option(
                "give_up",
                "Review the new head and ask again",
                "End this run. Review the pull request's new head, then ask for the fixes or the merge again; that makes a new run for the commit that is there now.",
                "The execution ends as FAILED. Nothing was committed or merged.",
                true,
            )],
        ),
        MERGE_CONFLICT => (
            told(format!(
                "GitHub cannot merge the pull request of '{}': it conflicts with its base branch. Nothing was merged.",
                step.title
            )),
            vec![option(
                "retry",
                "Retry after the branch is updated",
                "Resolve the conflict on GitHub, then check the pull request again. If resolving it adds a commit to the branch, the retry stops with HEAD_MOVED so the new head is reviewed first.",
                "The step returns to PENDING and reads the pull request again.",
                true,
            )],
        ),
        CHECKS_FAILING => (
            told(format!(
                "The checks of the pull request of '{}' are not green: at least one failed or is still running. Nothing was merged.",
                step.title
            )),
            vec![option(
                "retry",
                "Retry when the checks are green",
                "Wait for the running checks, or re-run the failed ones on GitHub, then check the pull request again.",
                "The step returns to PENDING and reads the checks again.",
                true,
            )],
        ),
        MERGE_BLOCKED => (
            told(format!(
                "GitHub will not merge the pull request of '{}' yet: a rule on GitHub is not satisfied. Nothing was merged.",
                step.title
            )),
            vec![option(
                "retry",
                "Retry after the rule is satisfied on GitHub",
                "Do what GitHub asks for (a required review, a draft marked ready, a branch protection rule), then try again.",
                "The step returns to PENDING and asks GitHub again.",
                true,
            )],
        ),
        HEALTH_CHECK_FAILED => (
            format!(
                "'{}' completed but its health check never turned green.",
                step.title
            ),
            vec![
                option(
                    "rollback_to_checkpoint",
                    "Roll back to the last checkpoint",
                    "Undo this step's changes and retry from the last verified state.",
                    "Marks the attempt as rolled back and retries the step.",
                    false,
                ),
                option(
                    "inspect_logs_and_retry",
                    "Inspect logs and retry",
                    "Review the collected output, then run the step again.",
                    "Retries the step unchanged.",
                    true,
                ),
            ],
        ),
        BUILD_FAILED | TESTS_FAILED => (
            if code == BUILD_FAILED {
                format!(
                    "The build in '{}' failed. See the raw output for the first error.",
                    step.title
                )
            } else {
                format!(
                    "Tests in '{}' failed. See the raw output for the failing cases.",
                    step.title
                )
            },
            vec![
                retry(true),
                option(
                    "skip_tests",
                    "Skip tests (not recommended)",
                    "Continue without the failing checks. Only do this when the failure is known to be unrelated.",
                    "Sets params.skip_tests = true and retries the step.",
                    false,
                ),
            ],
        ),
        DEPENDENCY_CONFLICT => (
            format!("'{}' hit conflicting dependency versions.", step.title),
            vec![
                option(
                    "pin_versions",
                    "Pin versions",
                    "Resolve the conflict by pinning the versions from the lockfile.",
                    "Sets params.pin_versions = true and retries the step.",
                    true,
                ),
                retry(false),
            ],
        ),
        _ => (
            if (delivery || gitops || is_pull_request_action(step.action_type))
                && !step.detail.is_empty()
            {
                format!("'{}' failed: {}.", step.title, clause(step.detail))
            } else if step.message.is_empty() {
                format!(
                    "'{}' failed for an unclassified reason. The raw output is attached.",
                    step.title
                )
            } else {
                format!(
                    "'{}' failed: {}.",
                    step.title,
                    step.message.trim_end_matches('.')
                )
            },
            vec![retry(true)],
        ),
    };

    // Skipping a step ends the run as SUCCEEDED without it. That is never
    // offered for the pull request codes, nor, whatever the code, for the
    // step that is the whole point of its run: a run that did not commit
    // the fixes, did not merge, or did not sync or roll back its
    // application must not read as one that did.
    let pull_request_code = matches!(
        code,
        HEAD_MOVED | MERGE_CONFLICT | CHECKS_FAILING | MERGE_BLOCKED
    );
    let is_the_run = matches!(
        step.action_type,
        "repo.commit_fixes" | "repo.merge_pull_request" | "gitops.sync" | "gitops.rollback"
    );
    if can_skip && !pull_request_code && !is_the_run {
        options.push(skip);
    }
    if !options.iter().any(|o| o.id == "give_up") {
        options.push(give_up(false));
    }
    options.push(cancel);
    if !options.iter().any(|o| o.recommended) {
        // POLICY_BLOCKED: the only safe default is to stop.
        if let Some(last) = options.last_mut() {
            last.recommended = true;
        }
    }
    (diagnosis, options)
}

/// What applying a remediation does to the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Retry the failed step after merging `patch` into its params.
    Retry {
        patch: BTreeMap<String, String>,
        note: String,
    },
    /// Grant the missing permissions to the identity, then retry.
    RequestAccess,
    /// Switch to an identity holding the permissions, then retry.
    UseAnotherIdentity,
    Skip,
    /// Skip the step together with the steps it blocked.
    SkipBranch,
    Cancel,
    /// End the run as FAILED and record the outcome against the pattern.
    GiveUp,
    /// End the deployment run as CANCELLED and start a rollback run for its
    /// environment.
    RollbackToPrevious,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ResolveError {
    #[error("option_id or instruction is required")]
    Nothing,
    #[error("unknown option: {0}")]
    UnknownOption(String),
}

/// Maps the chosen option (or a free-text instruction) to a `Resolution`.
/// Options are validated against what the diagnosis offered, so the UI
/// cannot pick `skip_step` where dependents exist.
pub fn resolve(
    option_id: &str,
    instruction: &str,
    failure: &FailureRecord,
    params: &BTreeMap<String, String>,
) -> Result<Resolution, ResolveError> {
    let option_id = option_id.trim();
    let instruction = instruction.trim();
    if option_id.is_empty() {
        if instruction.is_empty() {
            return Err(ResolveError::Nothing);
        }
        return Ok(Resolution::Retry {
            patch: BTreeMap::from([("instruction".to_owned(), instruction.to_owned())]),
            note: format!("instruction: {instruction}"),
        });
    }
    if !failure.options.iter().any(|o| o.id == option_id) {
        return Err(ResolveError::UnknownOption(option_id.to_owned()));
    }

    let patch = |pairs: &[(&str, String)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    };
    let current = |key: &str| params.get(key).cloned().unwrap_or_default();

    Ok(match option_id {
        "cancel" => Resolution::Cancel,
        "give_up" => Resolution::GiveUp,
        "skip_step" => Resolution::Skip,
        "skip_branch" => Resolution::SkipBranch,
        ROLLBACK_TO_PREVIOUS => Resolution::RollbackToPrevious,
        "request_access" => Resolution::RequestAccess,
        "use_another_identity" => Resolution::UseAnotherIdentity,
        "change_region" => {
            let region =
                next_region(params.get("region").map_or(DEFAULT_REGION, String::as_str)).to_owned();
            Resolution::Retry {
                note: format!("region changed to {region}"),
                patch: patch(&[("region", region)]),
            }
        }
        "use_supported_sku" => Resolution::Retry {
            patch: patch(&[("sku", SUPPORTED_SKU.to_owned())]),
            note: format!("sku changed to {SUPPORTED_SKU}"),
        },
        "reduce_size" => Resolution::Retry {
            patch: patch(&[("size", "small".to_owned())]),
            note: "size reduced to small".to_owned(),
        },
        "increase_timeout" => {
            let seconds = current("timeout_seconds").parse::<u64>().unwrap_or(300) * 2;
            Resolution::Retry {
                note: format!("timeout raised to {seconds}s"),
                patch: patch(&[("timeout_seconds", seconds.to_string())]),
            }
        }
        "rollback_to_checkpoint" => Resolution::Retry {
            patch: patch(&[("rollback", "true".to_owned())]),
            note: "rolled back to the last checkpoint".to_owned(),
        },
        "skip_tests" => Resolution::Retry {
            patch: patch(&[("skip_tests", "true".to_owned())]),
            note: "tests skipped".to_owned(),
        },
        "pin_versions" => Resolution::Retry {
            patch: patch(&[("pin_versions", "true".to_owned())]),
            note: "versions pinned".to_owned(),
        },
        _ => Resolution::Retry {
            patch: BTreeMap::new(),
            note: format!("retry ({option_id})"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step<'a>(params: &'a BTreeMap<String, String>) -> FailedStep<'a> {
        FailedStep {
            title: "Create database",
            action_type: "cloud.create_dev_resource",
            params,
            message: "",
            attempts: 1,
            refused_by_tool: false,
            detail: "",
            can_rollback: false,
        }
    }

    fn record(code: &str, options: Vec<RemediationOption>, blocked: &[&str]) -> FailureRecord {
        FailureRecord {
            step_key: "db".into(),
            code: code.into(),
            message: String::new(),
            raw_output: String::new(),
            diagnosis: String::new(),
            options,
            blocked_steps: blocked.iter().map(|s| s.to_string()).collect(),
            missing_permissions: vec![],
            occurred_at: Utc::now(),
        }
    }

    #[test]
    fn every_code_has_at_least_two_options_and_exactly_one_recommendation() {
        let params = BTreeMap::new();
        for code in CODES {
            let (diagnosis, options) =
                diagnose(code, &step(&params), &["x".into()], &["cloud:write".into()]);
            assert!(!diagnosis.is_empty(), "{code}");
            assert!(options.len() >= 2, "{code} has {} options", options.len());
            assert_eq!(
                options.iter().filter(|o| o.recommended).count(),
                1,
                "{code}"
            );
            assert!(options.iter().any(|o| o.id == "cancel"), "{code}");
            assert!(options.iter().any(|o| o.id == "give_up"), "{code}");
            assert!(
                !options.iter().any(|o| o.id == "skip_step"),
                "{code} offers skip with dependents"
            );
        }
    }

    /// CONTRACT.md §22: a failed deploy or health check offers `retry`,
    /// `rollback_to_previous` and `cancel`; the rollback only when the
    /// environment has something to go back to.
    #[test]
    fn delivery_failures_offer_retry_rollback_and_cancel() {
        let params = BTreeMap::new();
        let deploy = |can_rollback: bool| FailedStep {
            title: "Deploy to staging",
            action_type: "deploy.staging",
            detail: "workflow run concluded failure: job deploy, step Apply manifests.",
            can_rollback,
            ..step(&params)
        };
        let blocked = ["verify_health".to_owned(), "record".to_owned()];
        for code in [DEPLOYER_FAILED, DEPLOYER_TIMEOUT, GITOPS_CONFLICT] {
            let (diagnosis, options) = diagnose(code, &deploy(true), &blocked, &[]);
            let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
            for wanted in ["retry", ROLLBACK_TO_PREVIOUS, "cancel", "give_up"] {
                assert!(ids.contains(&wanted), "{code} lacks {wanted}: {ids:?}");
            }
            assert!(!ids.contains(&"skip_step"), "{code}: {ids:?}");
            assert_eq!(
                options.iter().filter(|o| o.recommended).count(),
                1,
                "{code}"
            );
            assert!(
                options
                    .iter()
                    .find(|o| o.id == "retry")
                    .unwrap()
                    .recommended,
                "{code}"
            );
            assert!(diagnosis.contains("Apply manifests"), "{code}: {diagnosis}");
            assert!(!diagnosis.contains(".."), "{code}: {diagnosis}");
            let failure = record(code, options, &["verify_health", "record"]);
            assert_eq!(
                resolve(ROLLBACK_TO_PREVIOUS, "", &failure, &params),
                Ok(Resolution::RollbackToPrevious)
            );
            assert!(matches!(
                resolve("retry", "", &failure, &params),
                Ok(Resolution::Retry { .. })
            ));

            // A first deployment has nothing to go back to.
            let (_, options) = diagnose(code, &deploy(false), &blocked, &[]);
            assert!(
                !options.iter().any(|o| o.id == ROLLBACK_TO_PREVIOUS),
                "{code}"
            );
            let failure = record(code, options, &[]);
            assert_eq!(
                resolve(ROLLBACK_TO_PREVIOUS, "", &failure, &params),
                Err(ResolveError::UnknownOption(ROLLBACK_TO_PREVIOUS.into()))
            );
        }
        let (_, options) = diagnose(DEPLOYER_TIMEOUT, &deploy(true), &blocked, &[]);
        assert!(options.iter().any(|o| o.id == "increase_timeout"));

        // A failed health check after a deploy recommends going back.
        let health = FailedStep {
            title: "Verify health",
            action_type: "deploy.verify_health",
            detail: "GET https://staging.example.com/health -> 503 after 40 polls",
            can_rollback: true,
            ..step(&params)
        };
        let (diagnosis, options) = diagnose(HEALTH_CHECK_FAILED, &health, &["record".into()], &[]);
        assert!(diagnosis.contains("503 after 40 polls"), "{diagnosis}");
        let recommended = options.iter().find(|o| o.recommended).unwrap();
        assert_eq!(recommended.id, ROLLBACK_TO_PREVIOUS);
        assert!(options.iter().any(|o| o.id == "retry"));
        assert!(!options.iter().any(|o| o.id == "rollback_to_checkpoint"));
        // The same code outside a deployment keeps its generic options.
        let (_, options) = diagnose(HEALTH_CHECK_FAILED, &step(&params), &[], &[]);
        assert!(options.iter().any(|o| o.id == "rollback_to_checkpoint"));

        // An image that is not there: nothing changed, so nothing to roll back.
        let image = FailedStep {
            title: "Verify image",
            action_type: "registry.verify_image",
            detail: "ghcr.io/acme/api:a1b2c3d is not in the registry: HEAD /v2/acme/api/manifests/a1b2c3d -> 404 Not Found",
            can_rollback: true,
            ..step(&params)
        };
        let (diagnosis, options) = diagnose(IMAGE_NOT_FOUND, &image, &["deploy".into()], &[]);
        assert!(
            diagnosis.contains("a1b2c3d is not in the registry"),
            "{diagnosis}"
        );
        assert!(diagnosis.contains("Nothing was deployed"), "{diagnosis}");
        let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["retry", "give_up", "cancel"]);

        // The record step has no dependents: it may be skipped.
        let record_step = FailedStep {
            title: "Record deployment",
            action_type: "deploy.record",
            detail: "POST /repos/acme/api/deployments -> 403 Forbidden",
            ..step(&params)
        };
        let (diagnosis, options) = diagnose(UNKNOWN, &record_step, &[], &[]);
        assert_eq!(
            diagnosis,
            "'Record deployment' failed: POST /repos/acme/api/deployments -> 403 Forbidden."
        );
        assert!(options.iter().any(|o| o.id == "skip_step"));
    }

    /// CONTRACT.md §23.3: what each pull request failure offers, in the
    /// contract's words, and never a way to call a run that did not commit
    /// or merge a success.
    #[test]
    fn pull_request_failures_offer_what_the_contract_says() {
        let params = BTreeMap::new();
        let failed = |action_type: &'static str, detail: &'static str| FailedStep {
            title: "Merge #12",
            action_type,
            detail,
            ..step(&params)
        };
        let cases = [
            (
                HEAD_MOVED,
                "pull request #12 is at 9f8e7d6 now; this merge was requested for a1b2c3d. Nothing was merged.",
                vec!["give_up", "cancel"],
                "Review the new head and ask again",
            ),
            (
                MERGE_CONFLICT,
                "pull request #12 conflicts with main: fix/lint cannot be merged into it until the conflicts are resolved. Nothing was merged.",
                vec!["retry", "give_up", "cancel"],
                "Retry after the branch is updated",
            ),
            (
                CHECKS_FAILING,
                "pull request #12 is not ready to merge at a1b2c3d: 1 check failed (unit) and 1 check is still running (e2e). Nothing was merged.",
                vec!["retry", "give_up", "cancel"],
                "Retry when the checks are green",
            ),
            (
                MERGE_BLOCKED,
                "GitHub refused to merge pull request #12: At least 1 approving review is required by reviewers with write access. Nothing was merged.",
                vec!["retry", "give_up", "cancel"],
                "Retry after the rule is satisfied on GitHub",
            ),
        ];
        for (code, detail, ids, label) in cases {
            // Nothing depends on the step: skipping would still be wrong.
            let (diagnosis, options) =
                diagnose(code, &failed("repo.merge_pull_request", detail), &[], &[]);
            assert_eq!(
                options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
                ids,
                "{code}"
            );
            assert_eq!(options[0].label, label, "{code}");
            assert!(options[0].recommended, "{code}");
            assert_eq!(options.iter().filter(|o| o.recommended).count(), 1);
            // The adapter's sentence is the diagnosis, as a sentence.
            assert!(
                diagnosis.starts_with(&sentence(detail)),
                "{code}: {diagnosis}"
            );
            assert!(
                diagnosis.starts_with("Pull request #12") || diagnosis.starts_with("GitHub"),
                "{code}: {diagnosis}"
            );
            assert!(!diagnosis.contains(".."), "{code}: {diagnosis}");
            // Without the tool's words there is still a sentence naming the step.
            let (bare, _) = diagnose(code, &failed("repo.merge_pull_request", ""), &[], &[]);
            assert!(bare.contains("'Merge #12'"), "{code}: {bare}");

            let failure = record(code, options, &[]);
            assert_eq!(
                resolve("cancel", "", &failure, &params),
                Ok(Resolution::Cancel)
            );
            assert_eq!(
                resolve("give_up", "", &failure, &params),
                Ok(Resolution::GiveUp)
            );
            assert_eq!(
                resolve("skip_step", "", &failure, &params),
                Err(ResolveError::UnknownOption("skip_step".into()))
            );
            // The free-text path stays open for every code.
            assert!(matches!(
                resolve(
                    "",
                    "the checks are flaky, run them again",
                    &failure,
                    &params
                ),
                Ok(Resolution::Retry { .. })
            ));
            if code == HEAD_MOVED {
                // "Review the new head and ask again" ends the run: the
                // plan names the old head, so retrying it is not offered.
                assert!(diagnosis.contains("cannot simply run again"), "{diagnosis}");
                assert!(failure.options[0].effect.contains("FAILED"));
                assert_eq!(
                    resolve("retry", "", &failure, &params),
                    Err(ResolveError::UnknownOption("retry".into()))
                );
            } else {
                assert_eq!(
                    resolve("retry", "", &failure, &params),
                    Ok(Resolution::Retry {
                        patch: BTreeMap::new(),
                        note: "retry (retry)".into(),
                    })
                );
            }
        }
        for code in [HEAD_MOVED, MERGE_CONFLICT, CHECKS_FAILING, MERGE_BLOCKED] {
            assert_eq!(normalize_code(&code.to_ascii_lowercase()), code);
        }

        // A plan the adapter refused (code UNKNOWN) is quoted too, instead
        // of "failed with UNKNOWN".
        let refused = FailedStep {
            message: "'Commit fixes' failed with UNKNOWN",
            ..failed(
                "repo.commit_fixes",
                "refusing to commit review fixes to main: it is the default branch of acme/api. No commit was created.",
            )
        };
        let (diagnosis, _) = diagnose(UNKNOWN, &refused, &[], &[]);
        assert_eq!(
            diagnosis,
            "'Merge #12' failed: refusing to commit review fixes to main: it is the default branch of acme/api. No commit was created."
        );

        // Whatever went wrong, the commit and the merge are never skipped:
        // the run would end as a success without them. Leaving a merged
        // branch in place is a fair thing to skip.
        for code in [UNKNOWN, TIMEOUT, PERMISSION_DENIED, QUOTA_EXCEEDED] {
            for action_type in ["repo.commit_fixes", "repo.merge_pull_request"] {
                let (_, options) = diagnose(
                    code,
                    &failed(action_type, ""),
                    &[],
                    &["repository:write".into()],
                );
                assert!(
                    !options.iter().any(|o| o.id == "skip_step"),
                    "{code} on {action_type}"
                );
                assert!(options.len() >= 2, "{code} on {action_type}");
            }
            let (_, options) = diagnose(
                code,
                &failed("repo.delete_branch", ""),
                &[],
                &["repository:write".into()],
            );
            assert!(options.iter().any(|o| o.id == "skip_step"), "{code}");
        }

        // GitHub refusing a repository setting names admin rights, not the
        // write scope.
        let settings = FailedStep {
            refused_by_tool: true,
            ..failed(
                "repo.enable_security_updates",
                "PUT /repos/acme/api/vulnerability-alerts -> 404 Not Found: Not Found",
            )
        };
        let (_, options) = diagnose(
            PERMISSION_DENIED,
            &settings,
            &[],
            &["repository:admin".into()],
        );
        assert_eq!(options[0].id, "request_access");
        assert!(
            options[0].description.contains("admin rights"),
            "{options:?}"
        );
        assert!(!options[0].description.contains("`repo` scope"));
    }

    /// CONTRACT.md §24.7: the GitOps engine's codes offer retry, give up
    /// and cancel (and going back to the previous release inside a
    /// deployment run), quote what the engine said, and never offer to
    /// skip the sync or the rollback, which would end the run as a success
    /// that changed nothing.
    #[test]
    fn gitops_failures_offer_retry_give_up_and_cancel_and_never_skip_the_sync() {
        let params = BTreeMap::new();
        let gitops =
            |action_type: &'static str, detail: &'static str, can_rollback: bool| FailedStep {
                title: "Sync api",
                action_type,
                detail,
                can_rollback,
                ..step(&params)
            };
        let cases = [
            (
                RENDER_FAILED,
                "the sync of application api could not start: its source did not render: kustomize build apps/api: missing file",
                "Nothing was applied to the cluster",
            ),
            (
                CLUSTER_UNREACHABLE,
                "the sync of application api could not reach the GitOps engine at http://127.0.0.1:50055 or its cluster: tcp connect error",
                "Workspace, Clusters",
            ),
            (
                SYNC_FAILED,
                "the sync of application api failed; 1 resource failed: Deployment shop/api (ImagePullBackOff)",
                "stay applied",
            ),
        ];
        for (code, detail, says) in cases {
            for action_type in ["gitops.sync", "gitops.rollback"] {
                // Nothing depends on the step: skipping would still be wrong.
                let (diagnosis, options) =
                    diagnose(code, &gitops(action_type, detail, false), &[], &[]);
                let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
                assert_eq!(ids, ["retry", "give_up", "cancel"], "{code} {action_type}");
                assert!(options[0].recommended);
                assert_eq!(options.iter().filter(|o| o.recommended).count(), 1);
                assert!(diagnosis.contains(clause(detail)), "{code}: {diagnosis}");
                assert!(diagnosis.contains(says), "{code}: {diagnosis}");
                assert!(!diagnosis.contains(".."), "{code}: {diagnosis}");
                let failure = record(code, options, &[]);
                assert_eq!(
                    resolve("skip_step", "", &failure, &params),
                    Err(ResolveError::UnknownOption("skip_step".into()))
                );
                assert!(matches!(
                    resolve("retry", "", &failure, &params),
                    Ok(Resolution::Retry { .. })
                ));
                assert_eq!(
                    resolve("give_up", "", &failure, &params),
                    Ok(Resolution::GiveUp)
                );
                assert_eq!(
                    resolve("cancel", "", &failure, &params),
                    Ok(Resolution::Cancel)
                );
            }
            assert_eq!(normalize_code(&code.to_ascii_lowercase()), code);
        }
        // Inside a deployment run with a release to go back to, a failed
        // sync may also roll the environment back; an unreachable cluster
        // would not take a rollback either.
        let (_, options) = diagnose(
            SYNC_FAILED,
            &gitops("gitops.sync", "boom", true),
            &["verify_health".into()],
            &[],
        );
        let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["retry", ROLLBACK_TO_PREVIOUS, "give_up", "cancel"]);
        let (_, options) = diagnose(
            CLUSTER_UNREACHABLE,
            &gitops("gitops.sync", "boom", true),
            &[],
            &[],
        );
        assert!(!options.iter().any(|o| o.id == ROLLBACK_TO_PREVIOUS));
        // Whatever the code, the sync and the rollback are never skipped.
        for code in [UNKNOWN, TIMEOUT, PERMISSION_DENIED, HEALTH_CHECK_FAILED] {
            for action_type in ["gitops.sync", "gitops.rollback"] {
                let (_, options) = diagnose(
                    code,
                    &gitops(action_type, "", false),
                    &[],
                    &["cluster:write".into()],
                );
                assert!(!options.iter().any(|o| o.id == "skip_step"), "{code}");
            }
        }
        // A health check that never turned green, in a deployment run.
        let health = FailedStep {
            title: "Verify health",
            ..gitops(
                "gitops.verify_health",
                "application api (0192) is degraded; not healthy: Deployment shop/api: degraded",
                true,
            )
        };
        let (diagnosis, options) = diagnose(HEALTH_CHECK_FAILED, &health, &["record".into()], &[]);
        assert!(
            diagnosis.starts_with(
                "The application behind 'Verify health' did not become healthy: application api"
            ),
            "{diagnosis}"
        );
        assert_eq!(options[0].id, ROLLBACK_TO_PREVIOUS);
        assert!(options[0].recommended);
        assert!(!options.iter().any(|o| o.id == "rollback_to_checkpoint"));
        // The engine refusing the caller is a role, not a missing grant.
        let refused = FailedStep {
            refused_by_tool: true,
            ..gitops(
                "gitops.sync",
                "the GitOps engine refused the sync of application api: requires the approver role",
                false,
            )
        };
        let (diagnosis, options) = diagnose(PERMISSION_DENIED, &refused, &[], &[]);
        assert!(
            diagnosis.contains("requires the approver role"),
            "{diagnosis}"
        );
        assert!(diagnosis.contains("must be an approver"), "{diagnosis}");
        let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["retry", "give_up", "cancel"]);
        // The same for the deploy step of an sdi_gitops environment, which
        // keeps its stage's action type and names the engine's operation.
        let operation = BTreeMap::from([("operation".to_owned(), "gitops.sync".to_owned())]);
        let deploy = FailedStep {
            title: "Deploy a1b2c3d to production",
            action_type: "deploy.production",
            params: &operation,
            refused_by_tool: true,
            detail: "the GitOps engine refused the sync of application api: requires the approver role",
            ..step(&params)
        };
        let (diagnosis, _) = diagnose(PERMISSION_DENIED, &deploy, &["verify_health".into()], &[]);
        assert!(diagnosis.contains("must be an approver"), "{diagnosis}");
        // The pre-flight check stays what it is: request a timed grant.
        let (diagnosis, options) = diagnose(
            PERMISSION_DENIED,
            &gitops("gitops.sync", "", false),
            &["verify_health".into()],
            &["cluster:write".into()],
        );
        assert!(diagnosis.contains("lacks cluster:write"), "{diagnosis}");
        assert_eq!(options[0].id, "request_access");
    }

    #[test]
    fn the_retry_cap_leaves_only_give_up_and_cancel() {
        let params = BTreeMap::new();
        let exhausted = FailedStep {
            attempts: MAX_ATTEMPTS,
            ..step(&params)
        };
        for code in CODES {
            let (diagnosis, options) = diagnose(code, &exhausted, &[], &[]);
            assert!(
                diagnosis.contains("no longer offered"),
                "{code}: {diagnosis}"
            );
            let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
            assert_eq!(ids, vec!["give_up", "cancel"], "{code}");
            assert!(options[0].recommended);
            let failure = record(code, options, &[]);
            assert_eq!(
                resolve("give_up", "", &failure, &params),
                Ok(Resolution::GiveUp)
            );
            assert!(resolve("retry", "", &failure, &params).is_err());
        }
        let (_, options) = diagnose(
            TIMEOUT,
            &FailedStep {
                attempts: MAX_ATTEMPTS - 1,
                ..step(&params)
            },
            &[],
            &[],
        );
        assert!(options.iter().any(|o| o.id == "retry"));
    }

    #[test]
    fn a_tool_refusing_the_token_explains_the_scope() {
        let params = BTreeMap::new();
        let refused = FailedStep {
            title: "Open pull request",
            action_type: "repo.open_pull_request",
            attempts: 1,
            refused_by_tool: true,
            ..step(&params)
        };
        let (diagnosis, options) = diagnose(
            PERMISSION_DENIED,
            &refused,
            &[],
            &["repository:write".into()],
        );
        assert!(diagnosis.contains("refused the connected credentials"));
        assert!(options[0].description.contains("GITHUB_TOKEN"));
        let preflight = FailedStep {
            refused_by_tool: false,
            ..refused
        };
        let (diagnosis, _) = diagnose(
            PERMISSION_DENIED,
            &preflight,
            &[],
            &["repository:write".into()],
        );
        assert!(diagnosis.contains("Nothing ran"));
    }

    #[test]
    fn skip_is_offered_only_without_dependents() {
        let params = BTreeMap::new();
        let (_, options) = diagnose(
            PERMISSION_DENIED,
            &step(&params),
            &[],
            &["cloud:write".into()],
        );
        assert!(options.iter().any(|o| o.id == "skip_step"));
        assert_eq!(
            options.iter().find(|o| o.recommended).unwrap().id,
            "request_access"
        );
    }

    #[test]
    fn unknown_codes_fall_back_to_unknown() {
        assert_eq!(normalize_code("nope"), UNKNOWN);
        assert_eq!(normalize_code(" timeout "), TIMEOUT);
    }

    #[test]
    fn options_translate_into_plan_changes() {
        let params = BTreeMap::from([("region".to_string(), "westeurope".to_string())]);
        let (_, options) = diagnose(SKU_UNAVAILABLE, &step(&params), &[], &[]);
        let failure = record(SKU_UNAVAILABLE, options, &[]);

        match resolve("change_region", "", &failure, &params).unwrap() {
            Resolution::Retry { patch, .. } => assert_eq!(patch["region"], "northeurope"),
            other => panic!("{other:?}"),
        }
        // A plan without a region still moves away from the adapter's default.
        let (_, options) = diagnose(SKU_UNAVAILABLE, &step(&BTreeMap::new()), &[], &[]);
        assert!(options[0].description.contains("northeurope"));
        let failure = record(SKU_UNAVAILABLE, options, &[]);
        match resolve("change_region", "", &failure, &BTreeMap::new()).unwrap() {
            Resolution::Retry { patch, .. } => assert_eq!(patch["region"], "northeurope"),
            other => panic!("{other:?}"),
        }
        match resolve("use_supported_sku", "", &failure, &params).unwrap() {
            Resolution::Retry { patch, .. } => assert_eq!(patch["sku"], SUPPORTED_SKU),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            resolve("cancel", "", &failure, &params),
            Ok(Resolution::Cancel)
        );
        assert_eq!(
            resolve("skip_step", "", &failure, &params),
            Ok(Resolution::Skip)
        );
    }

    #[test]
    fn instructions_become_a_retry_with_changed_params() {
        let params = BTreeMap::new();
        let failure = record(TIMEOUT, vec![], &[]);
        match resolve("", "use the cheapest option", &failure, &params).unwrap() {
            Resolution::Retry { patch, .. } => {
                assert_eq!(patch["instruction"], "use the cheapest option")
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            resolve("", "  ", &failure, &params),
            Err(ResolveError::Nothing)
        );
        assert_eq!(
            resolve("skip_step", "", &failure, &params),
            Err(ResolveError::UnknownOption("skip_step".into()))
        );
    }

    #[test]
    fn increase_timeout_doubles_the_limit() {
        let params = BTreeMap::from([("timeout_seconds".to_string(), "120".to_string())]);
        let (_, options) = diagnose(TIMEOUT, &step(&params), &[], &[]);
        let failure = record(TIMEOUT, options, &[]);
        match resolve("increase_timeout", "", &failure, &params).unwrap() {
            Resolution::Retry { patch, .. } => assert_eq!(patch["timeout_seconds"], "240"),
            other => panic!("{other:?}"),
        }
    }
}
