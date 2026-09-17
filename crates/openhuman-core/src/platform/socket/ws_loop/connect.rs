//! Connection setup: the redirect-following WebSocket connect, the
//! Engine.IO/Socket.IO handshake reads, and a single connection's full
//! event loop ([`run_connection`]).

use std::sync::Arc;

use parking_lot::Mutex;

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::{mpsc, watch};
use tokio::time::{Duration, Instant};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{http::StatusCode, Error as WsError, Message as WsMessage},
};

use crate::api::models::socket::ConnectionStatus;
use crate::util::utf8_safe_prefix_at_byte_boundary;

use super::dispatch::handle_eio_message;
use crate::platform::socket::manager::{emit_state_change, SharedState};
use crate::platform::socket::types::{ConnectionOutcome, WsStream};

/// Maximum HTTP redirect hops to follow during a single WebSocket connect attempt.
///
/// Cloudflare and similar edges return a single 301 (e.g. when the configured
/// `BACKEND_URL` is `http://...` and the server only serves the upgrade over TLS)
/// before the upgrade succeeds. Three hops is enough headroom for chained
/// redirects while still bounding pathological loops.
const MAX_REDIRECT_HOPS: u8 = 3;

/// Upper bound on one WebSocket connect attempt — DNS resolution, TCP
/// connect, TLS handshake and the HTTP upgrade response together — applied
/// per redirect hop.
///
/// `connect_async` carries no deadline of its own, so a path that accepts the
/// TCP connection and then goes silent (an ingress that blackholes the
/// upgrade, a stalled TLS handshake, a resolver that never answers) used to
/// park the reconnect loop until the far end gave up. Both ten-minute
/// reconnect gaps reported in #6256 measure ~608 s: the ~600 s ingress
/// ceiling documented on #5603, plus one backoff sleep and a handshake. The
/// client had no bound of its own. Ten seconds matches the two handshake
/// reads that follow (`read_eio_open`, `read_sio_connect_ack`) and the
/// renderer's own socket.io connect timeout: a healthy path completes in well
/// under a second and a remote or tunnelled one in a few, while a hang now
/// surfaces as a `Failed` attempt that the next backoff cycle retries.
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What the reconnect loop knows about the outage a connection attempt is
/// recovering from, so a successful handshake can say how long the socket
/// was down and how many attempts it took (#6256).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ReconnectContext {
    /// When the outage began: the moment the previous connection was lost,
    /// or the moment the first failed attempt *started* if there was no
    /// connection yet — so a dial that stalled for the whole connect deadline
    /// is counted. `None` on a first connect that has not failed.
    pub(super) outage_started: Option<Instant>,
    /// Whether a live connection preceded this outage — distinguishes
    /// "reconnected" from "connected after initial failures" in the log.
    pub(super) lost_previous: bool,
    /// Attempts that ended in `ConnectionOutcome::Failed` since the outage began.
    pub(super) failed_attempts: u32,
}

// ---------------------------------------------------------------------------
// Background loop
// ---------------------------------------------------------------------------

/// Run a single WebSocket connection through handshake and event loop.
///
/// `ws_url` is taken by mutable reference so that any HTTP redirect we follow
/// during the upgrade (see `connect_with_redirects`) is pinned for the next
/// reconnect attempt — we don't want to re-hit the redirect every time the
/// loop backs off and retries.
pub(super) async fn run_connection(
    ws_url: &mut String,
    token: &str,
    shared: &Arc<SharedState>,
    emit_rx: &mut mpsc::UnboundedReceiver<String>,
    shutdown_rx: &mut watch::Receiver<bool>,
    internal_tx: &mpsc::UnboundedSender<String>,
    emit_ready: &Mutex<bool>,
    reconnect: ReconnectContext,
) -> ConnectionOutcome {
    log::info!("[socket] WS URL: {}", ws_url);

    // 2. Connect via WebSocket (uses rustls TLS for wss://). Follow HTTP 3xx
    //    redirects up to MAX_REDIRECT_HOPS so a `http://` config behind a
    //    Cloudflare-style edge that 301s to `https://` connects cleanly
    //    instead of looping at error-level forever.
    let ws_stream = match connect_with_redirects(ws_url, shared).await {
        Ok(stream) => stream,
        Err(e) => return ConnectionOutcome::Failed(format!("WebSocket connect: {e}")),
    };

    log::info!("[socket] WebSocket connected, starting handshake");
    let (mut ws_write, mut ws_read) = ws_stream.split();

    // 3. Read Engine.IO OPEN packet (type 0)
    let open_data =
        match tokio::time::timeout(Duration::from_secs(10), read_eio_open(&mut ws_read)).await {
            Ok(Ok(data)) => data,
            Ok(Err(e)) => return ConnectionOutcome::Failed(format!("EIO OPEN: {e}")),
            Err(_) => return ConnectionOutcome::Failed("Timeout waiting for EIO OPEN".into()),
        };

    let ping_interval = open_data
        .get("pingInterval")
        .and_then(|v| v.as_u64())
        .unwrap_or(25000);
    let ping_timeout_ms = open_data
        .get("pingTimeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(20000);
    let eio_sid = open_data.get("sid").and_then(|v| v.as_str()).unwrap_or("?");
    log::info!(
        "[socket] EIO OPEN: sid={}, ping={}ms, timeout={}ms",
        eio_sid,
        ping_interval,
        ping_timeout_ms
    );

    // 4. Send Socket.IO CONNECT with auth token
    let connect_payload = json!({"token": token});
    let connect_msg = format!("40{}", serde_json::to_string(&connect_payload).unwrap());
    if let Err(e) = ws_write.send(WsMessage::Text(connect_msg.into())).await {
        return ConnectionOutcome::Failed(format!("Send SIO CONNECT: {e}"));
    }

    // 5. Read Socket.IO CONNECT ACK (type 40)
    let ack_data =
        match tokio::time::timeout(Duration::from_secs(10), read_sio_connect_ack(&mut ws_read))
            .await
        {
            Ok(Ok(data)) => data,
            Ok(Err(e)) => return ConnectionOutcome::Failed(format!("SIO CONNECT: {e}")),
            Err(_) => {
                return ConnectionOutcome::Failed("Timeout waiting for SIO CONNECT ACK".into())
            }
        };

    let sio_sid = ack_data
        .get("sid")
        .and_then(|v| v.as_str())
        .map(String::from);
    log::info!("[socket] SIO CONNECT ACK: sid={:?}", sio_sid);

    // 6. Update state to Connected and mark this connection ready to emit.
    // The readiness flag is the emit gate (see `SocketManager::emit`): only now,
    // past a completed Socket.IO CONNECT ACK, is a queued message guaranteed to
    // ride *this* live socket rather than be dropped by `drain_pending_emits`.
    // A later server `error` EVENT flips `status` to `Error` for the UI but does
    // not return from this function, so the socket stays live and this flag
    // stays set — emits keep flowing until the connection is actually torn down.
    *shared.status.write() = ConnectionStatus::Connected;
    *shared.socket_id.write() = sio_sid;
    *emit_ready.lock() = true;
    emit_state_change(shared);
    if let Some(started) = reconnect.outage_started {
        log::info!(
            "[socket] {} after {:.1}s ({} failed attempt(s))",
            if reconnect.lost_previous {
                "Reconnected"
            } else {
                "Connected"
            },
            started.elapsed().as_secs_f64(),
            reconnect.failed_attempts
        );
    }

    // 7. Main event loop
    // Deadline = pingInterval + pingTimeout + 5 s grace so minor server-side
    // jitter doesn't cause a spurious reconnect on a healthy connection.
    let timeout_ms = ping_interval + ping_timeout_ms + 5_000;
    let timeout_duration = Duration::from_millis(timeout_ms);
    let mut deadline = Instant::now() + timeout_duration;
    // Deadline diagnostics (#6256): when the deadline fires, the warning says
    // how old this connection is, how many Engine.IO pings it ever saw, and
    // how many frames we pushed into the silence — enough to tell "server
    // went quiet" from "path went dead" from the log alone. A drop that
    // always lands at the same connection age points at a lifetime ceiling
    // on the path (#5603); zero pings on a minutes-old connection points at
    // the server.
    let connected_at = Instant::now();
    let mut pings_received: u32 = 0;
    let mut sent_since_last_frame: u32 = 0;

    loop {
        tokio::select! {
            msg = ws_read.next() => {
                match msg {
                    Some(Ok(WsMessage::Text(text))) => {
                        deadline = Instant::now() + timeout_duration;
                        sent_since_last_frame = 0;
                        let frame: &str = &text;
                        if frame.starts_with('2') {
                            pings_received = pings_received.saturating_add(1);
                        }
                        handle_eio_message(frame, internal_tx, shared);
                    }
                    Some(Ok(WsMessage::Ping(data))) => {
                        sent_since_last_frame = 0;
                        let _ = ws_write.send(WsMessage::Pong(data)).await;
                        sent_since_last_frame = sent_since_last_frame.saturating_add(1);
                    }
                    Some(Ok(WsMessage::Close(_))) => {
                        log::info!("[socket] Server closed WebSocket");
                        return ConnectionOutcome::Lost("Server closed connection".into());
                    }
                    Some(Err(e)) => {
                        return ConnectionOutcome::Lost(format!("WebSocket error: {e}"));
                    }
                    None => {
                        return ConnectionOutcome::Lost("WebSocket stream ended".into());
                    }
                    _ => {
                        // Binary, Pong, Frame: still proof the path is alive.
                        sent_since_last_frame = 0;
                    }
                }
            }
            outgoing = emit_rx.recv() => {
                match outgoing {
                    Some(msg) => {
                        if let Err(e) = ws_write.send(WsMessage::Text(msg.into())).await {
                            return ConnectionOutcome::Lost(format!("Send failed: {e}"));
                        }
                        sent_since_last_frame = sent_since_last_frame.saturating_add(1);
                    }
                    None => {
                        let _ = ws_write.send(WsMessage::Close(None)).await;
                        return ConnectionOutcome::Shutdown;
                    }
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                log::warn!(
                    "[socket] No server ping received within {}ms (interval={}ms + timeout={}ms + 5s grace); connection age {:.0}s, {} EIO ping(s) received on this connection, {} frame(s) sent since the last server frame; reconnecting",
                    timeout_ms,
                    ping_interval,
                    ping_timeout_ms,
                    connected_at.elapsed().as_secs_f64(),
                    pings_received,
                    sent_since_last_frame,
                );
                return ConnectionOutcome::Lost("Ping timeout".into());
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    log::info!("[socket] Shutdown signal received");
                    let _ = ws_write.send(WsMessage::Close(None)).await;
                    return ConnectionOutcome::Shutdown;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Handshake helpers
// ---------------------------------------------------------------------------

/// Read the Engine.IO OPEN packet (type 0) from the WebSocket.
///
/// Format: `0{"sid":"...","upgrades":[],"pingInterval":25000,"pingTimeout":20000}`
async fn read_eio_open(
    ws_read: &mut futures_util::stream::SplitStream<WsStream>,
) -> Result<serde_json::Value, String> {
    loop {
        match ws_read.next().await {
            Some(Ok(WsMessage::Text(text))) => {
                let s: &str = &text;
                if let Some(json_str) = s.strip_prefix('0') {
                    return serde_json::from_str(json_str)
                        .map_err(|e| format!("Parse EIO OPEN JSON: {e}"));
                }
                log::debug!(
                    "[socket] Skipping non-OPEN packet: {}",
                    utf8_safe_prefix_at_byte_boundary(s, 40)
                );
            }
            Some(Ok(_)) => continue,
            Some(Err(e)) => return Err(format!("WS error during handshake: {e}")),
            None => return Err("WebSocket closed before OPEN".into()),
        }
    }
}

/// Read the Socket.IO CONNECT ACK (type 40) from the WebSocket.
///
/// Format: `40{"sid":"..."}` or `44{"message":"error"}` for connect error.
async fn read_sio_connect_ack(
    ws_read: &mut futures_util::stream::SplitStream<WsStream>,
) -> Result<serde_json::Value, String> {
    loop {
        match ws_read.next().await {
            Some(Ok(WsMessage::Text(text))) => {
                let s: &str = &text;
                // Engine.IO MESSAGE (4) + Socket.IO CONNECT (0)
                if let Some(json_str) = s.strip_prefix("40") {
                    if json_str.is_empty() {
                        return Ok(json!({}));
                    }
                    return serde_json::from_str(json_str)
                        .map_err(|e| format!("Parse CONNECT ACK: {e}"));
                }
                // Engine.IO MESSAGE (4) + Socket.IO CONNECT_ERROR (4)
                if let Some(json_str) = s.strip_prefix("44") {
                    let err: serde_json::Value =
                        serde_json::from_str(json_str).unwrap_or(json!({"message": "unknown"}));
                    let msg = err
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Connect error");
                    return Err(format!("Socket.IO connect error: {msg}"));
                }
                // Engine.IO PING (2) — respond via log, can't write from here
                if s.starts_with('2') {
                    log::debug!("[socket] EIO ping during handshake (will respond after)");
                    continue;
                }
                log::debug!(
                    "[socket] Skipping packet during SIO handshake: {}",
                    utf8_safe_prefix_at_byte_boundary(s, 40)
                );
            }
            Some(Ok(_)) => continue,
            Some(Err(e)) => return Err(format!("WS error during SIO handshake: {e}")),
            None => return Err("WebSocket closed before CONNECT ACK".into()),
        }
    }
}
/// Connect to `ws_url`, following HTTP 3xx redirects up to `MAX_REDIRECT_HOPS`.
///
/// Plain `connect_async` returns an error on any non-`101 Switching Protocols`
/// response, so a Cloudflare-style `http://… → https://…` 301 (which happens
/// whenever `BACKEND_URL` is configured without TLS) used to be fatal — the
/// reconnect loop would hammer the same dead URL forever at error level.
///
/// On each redirect we:
///   1. resolve the `Location` header against the current URL (handles relative
///      Location values),
///   2. upgrade the scheme so the next attempt is still a WebSocket
///      (`http` → `ws`, `https` → `wss`; `ws`/`wss` pass through),
///   3. mutate `ws_url` in place so the redirect target is pinned for
///      subsequent reconnects (no need to re-hit the redirect every retry),
///   4. record a one-shot warning in `SharedState.error` the first time we
///      follow a redirect so the UI can surface "your `BACKEND_URL` is stale".
///
/// On non-redirect failures the original error is returned and the caller
/// counts it toward the exponential backoff like before.
///
/// Every hop is bounded by [`CONNECT_TIMEOUT`]; a hop that outlives it fails
/// the attempt with a timed-out `WsError::Io` (#6256).
pub(super) async fn connect_with_redirects(
    ws_url: &mut String,
    shared: &Arc<SharedState>,
) -> Result<WsStream, WsError> {
    connect_with_redirects_within(ws_url, shared, CONNECT_TIMEOUT).await
}

/// [`connect_with_redirects`] with an explicit per-hop deadline. Split out so
/// a test can prove the bound with a sub-second budget instead of waiting out
/// [`CONNECT_TIMEOUT`].
pub(super) async fn connect_with_redirects_within(
    ws_url: &mut String,
    shared: &Arc<SharedState>,
    connect_timeout: Duration,
) -> Result<WsStream, WsError> {
    let original = ws_url.clone();
    for hop in 0..=MAX_REDIRECT_HOPS {
        let attempt =
            match tokio::time::timeout(connect_timeout, connect_async(ws_url.as_str())).await {
                Ok(attempt) => attempt,
                Err(_elapsed) => return Err(connect_timed_out(connect_timeout)),
            };
        match attempt {
            Ok((stream, _response)) => return Ok(stream),
            Err(WsError::Http(response)) if is_redirect_status(response.status()) => {
                if hop == MAX_REDIRECT_HOPS {
                    log::error!(
                        "[socket] Exceeded {MAX_REDIRECT_HOPS} redirect hops starting from {original}; giving up"
                    );
                    return Err(WsError::Http(response));
                }
                let location = match extract_location_header(&response) {
                    Some(loc) => loc,
                    None => {
                        log::error!(
                            "[socket] Redirect {} from {ws_url} missing Location header",
                            response.status()
                        );
                        return Err(WsError::Http(response));
                    }
                };
                let next_url = match resolve_redirect_target(ws_url, &location) {
                    Ok(url) => url,
                    Err(e) => {
                        log::error!(
                            "[socket] Cannot follow redirect to {location} from {ws_url}: {e}"
                        );
                        return Err(WsError::Http(response));
                    }
                };
                log::warn!(
                    "[socket] Server redirected ({}) {} → {}",
                    response.status(),
                    ws_url,
                    next_url
                );
                // Only persist a stale-BACKEND_URL warning for permanent
                // redirects (301 / 308). Temporary redirects (302 / 307) say
                // "this time, go elsewhere" — the configured BACKEND_URL is
                // still correct, and surfacing a "please update config" hint
                // for a transient hop would be misleading. Per CodeRabbit
                // review on PR #1547.
                if matches!(
                    response.status(),
                    StatusCode::MOVED_PERMANENTLY | StatusCode::PERMANENT_REDIRECT
                ) {
                    record_redirect_warning(shared, &original, &next_url);
                }
                *ws_url = next_url;
            }
            Err(e) => return Err(e),
        }
    }
    // Unreachable: the loop either returns Ok, returns the redirect error after
    // exhausting hops, or returns a non-redirect Err.
    unreachable!("connect_with_redirects exited loop without returning")
}

/// The error a connect attempt surfaces when it outlives its deadline.
///
/// Rendered through `WsError::Io` so `run_connection`'s
/// `"WebSocket connect: IO error: …"` wrapping stays uniform, and worded with
/// the `operation timed out` phrase the observability classifier already
/// treats as a user-environment transport shape
/// (`core::observability::is_network_unreachable_message`): a sustained hang
/// escalates to one `warn` breadcrumb on the fifth attempt, never to Sentry —
/// the same treatment `ETIMEDOUT` gets when the OS reports it.
fn connect_timed_out(after: Duration) -> WsError {
    WsError::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("operation timed out after {after:?} waiting for the WebSocket upgrade"),
    ))
}

/// Statuses we treat as "follow the Location and retry".
///
/// 308 (Permanent Redirect) and 307 (Temporary Redirect) explicitly preserve
/// the method; 301/302 historically do too for upgrade requests in practice.
/// Anything else (300, 304, ...) stays an error.
pub(super) fn is_redirect_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

pub(super) fn extract_location_header(
    response: &tokio_tungstenite::tungstenite::http::Response<Option<Vec<u8>>>,
) -> Option<String> {
    response
        .headers()
        .get(tokio_tungstenite::tungstenite::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Resolve `location` against `current_ws_url` and rewrite the scheme so the
/// result is still a valid WebSocket URL.
///
/// `location` may be absolute (`https://host/path?q=1`) or relative
/// (`/socket.io/?EIO=4`). We use the `url` crate's relative-URL parser to do
/// the join the same way browsers do, then map `http`→`ws` / `https`→`wss`.
pub(super) fn resolve_redirect_target(
    current_ws_url: &str,
    location: &str,
) -> Result<String, String> {
    let base = url::Url::parse(current_ws_url).map_err(|e| format!("invalid current URL: {e}"))?;
    let resolved = base
        .join(location)
        .map_err(|e| format!("invalid Location {location:?}: {e}"))?;

    let upgraded_scheme = match resolved.scheme() {
        "http" => "ws",
        "https" => "wss",
        "ws" | "wss" => resolved.scheme(),
        other => return Err(format!("unsupported scheme in Location: {other}")),
    };

    let mut next = resolved.clone();
    next.set_scheme(upgraded_scheme)
        .map_err(|_| format!("failed to set scheme {upgraded_scheme} on {resolved}"))?;
    Ok(next.to_string())
}

/// Persist a one-shot, user-visible warning that the backend redirected the
/// configured socket URL. Subsequent redirects in the same connect attempt
/// don't overwrite — the first hop carries the actionable signal.
pub(super) fn record_redirect_warning(shared: &Arc<SharedState>, original: &str, resolved: &str) {
    let mut slot = shared.error.write();
    if slot.is_some() {
        return;
    }
    *slot = Some(format!(
        "Backend redirected {original} → {resolved}. Update BACKEND_URL to the resolved URL to avoid the extra hop."
    ));
}
