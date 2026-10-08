use sdi_common::grpc::db_status;
use sdi_proto::intelligence::v1::IntelligenceStats;
use tonic::Status;

use super::IntelligenceGrpc;

fn count(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(super) async fn get(svc: &IntelligenceGrpc) -> Result<IntelligenceStats, Status> {
    let row = crate::db::stats(&svc.pool).await.map_err(db_status)?;
    Ok(IntelligenceStats {
        proposals: count(row.proposals),
        proposals_open: count(row.proposals_open),
        proposals_accepted: count(row.proposals_accepted),
        decisions_active: count(row.decisions_active),
        documents: count(row.documents),
        messages: count(row.messages),
    })
}
