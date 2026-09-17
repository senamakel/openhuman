//! The driver-backed run's stages and history row (openhuman#6257).

use std::cell::RefCell;

use super::*;
use crate::memory::sources::run_history::{read_runs, KEEP_ROWS};

type Stages = RefCell<Vec<(String, Option<String>)>>;

fn config_in(workspace: &std::path::Path) -> Config {
    Config {
        workspace_dir: workspace.to_path_buf(),
        ..Config::default()
    }
}

fn folder(id: &str) -> MemorySourceEntry {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "kind": "folder",
        "label": "Notes",
        "enabled": true,
        "path": ".",
    }))
    .expect("a valid folder source entry")
}

fn publisher(stages: &Stages) -> impl Fn(&str, Option<String>) + '_ {
    move |stage, detail| stages.borrow_mut().push((stage.to_string(), detail))
}

#[tokio::test]
async fn a_completed_run_publishes_its_start_and_finish_and_records_a_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config_in(dir.path());
    let entry = folder("src_notes");
    let stages = Stages::default();

    let outcome = run_recorded(
        &config,
        "src_notes",
        Some(&entry),
        || async {
            Ok(SyncRunOutcome {
                records_ingested: 3,
                ..SyncRunOutcome::default()
            })
        },
        |_| unreachable!("a completed run has no failure to describe"),
        publisher(&stages),
    )
    .await
    .expect("the run completed");

    assert_eq!(outcome.records_ingested, 3);
    assert_eq!(
        *stages.borrow(),
        vec![
            ("running".to_string(), None),
            (
                "completed".to_string(),
                Some("ingested 3 item(s)".to_string())
            ),
        ]
    );
    let rows = read_runs(dir.path(), KEEP_ROWS).expect("read the run log");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert!(row.success);
    assert_eq!(row.source_id, "src_notes");
    assert_eq!(row.source_kind, "folder");
    assert_eq!(row.scope, "src_notes", "a folder has no URL or toolkit");
    assert_eq!(row.items_fetched, 3);
}

#[tokio::test]
async fn a_completed_run_that_stopped_short_says_so_in_its_detail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config_in(dir.path());
    let stages = Stages::default();

    run_recorded(
        &config,
        "src_capped",
        Some(&folder("src_capped")),
        || async {
            Ok(SyncRunOutcome {
                records_ingested: 50,
                more_pending: true,
                note: Some("per-source item limit reached".to_string()),
                ..SyncRunOutcome::default()
            })
        },
        |_| unreachable!("a completed run has no failure to describe"),
        publisher(&stages),
    )
    .await
    .expect("the run completed");

    let finish = stages.borrow().last().cloned().expect("a terminal stage");
    assert_eq!(finish.0, "completed");
    let detail = finish.1.expect("a completed detail");
    assert!(
        detail.starts_with("ingested 50 item(s), more pending"),
        "{detail}"
    );
    assert!(
        detail.ends_with("; per-source item limit reached"),
        "{detail}"
    );
}

#[tokio::test]
async fn a_failed_run_publishes_the_described_failure_and_records_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config_in(dir.path());
    let entry = folder("src_gone");
    let stages = Stages::default();

    let message = run_recorded(
        &config,
        "src_gone",
        Some(&entry),
        || async {
            Err(MemoryError::NotFound(
                "no memory source registered as src_gone".to_string(),
            ))
        },
        |error| format!("described: {error}"),
        publisher(&stages),
    )
    .await
    .expect_err("the run failed");

    assert!(message.starts_with("described: "), "{message}");
    assert_eq!(
        *stages.borrow(),
        vec![
            ("running".to_string(), None),
            ("failed".to_string(), Some(message.clone())),
        ]
    );
    let rows = read_runs(dir.path(), KEEP_ROWS).expect("read the run log");
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].success);
    assert_eq!(rows[0].error.as_deref(), Some(message.as_str()));
    assert_eq!(rows[0].items_fetched, 0);
}

/// The history panel refetches when the terminal stage arrives, so the row
/// has to be on disk by then.
#[tokio::test]
async fn the_row_is_on_disk_before_the_terminal_stage_goes_out() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config_in(dir.path());
    let rows_at_finish = RefCell::new(None);

    run_recorded(
        &config,
        "src_unknown",
        None,
        || async { Ok(SyncRunOutcome::default()) },
        |_| unreachable!("a completed run has no failure to describe"),
        |stage: &str, _detail: Option<String>| {
            if stage == "completed" {
                let rows = read_runs(dir.path(), KEEP_ROWS).expect("read the run log");
                *rows_at_finish.borrow_mut() = Some(rows);
            }
        },
    )
    .await
    .expect("the run completed");

    let rows = rows_at_finish
        .into_inner()
        .expect("the terminal stage was published");
    assert_eq!(rows.len(), 1, "a refetch on `completed` must find the row");
    // A source the host registry does not hold is still recorded, under its id.
    assert_eq!(rows[0].source_kind, "unknown");
    assert_eq!(rows[0].scope, "src_unknown");
}

#[test]
fn the_history_scope_prefers_the_url_then_the_toolkit_then_the_id() {
    let mut entry = folder("src_scope");
    assert_eq!(history_scope(&entry), "src_scope");
    entry.toolkit = Some("gmail".to_string());
    assert_eq!(history_scope(&entry), "gmail");
    entry.url = Some("https://github.com/tinyhumansai/openhuman".to_string());
    assert_eq!(
        history_scope(&entry),
        "https://github.com/tinyhumansai/openhuman"
    );
}

#[test]
fn the_stage_event_names_the_source_for_the_row_indicator() {
    match stage_event(
        "src_ev",
        "folder",
        "completed",
        Some("ingested 1 item(s)".into()),
    ) {
        DomainEvent::MemorySyncStageChanged {
            trigger,
            stage,
            provider,
            connection_id,
            detail,
            source_id,
        } => {
            assert_eq!(trigger, "manual");
            assert_eq!(stage, "completed");
            assert_eq!(provider.as_deref(), Some("folder"));
            assert_eq!(connection_id.as_deref(), Some("src_ev"));
            assert_eq!(source_id.as_deref(), Some("src_ev"));
            assert_eq!(detail.as_deref(), Some("ingested 1 item(s)"));
        }
        other => panic!("expected a sync stage event, got {other:?}"),
    }
}
