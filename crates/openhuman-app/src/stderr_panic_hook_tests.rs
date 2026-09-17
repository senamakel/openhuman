use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[test]
fn classifies_windows_pipe_closing_232_as_broken_pipe() {
    assert!(is_broken_pipe_stderr_panic(
        "failed printing to stderr: The pipe is being closed. (os error 232)"
    ));
}

#[test]
fn classifies_chinese_locale_variant_via_os_error_code() {
    // Localized strerror, but the `(os error 232)` suffix is locale-stable.
    assert!(is_broken_pipe_stderr_panic(
        "failed printing to stderr: 管道正在被关闭。 (os error 232)"
    ));
}

#[test]
fn classifies_windows_broken_pipe_109() {
    assert!(is_broken_pipe_stderr_panic(
        "failed printing to stderr: The pipe has been ended. (os error 109)"
    ));
}

#[test]
fn classifies_posix_epipe_32() {
    assert!(is_broken_pipe_stderr_panic(
        "failed printing to stderr: Broken pipe (os error 32)"
    ));
}

#[test]
fn does_not_classify_assertion_panic() {
    assert!(!is_broken_pipe_stderr_panic(
        "assertion `left == right` failed\n  left: 1\n right: 2"
    ));
}

#[test]
fn does_not_classify_arbitrary_panic() {
    assert!(!is_broken_pipe_stderr_panic(
        "index out of bounds: the len is 0 but the index is 3"
    ));
}

#[test]
fn does_not_classify_stdout_broken_pipe() {
    assert!(!is_broken_pipe_stderr_panic(
        "failed printing to stdout: The pipe is being closed. (os error 232)"
    ));
}

#[test]
fn does_not_classify_stderr_panic_with_unrelated_error() {
    assert!(!is_broken_pipe_stderr_panic(
        "failed printing to stderr: Permission denied (os error 13)"
    ));
}

#[test]
fn empty_message_is_not_broken_pipe() {
    assert!(!is_broken_pipe_stderr_panic(""));
}

// The contract the Codex review demanded: the hook must NEVER hide a crash
// from Sentry. `handle_panic` must call `chain` for EVERY panic — including
// the broken-pipe family it used to swallow.
fn run_handle_panic_and_record_chain(panic_msg: &'static str) -> bool {
    let chained = Arc::new(AtomicBool::new(false));
    let chained_for_hook = Arc::clone(&chained);
    // Trigger a panic to obtain a real PanicHookInfo, observe it in our hook.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let flag = Arc::clone(&chained_for_hook);
        handle_panic(info, &move |_| flag.store(true, Ordering::SeqCst));
    }));
    let _ = std::panic::catch_unwind(|| panic!("{panic_msg}"));
    std::panic::set_hook(prev);
    chained.load(Ordering::SeqCst)
}

// Combined into one test: each case installs the GLOBAL panic hook, so they
// must run serially (cargo parallelises separate #[test] fns).
#[test]
fn every_panic_class_chains_to_previous_hook() {
    // The exact broken-pipe case that used to be swallowed must now reach
    // Sentry (Codex review, PR #3772) — and so must an ordinary panic.
    assert!(
        run_handle_panic_and_record_chain(
            "failed printing to stderr: The pipe is being closed. (os error 232)"
        ),
        "broken-pipe stderr panic must still chain to the previous (Sentry) hook"
    );
    assert!(
        run_handle_panic_and_record_chain("assertion `left == right` failed"),
        "ordinary panic must chain to the previous (Sentry) hook"
    );
}
