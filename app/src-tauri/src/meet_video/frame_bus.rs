//! Loopback WebSocket frame bus for the meet camera.
//!
//! ## Why this exists
//!
//! The camera bridge that we inject into the Meet CEF webview needs a
//! live source of pre-rendered pixels — the rich Remotion-driven mascot
//! lives in the main OpenHuman renderer process, not inside Meet's
//! origin sandbox (see CLAUDE.md: "no new JS injection in CEF child
//! webviews"). We can't ship the Remotion runtime into meet.google.com,
//! and Tauri events don't reach child webviews. So the producer (main
//! app) and the consumer (CEF bridge) meet on a tiny localhost
//! WebSocket hosted by the shell.
//!
//! ## Protocol
//!
//! One WS endpoint per session, bound to `127.0.0.1:0` (OS-picked port).
//! Any client may connect:
//! - Binary frames *received* from a client become the new "latest" and
//!   are broadcast to all other connections.
//! - On connect each client immediately receives the current latest (if
//!   any) so consumers never see a black hole on join.
//!
//! In practice there's exactly one producer (the hidden Remotion host
//! in the main app) and exactly one consumer (the camera bridge in the
//! Meet webview). The "any client can produce" shape keeps the wire
//! protocol trivial — no auth handshake, no role negotiation, no path
//! dispatch — and the scope is already gated by being on loopback only.
//!
//! ## Lifecycle
//!
//! [`MeetVideoFrameBusState::start_session`] is called from
//! `meet_audio::start` alongside the audio + camera bridge install, so
//! the WS port is known before the camera bridge JS is templated.
//! [`MeetVideoFrameBusState::stop_session`] runs from `meet_audio::stop`
//! during window teardown; dropping the session aborts the accept loop
//! and closes any open consumer connections.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

/// Process-wide registry of active camera frame buses, keyed by meet
/// `request_id`. One bus per concurrent meet call.
#[derive(Default)]
pub struct MeetVideoFrameBusState {
    inner: Mutex<HashMap<String, FrameBusSession>>,
}

impl MeetVideoFrameBusState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a fresh loopback listener and spawn its accept loop. Returns
    /// the OS-picked port so the caller can template it into the camera
    /// bridge JS. Idempotent: if a session already exists for
    /// `request_id`, the previous one is dropped first.
    pub async fn start_session(&self, request_id: String) -> Result<u16, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("[meet-video-bus] bind: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("[meet-video-bus] local_addr: {e}"))?
            .port();

        // Latest-frame channel. `Arc<Vec<u8>>` so the per-connection
        // writers clone cheaply rather than copying full JPEG payloads.
        let (latest_tx, latest_rx) = watch::channel::<Arc<Vec<u8>>>(Arc::new(Vec::new()));

        let req_id = request_id.clone();
        let tx_for_loop = latest_tx.clone();
        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        log::info!("[meet-video-bus] connect req={req_id} peer={peer}");
                        let tx = tx_for_loop.clone();
                        let rx = latest_rx.clone();
                        let req_id_inner = req_id.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, tx, rx).await {
                                log::debug!(
                                    "[meet-video-bus] conn ended req={req_id_inner} peer={peer} err={e}"
                                );
                            }
                        });
                    }
                    Err(e) => {
                        log::warn!("[meet-video-bus] accept: {e}");
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
            }
        });

        let session = FrameBusSession {
            _latest_tx: latest_tx,
            accept_handle,
        };

        let mut guard = self.inner.lock().unwrap();
        if guard.remove(&request_id).is_some() {
            log::info!("[meet-video-bus] replaced existing session req={request_id}");
        }
        guard.insert(request_id.clone(), session);
        log::info!("[meet-video-bus] session started req={request_id} port={port}");
        Ok(port)
    }

    /// Drop the session and abort its accept loop. No-op if absent.
    pub fn stop_session(&self, request_id: &str) {
        if self.inner.lock().unwrap().remove(request_id).is_some() {
            log::info!("[meet-video-bus] session stopped req={request_id}");
        }
    }

    #[cfg(test)]
    pub fn has_session(&self, request_id: &str) -> bool {
        self.inner.lock().unwrap().contains_key(request_id)
    }
}

struct FrameBusSession {
    /// Held purely so the channel stays alive while the session is in
    /// the registry; per-connection tasks own the actual senders /
    /// receivers used on the wire.
    _latest_tx: watch::Sender<Arc<Vec<u8>>>,
    accept_handle: JoinHandle<()>,
}

impl Drop for FrameBusSession {
    fn drop(&mut self) {
        self.accept_handle.abort();
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    latest_tx: watch::Sender<Arc<Vec<u8>>>,
    mut latest_rx: watch::Receiver<Arc<Vec<u8>>>,
) -> Result<(), String> {
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|e| format!("ws handshake: {e}"))?;
    let (mut sink, mut stream) = ws.split();

    // Writer task: pump every new "latest" frame to this peer. Sends an
    // initial frame on connect so consumers don't render a black tile
    // while waiting for the producer's next tick.
    let writer = tokio::spawn(async move {
        let initial = latest_rx.borrow().clone();
        if !initial.is_empty() {
            if sink
                .send(Message::Binary((*initial).clone()))
                .await
                .is_err()
            {
                return;
            }
        }
        while latest_rx.changed().await.is_ok() {
            let frame = latest_rx.borrow().clone();
            if sink.send(Message::Binary((*frame).clone())).await.is_err() {
                break;
            }
        }
    });

    // Reader: any binary frame from this peer becomes the new latest
    // and fans out to all other peers via the watch channel.
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Binary(b)) => {
                let _ = latest_tx.send(Arc::new(b));
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("ws recv: {e}")),
        }
    }

    writer.abort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt as _, StreamExt as _};
    use tokio_tungstenite::connect_async;

    #[tokio::test]
    async fn frame_round_trips_producer_to_consumer() {
        let bus = MeetVideoFrameBusState::new();
        let port = bus.start_session("req1".into()).await.unwrap();
        let url = format!("ws://127.0.0.1:{port}");

        // Two clients: producer sends, consumer receives.
        let (mut consumer, _) = connect_async(&url).await.unwrap();
        let (mut producer, _) = connect_async(&url).await.unwrap();

        producer
            .send(Message::Binary(b"hello".to_vec()))
            .await
            .unwrap();

        let received = tokio::time::timeout(Duration::from_secs(2), consumer.next())
            .await
            .expect("consumer recv timed out")
            .expect("stream closed")
            .expect("ws err");
        match received {
            Message::Binary(b) => assert_eq!(b, b"hello"),
            other => panic!("expected binary, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stop_session_removes_entry() {
        let bus = MeetVideoFrameBusState::new();
        bus.start_session("r".into()).await.unwrap();
        assert!(bus.has_session("r"));
        bus.stop_session("r");
        assert!(!bus.has_session("r"));
    }
}
