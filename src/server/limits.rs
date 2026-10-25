//! What one client can hold on to: connections, time and memory. Every request comes from the network, and the
//! security model promises that untrusted input cannot exhaust the server.
//!
//! - **Request headers within 10 seconds** of the connection waiting for them. hyper only applies its own limit
//!   with a timer, which `axum::serve` does not give it: a client that sent its headers one byte at a time held a
//!   connection forever (slowloris). The same timer closes a keep-alive connection left idle that long.
//! - **Request bodies at 16 KiB/s on average, after 10 seconds of grace**: a body must have arrived by
//!   `10 s + received / 16 KiB/s` from its headers, or it is cut with 408. A plain deadline would also cut a large
//!   upload streamed to a slow backend; this rate covers a slow mobile link, and makes whoever trickles a body pay
//!   16 KiB/s for each connection held.
//! - **1,024 requests in flight** (`MAX_REQUESTS_IN_FLIGHT`). The next one is answered 503 at once, with
//!   `Retry-After`, rather than queued: a queue would keep its connection and its memory while it waits. A request
//!   held by a chaos delay costs about 40 KiB (1,000 held at once added 41 MB), so the limit fits in the pod's
//!   memory next to the body budget, while a load test may keep hundreds waiting. `/api/health` is never
//!   refused, so that an orchestrator does not restart a server that is only busy.
//! - **48 MiB of request bodies held in memory at once** (`MAX_BUFFERED_BODY_SIZE`, in bytes), counted where a
//!   handler reads a whole body: mocked services, up to 10 MiB each to evaluate their conditions, and the management
//!   API, up to 2 MiB. Past it, 503 at once, before reading the body when it announces its length. A mocked request
//!   holds its body more than once (matching, then a copy for its scripts): ten 10 MiB requests at once to a rule with
//!   a JSON condition, a script and a `{{body.…}}` variable peak at 130 MB with four of them admitted, within the
//!   256 MiB the Kubernetes manifests give the pod. A request streamed to its backend (a proxied service) holds no
//!   body, and counts nothing.
//! - **256 event streams open at once** (`MAX_OPEN_STREAMS`): a rule that answers with server-sent events keeps its
//!   connection until its last event, up to ten minutes, after its request has left the count of requests in
//!   flight. The next stream is answered 503 at once, with `Retry-After`. A stream holds its connection and the
//!   events it has rendered, at most 1 MiB (`server::event_stream`).
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http_body::{Frame, SizeHint};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
const BODY_GRACE: Duration = Duration::from_secs(10);
const BODY_MIN_RATE: u64 = 16 * 1024;
const DEFAULT_MAX_REQUESTS_IN_FLIGHT: usize = 1024;
const DEFAULT_MAX_BUFFERED_BODY_SIZE: usize = 48 * 1024 * 1024;
const DEFAULT_MAX_OPEN_STREAMS: usize = 256;

/// The largest body a mocked service reads to evaluate its conditions.
pub(crate) const MAX_SERVICE_BODY: usize = 10 * 1024 * 1024;
/// The largest body of a management API call, axum's default for its extractors.
const MAX_API_BODY: usize = 2 * 1024 * 1024;

/// Seconds between two warnings that requests were refused, so that a flood does not flood the log too.
const REFUSAL_LOG_INTERVAL_SECS: u64 = 10;

#[derive(Clone)]
pub struct Limits {
    requests: Arc<Semaphore>,
    body_bytes: Arc<Semaphore>,
    streams: Arc<Semaphore>,
    header_timeout: Duration,
    body_grace: Duration,
    body_min_rate: u64,
    refusals: Arc<Refusals>,
}

impl Default for Limits {
    fn default() -> Self {
        Self::new(
            DEFAULT_MAX_REQUESTS_IN_FLIGHT,
            DEFAULT_MAX_BUFFERED_BODY_SIZE,
        )
    }
}

impl Limits {
    /// The limits the server runs with: the defaults, or `MAX_REQUESTS_IN_FLIGHT`, `MAX_BUFFERED_BODY_SIZE` and
    /// `MAX_OPEN_STREAMS`.
    pub fn from_env() -> Self {
        Self::from_lookup(crate::settings::env)
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        Self::new(
            crate::settings::number(
                &lookup,
                "MAX_REQUESTS_IN_FLIGHT",
                DEFAULT_MAX_REQUESTS_IN_FLIGHT,
            ),
            crate::settings::number(
                &lookup,
                "MAX_BUFFERED_BODY_SIZE",
                DEFAULT_MAX_BUFFERED_BODY_SIZE,
            ),
        )
        .with_streams(crate::settings::number(
            &lookup,
            "MAX_OPEN_STREAMS",
            DEFAULT_MAX_OPEN_STREAMS,
        ))
    }

    /// Zero, which would refuse every request, reads as one; values past what a semaphore holds are cut to it.
    pub(crate) fn new(requests: usize, body_bytes: usize) -> Self {
        let permits = |n: usize| n.clamp(1, Semaphore::MAX_PERMITS);
        Self {
            requests: Arc::new(Semaphore::new(permits(requests))),
            body_bytes: Arc::new(Semaphore::new(permits(body_bytes))),
            streams: Arc::new(Semaphore::new(DEFAULT_MAX_OPEN_STREAMS)),
            header_timeout: HEADER_READ_TIMEOUT,
            body_grace: BODY_GRACE,
            body_min_rate: BODY_MIN_RATE,
            refusals: Arc::default(),
        }
    }

    /// Zero reads as one, as for the other limits.
    pub(crate) fn with_streams(mut self, streams: usize) -> Self {
        self.streams = Arc::new(Semaphore::new(streams.clamp(1, Semaphore::MAX_PERMITS)));
        self
    }

    #[cfg(test)]
    pub(crate) fn with_timing(
        mut self,
        header_timeout: Duration,
        body_grace: Duration,
        body_min_rate: u64,
    ) -> Self {
        self.header_timeout = header_timeout;
        self.body_grace = body_grace;
        self.body_min_rate = body_min_rate;
        self
    }

    pub fn describe(&self) -> String {
        format!(
            "{} requests in flight, {} bytes of request bodies held, {} event streams open",
            self.requests.available_permits(),
            self.body_bytes.available_permits(),
            self.streams.available_permits()
        )
    }
}

/// Serves `app` on `listener` until `shutdown` completes, then waits for the requests in progress. What `axum::serve`
/// does, plus the timer that makes hyper bound the time a client takes to send its request headers.
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: Router,
    limits: &Limits,
    shutdown: impl Future<Output = ()> + Send + 'static,
) {
    use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
    use hyper_util::server::conn::auto::Builder;
    use hyper_util::service::TowerToHyperService;

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        shutdown.await;
        let _ = stop_tx.send(true);
    });
    // Each connection holds a receiver: the server has drained once none is left.
    let (drained_tx, drained_rx) = tokio::sync::watch::channel(());
    let mut builder = Builder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(limits.header_timeout);
    let builder = Arc::new(builder);

    let mut stop = stop_rx.clone();
    loop {
        let stream = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(e) => {
                    accept_error(e).await;
                    continue;
                }
            },
            _ = stopped(&mut stop) => break,
        };
        let service = TowerToHyperService::new(app.clone());
        let builder = builder.clone();
        let mut stop = stop_rx.clone();
        let drained = drained_rx.clone();
        tokio::spawn(async move {
            let conn = builder.serve_connection_with_upgrades(TokioIo::new(stream), service);
            let mut conn = std::pin::pin!(conn);
            tokio::select! {
                _ = conn.as_mut() => {}
                _ = stopped(&mut stop) => {
                    conn.as_mut().graceful_shutdown();
                    let _ = conn.await;
                }
            }
            drop(drained);
        });
    }
    drop(drained_rx);
    drop(listener);
    drained_tx.closed().await;
}

/// Completes once shutdown is asked for. The guard `wait_for` returns is dropped here: it is not `Send`, and a
/// spawned connection task holds this future across an await.
async fn stopped(stop: &mut tokio::sync::watch::Receiver<bool>) {
    let _ = stop.wait_for(|stopped| *stopped).await;
}

/// A connection that failed before it was accepted only concerns its client; any other error (out of file
/// descriptors, mostly) would fail again at once, so the loop waits a second, as `axum::serve` does.
async fn accept_error(e: std::io::Error) {
    use std::io::ErrorKind::{ConnectionAborted, ConnectionRefused, ConnectionReset};
    if matches!(
        e.kind(),
        ConnectionRefused | ConnectionAborted | ConnectionReset
    ) {
        return;
    }
    tracing::error!(error = %e, "cannot accept a connection");
    tokio::time::sleep(Duration::from_secs(1)).await;
}

/// Admits a request or refuses it at once, and puts a deadline on its body. Outermost layer of the router, so that
/// a refusal costs nothing else.
pub async fn admit(State(limits): State<Limits>, mut req: Request, next: Next) -> Response {
    let _in_flight = if req.uri().path() == "/api/health" {
        None
    } else {
        match limits.requests.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                limits.refusals.note("requests in flight");
                return busy();
            }
        }
    };
    req.extensions_mut().insert(BodyBudget {
        bytes: limits.body_bytes.clone(),
        refusals: limits.refusals.clone(),
    });
    req.extensions_mut().insert(StreamBudget {
        streams: limits.streams.clone(),
        refusals: limits.refusals.clone(),
    });
    let req = req.map(|body| {
        Body::new(DeadlineBody::new(
            body,
            limits.body_grace,
            limits.body_min_rate,
        ))
    });
    next.run(req).await
}

/// Reads the body of a management API call within the budget, before its handler: the extractors would read it
/// outside of it. The body is handed on in memory, and its share of the budget kept until the response.
pub async fn buffer_api_body(req: Request, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    let budget = parts.extensions.get::<BodyBudget>().cloned();
    match read_body(
        body,
        parts.headers.get(header::CONTENT_LENGTH),
        MAX_API_BODY,
        budget.as_ref(),
    )
    .await
    {
        Ok((bytes, _held)) => {
            next.run(Request::from_parts(parts, Body::from(bytes)))
                .await
        }
        Err(e) => e.into_response(),
    }
}

pub(crate) fn busy() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::RETRY_AFTER, HeaderValue::from_static("1"))],
        "Mimicway is handling too many requests at once; retry in a moment.",
    )
        .into_response()
}

/// Counts refused requests, and logs them at most once per `REFUSAL_LOG_INTERVAL_SECS`.
#[derive(Default)]
struct Refusals {
    count: AtomicU64,
    last_logged_secs: AtomicU64,
}

impl Refusals {
    fn note(&self, limit: &'static str) {
        let count = self.count.fetch_add(1, Ordering::Relaxed) + 1;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let last = self.last_logged_secs.load(Ordering::Relaxed);
        if now >= last + REFUSAL_LOG_INTERVAL_SECS
            && self
                .last_logged_secs
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            tracing::warn!(
                limit,
                refused_so_far = count,
                "requests refused with 503: the server is at its limit"
            );
        }
    }
}

/// The event streams that may still open, put in the extensions of a request by `admit`.
#[derive(Clone)]
pub(crate) struct StreamBudget {
    streams: Arc<Semaphore>,
    refusals: Arc<Refusals>,
}

impl StreamBudget {
    /// A place for one more stream, held until the permit is dropped with the stream; `None` when every place is
    /// taken, to be answered with `busy()`.
    pub(crate) fn open(&self) -> Option<OwnedSemaphorePermit> {
        let place = self.streams.clone().try_acquire_owned().ok();
        if place.is_none() {
            self.refusals.note("event streams open");
        }
        place
    }
}

/// The share of request bodies a request may still hold in memory, put in its extensions by `admit`.
#[derive(Clone)]
pub(crate) struct BodyBudget {
    bytes: Arc<Semaphore>,
    refusals: Arc<Refusals>,
}

impl BodyBudget {
    fn take(&self, bytes: usize, held: &mut Option<OwnedSemaphorePermit>) -> Result<(), BodyError> {
        if bytes == 0 {
            return Ok(());
        }
        let n = u32::try_from(bytes).map_err(|_| BodyError::TooLarge)?;
        match self.bytes.clone().try_acquire_many_owned(n) {
            Ok(permit) => {
                match held {
                    Some(h) => h.merge(permit),
                    None => *held = Some(permit),
                }
                Ok(())
            }
            Err(_) => {
                self.refusals.note("request bodies held");
                Err(BodyError::OverBudget)
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum BodyError {
    TooLarge,
    TooSlow,
    OverBudget,
    Unreadable,
}

impl IntoResponse for BodyError {
    fn into_response(self) -> Response {
        match self {
            BodyError::TooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "The request body is too large.",
            )
                .into_response(),
            // The rest of the body may still be on its way: close the connection rather than read it.
            BodyError::TooSlow => (
                StatusCode::REQUEST_TIMEOUT,
                [(header::CONNECTION, HeaderValue::from_static("close"))],
                "The request body arrived too slowly.",
            )
                .into_response(),
            BodyError::OverBudget => busy(),
            BodyError::Unreadable => (
                StatusCode::BAD_REQUEST,
                "The request body could not be read.",
            )
                .into_response(),
        }
    }
}

/// Reads a whole body of at most `limit` bytes, taking its size from `budget` (when the request went through
/// `admit`) as it arrives: all of it at once when `Content-Length` announces it, so that a body over the budget is
/// refused before it is read. The permit returned holds that share until it is dropped, after the response.
pub(crate) async fn read_body(
    mut body: Body,
    content_length: Option<&HeaderValue>,
    limit: usize,
    budget: Option<&BodyBudget>,
) -> Result<(Vec<u8>, Option<OwnedSemaphorePermit>), BodyError> {
    use http_body_util::BodyExt;

    let announced = content_length
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    if announced.is_some_and(|n| n > limit) {
        return Err(BodyError::TooLarge);
    }
    let mut held = None;
    let mut reserved = 0;
    if let (Some(budget), Some(n)) = (budget, announced) {
        budget.take(n, &mut held)?;
        reserved = n;
    }
    let mut buf = Vec::with_capacity(announced.unwrap_or(0));
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|e| {
            if is_too_slow(&e) {
                BodyError::TooSlow
            } else {
                BodyError::Unreadable
            }
        })?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        if buf.len() + data.len() > limit {
            return Err(BodyError::TooLarge);
        }
        let beyond = (buf.len() + data.len()).saturating_sub(reserved);
        if let Some(budget) = budget
            && beyond > 0
        {
            budget.take(beyond, &mut held)?;
            reserved += beyond;
        }
        buf.extend_from_slice(&data);
    }
    Ok((buf, held))
}

fn is_too_slow(error: &axum::Error) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(e) = current {
        if e.is::<BodyTooSlow>() {
            return true;
        }
        current = e.source();
    }
    false
}

#[derive(Debug)]
struct BodyTooSlow;

impl std::fmt::Display for BodyTooSlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the request body arrived too slowly")
    }
}

impl std::error::Error for BodyTooSlow {}

/// A request body that fails once it falls behind `grace + received / min_rate` from its creation. Only polled
/// while a handler reads it, so a body nobody reads costs no timer.
struct DeadlineBody {
    inner: Body,
    start: tokio::time::Instant,
    grace: Duration,
    min_rate: u64,
    received: u64,
    deadline: Pin<Box<tokio::time::Sleep>>,
    expired: bool,
}

impl DeadlineBody {
    fn new(inner: Body, grace: Duration, min_rate: u64) -> Self {
        let start = tokio::time::Instant::now();
        Self {
            inner,
            start,
            grace,
            min_rate: min_rate.max(1),
            received: 0,
            deadline: Box::pin(tokio::time::sleep_until(start + grace)),
            expired: false,
        }
    }
}

impl http_body::Body for DeadlineBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        let this = self.get_mut();
        if this.expired {
            return Poll::Ready(None);
        }
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    this.received += data.len() as u64;
                    let earned =
                        Duration::from_secs_f64(this.received as f64 / this.min_rate as f64);
                    this.deadline
                        .as_mut()
                        .reset(this.start + this.grace + earned);
                }
                return Poll::Ready(Some(Ok(frame)));
            }
            Poll::Ready(other) => return Poll::Ready(other),
            Poll::Pending => {}
        }
        if this.deadline.as_mut().poll(cx).is_ready() {
            this.expired = true;
            return Poll::Ready(Some(Err(axum::Error::new(BodyTooSlow))));
        }
        Poll::Pending
    }

    fn is_end_stream(&self) -> bool {
        self.expired || self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests;
