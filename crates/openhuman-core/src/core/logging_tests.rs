use super::*;

/// Serialize tests that mutate `RUST_LOG` / `OPENHUMAN_LOG_FILE_CONSTRAINTS` —
/// Cargo runs unit tests in parallel threads in the same process, so
/// concurrent env-var writes would race.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Serialize tests that mutate the process-global `FILE_GUARD` static.
/// Without this, `shutdown_file_guard_takes_installed_guard` can race
/// any concurrent test that calls `init_for_embedded` (or that itself
/// stashes / takes the guard), making one of them observe a guard it
/// did not install. Mirror of the `SCHEDULE_LOCK` pattern in
/// `crates/openhuman-app/src/reset_reboot_schedule.rs::tests`.
#[cfg(feature = "file-logging")]
static FILE_GUARD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_clean_rust_log<R>(f: impl FnOnce() -> R) -> R {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prior = std::env::var("RUST_LOG").ok();
    std::env::remove_var("RUST_LOG");
    let result = f();
    match prior {
        Some(v) => std::env::set_var("RUST_LOG", v),
        None => std::env::remove_var("RUST_LOG"),
    }
    result
}

#[test]
fn level_tag_covers_all_levels() {
    assert_eq!(level_tag(&Level::ERROR), "ERR");
    assert_eq!(level_tag(&Level::WARN), "WRN");
    assert_eq!(level_tag(&Level::INFO), "INF");
    assert_eq!(level_tag(&Level::DEBUG), "DBG");
    assert_eq!(level_tag(&Level::TRACE), "TRC");
}

#[test]
fn short_target_strips_module_path() {
    assert_eq!(short_target("openhuman_core::core::rpc"), "rpc");
    // Non-namespaced target stays as-is.
    assert_eq!(short_target("plain"), "plain");
}

#[test]
fn seed_rust_log_global_uses_info_by_default() {
    with_clean_rust_log(|| {
        seed_rust_log(false, CliLogDefault::Global);
        assert_eq!(std::env::var("RUST_LOG").unwrap(), "info");
    });
}

#[test]
fn seed_rust_log_global_uses_debug_when_verbose() {
    with_clean_rust_log(|| {
        seed_rust_log(true, CliLogDefault::Global);
        assert_eq!(std::env::var("RUST_LOG").unwrap(), "debug");
    });
}

#[test]
fn seed_rust_log_respects_existing_value() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prior = std::env::var("RUST_LOG").ok();
    std::env::set_var("RUST_LOG", "warn");
    seed_rust_log(true, CliLogDefault::Global);
    // Caller's existing setting must not be clobbered.
    assert_eq!(std::env::var("RUST_LOG").unwrap(), "warn");
    match prior {
        Some(v) => std::env::set_var("RUST_LOG", v),
        None => std::env::remove_var("RUST_LOG"),
    }
}

#[test]
fn build_env_filter_returns_a_filter() {
    // Smoke test: shouldn't panic and should produce *some* filter regardless of inputs.
    let _ = build_env_filter(false, CliLogDefault::Global);
    let _ = build_env_filter(true, CliLogDefault::Global);
}

#[test]
fn parse_log_file_constraints_handles_csv_and_whitespace() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prior = std::env::var("OPENHUMAN_LOG_FILE_CONSTRAINTS").ok();
    std::env::set_var("OPENHUMAN_LOG_FILE_CONSTRAINTS", "rpc, , agent ,memory");
    let parsed = parse_log_file_constraints();
    assert_eq!(parsed, vec!["rpc", "agent", "memory"]);

    std::env::remove_var("OPENHUMAN_LOG_FILE_CONSTRAINTS");
    assert!(parse_log_file_constraints().is_empty());

    match prior {
        Some(v) => std::env::set_var("OPENHUMAN_LOG_FILE_CONSTRAINTS", v),
        None => std::env::remove_var("OPENHUMAN_LOG_FILE_CONSTRAINTS"),
    }
}

#[test]
fn log_directory_is_none_before_init_for_embedded() {
    // In a fresh `cargo test` process where no test has called
    // `init_for_embedded`, `log_directory()` must return `None` so the
    // shell-side `reveal_logs_folder` command can surface a clear
    // error rather than launching against an empty path.
    if LOG_DIR.get().is_none() {
        assert!(log_directory().is_none());
    }
}

// Constructs a real `tracing_appender` appender, so it only exists when
// the crate does.
#[cfg(feature = "file-logging")]
#[test]
fn shutdown_file_guard_takes_installed_guard() {
    let _g = FILE_GUARD_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Simulate `init_for_embedded` having stashed a writer guard, then
    // assert that `shutdown_file_guard` empties the slot and reports
    // truthfully.
    let dir = tempfile::tempdir().expect("tempdir for guard test");
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("guard-test")
        .filename_suffix("log")
        .build(dir.path())
        .expect("rolling appender for guard test");
    let (_writer, guard) = tracing_appender::non_blocking(appender);
    {
        let mut slot = FILE_GUARD.lock().expect("file guard mutex poisoned");
        // Save any pre-existing guard (a prior test in this process may
        // have installed one) and restore it after the assertion runs.
        let prior = slot.replace(guard);
        drop(slot);

        assert!(
            shutdown_file_guard(),
            "expected shutdown_file_guard to take the installed guard"
        );
        assert!(
            !shutdown_file_guard(),
            "second call must be a no-op when the slot is already empty"
        );

        // Restore the prior guard so unrelated tests that depend on
        // `init_for_embedded` having installed one are not surprised.
        if let Ok(mut slot) = FILE_GUARD.lock() {
            *slot = prior;
        }
    }
}

#[test]
fn tui_log_writer_keeps_a_bounded_ordered_ring() {
    let buffer = std::sync::Arc::new(Mutex::new(VecDeque::new()));
    let mut writer = TuiLogWriter {
        buffer: buffer.clone(),
        pending: Vec::new(),
    };
    for index in 0..=TUI_LOG_CAPACITY {
        writeln!(writer, "line-{index}").expect("write log line");
        writer.flush().expect("flush log line");
    }
    let lines = buffer.lock().expect("buffer lock");
    assert_eq!(lines.len(), TUI_LOG_CAPACITY);
    assert_eq!(lines.front().map(String::as_str), Some("line-1"));
    let expected_last = format!("line-{TUI_LOG_CAPACITY}");
    assert_eq!(
        lines.back().map(String::as_str),
        Some(expected_last.as_str())
    );
}

#[test]
fn tui_log_writer_caps_individual_lines() {
    let buffer = std::sync::Arc::new(Mutex::new(VecDeque::new()));
    let mut writer = TuiLogWriter {
        buffer: buffer.clone(),
        pending: Vec::new(),
    };
    writeln!(writer, "{}", "x".repeat(TUI_LOG_LINE_MAX_CHARS + 50)).expect("write long line");
    writer.flush().expect("flush long line");
    let lines = buffer.lock().expect("buffer lock");
    assert_eq!(
        lines.front().map(|line| line.chars().count()),
        Some(TUI_LOG_LINE_MAX_CHARS)
    );
}
