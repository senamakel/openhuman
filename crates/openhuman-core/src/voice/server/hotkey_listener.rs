//! Hotkey listener dispatch: rdev everywhere except the macOS Fn/Globe key,
//! which needs the Swift-based globe listener instead.

#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
use log::{debug, info, warn};
use tokio_util::sync::CancellationToken;

use crate::voice::hotkey;

#[cfg(target_os = "macos")]
use super::LOG_PREFIX;

/// Opaque handle that keeps the hotkey listener alive. Drop to stop.
pub(super) enum HotkeyListenerKind {
    Rdev(hotkey::HotkeyListenerHandle),
    #[cfg(target_os = "macos")]
    Globe(CancellationToken),
}

impl HotkeyListenerKind {
    pub(super) fn stop(&self) {
        match self {
            HotkeyListenerKind::Rdev(handle) => handle.stop(),
            #[cfg(target_os = "macos")]
            HotkeyListenerKind::Globe(cancel) => cancel.cancel(),
        }
    }
}

/// Start the appropriate hotkey listener for the current platform and key.
///
/// On macOS, the Fn/Globe key is handled by the Swift-based globe listener
/// (`accessibility::globe`) which monitors `NSEvent.flagsChanged`. All other
/// keys return an error on macOS: rdev's CGEventTap callback calls
/// `TSMGetInputSourceProperty` off the main thread; macOS 26 enforces
/// `dispatch_assert_queue(main_queue)` inside that API and kills the process
/// with `EXC_BREAKPOINT` (`dispatch_assert_queue_fail`). Configure
/// `hotkey = "fn"` to avoid this. (#2677)
pub(super) fn start_hotkey_listener(
    hotkey_str: &str,
    mode: hotkey::ActivationMode,
    server_cancel: &CancellationToken,
) -> Result<
    (
        HotkeyListenerKind,
        tokio::sync::mpsc::UnboundedReceiver<hotkey::HotkeyEvent>,
    ),
    String,
> {
    #[cfg(target_os = "macos")]
    {
        if hotkey_str.trim().eq_ignore_ascii_case("fn") {
            return start_globe_hotkey_listener(mode, server_cancel);
        }
        // rdev calls TSMGetInputSourceProperty off the main thread; macOS 26
        // enforces main-queue-only access and crashes the process. Only the
        // Fn/Globe key is safe via the Swift globe listener. (#2677)
        Err(format!(
            "voice server hotkey '{}' is not supported on macOS — \
             only 'fn' (Fn/Globe key) is safe. rdev calls \
             TSMGetInputSourceProperty off the main thread on macOS 26, \
             causing EXC_BREAKPOINT. Set hotkey = \"fn\" in voice config \
             (issue #2677).",
            hotkey_str
        ))
    }

    // Non-macOS: rdev-based listener for all keys.
    #[cfg(not(target_os = "macos"))]
    {
        // `server_cancel` is only consumed by the macOS Swift-globe listener
        // branch above; the rdev listener manages its own lifecycle, so bind it
        // here to keep the shared signature warning-free on non-macOS.
        let _ = server_cancel;
        let combo = hotkey::parse_hotkey(hotkey_str)?;
        let (handle, rx) = hotkey::start_listener(combo, mode)?;
        Ok((HotkeyListenerKind::Rdev(handle), rx))
    }
}

/// macOS-only: start the Swift globe listener and bridge FN_DOWN / FN_UP
/// events into `HotkeyEvent::Pressed` / `HotkeyEvent::Released`.
#[cfg(target_os = "macos")]
fn start_globe_hotkey_listener(
    mode: hotkey::ActivationMode,
    server_cancel: &CancellationToken,
) -> Result<
    (
        HotkeyListenerKind,
        tokio::sync::mpsc::UnboundedReceiver<hotkey::HotkeyEvent>,
    ),
    String,
> {
    use crate::desktop::accessibility::{globe_listener_poll, globe_listener_start};

    info!("{LOG_PREFIX} hotkey is Fn on macOS — using Swift globe listener instead of rdev");

    let status = globe_listener_start()?;
    if !status.running {
        let err_msg = status
            .last_error
            .unwrap_or_else(|| "globe listener failed to start".to_string());
        return Err(format!("globe listener: {err_msg}"));
    }
    info!(
        "{LOG_PREFIX} globe listener started, permission={:?}",
        status.input_monitoring_permission
    );

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = server_cancel.child_token();
    let cancel_clone = cancel.clone();

    // Tap mode state: track whether we're currently active.
    let is_active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    tokio::spawn(async move {
        let mut poll_interval = tokio::time::interval(Duration::from_millis(50));
        poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = cancel_clone.cancelled() => {
                    debug!("{LOG_PREFIX} globe poller cancelled");
                    break;
                }
                _ = poll_interval.tick() => {
                    let poll_result = match globe_listener_poll() {
                        Ok(r) => r,
                        Err(e) => {
                            warn!("{LOG_PREFIX} globe poll error: {e}");
                            continue;
                        }
                    };

                    for event_str in &poll_result.events {
                        let hotkey_event = match event_str.as_str() {
                            "FN_DOWN" => match mode {
                                hotkey::ActivationMode::Push => {
                                    Some(hotkey::HotkeyEvent::Pressed)
                                }
                                hotkey::ActivationMode::Tap => {
                                    let was_active = is_active.load(std::sync::atomic::Ordering::SeqCst);
                                    if was_active {
                                        is_active.store(false, std::sync::atomic::Ordering::SeqCst);
                                        Some(hotkey::HotkeyEvent::Released)
                                    } else {
                                        is_active.store(true, std::sync::atomic::Ordering::SeqCst);
                                        Some(hotkey::HotkeyEvent::Pressed)
                                    }
                                }
                            },
                            "FN_UP" => match mode {
                                hotkey::ActivationMode::Push => {
                                    Some(hotkey::HotkeyEvent::Released)
                                }
                                hotkey::ActivationMode::Tap => None, // tap ignores release
                            },
                            _ => None, // ignore modifier events
                        };

                        if let Some(ev) = hotkey_event {
                            debug!("{LOG_PREFIX} globe event {event_str} → {ev:?}");
                            if tx.send(ev).is_err() {
                                debug!("{LOG_PREFIX} globe poller: receiver dropped, stopping");
                                return;
                            }
                        }
                    }
                }
            }
        }
    });

    Ok((HotkeyListenerKind::Globe(cancel), rx))
}
