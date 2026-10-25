//! Server-Sent Events bridged from the services' gRPC streams: the live tail
//! of an execution, the progress of a scan and the platform-wide change feed.
//! All three are kept off the request timeout because a watch legitimately
//! lives as long as the thing it watches (or, for the change feed, as long
//! as the browser tab).

use std::{convert::Infallible, future::Future, time::Duration};

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::{
    Stream, StreamExt,
    stream::{self, BoxStream},
};
use sdi_proto::{common::v1 as common, execution::v1 as execution, knowledge::v1 as knowledge};
use serde::Serialize;
use tonic::{Status, Streaming};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::{
        common::PlatformEvent,
        execution::{ExecutionEvent, WatchEventsQuery},
        knowledge::Scan,
    },
    error::{ApiError, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Scoped, StreamSlot},
};

/// Proxies in between (nginx, browsers) drop idle connections; a comment
/// line every 15s keeps the stream alive without any payload.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// Backoff for re-opening a dropped `WatchEvents` stream. Starts low so a
/// service restart is barely noticed, caps so a service that is down for
/// an hour is not hammered.
const RECONNECT_INITIAL: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);

/// The `data` key every service stamps its events with (CONTRACT.md §18).
pub const WORKSPACE_KEY: &str = "workspace_id";

/// Claims a stream slot for the caller's workspace or answers 429: a
/// workspace may hold `SSE_MAX_PER_WORKSPACE` open streams per replica,
/// which is one tab's worth fifty times over and far below what would pin
/// a replica's connections (each stream is a gRPC watch per service).
fn claim(state: &Scoped) -> ApiResult<StreamSlot> {
    state.state.open_stream(state.workspace_id()).ok_or_else(|| {
        ApiError::too_many_requests(format!(
            "this workspace already holds {} open event streams on this replica; close a tab and retry",
            state.state.settings.sse_max_per_workspace
        ))
    })
}

/// Keeps the slot alive for as long as the stream is polled; the slot (and
/// with it the workspace's count) goes when the stream is dropped.
fn hold<S: Stream>(slot: StreamSlot, stream: S) -> impl Stream<Item = S::Item> {
    stream::unfold((slot, Box::pin(stream)), |(slot, mut stream)| async move {
        stream.next().await.map(|item| (item, (slot, stream)))
    })
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(watch_execution))
        .routes(routes!(watch_scan))
        .routes(routes!(watch_platform_events))
}

/// Streams execution events live. Each `data:` line is an `ExecutionEvent`
/// JSON object; a final event named `end` is sent once the execution is
/// terminal, and one named `error` if the upstream watch fails midway.
#[utoipa::path(
    get,
    path = "/executions/{id}/events/stream",
    tag = "executions",
    params(("id" = String, Path, description = "Execution id"), WatchEventsQuery),
    responses(
        (status = 200, description = "Server-Sent Events, one ExecutionEvent per message", content_type = "text/event-stream", body = ExecutionEvent),
        ErrorResponses,
    ),
)]
async fn watch_execution(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<WatchEventsQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let slot = claim(&state)?;
    let upstream = state
        .execution()
        .watch_execution(execution::WatchExecutionRequest {
            execution_id: id,
            after_seq: query.after_seq.unwrap_or(0),
        })
        .await?
        .into_inner();

    Ok(finite(hold(
        slot,
        upstream.map(|item| {
            item.map(|event| {
                let event = ExecutionEvent::from(event);
                (Some(event.seq.to_string()), event)
            })
        }),
    )))
}

/// Streams a scan's progress live. Each `data:` line is the whole `Scan`
/// (stages included) after a change; a final event named `end` is sent once
/// the scan succeeded or failed, and one named `error` if the upstream watch
/// fails midway.
#[utoipa::path(
    get,
    path = "/scans/{id}/stream",
    tag = "scans",
    params(("id" = String, Path, description = "Scan id")),
    responses(
        (status = 200, description = "Server-Sent Events, one Scan per message", content_type = "text/event-stream", body = Scan),
        ErrorResponses,
    ),
)]
async fn watch_scan(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let slot = claim(&state)?;
    let upstream = state
        .knowledge()
        .watch_scan(knowledge::WatchScanRequest { id })
        .await?
        .into_inner();

    Ok(finite(hold(
        slot,
        upstream.map(|item| item.map(|scan| (None, Scan::from(scan)))),
    )))
}

/// Every change in the caller's workspace as it happens, merged from the
/// knowledge, intelligence, execution and gitops services (CONTRACT.md §8,
/// "Live events", and §24.8). Each `data:` line is a `PlatformEvent`. The
/// stream never ends: a service that goes away is
/// reconnected with backoff while the others keep flowing, so the web app
/// opens it once per tab and invalidates its caches by `entity`. Events
/// of other workspaces (or without one) are dropped here (§18).
#[utoipa::path(
    get,
    path = "/events/stream",
    tag = "system",
    responses(
        (status = 200, description = "Server-Sent Events, one PlatformEvent per message; never ends", content_type = "text/event-stream", body = PlatformEvent),
        (status = 429, description = "The workspace holds too many open streams on this replica", body = crate::dto::common::ErrorBody),
    ),
)]
async fn watch_platform_events(
    state: Scoped,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let slot = claim(&state)?;
    let knowledge = resilient("knowledge", {
        let state = state.clone();
        move || {
            let mut client = state.knowledge();
            async move {
                client
                    .watch_events(common::WatchEventsRequest {})
                    .await
                    .map(|response| response.into_inner())
            }
        }
    });
    let intelligence = resilient("intelligence", {
        let state = state.clone();
        move || {
            let mut client = state.intelligence();
            async move {
                client
                    .watch_events(common::WatchEventsRequest {})
                    .await
                    .map(|response| response.into_inner())
            }
        }
    });
    let execution = resilient("execution", {
        let state = state.clone();
        move || {
            let mut client = state.execution();
            async move {
                client
                    .watch_events(common::WatchEventsRequest {})
                    .await
                    .map(|response| response.into_inner())
            }
        }
    });

    let gitops = resilient("gitops", {
        let state = state.clone();
        move || {
            let mut client = state.gitops();
            async move {
                client
                    .watch_events(common::WatchEventsRequest {})
                    .await
                    .map(|response| response.into_inner())
            }
        }
    });

    let workspace_id = state.workspace_id().to_owned();
    let merged = stream::select_all([knowledge, intelligence, execution, gitops])
        .filter(move |event| {
            std::future::ready(event.data.get(WORKSPACE_KEY) == Some(&workspace_id))
        })
        .map(|event| Ok(data_event(None, &PlatformEvent::from(event))))
        .take_until(sdi_common::shutdown::cancelled());

    Ok(Sse::new(opened(hold(slot, merged))).keep_alive(keep_alive()))
}

/// Wraps a finite upstream (a watch that ends with its subject) into the SSE
/// shape shared by the execution and scan streams: `data:` per item, an
/// `error` event for an upstream failure, a final `end` event.
fn finite<T, S>(upstream: S) -> Sse<impl Stream<Item = Result<Event, Infallible>>>
where
    T: Serialize + Send + 'static,
    S: Stream<Item = Result<(Option<String>, T), Status>> + Send + 'static,
{
    let events = upstream
        .map(|item| {
            Ok(match item {
                Ok((id, payload)) => data_event(id, &payload),
                Err(status) => error_event(status),
            })
        })
        .chain(stream::once(async {
            Ok(Event::default().event("end").data(""))
        }))
        // On shutdown the connection simply closes (no `end`), so the
        // browser's EventSource reconnects once a replica is back.
        .take_until(sdi_common::shutdown::cancelled());
    Sse::new(opened(events)).keep_alive(keep_alive())
}

/// Puts a comment line in front of a stream, sent the moment it opens.
/// Intermediaries that hold a response back until its first body byte
/// (Node's proxy in development, some CDNs and corporate proxies) would
/// otherwise leave the browser's `EventSource` in `CONNECTING` until the
/// first event or keep-alive, up to `KEEP_ALIVE` later. Comments are not
/// delivered to listeners, so nothing downstream has to know about it.
fn opened<S>(events: S) -> impl Stream<Item = Result<Event, Infallible>>
where
    S: Stream<Item = Result<Event, Infallible>>,
{
    stream::once(async { Ok(Event::default().comment("open")) }).chain(events)
}

fn keep_alive() -> KeepAlive {
    KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive")
}

fn data_event<T: Serialize>(id: Option<String>, payload: &T) -> Event {
    let event = match id {
        Some(id) => Event::default().id(id),
        None => Event::default(),
    };
    event
        .json_data(payload)
        .unwrap_or_else(|error| Event::default().event("error").data(error.to_string()))
}

fn error_event(status: Status) -> Event {
    let error = ApiError::from(status);
    Event::default()
        .event("error")
        .json_data(error.body())
        .unwrap_or_else(|error| Event::default().event("error").data(error.to_string()))
}

/// One service's `WatchEvents` as an endless stream: when the upstream ends
/// or errors (service restart, network blip, service not up yet) the stream
/// goes quiet, waits with exponential backoff and opens a new watch, so the
/// merged feed the browser holds never closes because one service blinked.
fn resilient<C, F>(service: &'static str, connect: C) -> BoxStream<'static, common::PlatformEvent>
where
    C: Fn() -> F + Send + 'static,
    F: Future<Output = Result<Streaming<common::PlatformEvent>, Status>> + Send + 'static,
{
    struct State<C> {
        connect: C,
        current: Option<Streaming<common::PlatformEvent>>,
        /// True once a connect was attempted: the first one is immediate,
        /// every later one waits.
        tried: bool,
        /// Delay before the next reconnect; zero while events are flowing.
        backoff: Duration,
    }

    stream::unfold(
        State {
            connect,
            current: None,
            tried: false,
            backoff: Duration::ZERO,
        },
        move |mut state| async move {
            loop {
                if let Some(current) = state.current.as_mut() {
                    match current.next().await {
                        Some(Ok(event)) => {
                            // A live event proves the link is healthy again.
                            state.backoff = Duration::ZERO;
                            return Some((event, state));
                        }
                        Some(Err(status)) => {
                            tracing::warn!(service, error = %status, "event watch failed; reconnecting");
                        }
                        None => {
                            tracing::info!(service, "event watch ended; reconnecting");
                        }
                    }
                    state.current = None;
                }

                // Every reconnect waits at least the initial backoff, even
                // after a healthy stream, so a service that keeps closing
                // its stream is not hammered; the wait doubles while the
                // service stays away.
                if state.tried {
                    state.backoff = next_backoff(state.backoff);
                    tokio::time::sleep(state.backoff).await;
                }
                state.tried = true;
                match (state.connect)().await {
                    Ok(stream) => state.current = Some(stream),
                    Err(status) => {
                        tracing::warn!(service, error = %status, backoff = ?state.backoff, "event watch unavailable");
                    }
                }
            }
        },
    )
    .boxed()
}

fn next_backoff(current: Duration) -> Duration {
    if current.is_zero() {
        RECONNECT_INITIAL
    } else {
        (current * 2).min(RECONNECT_MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_errors_become_error_events() {
        let event = error_event(Status::not_found("gone"));
        let rendered = format!("{event:?}");
        assert!(rendered.contains("error"), "{rendered}");
    }

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let mut delay = Duration::ZERO;
        let mut seen = Vec::new();
        for _ in 0..8 {
            delay = next_backoff(delay);
            seen.push(delay.as_secs());
        }
        assert_eq!(seen, [1, 2, 4, 8, 16, 30, 30, 30]);
    }
}
