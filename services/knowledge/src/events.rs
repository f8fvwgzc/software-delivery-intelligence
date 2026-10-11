//! Process-wide broadcast of `PlatformEvent`s.
//!
//! One `tokio::sync::broadcast` channel feeds every `WatchEvents` and
//! `WatchScan` subscriber. Publishing never blocks and never fails: when
//! nobody listens the event is dropped, and a receiver that falls behind
//! skips the events it missed (the consumer re-reads the entity it cares
//! about, so a skipped progress event costs nothing but a log line).

use std::collections::HashMap;

use sdi_common::time::to_proto;
use sdi_proto::common::v1::PlatformEvent;
use tokio::sync::broadcast;

/// Buffered events per subscriber before the slowest one starts skipping.
/// A scan emits ~10 progress events; the web app invalidates query families
/// in the same tick, so a few hundred covers a burst of parallel scans.
pub const CAPACITY: usize = 512;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<PlatformEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CAPACITY);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PlatformEvent> {
        self.sender.subscribe()
    }

    /// Current subscriber count; only interesting for tests and diagnostics.
    pub fn receivers(&self) -> usize {
        self.sender.receiver_count()
    }

    pub fn emit(&self, event: PlatformEvent) {
        // `Err` only means no receiver is connected right now.
        let _ = self.sender.send(event);
    }

    pub fn project(&self, project_id: impl ToString, action: &str, status: &str) {
        let id = project_id.to_string();
        self.emit(event("project", &id, &id, action, status, HashMap::new()));
    }

    /// Scan progress: `data` carries the running stage and the file counters
    /// so a list view can render without fetching the scan.
    pub fn scan(
        &self,
        scan_id: impl ToString,
        project_id: impl ToString,
        action: &str,
        status: &str,
        data: HashMap<String, String>,
    ) {
        self.emit(event(
            "scan",
            &scan_id.to_string(),
            &project_id.to_string(),
            action,
            status,
            data,
        ));
    }

    pub fn pattern(&self, pattern_id: &str, action: &str, status: &str) {
        self.emit(event(
            "pattern",
            pattern_id,
            "",
            action,
            status,
            HashMap::new(),
        ));
    }
}

pub fn event(
    entity: &str,
    entity_id: &str,
    project_id: &str,
    action: &str,
    status: &str,
    data: HashMap<String, String>,
) -> PlatformEvent {
    PlatformEvent {
        entity: entity.to_owned(),
        entity_id: entity_id.to_owned(),
        project_id: project_id.to_owned(),
        action: action.to_owned(),
        status: status.to_owned(),
        at: Some(to_proto(chrono::Utc::now())),
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};

    #[tokio::test]
    async fn subscribers_receive_every_event_in_order() {
        let bus = EventBus::new();
        let mut a = bus.subscribe();
        let mut b = bus.subscribe();
        bus.project("p1", "created", "pending");
        bus.scan(
            "s1",
            "p1",
            "progress",
            "running",
            HashMap::from([("stage".into(), "tree".into())]),
        );
        bus.pattern("vm-docker-compose", "updated", "active");

        for rx in [&mut a, &mut b] {
            let first = rx.recv().await.unwrap();
            assert_eq!(
                (first.entity.as_str(), first.action.as_str()),
                ("project", "created")
            );
            assert_eq!(first.project_id, "p1");
            assert!(first.at.is_some());
            let second = rx.recv().await.unwrap();
            assert_eq!(second.entity, "scan");
            assert_eq!(second.data["stage"], "tree");
            let third = rx.recv().await.unwrap();
            assert_eq!(third.entity, "pattern");
            assert_eq!(third.project_id, "");
            assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
        }
    }

    #[tokio::test]
    async fn emitting_without_listeners_is_fine_and_laggards_skip() {
        let bus = EventBus::new();
        bus.project("nobody", "updated", "ready");
        assert_eq!(bus.receivers(), 0);

        let mut rx = bus.subscribe();
        for i in 0..(CAPACITY + 5) {
            bus.project(i, "updated", "ready");
        }
        // The oldest events are gone; the receiver learns how many it missed
        // and continues from the oldest one still buffered.
        assert!(matches!(rx.recv().await, Err(RecvError::Lagged(5))));
        let next = rx.recv().await.unwrap();
        assert_eq!(next.entity_id, "5");
    }
}
