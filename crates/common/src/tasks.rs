//! Background work that the process keeps track of.
//!
//! A bare `tokio::spawn` is invisible: nothing knows it exists, nothing
//! waits for it on shutdown, and a panic in it is lost. Every service spawns
//! its fire-and-forget work (audit entries, scans, runners, watchers,
//! sweepers) through [`spawn`] instead, so [`shutdown`] can let the work
//! that is nearly done finish and report what it had to abandon. Tasks that
//! must stop early take [`crate::shutdown::cancelled`] themselves; the
//! tracker only waits.

use std::{future::Future, sync::OnceLock, time::Duration};

use tokio::task::JoinHandle;
use tokio_util::task::TaskTracker;

fn tracker() -> &'static TaskTracker {
    static TRACKER: OnceLock<TaskTracker> = OnceLock::new();
    TRACKER.get_or_init(TaskTracker::new)
}

/// Spawns `future` on the runtime and tracks it until it ends. A panic
/// surfaces through the returned handle like with `tokio::spawn`; `name`
/// is logged so an abandoned task can be told apart at shutdown.
pub fn spawn<F>(name: &'static str, future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tracing::trace!(task = name, "background task spawned");
    tracker().spawn(future)
}

/// How many tracked tasks are still running.
pub fn running() -> usize {
    tracker().len()
}

/// Waits up to `grace` for every tracked task to finish, then returns how
/// many were still running (the caller logs it; the process ends anyway).
/// The tracker is closed first so nothing spawned afterwards is waited for.
pub async fn shutdown(grace: Duration) -> usize {
    let tracker = tracker();
    tracker.close();
    let pending = tracker.len();
    if pending == 0 {
        return 0;
    }
    tracing::info!(
        pending,
        grace_secs = grace.as_secs(),
        "waiting for background tasks"
    );
    if tokio::time::timeout(grace, tracker.wait()).await.is_err() {
        let left = tracker.len();
        tracing::warn!(left, "background tasks abandoned at shutdown");
        return left;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tracked_tasks_are_counted_and_awaited() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let handle = spawn("test", async move {
            let _ = rx.await;
            7
        });
        assert!(running() >= 1);
        tx.send(()).unwrap();
        assert_eq!(handle.await.unwrap(), 7);
        // Spawned work that is quick finishes inside the grace period.
        spawn("quick", async {});
        let left = shutdown(Duration::from_secs(2)).await;
        assert_eq!(left, 0);
    }
}
