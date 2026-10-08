use std::sync::Arc;

use sdi_common::grpc::db_status;
use sdi_proto::execution::v1::{DeliveryStats, GetDeliveryStatsRequest};
use tonic::Status;

use crate::{db, runner::Runtime};

fn count(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

pub async fn get(rt: &Arc<Runtime>, req: GetDeliveryStatsRequest) -> Result<DeliveryStats, Status> {
    let row = db::executions::stats(&rt.pool, req.project_id.trim())
        .await
        .map_err(db_status)?;
    let finished = row.succeeded + row.failed;
    Ok(DeliveryStats {
        executions_total: count(row.total),
        executions_active: count(row.active),
        executions_paused: count(row.paused),
        executions_succeeded: count(row.succeeded),
        executions_failed: count(row.failed),
        success_rate: if finished > 0 {
            row.succeeded as f64 / finished as f64
        } else {
            0.0
        },
        avg_duration_seconds: count(row.avg_duration_seconds.round() as i64),
        approvals_pending: count(row.approvals_pending),
        steps_executed: count(row.steps_executed),
        failures_recovered: count(row.failures_recovered),
    })
}
