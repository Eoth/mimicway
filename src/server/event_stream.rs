//! Responses sent as server-sent events (`text/event-stream`, HTML Living Standard, "Server-sent events").
//!
//! Each event of the rule is written after its delay as its `event:` line when it is named, one `data:` line per line
//! of its data, and a blank line. After the last event the response ends. At `cut_at`, after that event's delay, the
//! body fails instead: hyper then drops the connection without the last chunk of a chunked response (HTTP/1.1), or
//! resets the stream (HTTP/2), so the client sees a transport error, as when the server it reads goes away.
//!
//! The data of every event is rendered when the request arrives, like a body: the stream then holds no request body
//! while it runs (a body can weigh 10 MiB), and `{{now_iso}}` gives the time of the request. Rendering stops past
//! `MAX_RENDERED_BYTES`, so that the events a stream holds stay bounded. The connection is held only while events
//! remain, and each stream holds one of the places of `MAX_OPEN_STREAMS` (`server::limits`) until it ends, is cut or
//! its client leaves.
use crate::engine::{ChaosMode, TemplateContext, TemplateRenderer};
use crate::models::{EventStream, Rule, Service};
use crate::server::AppState;
use crate::server::limits::StreamBudget;
use crate::server::request_log::{CapturedRequest, StreamOutcome, StreamRecord};
use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::time::Duration;
use tokio::sync::OwnedSemaphorePermit;

/// The most events a rule may send, the cut included.
pub const MAX_EVENTS: usize = 1000;
/// The longest delay before one event, in milliseconds.
pub const MAX_EVENT_DELAY_MS: u64 = 60_000;
/// The longest a stream may last, in milliseconds: the sum of its delays.
pub const MAX_STREAM_DURATION_MS: u64 = 600_000;
/// The most bytes of rendered data a stream holds.
const MAX_RENDERED_BYTES: usize = 1024 * 1024;

struct RenderedEvent {
    name: Option<String>,
    data: String,
    delay: Duration,
}

/// Answers a mocked request whose rule sends `stream`, once the rule's script has run and `ctx` is ready.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn respond(
    state: &AppState,
    service: &Service,
    rule: &Rule,
    method: &str,
    path: &str,
    stream: &EventStream,
    ctx: &TemplateContext<'_>,
    captured: Option<CapturedRequest>,
    budget: Option<&StreamBudget>,
) -> Response {
    let log_refusal = |status: StatusCode, captured| {
        state
            .request_log
            .log_mock(service, method, path, &rule.name, status.as_u16(), captured);
    };
    if let Some(chaos) = &rule.response.chaos {
        ChaosMode::maybe_delay(chaos).await;
        if ChaosMode::should_inject_error(chaos) {
            let status = ChaosMode::error_status(chaos);
            log_refusal(status, captured);
            return (status, "chaos error injected").into_response();
        }
    }
    let permit = match budget.map(StreamBudget::open) {
        Some(None) => {
            log_refusal(StatusCode::SERVICE_UNAVAILABLE, captured);
            return crate::server::limits::busy();
        }
        Some(place) => place,
        None => None,
    };
    let Some((events, cut)) = render(stream, ctx) else {
        tracing::warn!(
            service_key = %service.name, rule = %rule.name,
            "event stream not sent: its events render to more than {MAX_RENDERED_BYTES} bytes"
        );
        log_refusal(StatusCode::INTERNAL_SERVER_ERROR, captured);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "The events of this rule render to more than 1 MiB.",
        )
            .into_response();
    };

    let status = StatusCode::from_u16(rule.response.status).unwrap_or(StatusCode::OK);
    let record = state.request_log.log_stream(
        service,
        method,
        path,
        &rule.name,
        status.as_u16(),
        captured,
        events.len() + usize::from(cut.is_some()),
    );
    tracing::info!(
        service_key = %service.name, method = %method, path = %path, mode = "mock", rule = %rule.name,
        events = events.len(), cut = cut.is_some(), "event stream opened"
    );

    let mut response = Response::builder().status(status);
    let headers = TemplateRenderer::render_headers(&rule.response.headers, ctx);
    let has = |name: &str| headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
    if !has("content-type") {
        response = response.header(header::CONTENT_TYPE, "text/event-stream");
    }
    // A cached copy of a stream would be one moment of it.
    if !has("cache-control") {
        response = response.header(header::CACHE_CONTROL, "no-cache");
    }
    for (name, value) in &headers {
        match HeaderValue::from_str(value) {
            Ok(hv) => response = response.header(name.as_str(), hv),
            Err(_) => tracing::warn!(
                service_key = %service.name, rule = %rule.name, header = %name,
                "response header left out: its value is not a valid header value"
            ),
        }
    }
    response
        .body(body(events, cut, record, permit))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// The events sent before the cut, rendered, and the delay before the cut; `None` past `MAX_RENDERED_BYTES`.
fn render(
    stream: &EventStream,
    ctx: &TemplateContext,
) -> Option<(Vec<RenderedEvent>, Option<Duration>)> {
    let sent = stream.cut_at.unwrap_or(stream.events.len());
    let mut total = 0;
    let mut events = Vec::with_capacity(sent.min(stream.events.len()));
    for event in stream.events.iter().take(sent) {
        let data = TemplateRenderer::render_body(
            &[crate::models::BodyFragment::Template {
                template: event.data.clone(),
            }],
            &[],
            ctx,
        );
        total += data.len() + event.event.as_ref().map_or(0, String::len);
        if total > MAX_RENDERED_BYTES {
            return None;
        }
        events.push(RenderedEvent {
            name: event.event.clone(),
            data,
            delay: Duration::from_millis(event.delay_ms),
        });
    }
    let cut = stream
        .cut_at
        .and_then(|at| stream.events.get(at))
        .map(|event| Duration::from_millis(event.delay_ms));
    Some((events, cut))
}

/// One event as the wire carries it. A line break of the data, whatever its form (`\r\n`, `\r`, `\n`, all three end
/// a line for the client), starts a new `data:` line; a name keeps to one line even if the configuration file was
/// edited by hand around the validation.
fn frame(name: Option<&str>, data: &str) -> Bytes {
    let mut out = String::with_capacity(data.len() + 16);
    if let Some(name) = name {
        out.push_str("event: ");
        out.push_str(&name.replace(['\r', '\n'], " "));
        out.push('\n');
    }
    for line in data.replace("\r\n", "\n").split(['\r', '\n']) {
        out.push_str("data: ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    Bytes::from(out)
}

/// What a running stream owns. Dropped when hyper drops the body: at the end, at the cut, or when the client leaves.
struct Progress {
    events: std::vec::IntoIter<RenderedEvent>,
    cut: Option<Duration>,
    record: StreamRecord,
    _place: Option<OwnedSemaphorePermit>,
}

impl Drop for Progress {
    fn drop(&mut self) {
        // Ignored when the stream already ended or was cut: the first outcome stays.
        self.record.finish(StreamOutcome::ClientGone);
    }
}

fn body(
    events: Vec<RenderedEvent>,
    cut: Option<Duration>,
    record: StreamRecord,
    place: Option<OwnedSemaphorePermit>,
) -> Body {
    let progress = Progress {
        events: events.into_iter(),
        cut,
        record,
        _place: place,
    };
    Body::from_stream(futures_util::stream::unfold(
        progress,
        |mut progress| async move {
            if let Some(event) = progress.events.next() {
                tokio::time::sleep(event.delay).await;
                progress.record.sent(event.name.as_deref(), &event.data);
                let bytes = frame(event.name.as_deref(), &event.data);
                return Some((Ok(bytes), progress));
            }
            match progress.cut.take() {
                Some(delay) => {
                    tokio::time::sleep(delay).await;
                    progress.record.finish(StreamOutcome::Cut);
                    Some((
                        Err(std::io::Error::other("event stream cut by its rule")),
                        progress,
                    ))
                }
                None => {
                    progress.record.finish(StreamOutcome::Ended);
                    None
                }
            }
        },
    ))
}

#[cfg(test)]
mod tests;
