//! Platform-wide change notifications (CONTRACT.md §8 "Live events"). One
//! broadcast channel for the whole service: every execution and approval
//! change is published here and `WatchEvents` streams it to the gateway,
//! which merges the three services' feeds into one SSE stream. The per-step
//! log stays on `WatchExecution`; this feed only says *what* changed.

use std::collections::BTreeMap;

use chrono::Utc;
use sdi_common::time;
use sdi_proto::common::v1::PlatformEvent;
use tokio::sync::broadcast;

const CAPACITY: usize = 1024;

#[derive(Clone)]
pub struct PlatformEvents {
    sender: broadcast::Sender<PlatformEvent>,
}

impl Default for PlatformEvents {
    fn default() -> Self {
        Self {
            sender: broadcast::channel(CAPACITY).0,
        }
    }
}

impl PlatformEvents {
    pub fn subscribe(&self) -> broadcast::Receiver<PlatformEvent> {
        self.sender.subscribe()
    }

    pub fn publish(&self, event: PlatformEvent) {
        // No subscriber is not an error: the gateway may not be connected.
        let _ = self.sender.send(event);
    }

    /// An execution changed: `status` is the lowercase contract vocabulary
    /// ("waiting_for_approval"), `data` carries the revision and whatever
    /// the caller finds useful (the step key, the failure code).
    pub fn execution(
        &self,
        id: &str,
        project_id: &str,
        action: &str,
        status: &str,
        data: BTreeMap<String, String>,
    ) {
        self.publish(PlatformEvent {
            entity: "execution".to_owned(),
            entity_id: id.to_owned(),
            project_id: project_id.to_owned(),
            action: action.to_owned(),
            status: status.to_owned(),
            at: Some(time::to_proto(Utc::now())),
            data: data.into_iter().collect(),
        });
    }

    /// An approval was requested (`created`, status `pending`) or decided
    /// (`updated`, status `approved` / `rejected` / `expired`).
    pub fn approval(
        &self,
        id: &str,
        execution_id: &str,
        project_id: &str,
        action: &str,
        status: &str,
    ) {
        self.publish(PlatformEvent {
            entity: "approval".to_owned(),
            entity_id: id.to_owned(),
            project_id: project_id.to_owned(),
            action: action.to_owned(),
            status: status.to_owned(),
            at: Some(time::to_proto(Utc::now())),
            data: [("execution_id".to_owned(), execution_id.to_owned())].into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribers_see_what_was_published_after_they_joined() {
        let events = PlatformEvents::default();
        events.execution("lost", "p", "created", "planning", BTreeMap::new());
        let mut rx = events.subscribe();
        events.execution(
            "e1",
            "p",
            "updated",
            "executing",
            [("revision".to_owned(), "1".to_owned())].into(),
        );
        events.approval("a1", "e1", "p", "created", "pending");
        let first = rx.recv().await.unwrap();
        assert_eq!(first.entity, "execution");
        assert_eq!(first.entity_id, "e1");
        assert_eq!(first.status, "executing");
        assert_eq!(first.data["revision"], "1");
        let second = rx.recv().await.unwrap();
        assert_eq!(second.entity, "approval");
        assert_eq!(second.action, "created");
        assert_eq!(second.data["execution_id"], "e1");
        assert!(rx.try_recv().is_err());
    }
}
