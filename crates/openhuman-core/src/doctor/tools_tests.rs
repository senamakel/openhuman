use super::*;
use crate::core::runtime::{ContextOverlay, CoreContext, DomainSet};
use crate::core::slots::Extensions;
use crate::tools::toolpacks::ToolGroups;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tinytools::{PermissionLevel, ToolScope};

fn cfg() -> Arc<Config> {
    Arc::new(Config::default())
}

fn config_in(tmp: &TempDir) -> Config {
    let workspace = tmp.path().join("workspace");
    let action_dir = tmp.path().join("action");
    std::fs::create_dir_all(&workspace).expect("create test workspace");
    std::fs::create_dir_all(&action_dir).expect("create test action directory");

    Config {
        workspace_dir: workspace.clone(),
        action_dir,
        config_path: workspace.join("config.toml"),
        ..Config::default()
    }
}

fn scoped_context(config: &Config, extensions: Extensions) -> Arc<CoreContext> {
    let parent = CoreContext::for_test_with_extensions(
        DomainSet::full(),
        Some(config.workspace_dir.clone()),
        extensions,
    );
    parent.derive_with(
        ContextOverlay::new(config.clone(), DomainSet::full(), ToolGroups::none())
            .without_user_skill_roots(),
    )
}

#[test]
fn metadata() {
    assert_eq!(DoctorHealthTool::new(cfg()).name(), "doctor_health");
    assert_eq!(DoctorModelsTool::new(cfg()).name(), "doctor_models");
    assert_eq!(
        DoctorHealthTool::new(cfg()).permission_level(),
        PermissionLevel::ReadOnly
    );
    assert_eq!(DoctorHealthTool::new(cfg()).scope(), ToolScope::All);
}

#[test]
fn health_schema_exposes_only_optional_tier_and_exact_id_selection() {
    let tool = DoctorHealthTool::new(cfg());
    let schema = tool.parameters_schema();

    assert_eq!(schema["type"], "object");
    assert_eq!(
        schema["properties"]["tier"]["enum"],
        serde_json::json!(["static", "connect", "live"])
    );
    assert_eq!(schema["properties"]["tier"]["default"], "connect");
    assert_eq!(schema["properties"]["only"]["type"], "array");
    assert_eq!(schema["properties"]["only"]["items"]["type"], "string");
    assert!(
        schema["required"].is_null(),
        "both inputs should be optional"
    );

    let description = format!(
        "{} {}",
        tool.description(),
        schema["properties"]["tier"]["description"]
            .as_str()
            .expect("tier description")
    )
    .to_ascii_lowercase();
    assert!(description.contains("live"), "{description}");
    assert!(description.contains("token"), "{description}");
    assert!(
        description.contains("explicit user request"),
        "{description}"
    );
}

#[tokio::test]
async fn health_tool_forwards_tier_and_only_to_the_scoped_check_registry() {
    let tmp = TempDir::new().expect("tempdir");
    let config = config_in(&tmp);
    let selected_calls = Arc::new(AtomicUsize::new(0));
    let unselected_calls = Arc::new(AtomicUsize::new(0));
    let extensions = Extensions::new();
    crate::doctor::register_check_source_in(
        &extensions,
        crate::doctor::test_support::FakeCheckSource::new(
            "doctor-health-tool-forwarding",
            vec![
                crate::doctor::test_support::FakeCheck::pass("test/selected")
                    .with_tier(crate::doctor::DoctorTier::Static)
                    .with_call_counter(Arc::clone(&selected_calls)),
                crate::doctor::test_support::FakeCheck::pass("test/unselected")
                    .with_tier(crate::doctor::DoctorTier::Static)
                    .with_call_counter(Arc::clone(&unselected_calls)),
            ],
        ),
    );
    let context = scoped_context(&config, extensions);
    let tool = DoctorHealthTool::new(Arc::new(config));

    let result = CoreContext::scope(
        context,
        tool.execute(serde_json::json!({
            "tier": "static",
            "only": ["test/selected"]
        })),
    )
    .await
    .expect("the scoped static check should return a report");
    let report: serde_json::Value =
        serde_json::from_str(&result.text()).expect("tool result is a JSON doctor report");

    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["tier"], "static");
    assert_eq!(report["checks_run"], serde_json::json!(["test/selected"]));
    assert_eq!(selected_calls.load(Ordering::SeqCst), 1);
    assert_eq!(unselected_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn health_tool_defaults_to_connect_when_tier_is_omitted() {
    let tmp = TempDir::new().expect("tempdir");
    let config = config_in(&tmp);
    let selected_calls = Arc::new(AtomicUsize::new(0));
    let extensions = Extensions::new();
    crate::doctor::register_check_source_in(
        &extensions,
        crate::doctor::test_support::FakeCheckSource::new(
            "doctor-health-tool-default-tier",
            vec![
                crate::doctor::test_support::FakeCheck::pass("test/default-connect")
                    .with_tier(crate::doctor::DoctorTier::Connect)
                    .with_call_counter(Arc::clone(&selected_calls)),
            ],
        ),
    );
    let context = scoped_context(&config, extensions);
    let tool = DoctorHealthTool::new(Arc::new(config));

    let result = CoreContext::scope(
        context,
        tool.execute(serde_json::json!({
            "only": ["test/default-connect"]
        })),
    )
    .await
    .expect("the selected connect check should return a report");
    let report: serde_json::Value =
        serde_json::from_str(&result.text()).expect("tool result is a JSON doctor report");

    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["tier"], "connect");
    assert_eq!(
        report["checks_run"],
        serde_json::json!(["test/default-connect"])
    );
    assert_eq!(selected_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn models_tool_reports_unavailable_until_catalog_probes_exist() {
    let error = DoctorModelsTool::new(cfg())
        .execute(serde_json::json!({ "use_cache": true }))
        .await
        .expect_err("a model probe that does not run must not return a healthy report")
        .to_string();

    assert!(
        error.to_lowercase().contains("not available"),
        "the unavailable model probe should explain its status: {error}"
    );
}
