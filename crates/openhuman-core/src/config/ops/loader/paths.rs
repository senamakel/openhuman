//! Resolution of the OpenHuman data directories and marker paths.

use std::path::{Path, PathBuf};

use crate::config::Config;

/// Returns the default workspace directory fallback (~/.openhuman/workspace).
pub(crate) fn fallback_workspace_dir() -> PathBuf {
    crate::config::default_root_openhuman_dir()
        .unwrap_or_else(|_| env_scoped_fallback_root_dir())
        .join("workspace")
}

/// Returns the default OpenHuman configuration directory (~/.openhuman).
pub(crate) fn default_openhuman_dir() -> PathBuf {
    crate::config::default_root_openhuman_dir().unwrap_or_else(|_| env_scoped_fallback_root_dir())
}

pub(crate) fn env_scoped_fallback_root_dir() -> PathBuf {
    let suffix = if crate::api::config::is_staging_app_env(
        crate::api::config::app_env_from_env().as_deref(),
    ) {
        "-staging"
    } else {
        ""
    };
    PathBuf::from(format!(".openhuman{suffix}"))
}

/// Returns the path to the active workspace marker file.
pub(crate) fn active_workspace_marker_path(default_openhuman_dir: &Path) -> PathBuf {
    default_openhuman_dir.join("active_workspace.toml")
}

/// Returns the parent directory of the config file.
pub(crate) fn config_openhuman_dir(config: &Config) -> PathBuf {
    config
        .config_path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}
