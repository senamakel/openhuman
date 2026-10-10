//! The desktop resolver and the shared `ProfileLayout` must name the same
//! bytes: `users/<id>/{config.toml,workspace}` under the root.

use super::*;
use crate::config::schema::ProfileLayout;

#[tokio::test]
async fn an_active_user_resolves_to_its_profile_layout_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_active_user_id(root, "65f1c0ffee0123456789abcd").unwrap();

    let (config_dir, workspace_dir, source) =
        resolve_runtime_config_dirs_with(root, &root.join("workspace"), &MapEnv::default())
            .await
            .unwrap();

    let layout = ProfileLayout::new(root, "65f1c0ffee0123456789abcd");
    assert!(matches!(source, ConfigResolutionSource::ActiveUser));
    assert_eq!(config_dir, layout.dir);
    assert_eq!(workspace_dir, layout.workspace_dir);
    // The literal paths the desktop has always used.
    let user_dir = root.join("users").join("65f1c0ffee0123456789abcd");
    assert_eq!(
        config_dir.as_os_str().as_encoded_bytes(),
        user_dir.as_os_str().as_encoded_bytes()
    );
    assert_eq!(
        workspace_dir.as_os_str().as_encoded_bytes(),
        user_dir.join("workspace").as_os_str().as_encoded_bytes()
    );
    // The loader reads `<config_dir>/config.toml`.
    assert_eq!(config_dir.join("config.toml"), layout.config_path);
}

#[tokio::test]
async fn the_pre_login_profile_resolves_to_its_profile_layout() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let (config_dir, workspace_dir, _) =
        resolve_runtime_config_dirs_with(root, &root.join("workspace"), &MapEnv::default())
            .await
            .unwrap();

    let layout = ProfileLayout::new(root, PRE_LOGIN_USER_ID);
    assert_eq!(config_dir, layout.dir);
    assert_eq!(workspace_dir, layout.workspace_dir);
    assert_eq!(config_dir, root.join("users").join("local"));
    assert_eq!(
        workspace_dir,
        root.join("users").join("local").join("workspace")
    );
}
