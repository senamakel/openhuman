use super::*;

fn failing() -> tinybus::Result<LinkedModule> {
    Err(tinybus::Error::failed("test factory"))
}

#[test]
fn registration_is_visible_and_idempotent() {
    let id = "linked-registration-idempotency-test";
    assert!(!is_registered(id));
    register_linked(id, failing);
    register_linked(id, failing);
    assert!(is_registered(id));
    assert!(factory_for(id).is_some());
    assert!(factory_for("linked-registration-absent-test").is_none());
}
