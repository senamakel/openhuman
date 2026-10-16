use super::*;
use crate::modules::ops::disabled_lib_stems;
use crate::modules::registry;
use crate::config::Config;
use std::path::PathBuf;
use tinybus::{Broker, module::ModuleHost};

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
    assert!(skip.contains(&"libtinycomputer_module".to_string()));

    let original = std::env::var_os("OPENHUMAN_MODULE_PATH");
    // The test uses an isolated search-path directory containing the released
    // artifact name; no other test mutates this process environment.
    unsafe { std::env::set_var("OPENHUMAN_MODULE_PATH", dir.path()) };
    let host = ModuleHost::new(Broker::new());
    let outcomes = load_enabled(&host, &skip);
    match original {
        Some(value) => unsafe { std::env::set_var("OPENHUMAN_MODULE_PATH", value) },
        None => unsafe { std::env::remove_var("OPENHUMAN_MODULE_PATH") },
    }

    assert!(outcomes.is_empty(), "disabled artifact produced load outcomes");
    assert!(host.list().is_empty(), "disabled artifact was loaded");
}
