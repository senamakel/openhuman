use super::*;

#[test]
fn is_openhuman_executable_matches_core_binary() {
    assert!(is_openhuman_executable("/usr/local/bin/openhuman-core"));
    assert!(is_openhuman_executable("openhuman-core"));
    assert!(is_openhuman_executable("/opt/OpenHuman/openhuman-core"));
}

#[test]
fn is_openhuman_executable_matches_app_binary() {
    assert!(is_openhuman_executable("/opt/OpenHuman/OpenHuman"));
    assert!(is_openhuman_executable("openhuman"));
}

#[test]
fn is_openhuman_executable_rejects_unrelated() {
    assert!(!is_openhuman_executable("bash"));
    assert!(!is_openhuman_executable("/usr/bin/python3"));
    assert!(!is_openhuman_executable("node"));
}

#[test]
fn enumerate_openhuman_processes_returns_no_self() {
    // Enumerate and confirm self is not in the result.
    let self_pid = std::process::id();
    let result = enumerate_openhuman_processes().expect("enumerate");
    assert!(
        result.iter().all(|p| p.pid != self_pid),
        "self pid {self_pid} must not appear in enumerated list"
    );
}
