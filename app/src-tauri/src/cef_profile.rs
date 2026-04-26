use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CEF_CACHE_PATH_ENV: &str = "OPENHUMAN_CEF_CACHE_PATH";
const ACTIVE_USER_STATE_FILE: &str = "active_user.toml";
const PENDING_PURGE_STATE_FILE: &str = "pending_cef_purge.toml";
const PRE_LOGIN_USER_ID: &str = "local";

#[derive(Debug, Deserialize)]
struct ActiveUserState {
    user_id: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct PendingCefPurgeState {
    #[serde(default)]
    paths: Vec<String>,
}

fn default_root_dir_name() -> &'static str {
    let app_env = std::env::var("OPENHUMAN_APP_ENV")
        .or_else(|_| std::env::var("VITE_OPENHUMAN_APP_ENV"))
        .ok()
        .map(|value| value.trim().to_ascii_lowercase());
    if matches!(app_env.as_deref(), Some("staging")) {
        ".openhuman-staging"
    } else {
        ".openhuman"
    }
}

fn default_root_openhuman_dir() -> Result<PathBuf, String> {
    if let Ok(workspace) = std::env::var("OPENHUMAN_WORKSPACE") {
        let trimmed = workspace.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    let home = directories::UserDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| "Could not find home directory".to_string())?;
    Ok(home.join(default_root_dir_name()))
}

fn read_active_user_id(default_openhuman_dir: &Path) -> Option<String> {
    let path = default_openhuman_dir.join(ACTIVE_USER_STATE_FILE);
    let contents = std::fs::read_to_string(path).ok()?;
    let state: ActiveUserState = toml::from_str(&contents).ok()?;
    let trimmed = state.user_id.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn user_openhuman_dir(default_openhuman_dir: &Path, user_id: &str) -> PathBuf {
    default_openhuman_dir.join("users").join(user_id)
}

fn cache_dir_for_user(default_openhuman_dir: &Path, user_id: &str) -> PathBuf {
    user_openhuman_dir(default_openhuman_dir, user_id).join("cef")
}

fn pending_purge_marker_path(default_openhuman_dir: &Path) -> PathBuf {
    default_openhuman_dir.join(PENDING_PURGE_STATE_FILE)
}

pub fn configured_cache_path_from_env() -> Option<PathBuf> {
    std::env::var(CEF_CACHE_PATH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn load_pending_purge_state(default_openhuman_dir: &Path) -> Result<PendingCefPurgeState, String> {
    let path = pending_purge_marker_path(default_openhuman_dir);
    if !path.exists() {
        return Ok(PendingCefPurgeState::default());
    }

    let raw = std::fs::read_to_string(&path)
        .map_err(|error| format!("read pending CEF purge marker {}: {error}", path.display()))?;
    toml::from_str(&raw)
        .map_err(|error| format!("parse pending CEF purge marker {}: {error}", path.display()))
}

fn save_pending_purge_state(
    default_openhuman_dir: &Path,
    state: &PendingCefPurgeState,
) -> Result<(), String> {
    std::fs::create_dir_all(default_openhuman_dir).map_err(|error| {
        format!(
            "create OpenHuman root dir {}: {error}",
            default_openhuman_dir.display()
        )
    })?;

    let path = pending_purge_marker_path(default_openhuman_dir);
    let raw = toml::to_string_pretty(state)
        .map_err(|error| format!("serialize pending CEF purge marker: {error}"))?;
    std::fs::write(&path, raw)
        .map_err(|error| format!("write pending CEF purge marker {}: {error}", path.display()))
}

pub fn queue_profile_purge_for_user(user_id: Option<&str>) -> Result<PathBuf, String> {
    let default_openhuman_dir = default_root_openhuman_dir()?;
    let user_id = user_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(PRE_LOGIN_USER_ID);
    let purge_path = cache_dir_for_user(&default_openhuman_dir, user_id);

    let mut state = load_pending_purge_state(&default_openhuman_dir)?;
    let mut unique = BTreeSet::new();
    for path in state.paths {
        unique.insert(path);
    }
    unique.insert(purge_path.display().to_string());
    state = PendingCefPurgeState {
        paths: unique.into_iter().collect(),
    };
    save_pending_purge_state(&default_openhuman_dir, &state)?;
    log::info!(
        "[cef-profile] queued purge for user={} path={}",
        user_id,
        purge_path.display()
    );
    Ok(purge_path)
}

pub fn prepare_process_cache_path() -> Result<PathBuf, String> {
    let default_openhuman_dir = default_root_openhuman_dir()?;
    drain_pending_purges(&default_openhuman_dir)?;

    let user_id = read_active_user_id(&default_openhuman_dir)
        .unwrap_or_else(|| PRE_LOGIN_USER_ID.to_string());
    let cache_dir = cache_dir_for_user(&default_openhuman_dir, &user_id);
    std::fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("create CEF cache dir {}: {error}", cache_dir.display()))?;
    std::env::set_var(CEF_CACHE_PATH_ENV, &cache_dir);
    log::info!(
        "[cef-profile] configured CEF cache user={} path={}",
        user_id,
        cache_dir.display()
    );
    Ok(cache_dir)
}

fn drain_pending_purges(default_openhuman_dir: &Path) -> Result<(), String> {
    let marker_path = pending_purge_marker_path(default_openhuman_dir);
    let state = load_pending_purge_state(default_openhuman_dir)?;
    if state.paths.is_empty() {
        if marker_path.exists() {
            let _ = std::fs::remove_file(&marker_path);
        }
        return Ok(());
    }

    for raw_path in &state.paths {
        let target = PathBuf::from(raw_path);
        if !target.exists() {
            log::debug!(
                "[cef-profile] pending purge target already absent path={}",
                target.display()
            );
            continue;
        }
        match std::fs::remove_dir_all(&target) {
            Ok(()) => {
                log::info!(
                    "[cef-profile] purged queued CEF cache path={}",
                    target.display()
                );
            }
            Err(error) => {
                log::warn!(
                    "[cef-profile] failed to purge queued CEF cache path={} error={}",
                    target.display(),
                    error
                );
            }
        }
    }

    if marker_path.exists() {
        std::fs::remove_file(&marker_path).map_err(|error| {
            format!(
                "remove pending CEF purge marker {}: {error}",
                marker_path.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_active_user_id_ignores_empty_values() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(ACTIVE_USER_STATE_FILE), "user_id = \"   \"").unwrap();
        assert_eq!(read_active_user_id(tmp.path()), None);
    }

    #[test]
    fn cache_dir_for_user_nests_under_users_tree() {
        let root = PathBuf::from("/tmp/openhuman");
        assert_eq!(
            cache_dir_for_user(&root, "u-123"),
            PathBuf::from("/tmp/openhuman/users/u-123/cef")
        );
    }
}
