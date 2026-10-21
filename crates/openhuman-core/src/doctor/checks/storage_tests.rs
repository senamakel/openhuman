use super::*;
use crate::core::runtime::ComponentSet;
use crate::doctor::contract::{DoctorCtx, FindingSeverity};

fn context(root: &std::path::Path, url: String) -> DoctorCtx {
    let mut config = crate::config::Config::default();
    config.workspace_dir = root.join("workspace");
    config.action_dir = root.join("actions");
    config.config_path = root.join("config.toml");
    config.storage.url = Some(url);
    DoctorCtx::from_config_with_components(config, ComponentSet::all())
}

#[test]
fn sqlite_connect_uses_read_only_probe_and_does_not_create_missing_paths() {
    let root = tempfile::tempdir().expect("temporary storage root");
    let missing = root.path().join("not-created/storage.db");
    let findings = StorageConnect.detect_blocking(&context(
        root.path(),
        format!("sqlite:{}", missing.display()),
    )).expect("blocking storage check");
    assert_eq!(findings[0].severity, FindingSeverity::Error);
    assert!(!missing.exists());
    assert!(!missing.parent().expect("parent").exists());
}

#[test]
fn corrupt_sqlite_is_an_error_without_echoing_its_location() {
    let root = tempfile::tempdir().expect("temporary storage root");
    let file = root.path().join("private-path.sqlite");
    std::fs::write(&file, b"not a database").expect("write corrupt fixture");
    let before = std::fs::read(&file).expect("read before probe");
    let findings = StorageConnect.detect_blocking(&context(
        root.path(),
        format!("sqlite:{}", file.display()),
    )).expect("blocking storage check");
    assert_eq!(findings[0].severity, FindingSeverity::Error);
    assert!(!findings[0].message.contains("private-path"));
    assert_eq!(std::fs::read(file).expect("read after probe"), before);
}

#[test]
fn unsupported_storage_driver_is_reported_as_skipped() {
    let root = tempfile::tempdir().expect("temporary storage root");
    let findings = StorageConnect.detect_blocking(&context(
        root.path(),
        "file:private-storage-root".to_owned(),
    )).expect("blocking storage check");
    assert_eq!(findings[0].severity, FindingSeverity::Skipped);
    assert!(!findings[0].message.contains("private-storage-root"));
}

