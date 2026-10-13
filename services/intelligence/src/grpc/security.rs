//! Vulnerabilities and Dependabot (CONTRACT.md §23.4). The overview is
//! Dependabot's own list when GitHub serves it, else an OSV lookup of the
//! manifest versions, else nothing (and it says why). Nothing is stored:
//! it is computed per request and remembered for five minutes.
//! `ConfigureDependabot` turns the feature on through one execution.

use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

use chrono::Utc;
use sdi_common::grpc::db_status;
use sdi_proto::{
    intelligence::v1::{
        ConfigureDependabotRequest, ConfigureDependabotResponse, GetSecurityOverviewRequest,
        SecurityOverview, Vulnerability,
    },
    knowledge::v1::RepositorySecurity,
};
use tonic::{Code, Status};

use super::{
    Caller, IntelligenceGrpc,
    autopilot::{self, Liveness, Recorded},
    require,
};
use crate::{
    db::autopilot::{self as autopilot_repo, KIND_DEPENDABOT},
    domain::{
        plans,
        review::rules::dependencies::AddedDependency,
        security::{self, SOURCE_DEPENDABOT, SOURCE_NONE, SOURCE_OSV},
    },
    generators,
};

/// The OSV lookup must never hold the overview up for long: the same
/// budget the review gives it (CONTRACT.md §20).
const OSV_BUDGET: Duration = Duration::from_secs(20);
/// Packages per OSV round (the client's own bound per call).
const OSV_ROUND: usize = 100;

/// Errors that are about the request, not about a peer being down: they
/// go back to the caller instead of turning into an empty overview.
fn callers_problem(code: Code) -> bool {
    matches!(
        code,
        Code::NotFound | Code::InvalidArgument | Code::PermissionDenied | Code::Unauthenticated
    )
}

fn sentence(text: &str) -> String {
    let text = text.trim().trim_end_matches('.');
    if text.is_empty() {
        String::new()
    } else {
        format!("{text}.")
    }
}

/// The OSV half: the dependencies with an exact version, looked up through
/// the review's client and cache. `Err` carries why nothing was answered.
async fn from_osv(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    project_id: &str,
    overview: &mut SecurityOverview,
) -> Result<(Vec<Vulnerability>, bool), String> {
    let Some(osv) = svc.osv.as_ref() else {
        return Err(
            "the OSV lookup is turned off on this installation (OSV_API_URL=off)".to_owned(),
        );
    };
    let listed = svc
        .knowledge
        .list_dependencies(&caller.principal, project_id)
        .await
        .map_err(|error| {
            format!(
                "the project's dependencies could not be read ({})",
                error.status.message()
            )
        })?;
    // What the scan saw of Dependabot, when GitHub's own answer is missing.
    if let Some(dependabot) = listed.dependabot.as_ref()
        && !overview.config_present
        && dependabot.configured
    {
        overview.config_present = true;
        overview.config_path = dependabot.config_path.clone();
        overview.config_ecosystems = dependabot.ecosystems.clone();
        overview.open_dependabot_pull_requests = dependabot.open_pull_requests;
    }
    let (lookups, over) = security::lookups(&listed.dependencies);
    if lookups.is_empty() {
        return Err(if listed.dependencies.is_empty() {
            "the last scan found no dependencies in this project".to_owned()
        } else {
            "none of the project's dependencies names an exact version in an ecosystem OSV covers, so there \
             is nothing to look up"
                .to_owned()
        });
    }
    let deadline = tokio::time::Instant::now() + OSV_BUDGET;
    let mut answers = BTreeMap::new();
    let mut timed_out = false;
    for round in lookups.chunks(OSV_ROUND) {
        let queries: Vec<AddedDependency> = round.iter().map(|l| l.query.clone()).collect();
        match tokio::time::timeout_at(deadline, osv.check(&queries)).await {
            Ok(found) => answers.extend(found),
            Err(_) => {
                tracing::warn!(
                    project = project_id,
                    "osv lookup ran out of its budget; the overview is partial"
                );
                timed_out = true;
                break;
            }
        }
    }
    if answers.is_empty() {
        return Err("OSV did not answer".to_owned());
    }
    let mut vulnerabilities = Vec::new();
    let mut unanswered = 0;
    for lookup in &lookups {
        let key = (
            lookup.query.ecosystem.to_owned(),
            lookup.query.name.clone(),
            lookup.query.version.clone(),
        );
        match answers.get(&key) {
            Some(advisories) => {
                vulnerabilities.extend(advisories.iter().map(|a| security::from_osv(lookup, a)))
            }
            None => unanswered += 1,
        }
    }
    Ok((vulnerabilities, over > 0 || timed_out || unanswered > 0))
}

async fn compute(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    project_id: &str,
    refresh: bool,
) -> Result<SecurityOverview, Status> {
    let mut overview = SecurityOverview {
        project_id: project_id.to_owned(),
        alerts: "unknown".to_owned(),
        security_updates: "unknown".to_owned(),
        ..Default::default()
    };
    let github: Option<RepositorySecurity> = match svc
        .knowledge
        .repository_security(&caller.principal, project_id, refresh)
        .await
    {
        Ok(security) => Some(security),
        Err(error) if callers_problem(error.status.code()) => return Err(error.into_status()),
        Err(error) => {
            tracing::warn!(%error, project = project_id, "repository security unavailable; falling back to OSV");
            None
        }
    };
    let why_not_dependabot = match &github {
        Some(github) => {
            overview.config_present = github.config_present;
            overview.config_path = github.config_path.clone();
            overview.config_ecosystems = github.config_ecosystems.clone();
            if !github.alerts.trim().is_empty() {
                overview.alerts = github.alerts.clone();
            }
            if !github.security_updates.trim().is_empty() {
                overview.security_updates = github.security_updates.clone();
            }
            overview.open_dependabot_pull_requests = github.open_pull_requests;
            overview.checked_at = github.checked_at;
            if github.alerts_status == "ok" {
                let mut vulnerabilities: Vec<Vulnerability> = github
                    .open_alerts
                    .iter()
                    .map(security::from_alert)
                    .collect();
                let cut = security::order_and_bound(&mut vulnerabilities);
                overview.source = SOURCE_DEPENDABOT.to_owned();
                overview.truncated = github.truncated || cut;
                overview.by_severity = security::by_severity(&vulnerabilities);
                overview.vulnerabilities = vulnerabilities;
                if overview.checked_at.is_none() {
                    overview.checked_at = Some(sdi_common::time::to_proto(Utc::now()));
                }
                return Ok(overview);
            }
            if github.alerts_message.trim().is_empty() {
                format!(
                    "GitHub did not serve Dependabot's alerts ({})",
                    if github.alerts_status.is_empty() {
                        "no reason given"
                    } else {
                        github.alerts_status.as_str()
                    }
                )
            } else {
                github.alerts_message.trim().to_owned()
            }
        }
        None => "Dependabot's state could not be read from GitHub right now".to_owned(),
    };

    match from_osv(svc, caller, project_id, &mut overview).await {
        Ok((mut vulnerabilities, partial)) => {
            let cut = security::order_and_bound(&mut vulnerabilities);
            overview.source = SOURCE_OSV.to_owned();
            overview.source_note = format!(
                "{} This list comes from OSV for the dependencies that name an exact version{}",
                sentence(&why_not_dependabot),
                if partial {
                    "; some could not be looked up in time, so it may be incomplete."
                } else {
                    "."
                }
            );
            overview.truncated = partial || cut;
            overview.by_severity = security::by_severity(&vulnerabilities);
            overview.vulnerabilities = vulnerabilities;
        }
        Err(why_not_osv) => {
            overview.source = SOURCE_NONE.to_owned();
            overview.source_note = format!(
                "{} OSV could not stand in: {}",
                sentence(&why_not_dependabot),
                sentence(&why_not_osv)
            );
        }
    }
    if overview.checked_at.is_none() {
        overview.checked_at = Some(sdi_common::time::to_proto(Utc::now()));
    }
    Ok(overview)
}

/// The Dependabot setup execution of the project that has not ended, if any.
async fn open_setup(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    project_id: &str,
) -> Result<Option<(String, Liveness)>, Status> {
    let Some(latest) =
        autopilot_repo::latest(&svc.pool, caller.workspace, project_id, KIND_DEPENDABOT, 0)
            .await
            .map_err(db_status)?
    else {
        return Ok(None);
    };
    Ok(match autopilot::liveness(svc, caller, &latest).await {
        Liveness::Ended(_) => None,
        live => Some((latest.execution_id, live)),
    })
}

pub(super) async fn overview(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: GetSecurityOverviewRequest,
) -> Result<SecurityOverview, Status> {
    let project_id = require("project_id", &request.project_id)?;
    let key = (caller.workspace, project_id.clone());
    let cached = if request.refresh {
        None
    } else {
        super::locked(&svc.security_cache).get(&key).cloned()
    };
    let mut overview = match cached {
        Some(overview) => overview,
        None => {
            let computed = compute(svc, caller, &project_id, request.refresh).await?;
            // An overview that says "nobody answered" is not worth
            // remembering: the next request should try again. It also
            // replaces whatever was remembered before, so a refresh is
            // never followed by an older answer.
            if computed.source == SOURCE_NONE {
                super::locked(&svc.security_cache).remove(&key);
            } else {
                super::locked(&svc.security_cache).insert(key, computed.clone());
            }
            computed
        }
    };
    // Never from the cache: an execution can end at any moment.
    overview.configure_execution_id = match open_setup(svc, caller, &project_id).await? {
        Some((execution_id, _)) => execution_id,
        None => String::new(),
    };
    Ok(overview)
}

pub(super) async fn configure(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: ConfigureDependabotRequest,
) -> Result<ConfigureDependabotResponse, Status> {
    let project_id = require("project_id", &request.project_id)?;
    if !(request.alerts || request.security_updates || request.config_file) {
        return Err(Status::invalid_argument(
            "choose at least one of alerts, security_updates and config_file",
        ));
    }
    let _one_at_a_time =
        autopilot::exclusive(svc, caller.workspace, &format!("dependabot:{project_id}")).await?;
    match open_setup(svc, caller, &project_id).await? {
        Some((execution_id, Liveness::Open(status))) => {
            return Err(Status::failed_precondition(format!(
                "Dependabot setup execution {execution_id} for this project is still open ({status}); approve, \
                 finish or cancel it first"
            )));
        }
        Some((_, Liveness::Unknown(status))) => return Err(status),
        _ => {}
    }
    let context = svc
        .knowledge
        .project_context(&caller.principal, &project_id)
        .await
        .map_err(|e| e.into_status())?;
    let project = context.project.clone().unwrap_or_default();
    if project.repo_full_name.trim().is_empty() {
        return Err(Status::failed_precondition(
            "the project does not know its repository",
        ));
    }

    // The configuration file is the generator's, from the project's facts.
    let mut artifacts: HashMap<String, String> = HashMap::new();
    let mut pull_request: Option<(String, String)> = None;
    if request.config_file {
        let input = generators::input::build(&context, &BTreeMap::new(), "");
        let files = generators::dependabot::generate(&input);
        let Some(file) = files
            .iter()
            .find(|f| f.path == plans::DEPENDABOT_CONFIG_PATH)
        else {
            return Err(Status::internal(
                "the Dependabot generator produced no configuration file",
            ));
        };
        pull_request = Some((
            "Add a Dependabot configuration".to_owned(),
            format!(
                "Adds `{}` so Dependabot opens grouped update pull requests every week.\n\n{}\n\nNothing else \
                 changes: no dependency is updated by this pull request. Requested from Software Delivery \
                 Intelligence.",
                file.path, file.purpose
            ),
        ));
        artifacts.insert(file.path.clone(), file.content.clone());
    }
    let steps = plans::dependabot_plan(&plans::DependabotPlan {
        repository: &project.repo_full_name,
        default_branch: &project.default_branch,
        alerts: request.alerts,
        security_updates: request.security_updates,
        config_pull_request: pull_request
            .as_ref()
            .map(|(title, body)| (title.as_str(), body.as_str())),
    });
    let execution_id = autopilot::create_execution(
        svc,
        caller,
        &project_id,
        format!("Turn on Dependabot for {}", project.repo_full_name),
        &steps,
        artifacts,
    )
    .await?;
    autopilot::record(
        svc,
        caller,
        Recorded {
            kind: KIND_DEPENDABOT,
            project_id: &project_id,
            pr_number: 0,
            review_id: None,
            head_sha: "",
            execution_id: &execution_id,
        },
    )
    .await?;
    let asked: Vec<&str> = [
        (request.alerts, "alerts"),
        (request.security_updates, "security updates"),
        (request.config_file, "configuration file"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    svc.audit(
        caller,
        "security.dependabot_requested",
        "project",
        &project_id,
        format!(
            "{} for {}, execution {execution_id}",
            asked.join(", "),
            project.repo_full_name
        ),
    );
    Ok(ConfigureDependabotResponse { execution_id })
}
