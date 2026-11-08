//! A deployment run's status is not stored independently: it is derived
//! from the execution that carries it, every time the runner commits a
//! change to that execution, inside the same transaction. So a run can
//! never say `deploying` while its execution waits for an approval, and a
//! crash between "execution succeeded" and "environment updated" cannot
//! happen: they are one commit.
//!
//! When the execution succeeds the projection also makes the release the
//! environment's current one, and as soon as the registry step confirmed
//! the image it marks the release verified (with the digest the registry
//! reported). The report of an image inspection (CONTRACT.md §22.1) moves
//! from the run's outputs onto the release in the same way: it describes
//! the release's one image, and the run keeps only its summary. What has to happen outside the transaction (events to the
//! platform feed, the automatic promotion of the next stage) is returned
//! to the runner as [`Projected`] and done after the commit.

use std::sync::Arc;

use sdi_proto::execution::v1::DeploymentStatus;

use super::{
    db::{
        environments, releases,
        runs::{self, RunRow},
    },
    model::{self, STEP_DEPLOY},
    runs as flow,
};
use crate::{
    adapters, convert,
    db::{self, Tx, executions::ExecutionRow},
    runner::Runtime,
};

/// What a projection changed, for the runner to announce after it commits.
pub struct Projected {
    pub run: RunRow,
    /// The run's status or link changed.
    pub changed: bool,
    /// The run just became SUCCEEDED: the environment has a new release.
    pub succeeded_now: bool,
    /// The release was just marked verified (or learned its digest).
    pub release_changed: bool,
    /// The release just got a new inspection report.
    pub release_inspected: bool,
}

/// Brings the run of `exec` in line with it. `None` for executions that
/// are not deployment runs. Call inside the transaction that changed the
/// execution, after its writes.
pub async fn project(tx: &mut Tx, exec: &ExecutionRow) -> sqlx::Result<Option<Projected>> {
    if exec.adapter != adapters::DELIVERY {
        return Ok(None);
    }
    let workspace = exec.workspace_id;
    let Some(run) = runs::lock_by_execution(tx, workspace, exec.id).await? else {
        return Ok(None);
    };
    // The batch's own copy may predate its writes: read what the
    // transaction now holds.
    let current = db::executions::get(&mut **tx, workspace, exec.id).await?;
    let steps = db::steps::list_for(&mut **tx, workspace, exec.id).await?;
    let deploy = steps
        .iter()
        .find(|step| step.key == STEP_DEPLOY)
        .map(|step| convert::step_status_from_db(&step.status));
    let execution_status = convert::status_from_db(&current.status);
    let status = model::derive_status(execution_status, deploy);
    let external_url = current
        .outputs
        .0
        .get("external_url")
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
        .unwrap_or(&run.external_url)
        .to_owned();
    let finished = crate::domain::state::is_terminal(execution_status);

    // The registry confirmed the image in this run.
    let mut release_changed = false;
    if current.outputs.0.get("verified").map(String::as_str) == Some("true") {
        let digest = current
            .outputs
            .0
            .get("digest")
            .map_or("", |digest| digest.trim());
        release_changed = releases::mark_verified(tx, workspace, run.release_id, digest).await?;
    }

    // An inspection reported on the image (success or a gate that stopped
    // the run alike): the report goes to the release, the run keeps the
    // summary outputs.
    let mut release_inspected = false;
    if let Some(raw) = current.outputs.0.get(adapters::registry::REPORT_OUTPUT) {
        if let Ok(report) = serde_json::from_str::<sdi_inspect::Report>(raw)
            && let Ok(value) = serde_json::to_value(&report)
        {
            let status = if !report.findings.is_empty() {
                "findings"
            } else if report
                .limitations
                .iter()
                .any(|l| l.starts_with("not read:"))
            {
                "incomplete"
            } else {
                "passed"
            };
            let severity = report
                .max_severity()
                .map_or("", sdi_inspect::Severity::as_str);
            release_inspected = releases::record_inspection(
                tx,
                workspace,
                run.release_id,
                &value,
                status,
                severity,
                i32::try_from(report.findings.len()).unwrap_or(i32::MAX),
            )
            .await?;
        }
        db::executions::remove_output(tx, workspace, exec.id, adapters::registry::REPORT_OUTPUT)
            .await?;
    }

    let status_text = model::status_to_db(status);
    let changed = run.status != status_text
        || run.external_url != external_url
        || run.finished_at.is_some() != finished;
    if !changed {
        return Ok(Some(Projected {
            run,
            changed: false,
            succeeded_now: false,
            release_changed,
            release_inspected,
        }));
    }
    let succeeded_now = status == DeploymentStatus::Succeeded
        && model::status_from_db(&run.status) != DeploymentStatus::Succeeded;
    if succeeded_now {
        environments::set_current_release(tx, workspace, run.environment_id, run.release_id)
            .await?;
    }
    let run = runs::project(tx, workspace, run.id, status_text, &external_url, finished).await?;
    Ok(Some(Projected {
        run,
        changed: true,
        succeeded_now,
        release_changed,
        release_inspected,
    }))
}

/// After the commit: tell the platform feed, then let the next stage follow.
pub fn announce(rt: &Arc<Runtime>, projected: Projected) {
    let Projected {
        run,
        changed,
        succeeded_now,
        release_changed,
        release_inspected,
    } = projected;
    let workspace = run.workspace_id;
    if changed {
        rt.platform.delivery(
            workspace,
            "deployment",
            &run.id.to_string(),
            &run.project_id,
            "updated",
            &model::status_text(model::status_from_db(&run.status)),
            flow::run_data(&run),
        );
    }
    if release_changed || succeeded_now || release_inspected {
        // `verified`, `digest`, `deployed_to` or the inspection of the
        // release changed.
        rt.platform.delivery(
            workspace,
            "release",
            &run.release_id.to_string(),
            &run.project_id,
            "updated",
            if succeeded_now {
                "deployed"
            } else if release_inspected {
                "inspected"
            } else {
                "verified"
            },
            [("environment".to_owned(), run.environment_name.clone())].into(),
        );
    }
    if succeeded_now {
        rt.platform.delivery(
            workspace,
            "environment",
            &run.environment_id.to_string(),
            &run.project_id,
            "updated",
            "deployed",
            [
                ("release_id".to_owned(), run.release_id.to_string()),
                ("environment".to_owned(), run.environment_name.clone()),
            ]
            .into(),
        );
        flow::after_success(rt, run);
    }
}
