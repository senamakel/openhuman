use super::load_declared_modules;
use crate::config::Config;

#[tokio::test]
async fn boot_is_a_no_op_when_modules_are_disabled() {
    // Must not start a broker as a side effect of being switched off.
    let mut config = Config::default();
    config.modules.enabled = false;
    load_declared_modules(&config).await;
}

#[tokio::test]
async fn boot_tolerates_an_empty_search_path() {
    // The ordinary case on a fresh machine: nothing installed, nothing eager,
    // and boot must complete rather than warn or fail.
    let mut config = Config::default();
    config.modules.enabled = true;
    config.modules.allow_download = false;
    load_declared_modules(&config).await;
}

#[tokio::test]
async fn boot_skips_disabled_components() {
    // A config with every component off must boot without resolving any
    // eager module (no download, no dlopen) and without error.
    let mut config = Config::default();
    config.modules.enabled = true;
    config.modules.allow_download = false;
    for name in crate::core::runtime::COMPONENT_NAMES {
        assert!(config.components.set(name, false));
    }
    load_declared_modules(&config).await;
    for record in crate::modules::registry::ALL {
        assert!(
            crate::modules::ops::disabled_component(&config, record.id).is_some(),
            "{} should be disabled",
            record.id
        );
    }
}

#[tokio::test]
async fn boot_filters_disabled_search_path_artifacts_and_keeps_enabled_ones() {
    use std::sync::Mutex;

    // TinyBus reads this process setting at load time. Keep this test's change
    // scoped and serialized with any other boot test that changes the setting.
    static MODULE_PATH_LOCK: Mutex<()> = Mutex::new(());
    let _lock = MODULE_PATH_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let extension = if cfg!(windows) {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    let disabled = [
        dir.path().join(format!("libtinycomputer_module.{extension}")),
        dir.path().join(format!("tinycomputer.{extension}")),
        dir.path().join(format!("libtinycomputer.{extension}")),
    ];
    let enabled = dir.path().join(format!("libtinysearch_module.{extension}"));
    for artifact in &disabled {
        std::fs::write(artifact, b"not a real shared library").unwrap();
    }
    std::fs::write(&enabled, b"not a real shared library").unwrap();

    let previous = std::env::var_os("OPENHUMAN_MODULE_PATH");
    let search_path = std::env::join_paths([dir.path()]).unwrap();
    // SAFETY: this test holds its local lock and restores the process setting
    // before returning. The focused test command runs only this boot test.
    unsafe { std::env::set_var("OPENHUMAN_MODULE_PATH", search_path) };
    struct RestoreModulePath(Option<std::ffi::OsString>);
    impl Drop for RestoreModulePath {
        fn drop(&mut self) {
            // SAFETY: paired with the scoped setting above.
            unsafe {
                if let Some(value) = self.0.take() {
                    std::env::set_var("OPENHUMAN_MODULE_PATH", value);
                } else {
                    std::env::remove_var("OPENHUMAN_MODULE_PATH");
                }
            }
        }
    }
    let _restore = RestoreModulePath(previous);

    let mut config = Config::default();
    config.modules.enabled = true;
    config.modules.allow_download = false;
    assert!(config.components.set("computer", false));
    assert!(crate::modules::ops::disabled_lib_stems(&config)
        .iter()
        .any(|stem| stem == "libtinycomputer_module"));
    let stems = crate::modules::ops::disabled_lib_stems(&config);
    assert!(stems.iter().any(|stem| stem == "tinycomputer"));
    assert!(stems.iter().any(|stem| stem == "libtinycomputer"));
    assert!(crate::modules::ops::disabled_component(&config, "tinysearch").is_none());

    load_declared_modules(&config).await;

    let runtime = crate::modules::runtime::runtime().await.unwrap();
    let outcomes = runtime
        .host()
        .load_search_paths_except(&crate::modules::ops::disabled_lib_stems(&config));
    assert_eq!(
        outcomes.len(),
        1,
        "the disabled artifact should be ignored while the enabled artifact is attempted"
    );
    assert!(outcomes[0].is_err(), "the enabled fake library should be rejected");
    assert!(runtime.host().list().is_empty());
}

#[test]
fn the_catalog_warm_up_runs_only_while_connectors_are_on() {
    use super::should_warm_connector_catalogs;
    let mut config = Config::default();
    assert!(should_warm_connector_catalogs(&config));
    config.composio.mode = crate::config::schema::COMPOSIO_MODE_DISABLED.into();
    assert!(
        !should_warm_connector_catalogs(&config),
        "composio off must not load the connector module at boot"
    );
}
