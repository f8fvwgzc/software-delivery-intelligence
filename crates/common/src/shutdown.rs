//! Graceful shutdown on Ctrl-C and SIGTERM.
//!
//! Both tonic and axum finish graceful shutdown only once every in-flight
//! request has ended. A `WatchEvents` stream (or its SSE mirror in the
//! gateway) is open for as long as a browser tab is, so without help a
//! single live feed would pin the process until compose or Kubernetes
//! killed it. `cancelled()` is the process-wide "we are leaving" future that
//! every long-lived stream takes until: the signal flips it, the streams
//! end, the connections close and the server exits.

use std::sync::OnceLock;

use tokio::sync::watch;

fn flag() -> &'static watch::Sender<bool> {
    static FLAG: OnceLock<watch::Sender<bool>> = OnceLock::new();
    FLAG.get_or_init(|| watch::channel(false).0)
}

/// Resolves once a shutdown signal was received (or `trigger` was called).
/// Never resolves otherwise, so it is safe as a `take_until` bound.
pub async fn cancelled() {
    let mut rx = flag().subscribe();
    // The sender lives in a static, so `wait_for` only errs if it was
    // never created, which the `flag()` call above rules out.
    let _ = rx.wait_for(|leaving| *leaving).await;
}

/// Marks the process as shutting down. `signal` calls it; tests may too.
pub fn trigger() {
    // `send` refuses to store the value while nobody subscribes yet;
    // `send_replace` stores it regardless, so late subscribers see it.
    flag().send_replace(true);
}

/// Waits for Ctrl-C or SIGTERM, then flips `cancelled()`. Hand it to
/// `serve_with_shutdown` / `with_graceful_shutdown`.
pub async fn signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut stream) = signal(SignalKind::terminate()) {
            stream.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
    trigger();
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn cancelled_resolves_only_after_trigger() {
        let pending = tokio::time::timeout(Duration::from_millis(50), cancelled()).await;
        assert!(pending.is_err(), "must not resolve before the signal");
        trigger();
        tokio::time::timeout(Duration::from_secs(1), cancelled())
            .await
            .expect("resolves after trigger");
        // Late subscribers see the flag too.
        tokio::time::timeout(Duration::from_secs(1), cancelled())
            .await
            .expect("stays resolved");
    }
}
