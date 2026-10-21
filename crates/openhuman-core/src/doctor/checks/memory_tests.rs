use super::*;
use crate::doctor::contract::{DoctorConfigSource, DoctorCtx, FindingSeverity};
use crate::doctor::core::MemoryEngineCheck;
use crate::memory::engine::DISABLED_ENGINE;
use crate::core::runtime::ComponentSet;

fn context(engine: &str) -> DoctorCtx {
    let mut config = crate::config::Config::default();
    config.memory.engine = engine.to_owned();
    DoctorCtx::from_config_with_components(config, ComponentSet::all())
        .with_profile("selected-b", DoctorConfigSource::Profile)
}

#[tokio::test]
async fn binding_uses_only_the_selected_config() {
    let finding = BindingCheck.detect(&context(crate::memory::engine::CORTEXDB_ENGINE)).await;
    assert_eq!(finding[0].check_id, "memory/binding");
    assert_eq!(finding[0].severity, FindingSeverity::Pass);
}

#[tokio::test]
async fn binding_reports_disabled_memory_without_resolving_an_engine() {
    let finding = BindingCheck.detect(&context(DISABLED_ENGINE)).await;
    assert_eq!(finding[0].severity, FindingSeverity::Skipped);
}

#[tokio::test]
async fn health_reports_a_safe_engine_down_reason() {
    let ctx = context(crate::memory::engine::CORTEXDB_ENGINE).with_memory_engine(
        MemoryEngineCheck {
            engine: Some(crate::memory::engine::CORTEXDB_ENGINE.to_owned()),
            status: "down".to_owned(),
            reason: Some("engine is down".to_owned()),
        },
    );
    let finding = HealthCheck.detect(&ctx).await;
    assert_eq!(finding[0].check_id, "memory/health");
    assert_eq!(finding[0].severity, FindingSeverity::Error);
    assert_eq!(finding[0].message, "engine is down");
}

#[tokio::test]
async fn health_redacts_urls_and_credential_like_reasons() {
    let ctx = context(crate::memory::engine::CORTEXDB_ENGINE).with_memory_engine(
        MemoryEngineCheck {
            engine: Some(crate::memory::engine::CORTEXDB_ENGINE.to_owned()),
            status: "down".to_owned(),
            reason: Some("request to https://user:secret@example.test failed".to_owned()),
        },
    );
    let finding = HealthCheck.detect(&ctx).await;
    assert!(!finding[0].message.contains("secret"));
}
