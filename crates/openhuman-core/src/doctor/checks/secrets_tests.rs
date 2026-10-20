//! Regression tests for keyring checks and their non-interactive probe gate.

use super::secrets::keyring_with_probe;
use crate::doctor::FindingSeverity;

#[test]
fn noninteractive_macos_keyring_check_skips_before_calling_the_probe() {
    let mut called = false;

    let finding = keyring_with_probe(true, false, || {
        called = true;
        ("host-keyring".to_owned(), true)
    });

    assert!(
        !called,
        "the OS keyring probe can display an interactive prompt"
    );
    assert_eq!(finding.check_id, "secrets/keyring");
    assert_eq!(finding.severity, FindingSeverity::Skipped);
}
