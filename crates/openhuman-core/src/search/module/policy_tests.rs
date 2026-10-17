use super::*;

use crate::core::runtime::DispatchCtx;

fn file_backed_config(dir: &std::path::Path) -> Config {
    let mut config = Config::default();
    config.config_path = dir.join("config.toml");
    config.workspace_dir = dir.join("workspace");
    config.secrets.encrypt = false;
    config
}

#[tokio::test]
async fn a_host_pinned_config_is_used_as_given_without_reading_config_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = file_backed_config(dir.path());
    config
        .search
        .pin_host_credential(
            "tavily",
            crate::config::SearchHostCredential::new("agent-key"),
        )
        .unwrap();
    // No config.toml exists; a reload would come back without the pin.
    let current = current_config(&DispatchCtx::unbound(), &config)
        .await
        .unwrap();
    assert_eq!(current.search.direct_key("tavily"), Some("agent-key"));
    assert!(current.search.is_host_pinned());
    assert!(!config.config_path.exists(), "nothing was written");
}

#[tokio::test]
async fn an_unpinned_config_is_reloaded_from_config_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut on_disk = file_backed_config(dir.path());
    on_disk.search.tavily.api_key = Some("file-key".into());
    std::fs::create_dir_all(&on_disk.workspace_dir).unwrap();
    std::fs::write(&on_disk.config_path, toml::to_string(&on_disk).unwrap()).unwrap();
    // The caller's snapshot is stale: the file has since gained a key.
    let stale = file_backed_config(dir.path());
    let current = current_config(&DispatchCtx::unbound(), &stale)
        .await
        .unwrap();
    assert_eq!(current.search.direct_key("tavily"), Some("file-key"));
}
