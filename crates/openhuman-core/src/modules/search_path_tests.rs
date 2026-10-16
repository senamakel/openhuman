use super::*;
use crate::config::Config;
use crate::modules::ops::disabled_lib_stems;
use std::path::PathBuf;
use tinybus::broker::Broker;
use tinybus::module::ModuleHost;

#[test]
fn skips_only_files_named_after_a_disabled_library() {
    let skip = vec!["libtinysearch".to_string(), "tinysearch".to_string()];
    assert!(is_skipped(&PathBuf::from("/x/libtinysearch.so"), &skip));
    assert!(is_skipped(&PathBuf::from("/x/tinysearch.dll"), &skip));
    assert!(!is_skipped(&PathBuf::from("/x/libtinydocs.so"), &skip));
}

#[test]
fn disabled_released_module_is_not_loaded_from_search_path() {
    let dir = tempfile::tempdir().unwrap();
    let artifact = dir.path().join("libtinycomputer_module.so");
    std::fs::write(&artifact, b"not a real shared library").unwrap();

    let mut config = Config::default();
    assert!(config.components.set("computer", false));
    let skip = disabled_lib_stems(&config);

    let host = ModuleHost::new(Broker::new());
    let outcomes = load_enabled_from_paths(&host, &skip, [dir.path().to_path_buf()]);

    assert!(
        outcomes.is_empty(),
        "disabled artifact produced load outcomes"
    );
    assert!(host.list().is_empty(), "disabled artifact was loaded");
}
