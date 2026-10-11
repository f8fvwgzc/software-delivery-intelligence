//! Pipeline runs and deployments from GitHub. Shared by the scan's delivery
//! stage and the `refresh` flag on the list RPCs.

use anyhow::{Context, Result, anyhow};
use sqlx::PgPool;

use crate::{
    db::{
        self,
        delivery::{Failure, NewDeployment, NewRun},
        projects::ProjectRow,
    },
    domain::failure,
    github::{GithubClient, Job, WorkflowRun, split_full_name},
};

/// Job look-ups cost one call per failed run; cap them so a red history does not burn the quota.
const MAX_FAILED_RUN_LOOKUPS: usize = 10;

/// `limit` is `settings.pipeline_runs_limit`: how many recent runs one sync
/// pulls (GitHub caps a page at 100).
pub async fn sync_runs(
    pool: &PgPool,
    github: &GithubClient,
    project: &ProjectRow,
    limit: u32,
) -> Result<usize> {
    let (owner, name) = split_full_name(&project.repo_full_name)
        .ok_or_else(|| anyhow!("invalid repository name"))?;
    let runs = github
        .workflow_runs(owner, name, limit.clamp(1, 100))
        .await
        .context("workflow runs")?;

    let mut new_runs = Vec::with_capacity(runs.len());
    let mut lookups = 0;
    for run in &runs {
        let status = run_status(run);
        let failure = if status == "failed" && lookups < MAX_FAILED_RUN_LOOKUPS {
            lookups += 1;
            match github.run_jobs(owner, name, run.id).await {
                Ok(jobs) => find_failure(&jobs),
                Err(error) => {
                    tracing::warn!(run = run.id, %error, "could not fetch jobs for failed run");
                    None
                }
            }
        } else {
            None
        };
        new_runs.push(NewRun {
            provider_run_id: run.id,
            pipeline_name: run.name.clone().unwrap_or_else(|| run.path.clone()),
            pipeline_path: run.path.clone(),
            run_number: run.run_number,
            status,
            branch: run.head_branch.clone().unwrap_or_default(),
            commit_sha: run.head_sha.clone(),
            commit_message: run
                .head_commit
                .as_ref()
                .map(|c| first_line(&c.message))
                .unwrap_or_default(),
            actor: run
                .actor
                .as_ref()
                .map(|a| a.login.clone())
                .unwrap_or_default(),
            event: run.event.clone(),
            started_at: run.run_started_at.or(Some(run.created_at)),
            duration_seconds: duration_seconds(run),
            html_url: run.html_url.clone(),
            failure,
        });
    }

    let mut conn = pool.acquire().await?;
    db::delivery::upsert_runs(&mut conn, project.workspace_id, project.id, &new_runs).await?;
    db::projects::set_runs_synced(&mut *conn, project.workspace_id, project.id).await?;
    Ok(new_runs.len())
}

pub async fn sync_deployments(
    pool: &PgPool,
    github: &GithubClient,
    project: &ProjectRow,
    limit: u32,
) -> Result<usize> {
    let (owner, name) = split_full_name(&project.repo_full_name)
        .ok_or_else(|| anyhow!("invalid repository name"))?;
    let mut new_deployments = Vec::new();
    let limit = limit.clamp(1, 100);

    let deployments = github
        .deployments(owner, name, limit)
        .await
        .context("deployments")?;
    for deployment in deployments {
        let statuses = github
            .deployment_statuses(owner, name, deployment.id)
            .await
            .unwrap_or_default();
        let latest = statuses.iter().max_by_key(|s| s.created_at);
        new_deployments.push(NewDeployment {
            provider_id: deployment.id,
            source: "github_deployment".to_owned(),
            environment: deployment.environment,
            r#ref: deployment.reference,
            commit_sha: deployment.sha,
            status: latest
                .map_or("pending", |s| deployment_state(&s.state))
                .to_owned(),
            created_at: deployment.created_at,
            creator: deployment.creator.map(|c| c.login).unwrap_or_default(),
            url: latest
                .and_then(|s| s.environment_url.clone().or_else(|| s.log_url.clone()))
                .unwrap_or_default(),
            description: deployment.description.unwrap_or_default(),
        });
    }

    // Repositories without the Deployments API still deploy through workflows.
    let runs = github
        .workflow_runs(owner, name, limit)
        .await
        .context("workflow runs")?;
    for run in runs
        .iter()
        .filter(|r| failure::is_deploy_workflow(r.name.as_deref().unwrap_or(""), &r.path, &r.event))
    {
        let workflow = run.name.clone().unwrap_or_else(|| run.path.clone());
        new_deployments.push(NewDeployment {
            provider_id: run.id,
            source: "workflow_run".to_owned(),
            environment: failure::environment_for_workflow(&workflow),
            r#ref: run.head_branch.clone().unwrap_or_default(),
            commit_sha: run.head_sha.clone(),
            status: run_to_deployment_state(&run_status(run)).to_owned(),
            created_at: run.run_started_at.unwrap_or(run.created_at),
            creator: run
                .actor
                .as_ref()
                .map(|a| a.login.clone())
                .unwrap_or_default(),
            url: run.html_url.clone(),
            description: workflow,
        });
    }

    let mut conn = pool.acquire().await?;
    db::delivery::upsert_deployments(
        &mut conn,
        project.workspace_id,
        project.id,
        &new_deployments,
    )
    .await?;
    db::projects::set_deployments_synced(&mut *conn, project.workspace_id, project.id).await?;
    Ok(new_deployments.len())
}

/// Stored run status from GitHub's `status` + `conclusion` pair.
fn run_status(run: &WorkflowRun) -> String {
    let status = run.status.as_deref().unwrap_or("");
    let conclusion = run.conclusion.as_deref().unwrap_or("");
    match (status, conclusion) {
        ("completed", "success") => "succeeded",
        ("completed", "failure" | "timed_out" | "startup_failure" | "action_required") => "failed",
        ("completed", "cancelled") => "cancelled",
        ("completed", _) => "skipped",
        ("in_progress", _) => "in_progress",
        _ => "queued",
    }
    .to_owned()
}

fn run_to_deployment_state(run_status: &str) -> &'static str {
    match run_status {
        "succeeded" => "succeeded",
        "failed" => "failed",
        "in_progress" => "in_progress",
        "cancelled" | "skipped" => "inactive",
        _ => "pending",
    }
}

fn deployment_state(state: &str) -> &'static str {
    match state {
        "success" => "succeeded",
        "failure" | "error" => "failed",
        "inactive" => "inactive",
        "in_progress" => "in_progress",
        _ => "pending",
    }
}

fn duration_seconds(run: &WorkflowRun) -> i32 {
    let started = run.run_started_at.unwrap_or(run.created_at);
    (run.updated_at - started)
        .num_seconds()
        .clamp(0, i64::from(i32::MAX)) as i32
}

fn first_line(message: &str) -> String {
    message.lines().next().unwrap_or_default().trim().to_owned()
}

fn find_failure(jobs: &[Job]) -> Option<Failure> {
    let job = jobs
        .iter()
        .find(|j| j.conclusion.as_deref() == Some("failure"))?;
    let step = job
        .steps
        .iter()
        .find(|s| s.conclusion.as_deref() == Some("failure"))
        .map(|s| s.name.clone())
        .unwrap_or_default();
    let (summary, suggested_fix) = failure::diagnose(&job.name, &step);
    Some(Failure {
        job: job.name.clone(),
        step,
        summary,
        suggested_fix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Step;

    #[test]
    fn maps_github_status_pairs() {
        let run = |status: &str, conclusion: Option<&str>| WorkflowRun {
            id: 1,
            name: None,
            path: String::new(),
            run_number: 1,
            status: Some(status.to_owned()),
            conclusion: conclusion.map(str::to_owned),
            head_branch: None,
            head_sha: String::new(),
            head_commit: None,
            actor: None,
            event: String::new(),
            run_started_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            html_url: String::new(),
        };
        assert_eq!(run_status(&run("completed", Some("success"))), "succeeded");
        assert_eq!(run_status(&run("completed", Some("timed_out"))), "failed");
        assert_eq!(run_status(&run("completed", Some("skipped"))), "skipped");
        assert_eq!(run_status(&run("in_progress", None)), "in_progress");
        assert_eq!(run_status(&run("queued", None)), "queued");
    }

    #[test]
    fn finds_first_failed_step() {
        let jobs = vec![
            Job {
                name: "lint".into(),
                conclusion: Some("success".into()),
                steps: vec![],
            },
            Job {
                name: "test".into(),
                conclusion: Some("failure".into()),
                steps: vec![
                    Step {
                        name: "Checkout".into(),
                        conclusion: Some("success".into()),
                    },
                    Step {
                        name: "Run cargo test".into(),
                        conclusion: Some("failure".into()),
                    },
                ],
            },
        ];
        let failure = find_failure(&jobs).unwrap();
        assert_eq!(failure.job, "test");
        assert_eq!(failure.step, "Run cargo test");
        assert_eq!(failure.summary, "Test suite failed");
        assert!(find_failure(&jobs[..1]).is_none());
    }
}
