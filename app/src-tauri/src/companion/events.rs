//! Frontend event delivery for the desktop companion.
//!
//! The former core module broadcast state changes over a Socket.IO
//! `companion:state_changed` channel. Shell-side we instead emit a Tauri
//! event `companion://state_changed` with a camelCase payload
//! (`{ sessionId, state, previousState }`). The session state machine lives
//! deep below any `#[tauri::command]` boundary (it is driven from the
//! pipeline), so we stash the `AppHandle` in a process-global `OnceLock`
//! (mirroring `cdp::in_process::set_cef_app_handle`) and read it from the
//! emit helper.

use std::sync::OnceLock;

use log::debug;
use tauri::{AppHandle, Emitter};

use super::types::{CompanionState, CompanionStateChangedEvent};

const LOG_PREFIX: &str = "[companion]";

/// Tauri event name the frontend companion slice + overlay/notch listen to.
pub const STATE_CHANGED_EVENT: &str = "companion://state_changed";

/// Process-global `AppHandle`, set once from `Builder::setup`.
static COMPANION_APP_HANDLE: OnceLock<AppHandle<crate::AppRuntime>> = OnceLock::new();

/// Store the app handle so the pipeline/session can emit frontend events from
/// outside a command context. Idempotent — a second write is ignored.
pub fn set_app_handle(app: AppHandle<crate::AppRuntime>) {
    if COMPANION_APP_HANDLE.set(app).is_err() {
        debug!("{LOG_PREFIX} set_app_handle called more than once — ignoring");
    }
}

/// Emit a `companion://state_changed` event to the frontend.
///
/// Fire-and-forget: if the handle is not yet installed (e.g. in unit tests) or
/// the emit fails, we log and move on rather than failing the turn.
pub fn emit_state_changed(session_id: &str, state: CompanionState, previous: CompanionState) {
    debug!("{LOG_PREFIX} state_changed session={session_id} {previous} -> {state}");
    let Some(app) = COMPANION_APP_HANDLE.get() else {
        debug!("{LOG_PREFIX} no app handle installed — state change not delivered to frontend");
        return;
    };
    let payload = CompanionStateChangedEvent {
        session_id: session_id.to_string(),
        state,
        previous_state: previous,
    };
    if let Err(e) = app.emit(STATE_CHANGED_EVENT, payload) {
        debug!("{LOG_PREFIX} emit {STATE_CHANGED_EVENT} failed: {e}");
    }
}
