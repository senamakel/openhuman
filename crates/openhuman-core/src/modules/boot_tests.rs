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
    use super::load_search_path_modules;
    let mut config = Config::default();
    assert!(config.components.set("computer", false));
    assert!(crate::modules::ops::disabled_component(&config, "tinysearch").is_none());

    let mut observed = Vec::new();
    let outcomes = load_search_path_modules(&config, |skip| {
        observed.extend_from_slice(skip);
        Vec::new()
    });

    assert!(outcomes.is_empty());
    assert!(observed.iter().any(|stem| stem == "tinycomputer_module"));
    assert!(observed.iter().any(|stem| stem == "libtinycomputer_module"));
    assert!(observed.iter().any(|stem| stem == "tinycomputer"));
    assert!(observed.iter().any(|stem| stem == "libtinycomputer"));
    assert!(!observed.iter().any(|stem| stem == "tinysearch_module"));
    assert!(!observed.iter().any(|stem| stem == "libtinysearch_module"));
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
