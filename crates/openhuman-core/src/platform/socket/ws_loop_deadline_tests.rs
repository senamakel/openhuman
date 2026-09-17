//! #6256 — the connect deadline, the `Reconnecting` status between attempts,
//! and the ping-deadline diagnostics. Sibling of `ws_loop_reconnect_tests.rs`,
//! which sits at the layout gate's line limit.

use super::*;

use crate::platform::socket::types::ConnectionStatus;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Error as WsError;

/// #6256: a connect attempt has a deadline of its own. A listener that
/// accepts the TCP connection and never answers the upgrade used to park
/// `connect_with_redirects` until the far end gave up — ten minutes in the
/// field, the ~600 s ingress ceiling from #5603 plus a backoff sleep. With
/// the bound the attempt fails fast, as the timed-out transport shape the
/// observability classifier already demotes.
#[tokio::test]
async fn connect_with_redirects_times_out_when_upgrade_never_answers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        // Hold the connection open without ever writing a byte back.
        let _ = release_rx.await;
        drop(stream);
    });

    let shared = make_shared();
    let mut url = format!("ws://{addr}/socket.io/?EIO=4&transport=websocket");
    let started = tokio::time::Instant::now();
    let err =
        connect_with_redirects_within(&mut url, &shared, tokio::time::Duration::from_millis(200))
            .await
            .expect_err("a silent upgrade must not hang the connect attempt");
    let elapsed = started.elapsed();
    assert!(
        elapsed < tokio::time::Duration::from_secs(5),
        "deadline did not bound the attempt: took {elapsed:?}"
    );
    assert!(
        matches!(&err, WsError::Io(io) if io.kind() == std::io::ErrorKind::TimedOut),
        "expected a timed-out IO error, got {err}"
    );
    assert!(
        err.to_string().contains("operation timed out"),
        "message must carry the classifier's transport shape: {err}"
    );
    let _ = release_tx.send(());
}

/// The production budget spans DNS + TCP + TLS + the upgrade response, so it
/// must never be tighter than the single 10 s reads that follow it
/// (`read_eio_open`, `read_sio_connect_ack`); otherwise a slow-but-healthy
/// link would fail at the connect step it used to survive.
#[test]
fn connect_timeout_is_not_tighter_than_a_handshake_read() {
    assert!(CONNECT_TIMEOUT >= tokio::time::Duration::from_secs(10));
}

/// #6256: between attempts the loop is alive and will retry, and it says so.
/// `Disconnected` is reserved for a loop that stopped (signed out, session
/// expired, shutdown), so the connectivity chip can tell "down, retrying"
/// from "never wanted". A listener that accepts and drops every connection
/// makes each attempt fail and sends the loop into its backoff sleep.
#[tokio::test]
async fn ws_loop_reports_reconnecting_between_attempts() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            drop(stream);
        }
    });

    let shared = make_shared();
    *shared.status.write() = ConnectionStatus::Disconnected;
    let (_emit_tx, emit_rx) = mpsc::unbounded_channel::<String>();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (internal_tx, _internal_rx) = mpsc::unbounded_channel::<String>();

    let loop_shared = Arc::clone(&shared);
    let handle = tokio::spawn(async move {
        ws_loop(
            http_base_for(addr),
            static_token_provider("backoff-token".to_string()),
            loop_shared,
            emit_rx,
            shutdown_rx,
            internal_tx,
            Arc::new(Mutex::new(false)),
        )
        .await;
    });

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    loop {
        if *shared.status.read() == ConnectionStatus::Reconnecting {
            // The loop is alive and retrying: that is what `connectivity_diag`
            // reports as `socket_loop_active` so the chip can show an outage.
            assert!(
                shared
                    .loop_active
                    .load(std::sync::atomic::Ordering::Acquire),
                "loop_active must be raised while the loop is retrying"
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "loop never reported Reconnecting during its backoff sleep; status={:?}",
            *shared.status.read()
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
    }

    let _ = shutdown_tx.send(true);
    let _ = tokio::time::timeout(tokio::time::Duration::from_secs(5), handle).await;
    // Once the loop is gone the status is `Disconnected` again — stopped, not
    // retrying — and the liveness flag is lowered by the loop's drop guard.
    assert_eq!(*shared.status.read(), ConnectionStatus::Disconnected);
    assert!(
        !shared
            .loop_active
            .load(std::sync::atomic::Ordering::Acquire),
        "loop_active must be lowered once the loop task has exited"
    );
    assert!(
        !shared
            .loop_stopped_on_failure
            .load(std::sync::atomic::Ordering::Acquire),
        "a requested shutdown is not a failure"
    );
}

/// A loop that stops for good — the backend rejects the stored token and the
/// provider has nothing fresher — lowers `loop_active` and raises
/// `loop_stopped_on_failure`, so `connectivity_diag` can tell "stopped, sign
/// in again" from "never wanted" (Codex review on #6270).
#[tokio::test]
async fn ws_loop_marks_a_terminal_stop_as_failure() {
    let addr = spawn_mock_invalid_token_server().await;

    let shared = make_shared();
    *shared.status.write() = ConnectionStatus::Disconnected;
    let (_emit_tx, emit_rx) = mpsc::unbounded_channel::<String>();
    // Kept alive: a dropped sender would make `shutdown_rx.changed()` fire.
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    let (internal_tx, _internal_rx) = mpsc::unbounded_channel::<String>();

    let loop_shared = Arc::clone(&shared);
    let handle = tokio::spawn(async move {
        ws_loop(
            http_base_for(addr),
            static_token_provider("dead-token".to_string()),
            loop_shared,
            emit_rx,
            shutdown_rx,
            internal_tx,
            Arc::new(Mutex::new(false)),
        )
        .await;
    });

    tokio::time::timeout(tokio::time::Duration::from_secs(10), handle)
        .await
        .expect("loop must stop on its own once the token is provably dead")
        .expect("loop task must not panic");

    assert_eq!(*shared.status.read(), ConnectionStatus::Disconnected);
    assert!(!shared
        .loop_active
        .load(std::sync::atomic::Ordering::Acquire));
    assert!(
        shared
            .loop_stopped_on_failure
            .load(std::sync::atomic::Ordering::Acquire),
        "a dead session token is a terminal failure"
    );
    assert!(
        shared
            .error
            .read()
            .as_deref()
            .is_some_and(|e| e.contains("session expired")),
        "the stop reason must be user-visible"
    );
}

/// Spawn an EIO server that answers two connections and reports each client
/// SIO CONNECT on `forward_tx`. Both connections complete the handshake and
/// are then **held open until `release_rx` fires** — the server never closes
/// them, so only the client's own deadline can end the first one. The first
/// connection additionally sends one Engine.IO ping, one WebSocket-level ping
/// and one binary frame before going silent, so every inbound arm of the
/// event loop runs. Short ping intervals keep the client's deadline
/// (`interval + timeout + 5 s grace`) at ~5.4 s instead of 50 s.
async fn spawn_mock_server_that_goes_silent(
    forward_tx: mpsc::UnboundedSender<String>,
    release_rx: tokio::sync::oneshot::Receiver<()>,
) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let open = r#"0{"sid":"mock-eio-sid","upgrades":[],"pingInterval":200,"pingTimeout":200}"#;
        let mut held = Vec::new();
        for connection in 0..2u8 {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let ws = accept_async(stream).await.expect("ws accept");
            let (mut write, mut read) = ws.split();
            let _ = write.send(WsMessage::Text(open.into())).await;
            if let Some(Ok(WsMessage::Text(t))) = read.next().await {
                let _ = forward_tx.send(t.to_string());
            }
            let _ = write
                .send(WsMessage::Text(r#"40{"sid":"mock-sio-sid"}"#.into()))
                .await;
            if connection == 0 {
                let _ = write.send(WsMessage::Text("2".into())).await;
                let _ = write.send(WsMessage::Ping(vec![1].into())).await;
                let _ = write.send(WsMessage::Binary(vec![0].into())).await;
            }
            held.push(write.reunite(read).expect("reunite"));
        }
        let _ = release_rx.await;
        drop(held);
    });
    addr
}

/// #6256 diagnostics path: a server that completes the handshake and then
/// goes silent without closing is ended by the client's own ping deadline,
/// the loop reports the loss and reconnects on the next attempt, and the
/// deadline warning plus the `Reconnected after …` line execute on the way.
#[tokio::test]
async fn ws_loop_reconnects_after_ping_deadline_on_a_silent_server() {
    let (fwd_tx, mut fwd_rx) = mpsc::unbounded_channel::<String>();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let addr = spawn_mock_server_that_goes_silent(fwd_tx, release_rx).await;

    let shared = make_shared();
    *shared.status.write() = ConnectionStatus::Disconnected;
    let (emit_tx, emit_rx) = mpsc::unbounded_channel::<String>();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    // The loop answers the server's Engine.IO ping through this channel.
    let internal_tx = emit_tx.clone();
    drop(emit_tx);

    let loop_shared = Arc::clone(&shared);
    let handle = tokio::spawn(async move {
        ws_loop(
            http_base_for(addr),
            static_token_provider("deadline-token".to_string()),
            loop_shared,
            emit_rx,
            shutdown_rx,
            internal_tx,
            Arc::new(Mutex::new(false)),
        )
        .await;
    });

    // Two SIO CONNECT frames mean two handshakes. The second can only follow
    // the client's deadline (200 + 200 + 5000 ms) ending the first connection,
    // because the server never closes it.
    let started = tokio::time::Instant::now();
    let mut connects = 0u8;
    while connects < 2 {
        match tokio::time::timeout(tokio::time::Duration::from_secs(20), fwd_rx.recv()).await {
            Ok(Some(frame)) if frame.starts_with("40") => connects += 1,
            Ok(Some(_)) => {}
            _ => break,
        }
    }
    let elapsed = started.elapsed();
    assert_eq!(
        connects, 2,
        "client never reconnected after the server went silent (elapsed {elapsed:?})"
    );
    assert!(
        elapsed >= tokio::time::Duration::from_millis(5_400),
        "second handshake happened before the ping deadline could have fired: {elapsed:?}"
    );

    for _ in 0..50 {
        if *shared.status.read() == ConnectionStatus::Connected {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
    assert_eq!(*shared.status.read(), ConnectionStatus::Connected);

    let _ = shutdown_tx.send(true);
    let _ = tokio::time::timeout(tokio::time::Duration::from_secs(5), handle).await;
    let _ = release_tx.send(());
}
