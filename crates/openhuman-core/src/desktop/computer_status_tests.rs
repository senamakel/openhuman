use super::*;

#[tokio::test]
async fn status_without_load_reports_routes_and_skips_describe() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.config_path = dir.path().join("config.toml");
    config.workspace_dir = dir.path().join("workspace");
    config.secrets.encrypt = false;
    config.computer.decision_model = crate::config::DecisionModel::Sage;
    let status = status(&config, false).await;
    assert_eq!(status.decision_model, "sage");
    assert_eq!(status.decision_route, "sage");
    assert_eq!(status.planner_route, "hosted");
    assert!(status.module.is_some());
    assert!(status.capabilities.is_none());
    assert!(status.error.is_none());
}

#[tokio::test]
async fn status_does_not_build_private_config_or_touch_credentials_and_trace_directory() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.config_path = dir.path().join("config.toml");
    config.workspace_dir = dir.path().join("workspace");
    config.secrets.encrypt = false;
    config.computer.trace = true;
    crate::desktop::computer_config::reset_private_config_credential_lookups();
    let _fake_credentials = crate::desktop::computer_config::FakePrivateConfigCredentialsGuard::new();

    let _status = status(&config, false).await;
    let observations = (
        crate::desktop::computer_config::private_config_credential_lookups(),
        crate::desktop::computer_config::trace_dir(&config).exists(),
    );
    assert_eq!(observations, (0, false));
}

#[tokio::test]
async fn load_on_a_disabled_host_reports_the_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.config_path = dir.path().join("config.toml");
    config.workspace_dir = dir.path().join("workspace");
    config.modules.enabled = false;
    let status = status(&config, true).await;
    assert!(status.capabilities.is_none());
    assert!(status.error.as_deref().is_some_and(|e| !e.is_empty()));
}

#[test]
fn capabilities_report_compatibility() {
    let capabilities: Capabilities = serde_json::from_value(serde_json::json!({
        "contract_version": [tinycomputer_bus::CONTRACT_VERSION.0, 99],
        "surfaces": [{"kind": "browser", "available": true}],
        "jev_configured": true,
        "planner_configured": true,
        "rescue_configured": true,
        "output_configured": false,
        "step_kinds": [],
        "guide": "",
        "members": [],
        "examples": []
    }))
    .unwrap();
    let view = ComputerCapabilities::from(capabilities);
    assert!(view.compatible);
    assert!(view.rescue_configured);
    assert_eq!(view.surfaces.len(), 1);
}
