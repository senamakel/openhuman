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
async fn models_tool_returns_an_inspection_report_when_no_provider_is_configured() {
    DoctorModelsTool::new(cfg())
        .execute(serde_json::json!({ "use_cache": true }))
        .await
        .expect("doctor_models returns the actual empty inspection result");
}
