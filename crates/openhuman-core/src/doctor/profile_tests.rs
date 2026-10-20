use std::collections::HashMap;

use tempfile::TempDir;

use crate::config::{Config, ProfileLayout};
use crate::doctor::profile::{load_desktop_profile, DesktopProfileError};
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
