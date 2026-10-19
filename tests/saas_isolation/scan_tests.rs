use super::*;

#[test]
fn scoped_record_audit_counts_own_data_and_rejects_foreign_canaries() {
    let alice = openhuman_core::storage::scope_for_profile("alice");
    let mut own = [0; 3];
    let mut violations = Vec::new();

    record_scope_content(
        &alice,
        Some(0),
        "CANARY-alice-thread",
        &mut own,
        &mut violations,
    );
    assert_eq!(own, [1, 0, 0]);
    assert!(violations.is_empty());

    record_scope_content(
        &alice,
        Some(0),
        "CANARY-bob-thread",
        &mut own,
        &mut violations,
    );
    assert_eq!(own, [1, 0, 0]);
    assert!(violations[0].contains("bob's canary"));
}
