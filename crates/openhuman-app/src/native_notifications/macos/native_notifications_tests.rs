use super::*;

#[test]
fn is_granted_treats_authorized_variants_as_granted() {
    assert!(is_granted("granted"));
    assert!(is_granted("provisional"));
    assert!(is_granted("ephemeral"));
}

#[test]
fn is_granted_rejects_unauthorized_states() {
    assert!(!is_granted("denied"));
    assert!(!is_granted("not_determined"));
    assert!(!is_granted("unknown"));
    assert!(!is_granted(""));
}

#[test]
fn status_to_str_maps_known_statuses() {
    assert_eq!(status_to_str(UNAuthorizationStatus::Authorized), "granted");
    assert_eq!(status_to_str(UNAuthorizationStatus::Denied), "denied");
    assert_eq!(
        status_to_str(UNAuthorizationStatus::NotDetermined),
        "not_determined"
    );
    assert_eq!(
        status_to_str(UNAuthorizationStatus::Provisional),
        "provisional"
    );
    assert_eq!(status_to_str(UNAuthorizationStatus::Ephemeral), "ephemeral");
}

#[test]
fn bundled_app_layout_is_accepted() {
    assert!(is_bundled_app_executable(Path::new(
        "/Applications/OpenHuman.app/Contents/MacOS/OpenHuman"
    )));
}

#[test]
fn unbundled_executable_is_rejected() {
    assert!(!is_bundled_app_executable(Path::new(
        "/tmp/openhuman/target/debug/OpenHuman"
    )));
}
