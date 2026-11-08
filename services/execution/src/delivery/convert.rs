//! Rows → proto messages of the delivery API.

use chrono::Utc;
use sdi_common::time;
use sdi_proto::execution::v1::{
    DeploymentRun, DeploymentStatus, Environment, Registry, Release, ReleaseInspection,
};

use super::{
    db::{
        environments::EnvironmentRow, registries::RegistryRow, releases::ReleaseRow, runs::RunRow,
    },
    model,
};

pub fn registry(row: RegistryRow) -> Registry {
    Registry {
        id: row.id.to_string(),
        name: row.name,
        kind: row.kind,
        host: row.host,
        namespace: row.namespace,
        auth: row.auth,
        username_secret: row.username_secret,
        password_secret: row.password_secret,
        is_default: row.is_default,
        created_at: Some(time::to_proto(row.created_at)),
        updated_at: Some(time::to_proto(row.updated_at)),
    }
}

pub fn environment(row: EnvironmentRow) -> Environment {
    Environment {
        id: row.id.to_string(),
        project_id: row.project_id,
        name: row.name,
        position: row.position.max(0) as u32,
        kind: row.kind,
        config: row.config.0.into_iter().collect(),
        url: row.url,
        health_path: row.health_path,
        registry_id: row.registry_id.map(|id| id.to_string()).unwrap_or_default(),
        protected: row.protected,
        auto_promote: row.auto_promote,
        source: row.source,
        current_release_id: row
            .current_release_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        created_at: Some(time::to_proto(row.created_at)),
        updated_at: Some(time::to_proto(row.updated_at)),
    }
}

/// `deployed_to` is the names of the environments the release currently
/// runs in, in promotion order.
pub fn release(row: ReleaseRow, deployed_to: Vec<String>) -> Release {
    Release {
        id: row.id.to_string(),
        project_id: row.project_id,
        version: row.version,
        commit_sha: row.commit_sha,
        image: row.image,
        tag: row.tag,
        digest: row.digest,
        source: row.source,
        pipeline_run_id: row.pipeline_run_id,
        notes: row.notes,
        verified: row.verified,
        created_by: row.created_by,
        created_at: Some(time::to_proto(row.created_at)),
        deployed_to,
        inspection: row.inspected_at.map(|at| ReleaseInspection {
            status: row.inspection_status,
            max_severity: row.inspection_max_severity,
            finding_count: u32::try_from(row.inspection_findings).unwrap_or(0),
            inspected_at: Some(time::to_proto(at)),
            report_json: String::new(),
        }),
    }
}

/// `duration_seconds` runs to the end of the run, or to now while it is
/// still open.
pub fn run(row: RunRow) -> DeploymentRun {
    let end = row.finished_at.unwrap_or_else(Utc::now);
    let duration = (end - row.created_at).num_seconds().max(0);
    let status: DeploymentStatus = model::status_from_db(&row.status);
    DeploymentRun {
        id: row.id.to_string(),
        project_id: row.project_id,
        environment_id: row.environment_id.to_string(),
        environment_name: row.environment_name,
        release_id: row.release_id.to_string(),
        release_version: row.release_version,
        kind: row.kind,
        status: status as i32,
        execution_id: row.execution_id.to_string(),
        external_url: row.external_url,
        previous_release_id: row
            .previous_release_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        started_by: row.started_by,
        created_at: Some(time::to_proto(row.created_at)),
        finished_at: time::opt_to_proto(row.finished_at),
        duration_seconds: u32::try_from(duration).unwrap_or(u32::MAX),
    }
}
