//! Tests for the sync activity tracker. The map is process-global, so every
//! test uses its own source ids rather than resetting it under the others.

use super::*;

fn stage_event(source_id: Option<&str>, stage: &str, detail: Option<&str>) -> DomainEvent {
    DomainEvent::MemorySyncStageChanged {
        trigger: "manual".into(),
        stage: stage.into(),
        provider: Some("gmail".into()),
        connection_id: Some("conn-1".into()),
        detail: detail.map(str::to_string),
        source_id: source_id.map(str::to_string),
    }
}

#[tokio::test]
async fn a_running_stage_puts_the_source_in_flight_until_its_terminal_stage() {
    let tracker = SyncActivityTracker;
    tracker
        .handle(&stage_event(Some("src-live"), "running", None))
        .await;
    let live = live_sync("src-live").expect("in flight after `running`");
    assert_eq!(live.stage, "running");
    assert_eq!(live.detail, None);

    tracker
        .handle(&stage_event(
            Some("src-live"),
            "completed",
            Some("ingested 3 item(s)"),
        ))
        .await;
    assert_eq!(live_sync("src-live"), None, "`completed` takes it out");
}

#[tokio::test]
async fn a_failed_stage_takes_the_source_out_too() {
    let tracker = SyncActivityTracker;
    tracker
        .handle(&stage_event(
            Some("src-fail"),
            "fetching",
            Some("1/4 pages"),
        ))
        .await;
    assert_eq!(
        live_sync("src-fail").map(|live| live.detail),
        Some(Some("1/4 pages".to_string()))
    );
    tracker
        .handle(&stage_event(Some("src-fail"), "failed", Some("boom")))
        .await;
    assert_eq!(live_sync("src-fail"), None);
}

#[tokio::test]
async fn a_stage_without_a_source_id_is_not_remembered() {
    SyncActivityTracker
        .handle(&stage_event(None, "running", None))
        .await;
    assert_eq!(live_sync("conn-1"), None, "the connection id is not a row");
}

#[test]
fn a_run_with_no_terminal_stage_goes_stale_after_the_ceiling() {
    note_stage_at("src-stale", "running", None, 1_000);
    assert!(live_sync_at("src-stale", 1_000 + STALE_AFTER_MS).is_some());
    assert_eq!(
        live_sync_at("src-stale", 1_000 + STALE_AFTER_MS + 1),
        None,
        "a lost `completed` must not pin the row forever"
    );
}

/// A stale entry leaves the map on the next touch of any kind — a read for
/// another source, a stage for another source, or the explicit sweep — so a
/// source removed mid-run does not keep its entry for the life of the process.
#[test]
fn stale_entries_are_pruned_on_the_next_touch() {
    let has = |id: &str| live().lock().unwrap().contains_key(id);

    note_stage_at("src-prune-a", "running", None, 1_000);
    assert!(has("src-prune-a"));
    let _ = live_sync_at("src-prune-other", 1_000 + STALE_AFTER_MS + 1);
    assert!(!has("src-prune-a"), "a read for another source sweeps it");

    note_stage_at("src-prune-b", "running", None, 5_000);
    note_stage_at("src-prune-c", "running", None, 5_000 + STALE_AFTER_MS + 1);
    assert!(!has("src-prune-b"), "a stage for another source sweeps it");
    assert!(has("src-prune-c"));

    note_stage_at("src-prune-d", "running", None, 0);
    prune_stale();
    assert!(!has("src-prune-d"), "the explicit sweep drops an old entry");
}

#[test]
fn the_latest_stage_wins() {
    note_stage_at("src-latest", "requested", None, 10);
    note_stage_at("src-latest", "ingesting", Some("queue_depth=2"), 20);
    let live = live_sync_at("src-latest", 25).expect("still in flight");
    assert_eq!(live.stage, "ingesting");
    assert_eq!(live.updated_at_ms, 20);
}

fn empty_maps() -> (HashMap<String, LiveSync>, HashMap<String, i64>) {
    (HashMap::new(), HashMap::new())
}

/// A per-item stage that trails the run's terminal stage does not reopen it:
/// the bridge re-emits the module's per-document events, and they can arrive
/// after `completed` (openhuman#6257).
#[test]
fn a_late_item_stage_does_not_reopen_a_finished_run() {
    let (mut in_flight, mut finished_at) = empty_maps();
    apply_stage(&mut in_flight, &mut finished_at, "src", "running", None, 10);
    apply_stage(
        &mut in_flight,
        &mut finished_at,
        "src",
        "queued",
        Some("queued chunk extraction"),
        20,
    );
    assert!(
        in_flight.contains_key("src"),
        "an item stage inside a run keeps the run live"
    );
    apply_stage(
        &mut in_flight,
        &mut finished_at,
        "src",
        "completed",
        Some("ingested 2 item(s)"),
        30,
    );
    for (stage, at_ms) in [("stored", 40), ("queued", 41), ("ingesting", 42)] {
        apply_stage(&mut in_flight, &mut finished_at, "src", stage, None, at_ms);
        assert!(
            !in_flight.contains_key("src"),
            "`{stage}` after `completed` must not reopen the run"
        );
    }
}

/// A new run is a new run: its start clears the finished marker, and its own
/// item stages count again.
#[test]
fn a_new_run_after_a_finished_one_is_tracked_again() {
    for start in ["running", "requested", "fetching"] {
        let (mut in_flight, mut finished_at) = empty_maps();
        apply_stage(
            &mut in_flight,
            &mut finished_at,
            "src",
            "failed",
            Some("boom"),
            10,
        );
        apply_stage(&mut in_flight, &mut finished_at, "src", start, None, 20);
        apply_stage(&mut in_flight, &mut finished_at, "src", "queued", None, 30);
        assert_eq!(
            in_flight.get("src").map(|live| live.stage.as_str()),
            Some("queued"),
            "after `{start}`"
        );
        assert!(!finished_at.contains_key("src"), "after `{start}`");
    }
}

/// The marker ages out with the same ceiling as a live entry, so a source
/// whose next run never announced its start is not ignored for the life of
/// the process.
#[test]
fn the_finished_marker_ages_out_with_the_ceiling() {
    let (mut in_flight, mut finished_at) = empty_maps();
    apply_stage(
        &mut in_flight,
        &mut finished_at,
        "src",
        "completed",
        None,
        0,
    );
    apply_stage(
        &mut in_flight,
        &mut finished_at,
        "src",
        "queued",
        None,
        STALE_AFTER_MS + 1,
    );
    assert!(in_flight.contains_key("src"));
}

/// Through the bus subscriber, against the process-wide maps.
#[tokio::test]
async fn the_tracker_ignores_a_queued_stage_that_trails_completed() {
    let tracker = SyncActivityTracker;
    for (stage, detail) in [
        ("running", None),
        ("completed", Some("ingested 1 item(s)")),
        ("queued", Some("queued chunk extraction")),
    ] {
        tracker
            .handle(&stage_event(Some("src-trailing"), stage, detail))
            .await;
    }
    assert_eq!(live_sync("src-trailing"), None);
}

#[test]
fn the_explicit_sweep_drops_old_finished_markers_too() {
    finished()
        .lock()
        .unwrap()
        .insert("src-old-marker".to_string(), 0);
    prune_stale();
    assert!(!finished().lock().unwrap().contains_key("src-old-marker"));
}
