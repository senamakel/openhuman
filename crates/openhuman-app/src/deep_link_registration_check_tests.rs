use super::*;
use std::path::PathBuf;

#[test]
fn extract_first_token_quoted_exe_with_args() {
    assert_eq!(
        extract_first_token("\"C:\\Program Files\\OpenHuman\\OpenHuman.exe\" \"%1\""),
        "C:\\Program Files\\OpenHuman\\OpenHuman.exe"
    );
}

#[test]
fn extract_first_token_unquoted_exe_with_args() {
    assert_eq!(
        extract_first_token("C:\\OpenHuman\\OpenHuman.exe %1"),
        "C:\\OpenHuman\\OpenHuman.exe"
    );
}

#[test]
fn extract_first_token_handles_leading_whitespace() {
    assert_eq!(
        extract_first_token("   C:\\OpenHuman\\OpenHuman.exe %1"),
        "C:\\OpenHuman\\OpenHuman.exe"
    );
}

#[test]
fn extract_first_token_single_value_no_args() {
    assert_eq!(extract_first_token("OpenHuman.exe"), "OpenHuman.exe");
}

#[test]
fn extract_first_token_empty_string() {
    // Defensive guard: an empty REG_SZ value must not panic. The caller
    // (`verify_protocol_registration`) classifies this as `MissingCommand`
    // before reaching the parser, but the parser itself stays total.
    assert_eq!(extract_first_token(""), "");
}

#[test]
fn extract_first_token_quoted_exe_with_no_trailing_args() {
    // Some installers register the command without the `"%1"` argv
    // placeholder. The first token is still the quoted exe path.
    assert_eq!(
        extract_first_token("\"C:\\OpenHuman\\OpenHuman.exe\""),
        "C:\\OpenHuman\\OpenHuman.exe"
    );
}

#[test]
fn extract_first_token_unterminated_quote_falls_through() {
    // Defensive: malformed REG_SZ should not panic. We return the rest of
    // the string instead of slicing past a missing terminator.
    assert_eq!(
        extract_first_token("\"C:\\OpenHuman\\OpenHuman.exe %1"),
        "C:\\OpenHuman\\OpenHuman.exe %1"
    );
}

#[test]
fn paths_equal_loose_is_case_insensitive_and_slash_agnostic() {
    assert!(paths_equal_loose(
        "C:\\Program Files\\OpenHuman\\OpenHuman.exe",
        "c:/program files/openhuman/openhuman.exe"
    ));
}

#[test]
fn paths_equal_loose_distinguishes_different_paths() {
    assert!(!paths_equal_loose(
        "C:\\OldPath\\OpenHuman.exe",
        "C:\\NewPath\\OpenHuman.exe"
    ));
}

#[test]
fn command_references_exe_matches_quoted_command_with_percent_one() {
    let exe = PathBuf::from("C:\\Program Files\\OpenHuman\\OpenHuman.exe");
    assert!(command_references_exe(
        "\"C:\\Program Files\\OpenHuman\\OpenHuman.exe\" \"%1\"",
        &exe
    ));
}

#[test]
fn command_references_exe_matches_unquoted_command() {
    // Some HKCU writers omit the quotes when the path has no spaces. The
    // matcher must still resolve to the exe via the unquoted code path in
    // `extract_first_token` rather than relying only on the quoted path.
    let exe = PathBuf::from("C:\\OpenHuman\\OpenHuman.exe");
    assert!(command_references_exe(
        "C:\\OpenHuman\\OpenHuman.exe %1",
        &exe
    ));
}

#[test]
fn command_references_exe_detects_stale_install_path() {
    // Repro of the "user moved the install" failure mode: registry still
    // points at the old location.
    let exe = PathBuf::from("C:\\NewLocation\\OpenHuman.exe");
    assert!(!command_references_exe(
        "\"C:\\OldLocation\\OpenHuman.exe\" \"%1\"",
        &exe
    ));
}

#[test]
fn redacted_drops_directory_components_for_stale_paths() {
    // Reproduce the Sentry-leak case: a Stale status carrying the running
    // user's home directory must produce a log line that contains the
    // exe basenames but neither the username nor the parent dirs.
    let status = RegistrationStatus::Stale {
        registered_command: "\"C:\\Users\\joe\\AppData\\Local\\OpenHuman\\OpenHuman.exe\" \"%1\""
            .into(),
        expected_exe: "C:\\Users\\joe\\AppData\\Local\\OpenHuman_new\\OpenHuman.exe".into(),
    };
    let rendered = status.redacted();
    assert!(
        rendered.contains("OpenHuman.exe"),
        "basename should survive redaction: {rendered}"
    );
    assert!(
        !rendered.contains("joe"),
        "username must not leak: {rendered}"
    );
    assert!(
        !rendered.contains("AppData"),
        "directory path must not leak: {rendered}"
    );
}

#[test]
fn redacted_preserves_valid_variant_label_and_basename() {
    let status = RegistrationStatus::Valid {
        command: "\"C:\\Program Files\\OpenHuman\\OpenHuman.exe\" \"%1\"".into(),
    };
    assert_eq!(status.redacted(), "Valid { exe: OpenHuman.exe }");
}

#[test]
fn redacted_passes_through_pathless_variants() {
    assert_eq!(
        RegistrationStatus::MissingCommand.redacted(),
        "MissingCommand"
    );
    assert_eq!(
        RegistrationStatus::NotRegistered.redacted(),
        "NotRegistered"
    );
    assert_eq!(
        RegistrationStatus::ReadError("win32 error 5".into()).redacted(),
        "ReadError(win32 error 5)"
    );
}

#[test]
fn is_healthy_only_for_valid_variant() {
    assert!(RegistrationStatus::Valid {
        command: "x".into()
    }
    .is_healthy());
    assert!(!RegistrationStatus::MissingCommand.is_healthy());
    assert!(!RegistrationStatus::NotRegistered.is_healthy());
    assert!(!RegistrationStatus::Stale {
        registered_command: "x".into(),
        expected_exe: "y".into()
    }
    .is_healthy());
    assert!(!RegistrationStatus::ReadError("foo".into()).is_healthy());
}
