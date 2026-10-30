//! Isolated configurations for tests of routing and synthetic credentials.

/// Keep fake credentials independent of the operator's profile and native
/// cipher consent. The profile document still encrypts its storage at rest.
/// Native encryption tests opt into their admitted module explicitly.
pub(crate) fn plaintext_fixture_config(directory: &std::path::Path) -> crate::config::Config {
    let mut config = crate::config::Config {
        config_path: directory.join("config.toml"),
        workspace_dir: directory.join("workspace"),
        action_dir: directory.join("workspace"),
        ..Default::default()
    };
    config.secrets.encrypt = false;
    config
}
