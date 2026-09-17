//! Which registry row a Composio connection belongs to (openhuman#6257).
//!
//! Driven through a real registry file: `list_sources_in` reads the config the
//! pass names, so each test writes one and points its `Config` at it.

use super::*;
use crate::memory::sources::MemorySourceEntry;

fn composio(
    id: &str,
    toolkit: &str,
    connection_id: &str,
    depth_days: Option<u32>,
) -> MemorySourceEntry {
    let mut entry: MemorySourceEntry = serde_json::from_value(serde_json::json!({
        "id": id,
        "kind": "composio",
        "label": format!("{toolkit} source"),
        "enabled": true,
        "toolkit": toolkit,
        "connection_id": connection_id,
    }))
    .expect("a valid composio source entry");
    entry.sync_depth_days = depth_days;
    entry
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

/// A config whose registry holds `entries`. The tempdir is returned so it
/// outlives the test body.
fn config_with(entries: &[MemorySourceEntry]) -> (tempfile::TempDir, Config) {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = Config {
        workspace_dir: dir.path().join("workspace"),
        config_path: dir.path().join("config.toml"),
        ..Config::default()
    };
    crate::memory::sources::registry::replace_sources_in(&config, entries)
        .expect("write the registry");
    (dir, config)
}

#[test]
fn a_connection_resolves_to_its_own_registry_row() {
    let (_dir, config) = config_with(&[
        folder("src_notes"),
        composio("src_gmail_work", "gmail", "ca_work", Some(30)),
        composio("src_gmail_home", "gmail", "ca_home", None),
        composio("src_notion", "notion", "ca_work", Some(14)),
    ]);

    assert_eq!(
        source_id_for_connection(&config, "gmail", "ca_home").as_deref(),
        Some("src_gmail_home"),
        "two connections of one toolkit are told apart by connection"
    );
    assert_eq!(
        source_id_for_connection(&config, " GMAIL ", "ca_work ").as_deref(),
        Some("src_gmail_work"),
        "matched the way the engine keys the rows: trimmed, toolkit case-insensitive"
    );
    assert_eq!(
        source_id_for_connection(&config, "notion", "ca_work").as_deref(),
        Some("src_notion"),
        "one connection id under two toolkits is told apart by toolkit"
    );
    assert_eq!(
        source_id_for_connection(&config, "gmail", "ca_unknown"),
        None
    );
}

#[test]
fn the_depth_cap_comes_from_the_same_row() {
    let (_dir, config) = config_with(&[
        composio("src_gmail_work", "gmail", "ca_work", Some(30)),
        composio("src_gmail_home", "gmail", "ca_home", None),
    ]);

    assert_eq!(
        source_sync_depth_days(&config, "gmail", "ca_work"),
        Some(30)
    );
    assert_eq!(source_sync_depth_days(&config, "gmail", "ca_home"), None);
    assert_eq!(source_sync_depth_days(&config, "slack", "ca_work"), None);
}

#[test]
fn an_unreadable_registry_degrades_to_no_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A directory where the config file should be cannot be read as one.
    let config_path = dir.path().join("config.toml");
    std::fs::create_dir_all(&config_path).expect("occupy the config path");
    let config = Config {
        workspace_dir: dir.path().join("workspace"),
        config_path,
        ..Config::default()
    };

    assert_eq!(source_id_for_connection(&config, "gmail", "ca_work"), None);
    assert_eq!(source_sync_depth_days(&config, "gmail", "ca_work"), None);
}
