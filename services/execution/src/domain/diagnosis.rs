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
pub const UNKNOWN: &str = "UNKNOWN";

pub const CODES: [&str; 10] = [
    PERMISSION_DENIED,
    POLICY_BLOCKED,
    SKU_UNAVAILABLE,
    QUOTA_EXCEEDED,
    TIMEOUT,
    HEALTH_CHECK_FAILED,
    BUILD_FAILED,
    TESTS_FAILED,
    DEPENDENCY_CONFLICT,
    UNKNOWN,
];

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
    let (diagnosis, mut options) = match code {
        PERMISSION_DENIED => (
            if refused_by_tool {
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
                    if refused_by_tool {
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
            if step.message.is_empty() {
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

    if can_skip {
        options.push(skip);
    }
    options.push(give_up(false));
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
