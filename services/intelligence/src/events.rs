//! In-process broadcast of `PlatformEvent`s (CONTRACT.md §8 live events).
//! Publishing never blocks and never fails: a slow or absent watcher must not
//! slow down a proposal, so lagging receivers simply skip ahead.

use std::collections::HashMap;

use sdi_common::time;
use sdi_proto::common::v1::PlatformEvent;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::Status;

const BUS_CAPACITY: usize = 256;
const WATCH_BUFFER: usize = 64;

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<PlatformEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(BUS_CAPACITY);
        Self { tx }
    }

    pub fn publish(&self, event: PlatformEvent) {
        // No receiver is not an error: nobody is watching right now.
        let _ = self.tx.send(event);
    }

    pub fn emit(
        &self,
        entity: &str,
        entity_id: &str,
        project_id: &str,
        action: &str,
        status: &str,
        data: &[(&str, String)],
    ) {
        self.publish(PlatformEvent {
            entity: entity.to_owned(),
            entity_id: entity_id.to_owned(),
            project_id: project_id.to_owned(),
            action: action.to_owned(),
            status: status.to_owned(),
            at: Some(time::to_proto(chrono::Utc::now())),
            data: data
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect::<HashMap<_, _>>(),
        });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PlatformEvent> {
        self.tx.subscribe()
    }

    /// A stream that forwards every event from now on and never ends on its
    /// own; it closes when the client hangs up or the service shuts down
    /// (an open feed would otherwise keep graceful shutdown waiting forever).
    pub fn watch(&self) -> ReceiverStream<Result<PlatformEvent, Status>> {
        let mut rx = self.subscribe();
        let (tx, out) = mpsc::channel(WATCH_BUFFER);
        tokio::spawn(async move {
            let leaving = sdi_common::shutdown::cancelled();
            tokio::pin!(leaving);
            loop {
                let received = tokio::select! {
                    () = &mut leaving => return,
                    received = rx.recv() => received,
                };
                match received {
                    Ok(event) => {
                        if tx.send(Ok(event)).await.is_err() {
                            return;
                        }
                    }
                    // Lost events are fine for a notification feed: the web app
                    // refetches whole query families on any event anyway.
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        ReceiverStream::new(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn watch_forwards_events_in_order() {
        let bus = EventBus::new();
        let mut stream = bus.watch();
        bus.emit(
            "proposal",
            "p1",
            "proj",
            "created",
            "proposed",
            &[("revision", "1".into())],
        );
        bus.emit("decision", "d1", "", "created", "active", &[]);
        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(
            (first.entity.as_str(), first.action.as_str()),
            ("proposal", "created")
        );
        assert_eq!(first.data.get("revision").map(String::as_str), Some("1"));
        let second = stream.next().await.unwrap().unwrap();
        assert_eq!(second.entity, "decision");
        assert!(second.at.is_some());
    }

    #[test]
    fn publishing_without_watchers_is_fine() {
        EventBus::new().emit("document", "x", "p", "created", "", &[]);
    }
}
