use std::sync::Arc;

use sdi_common::grpc::db_status;
use sdi_proto::{
    common::v1::PlatformEvent,
    execution::v1::{
        ExecutionEvent, ExecutionStatus, ListEventsRequest, ListEventsResponse,
        WatchExecutionRequest,
    },
};
use tokio::sync::{broadcast::error::RecvError, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::Status;
use uuid::Uuid;

use super::parse_id;
use crate::{convert, db, domain::state, runner::Runtime};

const DEFAULT_LIMIT: i64 = 200;
const MAX_LIMIT: i64 = 1000;
const REPLAY_PAGE: i64 = 500;
const WATCH_BUFFER: usize = 64;

pub async fn list(rt: &Arc<Runtime>, req: ListEventsRequest) -> Result<ListEventsResponse, Status> {
    let id = parse_id(&req.execution_id, "execution_id")?;
    let limit = match i64::from(req.limit) {
        0 => DEFAULT_LIMIT,
        n => n.min(MAX_LIMIT),
    };
    let after = i64::try_from(req.after_seq).unwrap_or(i64::MAX);
    let rows = db::events::list(&rt.pool, id, after, limit)
        .await
        .map_err(db_status)?;
    Ok(ListEventsResponse {
        events: rows.into_iter().map(convert::event_to_proto).collect(),
    })
}

fn is_terminal(event: &ExecutionEvent) -> bool {
    state::is_terminal(ExecutionStatus::try_from(event.status).unwrap_or_default())
}

/// Replays the log from the database, then forwards live events. The
/// subscription is taken before the replay so nothing falls in the gap; the
/// seq filter drops what both sources deliver.
pub async fn watch(
    rt: &Arc<Runtime>,
    req: WatchExecutionRequest,
) -> Result<ReceiverStream<Result<ExecutionEvent, Status>>, Status> {
    let id = parse_id(&req.execution_id, "execution_id")?;
    let row = db::executions::get(&rt.pool, id).await.map_err(db_status)?;
    let already_terminal = state::is_terminal(convert::status_from_db(&row.status));
    let mut live = (!already_terminal).then(|| rt.bus.subscribe(id));

    let (tx, rx) = mpsc::channel(WATCH_BUFFER);
    let rt = Arc::clone(rt);
    let mut last = i64::try_from(req.after_seq).unwrap_or(i64::MAX);

    tokio::spawn(async move {
        if replay(&rt, id, &mut last, &tx).await.is_break() {
            return;
        }
        let Some(mut live) = live.take() else {
            return;
        };
        // An execution can wait for approval for hours; shutdown must not.
        let leaving = sdi_common::shutdown::cancelled();
        tokio::pin!(leaving);
        loop {
            let received = tokio::select! {
                () = &mut leaving => return,
                received = live.recv() => received,
            };
            match received {
                Ok(event) => {
                    let seq = i64::try_from(event.seq).unwrap_or(i64::MAX);
                    if seq <= last {
                        continue;
                    }
                    // A hole means the broadcast skipped something committed
                    // before we subscribed; the database has it.
                    if seq > last + 1 && replay(&rt, id, &mut last, &tx).await.is_break() {
                        return;
                    }
                    if seq <= last {
                        continue;
                    }
                    last = seq;
                    let terminal = is_terminal(&event);
                    if tx.send(Ok(event)).await.is_err() || terminal {
                        return;
                    }
                }
                Err(RecvError::Lagged(_)) => {
                    if replay(&rt, id, &mut last, &tx).await.is_break() {
                        return;
                    }
                }
                Err(RecvError::Closed) => {
                    // The runner finished; whatever it wrote last is in the log.
                    let _ = replay(&rt, id, &mut last, &tx).await;
                    return;
                }
            }
        }
    });

    Ok(ReceiverStream::new(rx))
}

/// The platform feed (CONTRACT.md §8 live events): execution and approval
/// changes from now on, never ending. A slow consumer that falls behind the
/// broadcast buffer loses the oldest notifications, which is harmless: the
/// web app only uses them to refetch, so the next one catches it up.
pub fn watch_platform(rt: &Arc<Runtime>) -> ReceiverStream<Result<PlatformEvent, Status>> {
    let mut live = rt.platform.subscribe();
    let (tx, rx) = mpsc::channel(WATCH_BUFFER);
    tokio::spawn(async move {
        // The feed never ends on its own; the service's shutdown ends it so
        // graceful shutdown is not pinned by an open browser tab.
        let leaving = sdi_common::shutdown::cancelled();
        tokio::pin!(leaving);
        loop {
            let received = tokio::select! {
                () = &mut leaving => return,
                received = live.recv() => received,
            };
            match received {
                Ok(event) => {
                    if tx.send(Ok(event)).await.is_err() {
                        return;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::debug!(skipped, "platform event watcher lagged");
                }
                Err(RecvError::Closed) => return,
            }
        }
    });
    ReceiverStream::new(rx)
}

/// Sends every stored event after `last`. Breaks when the receiver is gone
/// or a terminal event was sent.
async fn replay(
    rt: &Runtime,
    id: Uuid,
    last: &mut i64,
    tx: &mpsc::Sender<Result<ExecutionEvent, Status>>,
) -> std::ops::ControlFlow<()> {
    use std::ops::ControlFlow::{Break, Continue};
    loop {
        let rows = match db::events::list(&rt.pool, id, *last, REPLAY_PAGE).await {
            Ok(rows) => rows,
            Err(error) => {
                let _ = tx.send(Err(db_status(error))).await;
                return Break(());
            }
        };
        if rows.is_empty() {
            return Continue(());
        }
        let page_full = rows.len() as i64 == REPLAY_PAGE;
        for row in rows {
            *last = row.seq;
            let event = convert::event_to_proto(row);
            let terminal = is_terminal(&event);
            if tx.send(Ok(event)).await.is_err() || terminal {
                return Break(());
            }
        }
        if !page_full {
            return Continue(());
        }
    }
}
