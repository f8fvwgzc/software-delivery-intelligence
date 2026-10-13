//! `GetRepositorySecurity` (CONTRACT.md §23.4): Dependabot's state for a
//! repository and its open alerts, read with the workspace's GitHub access.
//!
//! The call answers whatever GitHub does. A token that may not read alerts,
//! alerts that are switched off, an outage: each is a status and a sentence
//! (`alerts_status`, `alerts_message`), never an error, because the caller
//! (the intelligence service) falls back to OSV on anything but `ok` and
//! must be told why. Only an unknown project is an error.
//!
//! Answers are kept for five minutes per `(workspace, project)` in a
//! bounded in-memory cache; `refresh` asks GitHub again.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use chrono::Utc;
use sdi_analyzer::{RepoSnapshot, TreeEntry};
use sdi_common::{
    cache::TtlCache,
    grpc::db_status,
    time::{opt_to_proto, to_proto},
};
use sdi_proto::knowledge::v1::*;
use tonic::Status;
use uuid::Uuid;

use super::{KnowledgeState, pull_requests::repo_parts};
use crate::{
    auth::Caller,
    db::{self, dependencies::DependabotRow, projects::ProjectRow},
    domain::security,
    github::{Alert, AlertListing, GithubClient, GithubError, MAX_ALERTS, Pull, SecurityFixes},
};

/// How long an answer is served before GitHub is asked again.
pub const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
/// An answer that says GitHub could not be reached describes a moment, not
/// the repository; it is kept only this long.
pub const UNAVAILABLE_TTL: Duration = Duration::from_secs(30);
/// Projects whose answer is held at once. One answer is at most 300 alerts
/// of a few hundred bytes each.
pub const CACHE_CAPACITY: usize = 512;
/// Open pull requests looked through for Dependabot's (most recently
/// updated first). A repository with more open than this has its older
/// Dependabot pull requests left out of the count and of the fix links.
pub const MAX_OPEN_PULLS: usize = 300;

/// Where Dependabot's configuration may live, in the order GitHub prefers.
const CONFIG_PATHS: [&str; 2] = [".github/dependabot.yml", ".github/dependabot.yaml"];

const STATUS_OK: &str = "ok";
const STATUS_DISABLED: &str = "disabled";
const STATUS_FORBIDDEN: &str = "forbidden";
const STATUS_UNAVAILABLE: &str = "unavailable";

const DISABLED_MESSAGE: &str = "Dependabot alerts are turned off for this repository.";

/// The five-minute memory of `GetRepositorySecurity`.
pub struct SecurityCache {
    answers: Mutex<TtlCache<(Uuid, Uuid), (Instant, RepositorySecurity)>>,
}

impl Default for SecurityCache {
    fn default() -> Self {
        Self {
            answers: Mutex::new(TtlCache::new(CACHE_CAPACITY, CACHE_TTL)),
        }
    }
}

impl SecurityCache {
    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, TtlCache<(Uuid, Uuid), (Instant, RepositorySecurity)>> {
        self.answers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn get(&self, key: (Uuid, Uuid)) -> Option<RepositorySecurity> {
        self.lock()
            .get_if(&key, |(at, answer)| {
                answer.alerts_status != STATUS_UNAVAILABLE || at.elapsed() < UNAVAILABLE_TTL
            })
            .map(|(_, answer)| answer.clone())
    }

    fn put(&self, key: (Uuid, Uuid), answer: RepositorySecurity) {
        self.lock().insert(key, (Instant::now(), answer));
    }

    /// Forgets one project's answer (it was disconnected).
    pub fn forget_project(&self, workspace_id: Uuid, project_id: Uuid) {
        self.lock().remove(&(workspace_id, project_id));
    }

    /// Forgets every answer of the workspace (its data was deleted).
    pub fn forget_workspace(&self, workspace_id: Uuid) {
        self.lock().retain(|(ws, _), _| *ws != workspace_id);
    }

    /// Answers held right now (diagnostics, tests).
    pub fn held(&self) -> usize {
        self.lock().len()
    }
}

pub(super) async fn get_repository_security(
    state: &KnowledgeState,
    caller: &Caller,
    req: GetRepositorySecurityRequest,
) -> Result<RepositorySecurity, Status> {
    let ws = caller.workspace_id;
    let project = state.project(caller, &req.project_id).await?;
    let key = (ws, project.id);
    if !req.refresh
        && let Some(cached) = state.security.get(key)
    {
        return Ok(cached);
    }
    // What the last scan saw: the answer for whatever GitHub will not say now.
    let scanned = db::dependencies::get_dependabot(&state.pool, ws, project.id)
        .await
        .map_err(db_status)?;
    let (owner, name) = repo_parts(&project)?;
    let answer = match state.github(caller).await {
        Ok(github) => read(&github, &project, owner, name, scanned).await,
        // No GitHub access at all (identity down, nothing installed): the
        // same kind of answer as an outage, in the words of the cause.
        Err(status) => without_github(&project, scanned, status.message()),
    };
    state.security.put(key, answer.clone());
    Ok(answer)
}

/// Everything GitHub says about the repository's Dependabot, asked at once.
async fn read(
    github: &GithubClient,
    project: &ProjectRow,
    owner: &str,
    name: &str,
    scanned: Option<DependabotRow>,
) -> RepositorySecurity {
    let repository = project.repo_full_name.as_str();
    let (pulls, alerts_switch, fixes, listing, config) = tokio::join!(
        github.pulls_listing(owner, name, "open", MAX_OPEN_PULLS),
        github.vulnerability_alerts(owner, name),
        github.security_fixes(owner, name),
        github.dependabot_alerts(owner, name, MAX_ALERTS),
        live_config(github, owner, name),
    );

    // Dependabot's open pull requests, when the repository could be read.
    let dependabot_pulls: Option<Vec<&Pull>> = match &pulls {
        Ok(listing) => Some(
            listing
                .pulls
                .iter()
                .filter(|pull| pull.user.as_ref().is_some_and(|user| user.is_dependabot()))
                .collect(),
        ),
        Err(error) => {
            tracing::debug!(%error, repository, "open pull requests not read for the security answer");
            None
        }
    };

    // The configuration file. A `404` only means "no such file" when the
    // repository itself answered; a repository this token cannot see says
    // `404` to everything, and then the last scan knows better.
    let config = match config {
        Ok(Some(found)) => Some(found),
        Ok(None) if dependabot_pulls.is_some() => Some(Config::default()),
        Ok(None) => None,
        Err(error) => {
            tracing::debug!(%error, repository, "Dependabot configuration not read from GitHub");
            None
        }
    };
    let config = config.unwrap_or_else(|| Config::scanned(scanned.as_ref()));

    // Same caveat for the alerts switch: its `404` means "off" only when
    // the repository is one this token can see at all.
    let switch = alerts_word(&alerts_switch, dependabot_pulls.is_some());
    let (alerts_status, alerts_message) =
        classify(&listing, switch, repository, &github.access().token);
    // The list itself is the better witness of the switch: alerts that were
    // listed are on, and GitHub saying "disabled" means off, whatever a
    // token without admin rights was told by the settings endpoint.
    let alerts = match alerts_status {
        STATUS_OK => "enabled",
        STATUS_DISABLED => "disabled",
        _ => switch,
    };

    let branches: Vec<(u64, &str)> = dependabot_pulls
        .iter()
        .flatten()
        .filter_map(|pull| Some((pull.number, pull.head.as_ref()?.reference.as_str())))
        .collect();
    let AlertListing {
        alerts: found,
        truncated,
    } = listing.unwrap_or_default();
    let mut open_alerts: Vec<DependabotAlert> = found
        .into_iter()
        .map(|alert| dependabot_alert(alert, &branches))
        .collect();
    open_alerts.sort_by(|a, b| security::alert_order(sort_key(a), sort_key(b)));

    let open_pull_requests = match &dependabot_pulls {
        Some(pulls) => pulls.len() as u32,
        None => scanned
            .as_ref()
            .map_or(0, |row| row.open_pull_requests.max(0) as u32),
    };
    RepositorySecurity {
        project_id: project.id.to_string(),
        config_present: config.present,
        config_path: config.path,
        config_ecosystems: config.ecosystems,
        alerts: alerts.to_owned(),
        security_updates: security_updates_word(&fixes).to_owned(),
        alerts_status: alerts_status.to_owned(),
        alerts_message,
        open_alerts,
        truncated,
        open_pull_requests,
        checked_at: Some(to_proto(Utc::now())),
    }
}

/// The answer when the workspace has no GitHub access to ask with.
fn without_github(
    project: &ProjectRow,
    scanned: Option<DependabotRow>,
    reason: &str,
) -> RepositorySecurity {
    let config = Config::scanned(scanned.as_ref());
    RepositorySecurity {
        project_id: project.id.to_string(),
        config_present: config.present,
        config_path: config.path,
        config_ecosystems: config.ecosystems,
        alerts: "unknown".to_owned(),
        security_updates: "unknown".to_owned(),
        alerts_status: STATUS_UNAVAILABLE.to_owned(),
        alerts_message: format!(
            "Dependabot alerts of {} could not be read: {}.",
            project.repo_full_name,
            reason.trim().trim_end_matches('.')
        ),
        open_alerts: Vec::new(),
        truncated: false,
        open_pull_requests: scanned.map_or(0, |row| row.open_pull_requests.max(0) as u32),
        checked_at: Some(to_proto(Utc::now())),
    }
}

/// Whether a Dependabot configuration exists and what it names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Config {
    present: bool,
    path: String,
    ecosystems: Vec<String>,
}

impl Config {
    /// What the last scan stored (`ListDependencies` reports the same row).
    fn scanned(row: Option<&DependabotRow>) -> Self {
        row.map(|row| Self {
            present: row.configured,
            path: row.config_path.clone(),
            ecosystems: distinct(row.ecosystems.iter().cloned()),
        })
        .unwrap_or_default()
    }
}

/// The configuration on the default branch right now: `Some` with the file
/// that exists, `None` when neither spelling does.
async fn live_config(
    github: &GithubClient,
    owner: &str,
    name: &str,
) -> Result<Option<Config>, GithubError> {
    for path in CONFIG_PATHS {
        if let Some(content) = github.default_branch_file(owner, name, path).await? {
            return Ok(Some(Config {
                present: true,
                path: path.to_owned(),
                ecosystems: config_ecosystems(&content),
            }));
        }
    }
    Ok(None)
}

/// The `package-ecosystem` values of a Dependabot configuration, in file
/// order, each once. Read by the scan's own detector (the analyzer run on
/// a snapshot holding just this file), so this answer and the one
/// `ListDependencies` gives after a scan can never parse the file two ways.
fn config_ecosystems(content: &str) -> Vec<String> {
    // The detector knows the file by its `.yml` name, whichever spelling
    // the repository uses.
    let path = CONFIG_PATHS[0];
    let snapshot = RepoSnapshot {
        tree: vec![TreeEntry {
            path: path.to_owned(),
            size: content.len() as u64,
            is_dir: false,
        }],
        files: [(path.to_owned(), content.to_owned())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let today = Utc::now().date_naive();
    let today = {
        use chrono::Datelike;
        (today.year(), today.month(), today.day())
    };
    distinct(
        sdi_analyzer::analyze_at(&snapshot, today)
            .dependabot
            .ecosystems,
    )
}

/// Each value once, first occurrence kept (a monorepo names `npm` once per
/// directory).
fn distinct(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

/// `GET /vulnerability-alerts`: `204` enabled, `404` disabled, anything
/// else means the token was not told. GitHub also answers `404` for a
/// repository the token cannot see, so "disabled" is only believed when
/// the repository itself was readable.
fn alerts_word(switch: &Result<(), GithubError>, repository_readable: bool) -> &'static str {
    match switch {
        Ok(()) => "enabled",
        Err(GithubError::NotFound(_)) if repository_readable => "disabled",
        Err(_) => "unknown",
    }
}

/// `GET /automated-security-fixes`: only a `200` says anything.
fn security_updates_word(fixes: &Result<SecurityFixes, GithubError>) -> &'static str {
    match fixes {
        Ok(fixes) if fixes.enabled && fixes.paused => "paused",
        Ok(fixes) if fixes.enabled => "enabled",
        Ok(_) => "disabled",
        Err(_) => "unknown",
    }
}

/// `alerts_status` and its sentence, from how the alert list answered and
/// what the alerts switch said (`switch`, an [`alerts_word`]).
///
/// - listed → `ok`;
/// - GitHub says alerts are disabled (its `403`), or the switch is off →
///   `disabled`;
/// - the token was rejected or may not read them (`401`, another `403`,
///   `404`) → `forbidden`, naming what it needs;
/// - anything else (an archived repository, `5xx`, a timeout, the rate
///   limit) → `unavailable`, in GitHub's words.
fn classify(
    listing: &Result<AlertListing, GithubError>,
    switch: &str,
    repository: &str,
    token: &str,
) -> (&'static str, String) {
    let error = match listing {
        Ok(_) => return (STATUS_OK, String::new()),
        Err(error) => error,
    };
    let said = |needle: &str| match error {
        GithubError::Forbidden(message) => message.to_ascii_lowercase().contains(needle),
        _ => false,
    };
    if said("disabled") || said("not enabled") {
        return (STATUS_DISABLED, DISABLED_MESSAGE.to_owned());
    }
    // A rejected token says nothing about the repository's switches.
    if matches!(error, GithubError::Unauthorized) {
        return (
            STATUS_FORBIDDEN,
            format!(
                "GitHub rejected this workspace's token, so the Dependabot alerts of {repository} could not be read. Reconnect GitHub. {NEEDED}"
            ),
        );
    }
    if switch == "disabled" {
        return (STATUS_DISABLED, DISABLED_MESSAGE.to_owned());
    }
    match error {
        GithubError::Forbidden(_) if said("archived") => (
            STATUS_UNAVAILABLE,
            unavailable_message(repository, error, token),
        ),
        GithubError::Forbidden(_) | GithubError::NotFound(_) => (
            STATUS_FORBIDDEN,
            format!(
                "This workspace's GitHub access may not read the Dependabot alerts of {repository}. {NEEDED}"
            ),
        ),
        _ => (
            STATUS_UNAVAILABLE,
            unavailable_message(repository, error, token),
        ),
    }
}

/// What reading alerts takes, for every kind of access a workspace can have.
const NEEDED: &str = "A classic token needs the `repo` or `security_events` scope; a fine-grained token or the GitHub App needs the \"Dependabot alerts: read\" permission.";

/// GitHub's own words for why the list did not come, with the workspace's
/// token struck out should a transport error ever quote it.
fn unavailable_message(repository: &str, error: &GithubError, token: &str) -> String {
    let words = match error {
        GithubError::Forbidden(message) => message.clone(),
        other => other.to_string(),
    };
    let words = if token.is_empty() {
        words
    } else {
        words.replace(token, "[token]")
    };
    let words = words.trim().trim_end_matches('.');
    format!("Dependabot alerts of {repository} could not be read: {words}.")
}

fn sort_key(alert: &DependabotAlert) -> (&str, Option<chrono::DateTime<Utc>>, u64) {
    (
        alert.severity.as_str(),
        alert
            .created_at
            .as_ref()
            .and_then(sdi_common::time::from_proto),
        alert.number,
    )
}

/// One GitHub alert in the contract's shape. `branches` are the open
/// Dependabot pull requests, for the fix link.
fn dependabot_alert(alert: Alert, branches: &[(u64, &str)]) -> DependabotAlert {
    let dependency = alert.dependency.unwrap_or_default();
    let package = dependency.package.unwrap_or_default();
    let advisory = alert.security_advisory.unwrap_or_default();
    let vulnerability = alert.security_vulnerability.unwrap_or_default();
    let manifest_path = dependency.manifest_path.unwrap_or_default();
    let ecosystem = security::ecosystem(&package.ecosystem);
    let severity = advisory
        .severity
        .as_deref()
        .map(security::severity)
        .filter(|word| !word.is_empty())
        .or_else(|| vulnerability.severity.as_deref().map(security::severity))
        .unwrap_or_default();
    DependabotAlert {
        number: alert.number,
        fix_pull_request: security::fix_pull_request(
            &ecosystem,
            &package.name,
            &manifest_path,
            branches,
        ),
        ecosystem,
        package: package.name,
        manifest_path,
        scope: dependency.scope.unwrap_or_default(),
        severity: severity.to_owned(),
        ghsa_id: advisory.ghsa_id.unwrap_or_default(),
        cve_id: advisory.cve_id.unwrap_or_default(),
        summary: advisory.summary.unwrap_or_default(),
        vulnerable_range: vulnerability.vulnerable_version_range.unwrap_or_default(),
        fixed_version: vulnerability
            .first_patched_version
            .and_then(|version| version.identifier)
            .unwrap_or_default(),
        html_url: alert.html_url,
        created_at: opt_to_proto(alert.created_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing_error(error: GithubError) -> Result<AlertListing, GithubError> {
        Err(error)
    }

    const TOKEN: &str = "ghs_secret_token_value";

    #[test]
    fn a_listed_answer_is_ok_whatever_the_switch_said() {
        let (status, message) =
            classify(&Ok(AlertListing::default()), "disabled", "acme/api", TOKEN);
        assert_eq!(status, STATUS_OK);
        assert!(message.is_empty());
    }

    #[test]
    fn githubs_disabled_403_and_a_switch_that_is_off_are_disabled() {
        let said = classify(
            &listing_error(GithubError::Forbidden(
                "Dependabot alerts are disabled for this repository.".into(),
            )),
            "unknown",
            "acme/api",
            TOKEN,
        );
        assert_eq!(said, (STATUS_DISABLED, DISABLED_MESSAGE.to_owned()));

        // The list only said "no", but the settings endpoint said off.
        for error in [
            GithubError::Forbidden("Resource not accessible by integration".into()),
            GithubError::NotFound("/x".into()),
            GithubError::Api {
                status: 502,
                message: "Bad Gateway".into(),
            },
        ] {
            let (status, message) = classify(&listing_error(error), "disabled", "acme/api", TOKEN);
            assert_eq!(status, STATUS_DISABLED);
            assert_eq!(message, DISABLED_MESSAGE);
        }
    }

    #[test]
    fn a_token_that_may_not_read_alerts_is_forbidden_and_told_what_it_needs() {
        for error in [
            GithubError::Unauthorized,
            GithubError::Forbidden("Resource not accessible by personal access token".into()),
            GithubError::NotFound("/x".into()),
        ] {
            let (status, message) = classify(&listing_error(error), "enabled", "acme/api", TOKEN);
            assert_eq!(status, STATUS_FORBIDDEN, "{message}");
            assert!(message.contains("acme/api"), "{message}");
            assert!(message.contains("`repo` or `security_events`"), "{message}");
            assert!(message.contains("\"Dependabot alerts: read\""), "{message}");
            assert!(!message.contains(TOKEN));
        }
        // A rejected token is forbidden even when the switch (asked with
        // the same token) seemed to say "off".
        let (status, message) = classify(
            &listing_error(GithubError::Unauthorized),
            "disabled",
            "acme/api",
            TOKEN,
        );
        assert_eq!(status, STATUS_FORBIDDEN);
        assert!(message.contains("rejected"), "{message}");
    }

    #[test]
    fn everything_else_is_unavailable_in_githubs_words_without_the_token() {
        let cases = [
            (
                GithubError::Api {
                    status: 503,
                    message: "Service Unavailable".into(),
                },
                "503",
            ),
            (GithubError::RateLimited { retry_after: 42 }, "42s"),
            (
                GithubError::Forbidden(
                    "Dependabot alerts are not available for archived repositories.".into(),
                ),
                "archived repositories",
            ),
            (
                GithubError::Decode(format!("unexpected body near {TOKEN}")),
                "[token]",
            ),
        ];
        for (error, expected) in cases {
            let (status, message) = classify(&listing_error(error), "enabled", "acme/api", TOKEN);
            assert_eq!(status, STATUS_UNAVAILABLE, "{message}");
            assert!(message.contains(expected), "{message}");
            assert!(!message.contains(TOKEN), "{message}");
            assert!(
                message.ends_with('.') && !message.ends_with(".."),
                "{message}"
            );
        }
    }

    #[test]
    fn switches_read_as_the_contract_says() {
        assert_eq!(alerts_word(&Ok(()), true), "enabled");
        assert_eq!(
            alerts_word(&Err(GithubError::NotFound("/x".into())), true),
            "disabled"
        );
        // A repository this token cannot see answers 404 to everything:
        // that is not GitHub saying the alerts are off.
        assert_eq!(
            alerts_word(&Err(GithubError::NotFound("/x".into())), false),
            "unknown"
        );
        assert_eq!(
            alerts_word(&Err(GithubError::Forbidden("no".into())), true),
            "unknown"
        );
        assert_eq!(
            alerts_word(&Err(GithubError::Unauthorized), true),
            "unknown"
        );

        let fixes = |enabled, paused| Ok(SecurityFixes { enabled, paused });
        assert_eq!(security_updates_word(&fixes(true, false)), "enabled");
        assert_eq!(security_updates_word(&fixes(true, true)), "paused");
        assert_eq!(security_updates_word(&fixes(false, false)), "disabled");
        assert_eq!(security_updates_word(&fixes(false, true)), "disabled");
        assert_eq!(
            security_updates_word(&Err(GithubError::NotFound("/x".into()))),
            "unknown"
        );
    }

    #[test]
    fn the_scans_detector_reads_the_live_configuration() {
        let content = "version: 2\nupdates:\n  - package-ecosystem: \"npm\"\n    directory: \"/\"\n    schedule: { interval: weekly }\n  - package-ecosystem: cargo\n    directory: /\n    schedule:\n      interval: daily\n  - package-ecosystem: npm\n    directory: /web\n    schedule: { interval: weekly }\n  - package-ecosystem: github-actions\n    directory: /\n    schedule: { interval: weekly }\n";
        assert_eq!(
            config_ecosystems(content),
            ["npm", "cargo", "github-actions"]
        );
        // A file that is not a Dependabot configuration names nothing.
        assert!(config_ecosystems("just: text\n").is_empty());
        assert!(config_ecosystems("").is_empty());
        assert!(config_ecosystems(": : not yaml [").is_empty());
    }

    #[test]
    fn alerts_map_field_by_field_and_tolerate_missing_parts() {
        let alert: Alert = serde_json::from_value(serde_json::json!({
            "number": 7,
            "state": "open",
            "html_url": "https://github.com/acme/api/security/dependabot/7",
            "created_at": "2026-10-01T08:00:00Z",
            "dependency": {
                "package": {"ecosystem": "npm", "name": "@babel/traverse"},
                "manifest_path": "web/package-lock.json",
                "scope": "development"
            },
            "security_advisory": {
                "ghsa_id": "GHSA-67hx-6x53-jw92",
                "cve_id": "CVE-2023-45133",
                "summary": "Babel vulnerable to arbitrary code execution",
                "severity": "critical"
            },
            "security_vulnerability": {
                "severity": "critical",
                "vulnerable_version_range": "< 7.23.2",
                "first_patched_version": {"identifier": "7.23.2"}
            }
        }))
        .unwrap();
        let branches = [(41, "dependabot/npm_and_yarn/web/babel/traverse-7.23.2")];
        let mapped = dependabot_alert(alert, &branches);
        assert_eq!(mapped.number, 7);
        assert_eq!(mapped.ecosystem, "npm");
        assert_eq!(mapped.package, "@babel/traverse");
        assert_eq!(mapped.manifest_path, "web/package-lock.json");
        assert_eq!(mapped.scope, "development");
        assert_eq!(mapped.severity, "critical");
        assert_eq!(mapped.ghsa_id, "GHSA-67hx-6x53-jw92");
        assert_eq!(mapped.cve_id, "CVE-2023-45133");
        assert_eq!(
            mapped.summary,
            "Babel vulnerable to arbitrary code execution"
        );
        assert_eq!(mapped.vulnerable_range, "< 7.23.2");
        assert_eq!(mapped.fixed_version, "7.23.2");
        assert!(mapped.html_url.ends_with("/dependabot/7"));
        assert!(mapped.created_at.is_some());
        assert_eq!(mapped.fix_pull_request, 41);

        // No fix yet, no CVE, a crate (GitHub says "rust"), and the
        // severity only on the vulnerability, in the advisory database's word.
        let sparse: Alert = serde_json::from_value(serde_json::json!({
            "number": 8,
            "dependency": {"package": {"ecosystem": "rust", "name": "time"}, "manifest_path": "Cargo.lock"},
            "security_advisory": {"ghsa_id": "GHSA-xxxx", "cve_id": null, "summary": "Segfault"},
            "security_vulnerability": {"severity": "moderate", "first_patched_version": null}
        }))
        .unwrap();
        let mapped = dependabot_alert(sparse, &branches);
        assert_eq!(mapped.ecosystem, "cargo");
        assert_eq!(mapped.severity, "medium");
        assert_eq!(mapped.cve_id, "");
        assert_eq!(mapped.fixed_version, "");
        assert_eq!(mapped.scope, "");
        assert_eq!(mapped.fix_pull_request, 0);
        assert!(mapped.created_at.is_none());

        // An alert with nothing but a number is still an alert.
        let bare: Alert = serde_json::from_value(serde_json::json!({"number": 9})).unwrap();
        assert_eq!(dependabot_alert(bare, &branches).number, 9);
    }

    fn answer(status: &str) -> RepositorySecurity {
        RepositorySecurity {
            alerts_status: status.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn the_cache_is_per_project_bounded_and_forgets_on_request() {
        let cache = SecurityCache::default();
        let (ws, project) = (Uuid::now_v7(), Uuid::now_v7());
        assert!(cache.get((ws, project)).is_none());
        cache.put((ws, project), answer(STATUS_OK));
        assert_eq!(cache.get((ws, project)).unwrap().alerts_status, STATUS_OK);
        // Another workspace asking for the same project id sees nothing.
        assert!(cache.get((Uuid::now_v7(), project)).is_none());

        cache.forget_project(ws, project);
        assert!(cache.get((ws, project)).is_none());

        let other = Uuid::now_v7();
        cache.put((ws, Uuid::now_v7()), answer(STATUS_OK));
        cache.put((ws, Uuid::now_v7()), answer(STATUS_DISABLED));
        cache.put((other, Uuid::now_v7()), answer(STATUS_OK));
        cache.forget_workspace(ws);
        assert_eq!(cache.held(), 1);

        for _ in 0..(CACHE_CAPACITY + 20) {
            cache.put((ws, Uuid::now_v7()), answer(STATUS_OK));
        }
        assert_eq!(cache.held(), CACHE_CAPACITY);
    }

    #[test]
    fn an_unavailable_answer_is_not_kept_past_its_short_life() {
        let cache = SecurityCache::default();
        let key = (Uuid::now_v7(), Uuid::now_v7());
        let long_ago = Instant::now()
            .checked_sub(UNAVAILABLE_TTL + Duration::from_secs(1))
            .expect("the process clock is older than half a minute");
        // Still within the five minutes, but GitHub being down then says
        // nothing about now.
        cache
            .lock()
            .insert(key, (long_ago, answer(STATUS_UNAVAILABLE)));
        assert!(cache.get(key).is_none());
        assert_eq!(cache.held(), 0, "and it is dropped, not just skipped");

        // The same age is fine for an answer about the repository itself.
        cache
            .lock()
            .insert(key, (long_ago, answer(STATUS_FORBIDDEN)));
        assert_eq!(cache.get(key).unwrap().alerts_status, STATUS_FORBIDDEN);
        // A fresh outage answer is served, so a burst of callers does not
        // hammer a GitHub that is already struggling.
        cache.put(key, answer(STATUS_UNAVAILABLE));
        assert_eq!(cache.get(key).unwrap().alerts_status, STATUS_UNAVAILABLE);
    }
}
