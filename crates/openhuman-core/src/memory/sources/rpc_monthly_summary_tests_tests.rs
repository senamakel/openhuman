use chrono::{DateTime, Utc};

use crate::memory::api::provider::sync::SyncAuditEntry;
use crate::memory::sources::rpc::cost_reporting::{summarise_logs, summarise_month};
use crate::memory::sources::run_history::KEEP_ROWS;
/// One audit row, dated `stamp`, costing `estimated` with no real charge.
fn row(stamp: &str, items: u32, input: u64, output: u64, estimated: f64) -> SyncAuditEntry {
    SyncAuditEntry {
        timestamp: stamp
            .parse::<DateTime<Utc>>()
            .expect("an RFC 3339 timestamp"),
        source_id: "src_1".to_string(),
        source_kind: "composio".to_string(),
        scope: "gmail".to_string(),
        items_fetched: items,
        batches: 1,
        input_tokens: input,
        output_tokens: output,
        estimated_cost_usd: estimated,
        composio_actions_called: 0,
        composio_cost_usd: 0.0,
        actual_charged_usd: None,
        duration_ms: 10,
        success: true,
        error: None,
        tree_ingest_failures: 0,
        tree_error: None,
    }
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-9
}

/// Only the requested month is totalled, and a row from an earlier month
/// proves the read crossed the boundary.
#[test]
fn totals_the_month_and_reports_complete_when_the_read_reached_past_it() {
    let entries = vec![
        row("2026-08-20T10:00:00Z", 3, 100, 20, 0.5),
        row("2026-08-01T00:00:00Z", 2, 50, 10, 0.25),
        // Older month — excluded from the totals, and the proof the read
        // covered the whole of August.
        row("2026-07-31T23:59:59Z", 99, 9_999, 9_999, 42.0),
    ];
    let summary = summarise_month(&entries, "2026-08");

    assert_eq!(summary.month, "2026-08");
    assert_eq!(summary.total_syncs, 2);
    assert_eq!(summary.total_items, 5);
    assert_eq!(summary.total_input_tokens, 150);
    assert_eq!(summary.total_output_tokens, 30);
    assert!(close(summary.total_cost_usd, 0.75));
    assert!(
        summary.totals_complete,
        "a row older than the month proves every row inside it was read"
    );
}

/// The cap case: every row the driver returned is inside the month, so the
/// totals may be a floor and must not claim to be complete.
#[test]
fn reports_incomplete_when_every_returned_row_is_inside_the_month() {
    let entries = vec![
        row("2026-08-20T10:00:00Z", 1, 10, 1, 0.1),
        row("2026-08-19T10:00:00Z", 1, 10, 1, 0.1),
    ];
    let summary = summarise_month(&entries, "2026-08");

    assert_eq!(summary.total_syncs, 2);
    assert!(
        !summary.totals_complete,
        "the read may have been cut off at the driver's cap — totals are a floor"
    );
}

/// An empty log has no rows the cap could have hidden, so zero is exact.
#[test]
fn an_empty_log_totals_zero_and_is_complete() {
    let summary = summarise_month(&[], "2026-08");

    assert_eq!(summary.total_syncs, 0);
    assert_eq!(summary.total_items, 0);
    assert!(close(summary.total_cost_usd, 0.0));
    assert!(summary.totals_complete);
}

/// The real charge wins over the estimate, and Composio's own action cost
/// is added on top — the audit's own view of what a run cost, unchanged by
/// the move onto the contract.
#[test]
fn cost_uses_the_actual_charge_and_adds_composio() {
    let mut charged = row("2026-08-20T10:00:00Z", 1, 10, 1, 0.10);
    charged.actual_charged_usd = Some(0.30);
    charged.composio_cost_usd = 0.05;
    let estimated_only = row("2026-08-21T10:00:00Z", 1, 10, 1, 0.20);

    let summary = summarise_month(&[charged, estimated_only], "2026-08");

    // 0.30 (actual) + 0.05 (composio) + 0.20 (estimate) — the estimate on
    // the charged row is not counted.
    assert!(close(summary.total_cost_usd, 0.55));
}

/// A row stamped in a later month (clock skew) is skipped rather than
/// counted, and does not count as proof the read crossed the boundary —
/// the same exclusion the engine-backed month filter made.
#[test]
fn a_future_stamped_row_is_skipped_and_proves_nothing() {
    let entries = vec![
        row("2026-09-01T00:00:00Z", 7, 700, 70, 9.0),
        row("2026-08-20T10:00:00Z", 1, 10, 1, 0.1),
    ];
    let summary = summarise_month(&entries, "2026-08");

    assert_eq!(summary.total_syncs, 1, "the September row is not August's");
    assert_eq!(summary.total_items, 1);
    assert!(
        !summary.totals_complete,
        "a newer row says nothing about how far back the read reached"
    );
}

/// The host's runs add to the driver's (openhuman#6257), and a host log short
/// of a compaction's worth of rows has dropped none, so it cannot make the
/// totals a floor.
#[test]
fn both_logs_are_totalled_and_a_short_host_log_is_complete() {
    let driver = vec![
        row("2026-08-20T10:00:00Z", 3, 100, 20, 0.5),
        row("2026-07-31T23:59:59Z", 1, 1, 1, 1.0),
    ];
    let host = vec![row("2026-08-21T10:00:00Z", 4, 0, 0, 0.0)];

    let summary = summarise_logs(&driver, &host, "2026-08");

    assert_eq!(summary.month, "2026-08");
    assert_eq!(summary.total_syncs, 2);
    assert_eq!(summary.total_items, 7);
    assert_eq!(summary.total_input_tokens, 100);
    assert_eq!(summary.total_output_tokens, 20);
    assert!(close(summary.total_cost_usd, 0.5));
    assert!(
        summary.totals_complete,
        "the driver read reached past August and the host log never compacted"
    );
}

/// Completeness needs both halves: a driver read cut off at its cap leaves
/// the totals a floor however complete the host log is, and a host log at a
/// compaction's size has to prove its own reach the way the driver's does.
#[test]
fn either_log_can_leave_the_totals_a_floor() {
    let driver_capped = vec![row("2026-08-20T10:00:00Z", 1, 0, 0, 0.0)];
    let host_short = vec![row("2026-08-21T10:00:00Z", 1, 0, 0, 0.0)];
    assert!(!summarise_logs(&driver_capped, &host_short, "2026-08").totals_complete);

    let driver_complete = vec![row("2026-07-01T00:00:00Z", 1, 0, 0, 0.0)];
    let mut host_full: Vec<SyncAuditEntry> = (0..KEEP_ROWS)
        .map(|_| row("2026-08-21T10:00:00Z", 1, 0, 0, 0.0))
        .collect();
    assert!(
        !summarise_logs(&driver_complete, &host_full, "2026-08").totals_complete,
        "a compacted host log holding only this month's rows may have dropped some"
    );

    host_full.push(row("2026-07-30T00:00:00Z", 1, 0, 0, 0.0));
    assert!(summarise_logs(&driver_complete, &host_full, "2026-08").totals_complete);
}
