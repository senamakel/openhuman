use super::*;
use crate::core::runtime::ComponentSet;
use crate::doctor::contract::{DoctorConfigSource, DoctorCtx, FindingSeverity};
use crate::doctor::core::MemoryEngineCheck;
use crate::memory::engine::DISABLED_ENGINE;

fn context(engine: &str) -> DoctorCtx {
    let mut config = crate::config::Config::default();
    config.memory.engine = engine.to_owned();
    DoctorCtx::from_config_with_components(config, ComponentSet::all())
        .with_profile("selected-b", DoctorConfigSource::Profile)
}

#[tokio::test]
async fn binding_uses_only_the_selected_config() {
    let finding = BindingCheck
        .detect(&context(crate::memory::engine::CORTEXDB_ENGINE))
        .await;
    assert_eq!(finding[0].check_id, "memory/binding");
    assert_eq!(finding[0].severity, FindingSeverity::Pass);
}

#[tokio::test]
async fn binding_reports_disabled_memory_without_resolving_an_engine() {
    let finding = BindingCheck.detect(&context(DISABLED_ENGINE)).await;
    assert_eq!(finding[0].severity, FindingSeverity::Skipped);
}

#[test]
fn health_reports_a_safe_engine_down_reason() {
    let finding = findings_for_health(MemoryEngineCheck {
        engine: Some(crate::memory::engine::CORTEXDB_ENGINE.to_owned()),
        status: "down".to_owned(),
        reason: Some("engine is down".to_owned()),
    });
    assert_eq!(finding[0].check_id, "memory/health");
    assert_eq!(finding[0].severity, FindingSeverity::Error);
    assert_eq!(finding[0].message, "engine is down");
}

#[test]
fn health_redacts_urls_and_credential_like_reasons() {
    let finding = findings_for_health(MemoryEngineCheck {
        engine: Some(crate::memory::engine::CORTEXDB_ENGINE.to_owned()),
        status: "down".to_owned(),
        reason: Some("request to https://user:secret@example.test failed".to_owned()),
    });
    assert!(!finding[0].message.contains("secret"));
}

#[test]
fn health_withholds_bare_secrets_and_local_paths() {
    for private_reason in ["sk-live-secret-value", "/Users/private/person/data.db"] {
        let finding = findings_for_health(MemoryEngineCheck {
            engine: Some(crate::memory::engine::CORTEXDB_ENGINE.to_owned()),
            status: "down".to_owned(),
            reason: Some(private_reason.to_owned()),
        });
        assert!(!finding[0].message.contains(private_reason));
        assert!(finding[0].message.contains("details are withheld"));
    }
}

#[test]
fn hosted_memory_binding_requires_a_registered_host_adapter() {
    assert!(!binding_supported(
        crate::memory::engine::TINYHUMANS_ENGINE,
        false
    ));
    assert!(binding_supported(
        crate::memory::engine::TINYHUMANS_ENGINE,
        true
    ));
    assert!(binding_supported(
        crate::memory::engine::CORTEXDB_ENGINE,
        false
    ));
}
