//! Terminal chat event loop.
//!
//! Bridges three async sources over `tokio::select!`:
//!   * **keyboard** — a blocking crossterm reader thread forwards `Event`s over
//!     an mpsc channel (crossterm's own async `EventStream` needs the
//!     `event-stream` feature; the poll+forward thread keeps the dep surface
//!     minimal and exits promptly via the shared `shutdown` flag),
//!   * **web-channel broadcast** — the same `web_chat` event stream the desktop
//!     app consumes, folded into [`TranscriptState`] by its reducer,
//!   * **a spinner ticker** — animates the streaming indicator.
//!
//! All state transitions are logged with the `[tui]` prefix to the file-only
//! subscriber (see `logging::init_for_tui`); nothing is ever `println!`'d.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde_json::json;
use tokio::sync::broadcast;

use crate::core::runtime::CoreRuntime;
use crate::core::socketio::WebChannelEvent;
use crate::openhuman::web_chat;

use super::render::{self, UiState};
use super::state::TranscriptState;
use super::terminal::TerminalGuard;

/// Run the terminal chat loop until the user quits (Ctrl+C / Ctrl+D) or the
/// web-channel bus closes. The [`TerminalGuard`] restores the terminal on every
/// exit path, including panics.
pub async fn run(
    runtime: Arc<CoreRuntime>,
    client_id: String,
    thread_id: String,
    mut web_rx: broadcast::Receiver<WebChannelEvent>,
) -> anyhow::Result<()> {
    let mut guard = TerminalGuard::enter()?;

    let mut state = TranscriptState::new(client_id.clone());
    state.push_system(format!(
        "Connected · thread {thread_id}. Type a message and press Enter. Ctrl+C to quit."
    ));
    let mut ui = UiState::new(thread_id, client_id.clone());

    // Blocking crossterm reader → async channel.
    let (input_tx, mut input_rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let reader_shutdown = shutdown.clone();
    let reader = std::thread::spawn(move || {
        while !reader_shutdown.load(Ordering::Relaxed) {
            match event::poll(Duration::from_millis(100)) {
                Ok(true) => match event::read() {
                    Ok(ev) => {
                        if input_tx.send(ev).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Ok(false) => {}
                Err(_) => break,
            }
        }
    });

    let mut ticker = tokio::time::interval(Duration::from_millis(120));
    let mut quit = false;

    while !quit {
        guard.terminal().draw(|f| render::draw(f, &state, &ui))?;

        tokio::select! {
            maybe_ev = input_rx.recv() => match maybe_ev {
                Some(Event::Key(key)) => {
                    if handle_key(key, &runtime, &client_id, &mut state, &mut ui).await {
                        quit = true;
                    }
                }
                Some(_) => {} // resize / mouse / paste — redraw next iteration
                None => quit = true, // reader thread gone
            },
            recv = web_rx.recv() => match recv {
                Ok(ev) => state.apply_event(&ev),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("[tui] web-channel lagged, dropped {n} events");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    log::warn!("[tui] web-channel closed — exiting");
                    quit = true;
                }
            },
            _ = ticker.tick() => {
                ui.spinner_tick = ui.spinner_tick.wrapping_add(1);
            }
        }
    }

    shutdown.store(true, Ordering::Relaxed);
    let _ = reader.join();
    log::info!("[tui] event loop exited");
    Ok(())
}

/// Handle a key event. Returns `true` when the app should quit.
async fn handle_key(
    key: KeyEvent,
    runtime: &Arc<CoreRuntime>,
    client_id: &str,
    state: &mut TranscriptState,
    ui: &mut UiState,
) -> bool {
    // Ignore key-release events (Windows / kitty report both edges).
    if key.kind == KeyEventKind::Release {
        return false;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Char('c') if ctrl => {
            log::info!("[tui] quit via Ctrl+C");
            return true;
        }
        KeyCode::Char('d') if ctrl => {
            log::info!("[tui] quit via Ctrl+D");
            return true;
        }
        KeyCode::Char('n') if ctrl => new_thread(runtime, state, ui).await,
        KeyCode::Esc => cancel_turn(runtime, client_id, &ui.thread_id, state),
        KeyCode::PageUp => {
            ui.scroll_from_bottom = ui.scroll_from_bottom.saturating_add(5);
        }
        KeyCode::PageDown => {
            ui.scroll_from_bottom = ui.scroll_from_bottom.saturating_sub(5);
        }
        KeyCode::Enter => send_message(runtime, client_id, state, ui),
        KeyCode::Backspace => {
            ui.input.pop();
        }
        KeyCode::Char(c) if !ctrl => ui.input.push(c),
        _ => {}
    }
    false
}

/// Queue a chat turn on the current thread. Fire-and-forget: the reply streams
/// back over the web-channel bus and is folded in by the reducer.
fn send_message(
    runtime: &Arc<CoreRuntime>,
    client_id: &str,
    state: &mut TranscriptState,
    ui: &mut UiState,
) {
    let message = ui.input.trim().to_string();
    if message.is_empty() {
        return;
    }
    ui.input.clear();
    ui.scroll_from_bottom = 0;
    state.begin_user_turn(&message);
    log::info!(
        "[tui] send message len={} thread={}",
        message.len(),
        ui.thread_id
    );

    let rt = runtime.clone();
    let cid = client_id.to_string();
    let tid = ui.thread_id.clone();
    tokio::spawn(async move {
        let params = json!({
            "client_id": cid,
            "thread_id": tid,
            "message": message,
            "source": "tui",
        });
        if let Err(e) = rt.invoke("openhuman.channel_web_chat", params).await {
            log::error!("[tui] openhuman.channel_web_chat failed: {e}");
            // Surface the failure in-transcript via a synthetic chat_error so
            // the reducer clears the streaming state and shows the reason.
            web_chat::publish_web_channel_event(WebChannelEvent {
                event: "chat_error".to_string(),
                client_id: cid,
                thread_id: tid,
                message: Some(format!("Failed to send: {e}")),
                error_type: Some("transport".to_string()),
                ..Default::default()
            });
        }
    });
}

/// Cancel the in-flight turn on the current thread. The core emits a
/// `chat_error` ("Cancelled") which the reducer renders.
fn cancel_turn(
    runtime: &Arc<CoreRuntime>,
    client_id: &str,
    thread_id: &str,
    state: &TranscriptState,
) {
    if !state.is_streaming() {
        return;
    }
    log::info!("[tui] cancel turn thread={thread_id}");
    let rt = runtime.clone();
    let cid = client_id.to_string();
    let tid = thread_id.to_string();
    tokio::spawn(async move {
        // Omit `request_id` → stop whatever is running on the thread.
        let params = json!({ "client_id": cid, "thread_id": tid });
        if let Err(e) = rt.invoke("openhuman.channel_web_cancel", params).await {
            log::error!("[tui] openhuman.channel_web_cancel failed: {e}");
        }
    });
}

/// Create a fresh thread and switch the UI to it. Awaited inline (fast, local
/// SQLite write) so `ui.thread_id` can be updated with the result.
async fn new_thread(runtime: &Arc<CoreRuntime>, state: &mut TranscriptState, ui: &mut UiState) {
    log::info!("[tui] creating new thread");
    match runtime
        .invoke("openhuman.threads_create_new", json!({}))
        .await
        .ok()
        .and_then(|v| super::runner::extract_thread_id(&v))
    {
        Some(new_id) => {
            ui.thread_id = new_id.clone();
            ui.scroll_from_bottom = 0;
            state.push_system(format!("Started a new thread · {new_id}"));
            log::info!("[tui] switched to new thread {new_id}");
        }
        None => {
            state.push_system("Could not create a new thread (see logs).".to_string());
            log::error!("[tui] threads.create_new returned no thread id");
        }
    }
}
