use std::collections::HashMap;

use tempfile::TempDir;

use crate::config::Config;
use crate::doctor::profile::{load_desktop_profile, DesktopProfileError};
use crate::doctor::{
    DoctorCheck, DoctorConfigSource, DoctorCtx, DoctorOptions, DoctorTier, FindingSeverity,
};
use crate::profiles::ProfileLayout;
use crate::security::credentials::{
    session_support::get_session_token, AuthService, APP_SESSION_PROVIDER,
    DEFAULT_AUTH_PROFILE_NAME,
};

fn config_for(layout: &ProfileLayout) -> Config {
    Config {
        config_path: layout.config_path.clone(),
        workspace_dir: layout.workspace_dir.clone(),
        action_dir: layout.sandbox_dir.clone(),
        ..Config::default()
    }
}

#[tokio::test]
async fn explicit_profile_reads_its_own_session_without_switching_active_user() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    tokio::fs::create_dir_all(root).await.unwrap();
    let active_user_path = root.join("active_user.toml");
    tokio::fs::write(&active_user_path, "user_id = \"account-a\"\n")
        .await
        .unwrap();

    let account_a = ProfileLayout::new(root, "account-a");
    tokio::fs::create_dir_all(&account_a.dir).await.unwrap();
    tokio::fs::write(&account_a.config_path, "default_model = \"account-a-model\"\n")
        .await
        .unwrap();
    AuthService::from_config(&config_for(&account_a))
        .store_provider_token(
            APP_SESSION_PROVIDER,
            DEFAULT_AUTH_PROFILE_NAME,
            "account-a-session",
            HashMap::new(),
            true,
        )
        .unwrap();

    let account_b = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&account_b.dir).await.unwrap();
    tokio::fs::write(&account_b.config_path, "default_model = \"account-b-model\"\n")
        .await
        .unwrap();

    let active_user_before = tokio::fs::read(&active_user_path).await.unwrap();
    let selected = load_desktop_profile(root, "account-b").await.unwrap();

    assert_eq!(selected.config.config_path, account_b.config_path);
    assert_eq!(selected.config.workspace_dir, account_b.workspace_dir);
    assert_eq!(selected.layout.version, account_b.version);
    assert_eq!(selected.config.default_model.as_deref(), Some("account-b-model"));
    assert_eq!(get_session_token(&selected.config).unwrap(), None);
    assert_eq!(
        get_session_token(&config_for(&account_a)).unwrap().as_deref(),
        Some("account-a-session")
    );
    assert_eq!(
        tokio::fs::read(&active_user_path).await.unwrap(),
        active_user_before
    );
}

#[tokio::test]
async fn invalid_explicit_profile_returns_one_safe_profile_exists_error() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let layout = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(layout.dir.join(".layout"), "2\n")
        .await
        .unwrap();
    tokio::fs::write(&layout.config_path, "default_model = [\"private-value\"\n")
        .await
        .unwrap();
    let active_config = Config {
        config_path: root.join("users/account-a/config.toml"),
        ..Config::default()
    };

    let outcome = crate::doctor::ops::doctor_report_with_options(
        &active_config,
        DoctorOptions {
            tier: DoctorTier::Connect,
            profile: Some("account-b".into()),
            ..DoctorOptions::default()
        },
    )
    .await
    .unwrap();
    let report = outcome.value;

    assert_eq!(report.profile.as_deref(), Some("account-b"));
    assert_eq!(report.config_source, crate::doctor::DoctorConfigSource::Profile);
    assert_eq!(report.checks_run, ["profile/exists"]);
    assert!(report.checks_skipped.is_empty());
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].check_id, "profile/exists");
    assert_eq!(report.findings[0].severity, crate::doctor::FindingSeverity::Error);
    assert!(!report.findings[0].message.contains("private-value"));
    assert_eq!(report.summary.error, 1);
    assert!(!report.ok);
}

#[tokio::test]
async fn selected_profile_context_pins_id_layout_source_and_components() {
    let temp = TempDir::new().unwrap();
    let layout = ProfileLayout::new(temp.path(), "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(&layout.config_path, "[components]\nmemory = false\n")
        .await
        .unwrap();
    let selected = load_desktop_profile(temp.path(), "account-b").await.unwrap();
    let components = crate::core::runtime::ComponentSet::from_config(&selected.config.components);
    let ctx = DoctorCtx::from_config_with_components(selected.config, components)
        .with_profile("account-b", DoctorConfigSource::Profile)
        .with_profile_metadata(Some(selected.layout.version), Some("--profile".into()))
        .with_profile_status(super::contract::DoctorProfileStatus::Valid);

    assert_eq!(ctx.profile(), Some("account-b"));
    assert_eq!(ctx.config_source(), DoctorConfigSource::Profile);
    assert_eq!(ctx.profile_layout_version(), Some(selected.layout.version));
    assert_eq!(ctx.profile_resolution_source(), Some("--profile"));
    assert!(!ctx.components().memory);

    let check = super::profile::built_in_checks().pop().unwrap();
    let findings = check.detect(&ctx).await;
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, FindingSeverity::Pass);
}

#[tokio::test]
async fn missing_profile_is_reported_without_creating_or_switching_a_profile() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let active_user_path = root.join("active_user.toml");
    tokio::fs::create_dir_all(root).await.unwrap();
    tokio::fs::write(&active_user_path, "user_id = \"account-a\"\n")
        .await
        .unwrap();
    let marker_before = tokio::fs::read(&active_user_path).await.unwrap();

    let result = load_desktop_profile(root, "missing").await;

    assert!(matches!(result, Err(DesktopProfileError::Missing)));
    assert!(!root.join("users/missing").exists());
    assert_eq!(tokio::fs::read(&active_user_path).await.unwrap(), marker_before);
}

#[tokio::test]
async fn unknown_layout_marker_is_rejected_without_repairing_it() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let layout = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(layout.dir.join(".layout"), "99\n")
        .await
        .unwrap();
    tokio::fs::write(&layout.config_path, "default_model = \"account-b-model\"\n")
        .await
        .unwrap();

    let result = load_desktop_profile(root, "account-b").await;

    assert!(matches!(
        result,
        Err(DesktopProfileError::UnknownLayoutMarker)
    ));
    assert_eq!(tokio::fs::read_to_string(layout.dir.join(".layout")).await.unwrap(), "99\n");
}

#[tokio::test]
async fn invalid_profile_config_is_rejected_without_persisting_recovery() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let layout = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(layout.dir.join(".layout"), "2\n")
        .await
        .unwrap();
    let invalid = "default_model = [\"unfinished\"\n";
    tokio::fs::write(&layout.config_path, invalid).await.unwrap();
    tokio::fs::write(layout.config_path.with_extension("toml.bak"), "")
        .await
        .unwrap();

    let result = load_desktop_profile(root, "account-b").await;

    assert!(matches!(result, Err(DesktopProfileError::InvalidConfig)));
    assert_eq!(tokio::fs::read_to_string(&layout.config_path).await.unwrap(), invalid);
    assert_eq!(
        tokio::fs::read_to_string(layout.config_path.with_extension("toml.bak"))
            .await
            .unwrap(),
        ""
    );
}

#[tokio::test]
async fn empty_profile_resolution_does_not_write_a_layout_marker() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let layout = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();

    let _ = load_desktop_profile(root, "account-b").await;

    assert!(!layout.dir.join(".layout").exists());
}
