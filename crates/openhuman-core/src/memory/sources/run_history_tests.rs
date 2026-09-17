//! The host's sync-run log and the rows it holds (openhuman#6257).

use chrono::{DateTime, TimeZone, Utc};

use super::*;

fn at(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_789_000_000 + seconds, 0)
        .single()
        .expect("a valid instant")
}

fn run(source_id: &str, items: u64) -> HostRun {
    HostRun {
        source_id: source_id.to_string(),
        source_kind: "folder".to_string(),
        scope: source_id.to_string(),
        items,
        duration_ms: 12,
        ..HostRun::default()
    }
}

fn ids(rows: &[SyncAuditEntry]) -> Vec<&str> {
    rows.iter().map(|row| row.source_id.as_str()).collect()
}

#[test]
fn a_missing_log_reads_as_no_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let rows = read_runs(dir.path(), KEEP_ROWS).expect("a missing log is not an error");
    assert!(rows.is_empty());
}

#[test]
fn recorded_runs_read_back_newest_first_within_the_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (offset, id) in ["src_a", "src_b", "src_c"].into_iter().enumerate() {
        let seconds = i64::try_from(offset).expect("small offset");
        append_run(dir.path(), &run(id, 1).into_entry(at(seconds))).expect("append");
    }

    let rows = read_runs(dir.path(), KEEP_ROWS).expect("read");
    assert_eq!(ids(&rows), ["src_c", "src_b", "src_a"]);

    let newest = read_runs(dir.path(), 2).expect("read");
    assert_eq!(ids(&newest), ["src_c", "src_b"]);
}

#[test]
fn a_torn_line_is_skipped_rather_than_hiding_the_rest() {
    let dir = tempfile::tempdir().expect("tempdir");
    append_run(dir.path(), &run("src_a", 1).into_entry(at(1))).expect("append");

    let path = log_path(dir.path());
    let mut content = std::fs::read_to_string(&path).expect("read the raw log");
    content.push_str("{\"timestamp\":\"2026-09-\n");
    std::fs::write(&path, content).expect("write a torn line");

    append_run(dir.path(), &run("src_b", 2).into_entry(at(2))).expect("append after it");
    let rows = read_runs(dir.path(), KEEP_ROWS).expect("a torn line does not fail the read");
    assert_eq!(ids(&rows), ["src_b", "src_a"]);
}

#[test]
fn compaction_keeps_only_the_newest_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let total = KEEP_ROWS + 25;
    for offset in 0..total {
        let seconds = i64::try_from(offset).expect("small offset");
        let entry = run(&format!("src_{offset}"), 1).into_entry(at(seconds));
        append_run_compacting_at(dir.path(), &entry, u64::MAX).expect("append");
    }
    let last = run("src_last", 1).into_entry(at(i64::try_from(total).expect("small total")));
    append_run_compacting_at(dir.path(), &last, 0).expect("the append that compacts");

    let raw = std::fs::read_to_string(log_path(dir.path())).expect("read the raw log");
    assert_eq!(
        raw.lines().filter(|line| !line.trim().is_empty()).count(),
        KEEP_ROWS,
        "compaction keeps exactly the newest KEEP_ROWS rows"
    );

    let rows = read_runs(dir.path(), KEEP_ROWS).expect("read");
    assert_eq!(rows[0].source_id, "src_last");
    // `total` rows plus `src_last`, KEEP_ROWS kept: the oldest survivor is
    // `src_{total + 1 - KEEP_ROWS}`.
    let oldest = format!("src_{}", total + 1 - KEEP_ROWS);
    assert_eq!(
        rows.last().map(|row| row.source_id.as_str()),
        Some(oldest.as_str())
    );
}

#[test]
fn a_log_that_stays_over_the_ceiling_is_not_rewritten_on_every_append() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = log_path(dir.path());
    let failed = |source_id: &str, seconds: i64| {
        let mut row = run(source_id, 0);
        row.error = Some("e".repeat(2_000));
        row.into_entry(at(seconds))
    };
    // A ceiling below one row: every append is over it, and so is every
    // compacted file.
    let ceiling = 64;
    append_run_compacting_at(dir.path(), &failed("src_a", 1), ceiling).expect("append a");

    // A torn line only a compaction removes: while it survives, nothing has
    // rewritten the file.
    let mut raw = std::fs::read_to_string(&path).expect("read the raw log");
    raw.push_str("{\"torn\n");
    std::fs::write(&path, raw).expect("write a torn line");

    let short = run("src_b", 1).into_entry(at(2));
    append_run_compacting_at(dir.path(), &short, ceiling).expect("append b");
    let raw = std::fs::read_to_string(&path).expect("read the raw log");
    assert!(
        raw.contains("{\"torn"),
        "an append short of doubling the last compacted size does not compact again"
    );

    append_run_compacting_at(dir.path(), &failed("src_c", 3), ceiling).expect("append c");
    let raw = std::fs::read_to_string(&path).expect("read the raw log");
    assert!(
        !raw.contains("{\"torn"),
        "past double the last compacted size it compacts again"
    );
    let rows = read_runs(dir.path(), KEEP_ROWS).expect("read");
    assert_eq!(ids(&rows), ["src_c", "src_b", "src_a"]);
}

#[test]
fn a_host_run_maps_onto_the_driver_row_shape() {
    let mut failed = run("src_a", u64::from(u32::MAX) + 5);
    failed.error = Some("boom".to_string());
    failed.actions_called = 3;
    failed.provider_cost_usd = 0.25;
    let row = failed.into_entry(at(0));
    assert!(!row.success);
    assert_eq!(row.error.as_deref(), Some("boom"));
    assert_eq!(
        row.items_fetched,
        u32::MAX,
        "an item count past u32 saturates"
    );
    assert_eq!(row.composio_actions_called, 3);
    assert!((row.composio_cost_usd - 0.25).abs() < 1e-9);
    assert!(
        row.estimated_cost_usd.abs() < 1e-9,
        "the host prices nothing"
    );
    assert_eq!(
        (row.batches, row.input_tokens, row.output_tokens),
        (0, 0, 0)
    );
    assert_eq!((row.tree_ingest_failures, row.tree_error), (0, None));
    assert_eq!(row.timestamp, at(0));

    let completed = run("src_b", 7).into_entry(at(1));
    assert!(completed.success);
    assert_eq!(completed.error, None);
    assert_eq!(completed.items_fetched, 7);
    assert_eq!(completed.duration_ms, 12);
}

#[test]
fn merging_orders_both_logs_newest_first_and_caps_the_result() {
    let driver = vec![
        run("driver_new", 0).into_entry(at(30)),
        run("driver_old", 0).into_entry(at(10)),
    ];
    let host = vec![
        run("host_mid", 0).into_entry(at(20)),
        run("host_oldest", 0).into_entry(at(0)),
    ];

    let merged = merge_newest_first(driver.clone(), host.clone(), 10);
    assert_eq!(
        ids(&merged),
        ["driver_new", "host_mid", "driver_old", "host_oldest"]
    );
    assert_eq!(
        ids(&merge_newest_first(driver, host, 2)),
        ["driver_new", "host_mid"]
    );
}

#[test]
fn a_recorded_run_lands_in_the_configured_workspace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = Config {
        workspace_dir: dir.path().to_path_buf(),
        ..Config::default()
    };
    record_run(&config, run("src_cfg", 4).into_entry(at(0)));
    let rows = read_runs(dir.path(), KEEP_ROWS).expect("read");
    assert_eq!(ids(&rows), ["src_cfg"]);
}

/// A row that cannot be written is a gap in the history, never a failed sync:
/// `record_run` returns nothing to fail with and must not panic.
#[test]
fn a_run_that_cannot_be_recorded_does_not_take_the_caller_down() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A file where the state directory should be makes every append fail.
    std::fs::write(dir.path().join(STATE_DIR), "not a directory").expect("occupy the state path");
    let config = Config {
        workspace_dir: dir.path().to_path_buf(),
        ..Config::default()
    };
    record_run(&config, run("src_blocked", 1).into_entry(at(0)));
    assert!(append_run(dir.path(), &run("src_blocked", 1).into_entry(at(0))).is_err());
}
