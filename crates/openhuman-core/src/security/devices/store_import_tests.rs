use super::*;
use crate::security::devices::store as live;
use crate::storage::local::{forget, table_names};
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

#[test]
fn legacy_devices_are_imported_once_through_the_public_api() {
    let (_dir, classic, default) = workspace();
    live::insert_device(&classic, "chan-a", "Phone", "pk-a", "hash-a").unwrap();
    live::insert_device(&classic, "chan-b", "Tablet", "pk-b", "hash-b").unwrap();
    live::insert_device(&classic, "chan-c", "Old laptop", "pk-c", "hash-c").unwrap();
    live::touch_device(&classic, "chan-a").unwrap();
    live::revoke_device(&classic, "chan-c").unwrap();
    let db = live::db_path(&classic);

    let listed = live::list_devices(&default).unwrap();
    let labels: Vec<_> = listed.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(labels, ["Phone", "Tablet"]);
    let phone = live::get_device(&default, "chan-a").unwrap().unwrap();
    assert_eq!(phone.device_pubkey, "pk-a");
    assert!(phone.last_seen_at.is_some(), "last_seen_at survives");
    assert!(!phone.revoked);
    let tablet = live::get_device(&default, "chan-b").unwrap().unwrap();
    assert!(tablet.last_seen_at.is_none());
    let old = live::get_device(&default, "chan-c").unwrap().unwrap();
    assert!(old.revoked, "a revoked device stays revoked");

    let tables = table_names(&db);
    assert!(tables.contains(&"_legacy_paired_devices".to_string()));
    assert!(!tables.contains(&"paired_devices".to_string()));
    let conn = rusqlite::Connection::open(&db).unwrap();
    let kept: i64 = conn
        .query_row("SELECT COUNT(*) FROM _legacy_paired_devices", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(kept, 3);

    // A restart does not import again.
    live::revoke_device(&default, "chan-b").unwrap();
    forget(&db);
    assert_eq!(live::list_devices(&default).unwrap().len(), 1);
}
