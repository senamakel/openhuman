use super::*;
use tinymemory_integrations::sources::SourceStatus;

#[test]
fn corrupt_source_state_file_is_set_aside() {
    let tmp = tempfile::tempdir().unwrap();
    let file = path(tmp.path());
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"{\"source-1\":").unwrap();

    assert!(load(tmp.path()).is_empty());
    assert!(
        !file.exists(),
        "corrupt state must not be silently replaced"
    );
    assert_eq!(
        std::fs::read(file.with_extension("json.corrupt")).unwrap(),
        b"{\"source-1\":"
    );
}

#[test]
fn native_source_state_keeps_the_persisted_host_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let file = path(tmp.path());
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let saved = serde_json::json!({
        "src-1": {
            "last_sync_at": "2025-01-02T03:04:05Z",
            "status": "error",
            "error": "reader failed",
            "items": 7
        }
    });
    std::fs::write(&file, serde_json::to_vec(&saved).unwrap()).unwrap();

    let all = load(tmp.path());
    assert_eq!(all["src-1"].status, SourceStatus::Error);
    assert_eq!(all["src-1"].error.as_deref(), Some("reader failed"));
    assert_eq!(all["src-1"].items, 7);
    assert_eq!(serde_json::to_value(all).unwrap(), saved);
}
