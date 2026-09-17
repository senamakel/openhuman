//! Global push-to-talk hotkey state + parsing.
//!
//!
//! `expand_ptt_shortcuts` mirrors `dictation_hotkeys::expand_dictation_shortcuts`
//! but rejects pure-modifier shortcuts (Ctrl, Cmd+Shift, etc.) because they
//! would fire constantly during normal typing.

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Mutex;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PttError {
    EmptyShortcut,
    ModifierOnlyShortcut,
    ConflictsWithDictation(String),
}

impl std::fmt::Display for PttError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PttError::EmptyShortcut => write!(f, "ptt shortcut cannot be empty"),
            PttError::ModifierOnlyShortcut => write!(
                f,
                "ptt shortcut cannot be only modifier keys (Ctrl/Cmd/Shift/Alt)"
            ),
            PttError::ConflictsWithDictation(s) => {
                write!(f, "ptt shortcut '{s}' conflicts with the dictation hotkey")
            }
        }
    }
}

impl std::error::Error for PttError {}

/// Process-wide PTT state. Held in the Tauri-managed `State<PttHotkeyState>`.
pub(crate) struct PttHotkeyState {
    /// Currently-registered shortcut variants (e.g. `["Cmd+F13", "Ctrl+F13"]` on macOS).
    pub(crate) shortcut: Mutex<Vec<String>>,
    /// Monotonic counter for session IDs.
    pub(crate) session_counter: AtomicU64,
    /// CAS-guarded: true iff a PTT session is currently mid-hold.
    /// Used to drop OS key-repeat Pressed events so each press/release pair
    /// produces exactly one session_id.
    pub(crate) is_held: AtomicBool,
}

impl PttHotkeyState {
    pub(crate) fn new() -> Self {
        Self {
            shortcut: Mutex::new(Vec::new()),
            session_counter: AtomicU64::new(0),
            is_held: AtomicBool::new(false),
        }
    }
}

const MODIFIER_TOKENS: &[&str] = &[
    "ctrl",
    "control",
    "cmd",
    "command",
    "meta",
    "super",
    "win",
    "windows",
    "alt",
    "option",
    "shift",
    "cmdorctrl",
];

fn is_modifier_token(token: &str) -> bool {
    let trimmed = token.trim();
    MODIFIER_TOKENS
        .iter()
        .any(|m| trimmed.eq_ignore_ascii_case(m))
}

/// Expand a user-typed shortcut into one or two OS-specific variants and
/// validate it isn't empty / modifier-only.
pub(crate) fn expand_ptt_shortcuts(shortcut: &str) -> Result<Vec<String>, PttError> {
    let trimmed = shortcut.trim();
    if trimmed.is_empty() {
        return Err(PttError::EmptyShortcut);
    }

    let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err(PttError::EmptyShortcut);
    }
    if parts.iter().all(|p| is_modifier_token(p)) {
        return Err(PttError::ModifierOnlyShortcut);
    }

    #[cfg(target_os = "macos")]
    {
        if trimmed.contains("CmdOrCtrl") {
            let cmd_variant = trimmed.replace("CmdOrCtrl", "Cmd");
            let ctrl_variant = trimmed.replace("CmdOrCtrl", "Ctrl");
            if cmd_variant == ctrl_variant {
                return Ok(vec![cmd_variant]);
            }
            return Ok(vec![cmd_variant, ctrl_variant]);
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if trimmed.contains("CmdOrCtrl") {
            return Ok(vec![trimmed.replace("CmdOrCtrl", "Ctrl")]);
        }
    }

    Ok(vec![trimmed.to_string()])
}

/// Returns `Some(conflicting_variant)` if any expanded PTT variant overlaps
/// any expanded dictation variant. Comparison is case-insensitive.
pub(crate) fn first_conflict_with(ptt: &[String], dictation: &[String]) -> Option<String> {
    for p in ptt {
        let p_lc = p.to_ascii_lowercase();
        for d in dictation {
            if d.to_ascii_lowercase() == p_lc {
                return Some(p.clone());
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "ptt_hotkeys_tests.rs"]
mod tests;
