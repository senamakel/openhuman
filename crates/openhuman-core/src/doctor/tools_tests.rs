use super::*;
use tinytools::{PermissionLevel, ToolScope};

fn cfg() -> Arc<Config> {
    Arc::new(Config::default())
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
