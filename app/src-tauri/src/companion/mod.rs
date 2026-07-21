//! Desktop companion — Clicky-style interaction loop (shell-side).
//!
//! Migrated out of the Rust core (`src/openhuman/desktop_companion`) into the
//! Tauri shell. Ties hotkey activation, **native** microphone capture, screen
//! context, LLM reasoning, speech synthesis, and visual pointing into one
//! product experience:
//!
//! - `audio`    — native `cpal` mic capture (mono i16 PCM ~16 kHz)
//! - `session`  — session lifecycle + state machine (idle→listening→…→idle)
//! - `pipeline` — one interaction turn (STT → LLM → TTS → pointing)
//! - `pointing` — `[POINT:x,y:label:screenN]` tag parsing + monitor mapping
//! - `events`   — `companion://state_changed` Tauri event delivery
//!
//! STT/TTS run in the embedded core over JSON-RPC; the LLM turn runs shell-side
//! against the backend using creds fetched from core. The five Tauri commands
//! below mirror the five RPC methods the old core `desktop_companion` exposed.

pub mod audio;
pub mod events;
pub mod pipeline;
pub mod pointing;
pub mod session;
pub mod types;

use log::{debug, info, warn};
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

use crate::AppRuntime;
use audio::MicCapture;
use pointing::ScreenGeometry;
use types::*;

const LOG_PREFIX: &str = "[companion]";

/// Process-global companion configuration (persisted only in-memory for now —
/// mirrors the old core stub, but round-trips get/set instead of erroring).
static CONFIG: Mutex<Option<CompanionConfig>> = Mutex::new(None);

/// The in-flight mic capture, if the companion is currently Listening. Used to
/// implement tap-to-talk: the first activation starts capture, the second stops
/// it and runs the turn.
static ACTIVE_CAPTURE: Mutex<Option<MicCapture>> = Mutex::new(None);

/// Install the app handle so the session/pipeline can emit frontend events from
/// outside a command context. Call once from `Builder::setup`.
pub fn setup(app: &AppHandle<AppRuntime>) {
    events::set_app_handle(app.clone());
    info!("{LOG_PREFIX} setup complete");
}

fn current_config() -> CompanionConfig {
    CONFIG.lock().clone().unwrap_or_default()
}

// ── Tauri commands (mirror the 5 former core RPC methods) ──────────────

/// Start a desktop companion session with explicit consent.
#[tauri::command]
pub(crate) async fn companion_start_session(
    consent: bool,
) -> Result<StartCompanionSessionResult, String> {
    debug!("{LOG_PREFIX} companion_start_session consent={consent}");
    let ttl_secs = current_config().ttl_secs;
    session::start_session(&StartCompanionSessionParams {
        consent,
        ttl_secs: Some(ttl_secs),
    })
}

/// Stop the active desktop companion session.
#[tauri::command]
pub(crate) async fn companion_stop_session() -> Result<StopCompanionSessionResult, String> {
    debug!("{LOG_PREFIX} companion_stop_session");
    // Tear down any in-flight capture so the mic is released.
    if let Some(capture) = ACTIVE_CAPTURE.lock().take() {
        let _ = capture.stop();
    }
    session::stop_session(&StopCompanionSessionParams {
        reason: Some("user_requested".into()),
    })
}

/// Get the current desktop companion session status.
#[tauri::command]
pub(crate) async fn companion_status() -> Result<CompanionSessionStatus, String> {
    Ok(session::session_status())
}

/// Get the current desktop companion configuration.
#[tauri::command]
pub(crate) async fn companion_config_get() -> Result<CompanionConfig, String> {
    Ok(current_config())
}

/// Update the desktop companion configuration.
#[tauri::command]
pub(crate) async fn companion_config_set(
    config: CompanionConfig,
) -> Result<CompanionConfig, String> {
    debug!(
        "{LOG_PREFIX} companion_config_set hotkey={} mode={}",
        config.hotkey, config.activation_mode
    );
    *CONFIG.lock() = Some(config.clone());
    Ok(config)
}

// ── Activation (tap-to-talk) driven by the hotkey / companion_activate ──

/// Handle a companion activation (hotkey press or the `companion_activate`
/// command). Tap-to-talk: the first activation while a session is active starts
/// native mic capture (Listening); the next activation stops capture and spawns
/// the interaction turn.
pub fn handle_activation(app: AppHandle<AppRuntime>) {
    let status = session::session_status();
    if !status.active {
        debug!("{LOG_PREFIX} activation ignored — no active session");
        return;
    }

    let mut cap = ACTIVE_CAPTURE.lock();
    if cap.is_none() {
        // Begin listening.
        if let Err(e) = session::transition_state(CompanionState::Listening, None) {
            warn!("{LOG_PREFIX} could not enter Listening: {e}");
            return;
        }
        match MicCapture::start() {
            Ok(c) => {
                *cap = Some(c);
                info!("{LOG_PREFIX} listening — mic capture started");
            }
            Err(e) => {
                warn!("{LOG_PREFIX} mic capture failed to start: {e}");
                let _ = session::transition_state(
                    CompanionState::Error,
                    Some(format!("mic capture failed: {e}")),
                );
            }
        }
    } else {
        // Stop listening and run the turn.
        let capture = cap.take().expect("checked is_some");
        drop(cap);
        let (samples, rate) = capture.stop();
        info!("{LOG_PREFIX} captured {} samples @ {rate}Hz", samples.len());
        let screens = monitors_geometry(&app);
        tauri::async_runtime::spawn(async move {
            let cancel = CancellationToken::new();
            if let Err(e) = pipeline::run_audio_turn(&samples, rate, &screens, cancel).await {
                warn!("{LOG_PREFIX} companion turn failed: {e}");
            }
        });
    }
}

/// Enumerate connected monitors as [`ScreenGeometry`] for POINT-tag coordinate
/// mapping. Best-effort — returns an empty vec if no window/monitor info is
/// available (pointing then falls back to raw coordinates).
fn monitors_geometry(app: &AppHandle<AppRuntime>) -> Vec<ScreenGeometry> {
    let Some(window) = app.get_webview_window("main") else {
        debug!("{LOG_PREFIX} no main window — cannot enumerate monitors");
        return Vec::new();
    };
    let Ok(monitors) = window.available_monitors() else {
        return Vec::new();
    };
    monitors
        .iter()
        .enumerate()
        .map(|(index, m)| {
            let pos = m.position();
            let size = m.size();
            ScreenGeometry {
                index,
                x: pos.x as f64,
                y: pos.y as f64,
                width: size.width as f64,
                height: size.height as f64,
            }
        })
        .collect()
}
