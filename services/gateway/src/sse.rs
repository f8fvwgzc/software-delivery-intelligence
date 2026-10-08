//! Live tail of an execution's event log as Server-Sent Events, bridged from
//! the execution service's `WatchExecution` gRPC stream. Kept off the request
//! timeout because a watch legitimately lives as long as the execution.

use std::{convert::Infallible, time::Duration};

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use futures::{Stream, StreamExt, stream};
use sdi_proto::execution::v1 as proto;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::execution::{ExecutionEvent, WatchEventsQuery},
    error::{ApiError, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::AppState,
};

/// Proxies in between (nginx, browsers) drop idle connections; a comment
/// line every 15s keeps the stream alive without any payload.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(watch_execution))
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
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<WatchEventsQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let upstream = state
        .execution()
        .watch_execution(proto::WatchExecutionRequest {
            execution_id: id,
            after_seq: query.after_seq.unwrap_or(0),
        })
        .await?
        .into_inner();

    let events = upstream
        .map(|item| Ok(to_event(item)))
        .chain(stream::once(async {
            Ok(Event::default().event("end").data(""))
        }));

    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive")))
}

fn to_event(item: Result<proto::ExecutionEvent, tonic::Status>) -> Event {
    match item {
        Ok(event) => {
            let event = ExecutionEvent::from(event);
            Event::default()
                .id(event.seq.to_string())
                .json_data(&event)
                .unwrap_or_else(|error| Event::default().event("error").data(error.to_string()))
        }
        Err(status) => {
            let error = ApiError::from(status);
            Event::default()
                .event("error")
                .json_data(error.body())
                .unwrap_or_else(|error| Event::default().event("error").data(error.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_errors_become_error_events() {
        let event = to_event(Err(tonic::Status::not_found("gone")));
        let rendered = format!("{event:?}");
        assert!(rendered.contains("error"), "{rendered}");
    }
}
