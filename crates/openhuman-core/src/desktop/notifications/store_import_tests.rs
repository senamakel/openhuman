use super::*;
use crate::desktop::notifications::store as live;
use crate::desktop::notifications::types::{
    CoreNotificationCategory, CoreNotificationEvent, IntegrationNotification, NotificationSettings,
    NotificationStatus,
};
use crate::storage::local::{forget, table_names};
use chrono::{Duration, Utc};
use tempfile::TempDir;

fn workspace() -> (TempDir, Config, Config) {
    let dir = TempDir::new().unwrap();
    let mut classic = Config {
        workspace_dir: dir.path().to_path_buf(),
        ..Config::default()
    };
    classic.storage.url = Some("classic".into());
    let default = Config {
        workspace_dir: dir.path().to_path_buf(),
        ..Config::default()
    };
    (dir, classic, default)
}

fn note(id: &str, provider: &str, secs_ago: i64) -> IntegrationNotification {
    IntegrationNotification {
        id: id.to_string(),
        provider: provider.to_string(),
        account_id: Some("acct".into()),
        title: format!("title {id}"),
        body: format!("body {id}"),
        raw_payload: serde_json::json!({ "k": id }),
        importance_score: None,
        triage_action: None,
        triage_reason: None,
        status: NotificationStatus::Unread,
        received_at: Utc::now() - Duration::seconds(secs_ago),
        scored_at: None,
    }
}

fn event(id: &str, ts: u64) -> CoreNotificationEvent {
    CoreNotificationEvent {
        id: id.to_string(),
        category: CoreNotificationCategory::Agents,
        title: "Cron job completed".into(),
        body: "done".into(),
        deep_link: None,
        timestamp_ms: ts,
        actions: None,
        workspace: None,
        workspace_revision: None,
    }
}

#[test]
fn legacy_notifications_are_imported_once_through_the_public_api() {
    let (_dir, classic, default) = workspace();
    live::insert(&classic, &note("n-old", "slack", 60)).unwrap();
    live::insert(&classic, &note("n-new", "gmail", 0)).unwrap();
    live::insert(&classic, &note("n-read", "gmail", 30)).unwrap();
    live::update_triage(&classic, "n-new", 0.8, "react", "important").unwrap();
    live::mark_read(&classic, "n-read").unwrap();
    live::upsert_settings(
        &classic,
        &NotificationSettings {
            provider: "slack".into(),
            enabled: false,
            importance_threshold: 0.5,
            route_to_orchestrator: false,
        },
    )
    .unwrap();
    live::insert_core_notification(&classic, &event("e1", 1_000)).unwrap();
    live::insert_core_notification(&classic, &event("e2", 2_000)).unwrap();
    live::mark_core_notification_read(&classic, "e1").unwrap();
    let db = live::db_path(&classic);

    let listed = live::list(&default, 10, 0, None, None).unwrap();
    let ids: Vec<_> = listed.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["n-new", "n-read", "n-old"]);
    let scored = &listed[0];
    assert_eq!(scored.importance_score, Some(0.8));
    assert_eq!(scored.triage_action.as_deref(), Some("react"));
    assert_eq!(scored.raw_payload["k"], "n-new");
    assert_eq!(scored.account_id.as_deref(), Some("acct"));
    assert_eq!(listed[1].status, NotificationStatus::Read);
    assert_eq!(live::unread_count(&default).unwrap(), 2);
    let settings = live::get_settings(&default, "slack").unwrap();
    assert!(!settings.enabled);
    assert!(!settings.route_to_orchestrator);
    assert!((settings.importance_threshold - 0.5).abs() < f32::EPSILON);
    let core = live::list_core_notifications(&default, false, 10).unwrap();
    let core_ids: Vec<_> = core.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(core_ids, ["e2", "e1"]);
    assert_eq!(live::unread_core_notification_count(&default).unwrap(), 1);

    let tables = table_names(&db);
    for name in [
        "integration_notifications",
        "notification_settings",
        "core_notifications",
    ] {
        assert!(tables.contains(&format!("_legacy_{name}")), "{name}");
        assert!(!tables.contains(&name.to_string()), "{name} was renamed");
    }

    // A restart does not import again.
    live::mark_dismissed(&default, "n-old").unwrap();
    forget(&db);
    let after = live::list(&default, 10, 0, None, None).unwrap();
    let old = after.iter().find(|n| n.id == "n-old").unwrap();
    assert_eq!(old.status, NotificationStatus::Dismissed);
    assert_eq!(after.len(), 3);
}

#[test]
fn core_notifications_survive_a_moved_workspace() {
    let (dir, classic, default) = workspace();
    live::insert_core_notification(&classic, &event("e1", 1_000)).unwrap();
    // Imported, then one more written through the default store.
    assert_eq!(live::unread_core_notification_count(&default).unwrap(), 1);
    live::insert_core_notification(&default, &event("e2", 2_000)).unwrap();
    let db = live::db_path(&default);
    forget(&db);

    // The workspace is moved; its database comes along.
    let moved_root = TempDir::new().unwrap();
    let moved_dir = moved_root.path().join("moved");
    std::fs::rename(dir.path(), &moved_dir).unwrap();
    let moved = Config {
        workspace_dir: moved_dir.clone(),
        ..Config::default()
    };
    let core = live::list_core_notifications(&moved, false, 10).unwrap();
    let ids: Vec<_> = core.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["e2", "e1"]);
    assert!(live::mark_core_notification_read(&moved, "e1").unwrap());
    assert_eq!(live::unread_core_notification_count(&moved).unwrap(), 1);
    forget(&live::db_path(&moved));
    // Put it back so the TempDir cleans up.
    std::fs::rename(&moved_dir, dir.path()).unwrap();
}
