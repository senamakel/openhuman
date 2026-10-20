use crate::config::Config;

use super::ConfigFileIntegrityCheck;
use crate::doctor::contract::{DoctorCheck, DoctorCtx, FindingSeverity};

async fn load_config(contents: &str) -> (tempfile::TempDir, Config) {
    let directory = tempfile::tempdir().expect("temporary config directory");
    let config_path = directory.path().join("config.toml");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("create workspace");
    std::fs::write(&config_path, contents).expect("write config");
    let config = Config::load_from_config_path(&config_path, &workspace)
        .await
        .expect("load config");
    (directory, config)
}

#[tokio::test]
async fn unknown_keys_suggest_the_current_field_without_exposing_values() {
    let (_directory, config) = load_config("[memroy]\nengine = \"private-value\"\n").await;
    let findings = ConfigFileIntegrityCheck
        .detect(&DoctorCtx::from_config(config))
        .await;

    let finding = findings
        .iter()
        .find(|finding| finding.severity == FindingSeverity::Warn)
        .expect("an ignored config key is a warning");
    assert_eq!(finding.check_id, "config/file-integrity");
    assert!(finding.message.contains("memroy"), "{}", finding.message);
    assert!(finding.message.contains("memory"), "{}", finding.message);
    assert!(!finding.message.contains("private-value"));
}

#[tokio::test]
async fn invalid_primary_config_reports_backup_recovery_without_raw_parse_text() {
    let directory = tempfile::tempdir().expect("temporary config directory");
    let config_path = directory.path().join("config.toml");
    let backup_path = directory.path().join("config.toml.bak");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("create workspace");
    std::fs::write(&config_path, "default_model = \"private-token\n")
        .expect("write invalid config");
    std::fs::write(&backup_path, "default_model = \"backup-model\"\n").expect("write valid backup");
    let config = Config::load_from_config_path(&config_path, &workspace)
        .await
        .expect("load config from backup");

    let findings = ConfigFileIntegrityCheck
        .detect(&DoctorCtx::from_config(config))
        .await;
    let finding = findings
        .iter()
        .find(|finding| finding.severity == FindingSeverity::Error)
        .expect("recovering from a backup is an error finding");
    assert_eq!(finding.check_id, "config/file-integrity");
    assert!(finding.message.contains("backup"), "{}", finding.message);
    assert!(!finding.message.contains("private-token"));
    assert!(
        !finding
            .message
            .contains(&directory.path().display().to_string()),
        "{}",
        finding.message
    );
}

#[tokio::test]
async fn legacy_keys_are_reported_as_info_with_the_replacement_path() {
    let (_directory, config) =
        load_config("api_url = \"https://example.test/v1/chat/completions\"\n").await;
    let findings = ConfigFileIntegrityCheck
        .detect(&DoctorCtx::from_config(config))
        .await;

    let finding = findings
        .iter()
        .find(|finding| finding.severity == FindingSeverity::Info)
        .expect("a migrated legacy key is informational");
    assert_eq!(finding.check_id, "config/file-integrity");
    assert!(finding.message.contains("api_url"), "{}", finding.message);
    assert!(
        finding.message.contains("inference_url"),
        "{}",
        finding.message
    );
}

#[tokio::test]
async fn clean_default_config_has_only_a_pass_finding() {
    let findings = ConfigFileIntegrityCheck
        .detect(&DoctorCtx::from_config(Config::default()))
        .await;

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].check_id, "config/file-integrity");
    assert_eq!(findings[0].severity, FindingSeverity::Pass);
    assert_eq!(findings[0].fix_hint, "No action required.");
}
