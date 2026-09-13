#![cfg(windows)]

use super::*;

#[test]
fn is_protected_windows_pid_matches_kernel_pids() {
    assert!(is_protected_windows_pid(0));
    assert!(is_protected_windows_pid(4));
    assert!(!is_protected_windows_pid(1));
    assert!(!is_protected_windows_pid(8));
    assert!(!is_protected_windows_pid(1234));
}

#[test]
fn classify_taskkill_force_treats_exit_0_as_success() {
    assert!(classify_taskkill_force_status(Some(0), b"", 1234).is_ok());
}

#[test]
fn classify_taskkill_force_treats_exit_128_as_success() {
    // Exit 128 = "There is no running instance of the task." — process
    // already gone between the pid lookup and our kill call. The port is
    // freeing on its own; recovery must NOT bail out here.
    assert!(classify_taskkill_force_status(Some(128), b"", 1234).is_ok());
}

#[test]
fn classify_taskkill_force_treats_not_found_stderr_as_success() {
    // Some hosts/wrappers normalize exit codes to 1 but still emit the
    // canonical "not found" message on stderr.
    let stderr = b"ERROR: The process \"1234\" not found.\r\n";
    assert!(classify_taskkill_force_status(Some(1), stderr, 1234).is_ok());
}

#[test]
fn classify_taskkill_force_treats_no_running_instance_as_success() {
    // The `/T` (tree) flag emits this shape when the parent is already
    // gone but child traversal still runs. Pass a *non-128* exit code
    // here so the test actually exercises the stderr-matching branch —
    // `Some(128)` short-circuits before we ever inspect stderr.
    let stderr = b"ERROR: The process with PID 1234 (child process of PID 999) \
        could not be terminated.\r\n\
        Reason: There is no running instance of the task.\r\n";
    assert!(classify_taskkill_force_status(Some(1), stderr, 1234).is_ok());
}

#[test]
fn classify_taskkill_force_propagates_access_denied() {
    // Access-denied has the SAME "could not be terminated" prefix as
    // the process-gone case, so the predicate must require additional
    // tokens before treating it as success. Otherwise we silently mark
    // a live, unreachable process as killed and recovery proceeds
    // against a still-bound port.
    let stderr = b"ERROR: The process with PID 1234 could not be terminated.\r\n\
        Reason: Access is denied.\r\n";
    let err = classify_taskkill_force_status(Some(1), stderr, 1234).unwrap_err();
    assert!(err.contains("code Some(1)"), "got: {err}");
    assert!(err.contains("Access is denied"), "got: {err}");
}

#[test]
fn classify_taskkill_force_propagates_bare_access_denied() {
    let stderr = b"ERROR: Access is denied.\r\n";
    let err = classify_taskkill_force_status(Some(5), stderr, 1234).unwrap_err();
    assert!(err.contains("code Some(5)"), "got: {err}");
    assert!(err.contains("Access is denied"), "got: {err}");
}

#[test]
fn kill_pid_term_refuses_protected_pids() {
    assert!(kill_pid_term(0).is_err());
    assert!(kill_pid_term(4).is_err());
}

#[test]
fn kill_pid_force_refuses_protected_pids() {
    assert!(kill_pid_force(0).is_err());
    assert!(kill_pid_force(4).is_err());
}

#[test]
fn kill_pid_force_no_tree_refuses_protected_pids() {
    assert!(kill_pid_force_no_tree(0).is_err());
    assert!(kill_pid_force_no_tree(4).is_err());
}

/// The non-tree force-kill (issue #3900) terminates the target and is
/// idempotent on an already-gone pid, exactly like `kill_pid_force`.
#[test]
fn kill_pid_force_no_tree_terminates_real_process_and_is_idempotent() {
    let mut child = std::process::Command::new("cmd")
        .args(["/C", "timeout", "/T", "30", "/NOBREAK"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("spawn child process");
    let pid = child.id();

    kill_pid_force_no_tree(pid).expect("force-kill running process (no tree)");
    let _ = child.wait();
    kill_pid_force_no_tree(pid).expect("force-kill of already-gone pid is success");
}

/// End-to-end-on-Windows: spawn a real child process, force-kill it, and
/// verify it exits. Also covers the "process already gone" case by
/// killing the same PID twice — the second call must succeed (this is
/// the bug the patch above fixes).
#[test]
fn kill_pid_force_terminates_real_process_and_is_idempotent() {
    // `timeout` is a builtin shipped with every Windows install; sleeps
    // for ~30s which is plenty for the kill round-trip.
    let mut child = std::process::Command::new("cmd")
        .args(["/C", "timeout", "/T", "30", "/NOBREAK"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("spawn child process");
    let pid = child.id();

    kill_pid_force(pid).expect("force-kill running process");

    // Reap so we don't leave a zombie regardless of test outcome.
    let _ = child.wait();

    // Second call: same pid is now gone. Must be Ok — this is the
    // regression we're guarding against.
    kill_pid_force(pid).expect("force-kill of already-gone pid is success");
}
