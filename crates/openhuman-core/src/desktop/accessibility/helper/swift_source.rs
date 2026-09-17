//! Assembles the embedded Swift source for the unified helper process from
//! its per-responsibility fragments (focus query, paste, AX actions, overlay
//! and main loop). Splitting the fragments keeps each one reviewable; the
//! compiled Swift program is unaffected — this just concatenates them back
//! into one source string for `swiftc`.

#[cfg(target_os = "macos")]
use super::swift_ax_actions::SWIFT_AX_ACTIONS;
#[cfg(target_os = "macos")]
use super::swift_focus::SWIFT_HEADER_AND_FOCUS;
#[cfg(target_os = "macos")]
use super::swift_overlay::SWIFT_OVERLAY_AND_MAIN;
#[cfg(target_os = "macos")]
use super::swift_paste::SWIFT_PASTE;

#[cfg(target_os = "macos")]
pub(super) fn unified_swift_source() -> String {
    format!("{SWIFT_HEADER_AND_FOCUS}{SWIFT_PASTE}{SWIFT_AX_ACTIONS}{SWIFT_OVERLAY_AND_MAIN}")
}
