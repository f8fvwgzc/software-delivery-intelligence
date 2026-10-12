use sdi_common::grpc::db_status;
use sdi_proto::intelligence::v1::IntelligenceStats;
use tonic::Status;

use super::{Caller, IntelligenceGrpc};

fn count(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(super) async fn get(
    svc: &IntelligenceGrpc,
    caller: &Caller,
) -> Result<IntelligenceStats, Status> {
    let row = crate::db::stats(&svc.pool, caller.workspace)
        .await
        .map_err(db_status)?;
    let usage = crate::db::usage::today_for(&svc.pool, caller.workspace)
        .await
        .map_err(db_status)?;
    let settings = svc.settings(caller).await?;
    Ok(IntelligenceStats {
        proposals: count(row.proposals),
        proposals_open: count(row.proposals_open),
        proposals_accepted: count(row.proposals_accepted),
        decisions_active: count(row.decisions_active),
        documents: count(row.documents),
        messages: count(row.messages),
        reasoner_tokens_today: usage.total(),
        reasoner_daily_token_cap: settings.daily_token_cap(),
        reasoner_calls_today: count(i64::from(usage.calls)),
    })
}
