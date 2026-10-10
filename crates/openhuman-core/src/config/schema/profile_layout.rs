//! Where one user profile's state lives: the layout the desktop and a SaaS
//! core share.
//!
//! ```text
//! <root>/users/<profile-id>/
//!   profile.toml      SaaS profile metadata (the desktop does not write it)
//!   config.toml       the profile's config_path
//!   workspace/        sessions, memory, threads, cron, cost — internal state
//!   sandbox/          a SaaS profile's action_dir
//! ```
//!
//! On the desktop `<root>` is `~/.openhuman` and the profile id is the
//! signed-in user's id (or `local` before login); the config loader resolves
//! `config.toml` and `workspace/` from here
//! (`load::dirs::resolve_config_dirs_ignoring_env`). A SaaS core roots it at
//! its operator-chosen directory (`profiles::layout`). Both build every path
//! through [`ProfileLayout::new`], so the two cannot drift apart.

use std::path::{Path, PathBuf};

use super::user_openhuman_dir;

/// The profile metadata file, beside `config.toml`.
pub const PROFILE_META_FILE: &str = "profile.toml";
/// The profile's config file.
pub const PROFILE_CONFIG_FILE: &str = "config.toml";
/// The profile's internal-state directory.
pub const PROFILE_WORKSPACE_DIR: &str = "workspace";
/// The profile's sandbox (a SaaS profile's `action_dir`).
pub const PROFILE_SANDBOX_DIR: &str = "sandbox";

/// Resolved paths of one profile under a root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileLayout {
    /// `<root>/users/<id>`.
    pub dir: PathBuf,
    /// `<dir>/profile.toml`.
    pub meta_path: PathBuf,
    /// `<dir>/config.toml`.
    pub config_path: PathBuf,
    /// `<dir>/workspace`.
    pub workspace_dir: PathBuf,
    /// `<dir>/sandbox`.
    pub sandbox_dir: PathBuf,
}

impl ProfileLayout {
    /// The layout of profile `id` under `root`. The id is used as one path
    /// segment; callers pass a validated id (`profiles::ProfileId`, or the
    /// desktop's active user id).
    pub fn new(root: &Path, id: impl AsRef<str>) -> Self {
        Self::at(user_openhuman_dir(root, id.as_ref()))
    }

    /// The layout of the profile whose directory is `dir`.
    pub fn at(dir: PathBuf) -> Self {
        Self {
            meta_path: dir.join(PROFILE_META_FILE),
            config_path: dir.join(PROFILE_CONFIG_FILE),
            workspace_dir: dir.join(PROFILE_WORKSPACE_DIR),
            sandbox_dir: dir.join(PROFILE_SANDBOX_DIR),
            dir,
        }
    }
}

/// The directory under a root that holds every profile.
pub const USERS_DIR: &str = "users";

/// `<root>/users`: the parent of every profile directory. The desktop's
/// [`user_openhuman_dir`] is built on it.
pub fn users_dir(root: &Path) -> PathBuf {
    root.join(USERS_DIR)
}

#[cfg(test)]
#[path = "profile_layout_tests.rs"]
mod tests;
