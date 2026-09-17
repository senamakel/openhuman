//! Which connector runs earn a history row, and the row they write
//! (openhuman#6257).

use chrono::TimeZone;

use super::*;
use crate::memory::sources::run_history::{read_runs, KEEP_ROWS};

fn finished_at() -> DateTime<Utc> {
    Utc.timestamp_opt(1_789_000_000, 0)
        .single()
        .expect("a valid instant")
}

fn gmail_run(
    reason: &'static str,
    written: u64,
    error: Option<&'static str>,
) -> ConnectorRun<'static> {
    ConnectorRun {
        toolkit: "gmail",
        connection_id: "ca_1",
        source_id: None,
        reason,
        started: Instant::now(),
        written,
        error,
    }
}

#[test]
fn a_run_someone_started_always_earns_a_row() {
    for reason in ["manual", "connection_created"] {
        assert!(
            should_record(reason, 0, false),
            "{reason} with nothing written"
        );
        assert!(should_record(reason, 3, false), "{reason} with items");
        assert!(should_record(reason, 0, true), "{reason} that failed");
    }
}

#[test]
fn a_periodic_run_earns_a_row_only_when_it_wrote_or_failed() {
    assert!(
        !should_record("periodic", 0, false),
        "a quiet poll adds no row"
    );
    assert!(should_record("periodic", 1, false));
    assert!(should_record("periodic", 0, true));
}

#[test]
fn the_scope_is_the_ingest_funnels_path_scope() {
    assert_eq!(connector_scope(" GMAIL ", " ca_1 "), "gmail:ca_1");
}

#[test]
fn the_row_names_the_source_when_one_is_known_and_the_scope_otherwise() {
    let run = gmail_run("manual", 12, None);

    let known = connector_run_entry(&run, Some("src_gmail".to_string()), finished_at());
    assert_eq!(known.source_id, "src_gmail");
    assert_eq!(known.source_kind, "composio");
    assert_eq!(known.scope, "gmail:ca_1");
    assert!(known.success);
    assert_eq!(known.items_fetched, 12);
    assert_eq!(known.timestamp, finished_at());

    let unknown = connector_run_entry(&run, None, finished_at());
    assert_eq!(unknown.source_id, "gmail:ca_1");
}

#[test]
fn a_failed_run_row_carries_the_error_and_what_it_wrote() {
    let row = connector_run_entry(
        &gmail_run("manual", 4, Some("module unavailable")),
        None,
        finished_at(),
    );
    assert!(!row.success);
    assert_eq!(row.error.as_deref(), Some("module unavailable"));
    assert_eq!(
        row.items_fetched, 4,
        "items written before the failure are committed"
    );
}

#[test]
fn record_skips_a_quiet_periodic_poll_and_writes_the_rest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = Config {
        workspace_dir: dir.path().join("workspace"),
        config_path: dir.path().join("config.toml"),
        ..Config::default()
    };

    record(&config, &gmail_run("periodic", 0, None));
    assert!(
        read_runs(&config.workspace_dir, KEEP_ROWS)
            .expect("read the run log")
            .is_empty(),
        "a periodic poll that wrote nothing is not recorded"
    );

    let mut started_by_a_row = gmail_run("manual", 2, None);
    started_by_a_row.source_id = Some("src_gmail");
    record(&config, &started_by_a_row);
    record(
        &config,
        &gmail_run("periodic", 0, Some("module unavailable")),
    );

    let rows = read_runs(&config.workspace_dir, KEEP_ROWS).expect("read the run log");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].error.as_deref(), Some("module unavailable"));
    assert_eq!(
        rows[0].source_id, "gmail:ca_1",
        "no registry row for the connection, so the row names its scope"
    );
    assert_eq!(rows[1].source_id, "src_gmail");
}
