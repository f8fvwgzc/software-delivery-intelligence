use sdi_common::{grpc::db_status, principal::Role};
use sdi_proto::knowledge::v1::*;
use tonic::Status;

use super::KnowledgeState;
use crate::{auth::Caller, convert, db::settings::SettingsRow};

/// Bounds keep a typo from turning the scanner into a GitHub quota burner
/// (or into a no-op). They are generous: the defaults sit well inside.
const MAX_FILES: u32 = 2_000;
const MAX_FILE_BYTES: u32 = 5_000_000;
const MAX_CONCURRENCY: u32 = 32;
const MAX_RUNS_LIMIT: u32 = 100;

/// The workspace's settings, seeded with the defaults on first read.
pub(super) async fn get_settings(
    state: &KnowledgeState,
    caller: &Caller,
    _req: GetSettingsRequest,
) -> Result<KnowledgeSettings, Status> {
    let row = crate::db::settings::get(&state.pool, caller.workspace_id)
        .await
        .map_err(db_status)?;
    Ok(convert::settings(row))
}

/// Replaces every field (the message has no optionals); a zero is rejected
/// rather than read as "keep", because a client that wants to keep a value
/// sends the one it read. The GitHub cache switch applies immediately; the
/// scan limits apply to the next scan, which reads the row when it starts.
pub(super) async fn update_settings(
    state: &KnowledgeState,
    caller: &Caller,
    req: UpdateSettingsRequest,
) -> Result<KnowledgeSettings, Status> {
    caller.require(Role::Admin)?;
    let settings = req
        .settings
        .ok_or_else(|| Status::invalid_argument("settings are required"))?;
    let row = validate(&settings)?;
    let row = crate::db::settings::update(&state.pool, caller.workspace_id, &row)
        .await
        .map_err(db_status)?;
    if !row.cache_github {
        state.github.forget_cache(caller.workspace_id);
    }
    state.identity.audit(
        caller,
        "settings.update",
        "settings",
        "knowledge",
        &format!("{row:?}"),
    );
    tracing::info!(?row, workspace = %caller.workspace_id, "settings updated");
    Ok(convert::settings(row))
}

fn validate(settings: &KnowledgeSettings) -> Result<SettingsRow, Status> {
    let bounded = |what: &str, value: u32, max: u32| -> Result<i32, Status> {
        if value == 0 || value > max {
            return Err(Status::invalid_argument(format!(
                "{what} must be between 1 and {max}"
            )));
        }
        Ok(value as i32)
    };
    Ok(SettingsRow {
        scan_max_files: bounded("scan_max_files", settings.scan_max_files, MAX_FILES)?,
        scan_max_file_bytes: bounded(
            "scan_max_file_bytes",
            settings.scan_max_file_bytes,
            MAX_FILE_BYTES,
        )?,
        scan_concurrency: bounded(
            "scan_concurrency",
            settings.scan_concurrency,
            MAX_CONCURRENCY,
        )?,
        pipeline_runs_limit: bounded(
            "pipeline_runs_limit",
            settings.pipeline_runs_limit,
            MAX_RUNS_LIMIT,
        )?,
        cache_github: settings.cache_github,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_and_oversized_values_are_rejected() {
        let ok = KnowledgeSettings {
            scan_max_files: 150,
            scan_max_file_bytes: 200_000,
            scan_concurrency: 6,
            pipeline_runs_limit: 50,
            cache_github: false,
        };
        let row = validate(&ok).unwrap();
        assert_eq!(row.scan_max_files, 150);
        assert!(!row.cache_github);

        let zero = KnowledgeSettings {
            scan_concurrency: 0,
            ..ok
        };
        assert_eq!(
            validate(&zero).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
        let huge = KnowledgeSettings {
            pipeline_runs_limit: 1_000,
            ..ok
        };
        assert!(validate(&huge).is_err());
    }
}
