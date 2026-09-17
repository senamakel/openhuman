use super::*;
use std::sync::atomic::Ordering;

// -- from tests --

#[test]
fn empty_shortcut_is_rejected() {
    assert_eq!(expand_ptt_shortcuts(""), Err(PttError::EmptyShortcut));
    assert_eq!(expand_ptt_shortcuts("   "), Err(PttError::EmptyShortcut));
}

#[test]
fn modifier_only_shortcut_is_rejected() {
    assert_eq!(
        expand_ptt_shortcuts("Ctrl"),
        Err(PttError::ModifierOnlyShortcut)
    );
    assert_eq!(
        expand_ptt_shortcuts("Cmd+Shift"),
        Err(PttError::ModifierOnlyShortcut)
    );
    assert_eq!(
        expand_ptt_shortcuts("Alt+Shift+Ctrl"),
        Err(PttError::ModifierOnlyShortcut)
    );
    assert_eq!(
        expand_ptt_shortcuts("CmdOrCtrl+Shift"),
        Err(PttError::ModifierOnlyShortcut)
    );
}

#[test]
fn plain_function_key_is_accepted() {
    assert_eq!(expand_ptt_shortcuts("F13"), Ok(vec!["F13".to_string()]));
}

#[test]
fn modifier_plus_letter_is_accepted() {
    assert_eq!(
        expand_ptt_shortcuts("Ctrl+Alt+T"),
        Ok(vec!["Ctrl+Alt+T".to_string()])
    );
}

#[test]
#[cfg(target_os = "macos")]
fn cmd_or_ctrl_expands_to_both_on_macos() {
    let result = expand_ptt_shortcuts("CmdOrCtrl+Shift+P").unwrap();
    assert_eq!(result.len(), 2);
    assert!(result.contains(&"Cmd+Shift+P".to_string()));
    assert!(result.contains(&"Ctrl+Shift+P".to_string()));
}

#[test]
#[cfg(not(target_os = "macos"))]
fn cmd_or_ctrl_expands_to_ctrl_off_macos() {
    let result = expand_ptt_shortcuts("CmdOrCtrl+Shift+P").unwrap();
    assert_eq!(result, vec!["Ctrl+Shift+P".to_string()]);
}

#[test]
fn malformed_shortcut_with_empty_tokens_is_rejected() {
    assert_eq!(expand_ptt_shortcuts("+F13"), Err(PttError::EmptyShortcut));
    assert_eq!(expand_ptt_shortcuts("F13+"), Err(PttError::EmptyShortcut));
    assert_eq!(
        expand_ptt_shortcuts("Ctrl++T"),
        Err(PttError::EmptyShortcut)
    );
}

// -- from conflict_tests --

#[test]
fn no_conflict_returns_none() {
    let ptt = vec!["F13".into()];
    let dict = vec!["F14".into()];
    assert_eq!(first_conflict_with(&ptt, &dict), None);
}

#[test]
fn case_insensitive_conflict_detected() {
    let ptt = vec!["ctrl+space".into()];
    let dict = vec!["Ctrl+Space".into()];
    assert_eq!(
        first_conflict_with(&ptt, &dict),
        Some("ctrl+space".to_string())
    );
}

#[test]
fn only_one_variant_overlaps_returns_first() {
    let ptt = vec!["Cmd+P".into(), "Ctrl+P".into()];
    let dict = vec!["Ctrl+P".into()];
    assert_eq!(first_conflict_with(&ptt, &dict), Some("Ctrl+P".to_string()));
}

// -- from state_tests --

#[test]
fn new_state_is_not_held_and_counter_is_zero() {
    let s = PttHotkeyState::new();
    assert!(!s.is_held.load(Ordering::Relaxed));
    assert_eq!(s.session_counter.load(Ordering::Relaxed), 0);
}

#[test]
fn cas_false_to_true_succeeds_then_repeat_fails() {
    let s = PttHotkeyState::new();
    // First press: false → true succeeds.
    assert!(
        s.is_held
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok(),
        "first press CAS should succeed"
    );
    // Repeat press: false → true fails because we're already true.
    assert!(
        s.is_held
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err(),
        "repeat press CAS should fail (already held)"
    );
    // Release: swap true → false returns the old true.
    assert!(
        s.is_held.swap(false, Ordering::AcqRel),
        "swap should return prior true"
    );
    // Subsequent stale release: swap returns the current false.
    assert!(
        !s.is_held.swap(false, Ordering::AcqRel),
        "stale swap should return false"
    );
}
