//! Background housekeeping for the session code cache (CONTRACT.md §19):
//! every ten minutes, file rows whose `expires_at` has passed are deleted.
//! Reads already treat an expired row as a miss, so the sweeper only
//! reclaims space; a failed sweep is logged and retried next time.

use std::time::Duration;

use sqlx::PgPool;

pub const INTERVAL: Duration = Duration::from_secs(10 * 60);

/// Spawns the sweeper. It ends with the process (shutdown cancels it).
pub fn spawn(pool: PgPool) -> tokio::task::JoinHandle<()> {
    sdi_common::tasks::spawn("code-cache-sweeper", async move {
        let mut ticker = tokio::time::interval(INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                () = sdi_common::shutdown::cancelled() => return,
            }
            match crate::db::code::sweep_expired(&pool).await {
                Ok(0) => tracing::debug!("code cache sweep: nothing expired"),
                Ok(rows) => tracing::info!(rows, "code cache sweep: expired files deleted"),
                Err(error) => tracing::warn!(%error, "code cache sweep failed"),
            }
            match crate::db::pull_requests::sweep_expired(&pool).await {
                Ok(0) => {}
                Ok(rows) => {
                    tracing::info!(rows, "code cache sweep: expired pull request diffs deleted")
                }
                Err(error) => tracing::warn!(%error, "pull request diff sweep failed"),
            }
        }
    })
}
