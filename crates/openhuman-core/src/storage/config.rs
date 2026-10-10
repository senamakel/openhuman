//! Which storage the process uses: the resolution rule behind
//! `OPENHUMAN_STORAGE_URL`, `[storage] url` and the default.
//!
//! In order:
//!
//! 1. `OPENHUMAN_STORAGE_URL`;
//! 2. `[storage] url` in `config.toml`;
//! 3. the default, `sqlite:<workspace_dir>`.
//!
//! The first two name a backend the host opens once at startup and installs
//! ([`super::install`]); every domain that has moved onto the ports then uses
//! it, and the session store follows (`DriverSessionStores`). That is
//! [`StorageMode::Url`].
//!
//! The default is [`StorageMode::Default`]: no backend is installed and
//! nothing about the large stores changes (`sessions.db`, `flows.db`,
//! `jobs.db` and `graph_checkpoints.db` are already SQLite files served as
//! they are). The small stores (approvals, devices, notifications, task
//! sources) keep their rows as document tables inside their own `.db` files,
//! `<workspace>/<domain>/<name>.db`, importing the rows of their old tables
//! once ([`super::local`]). The workspace directory is therefore the default's
//! root, but each database is placed at its established path instead of
//! `<workspace>/<name>.db`, so an existing install keeps one file per store.
//!
//! The value `classic` (alias `legacy`) opts out: no backend, the pure
//! pre-storage layout with the legacy tables. It is the escape hatch for a
//! process that must not import its small-store files. The tables of a
//! database that was already imported are kept as `_legacy_<name>` for one
//! release so their rows can be recovered by hand; an older build looks for
//! the original names, so it starts from empty tables, not from those rows.

use std::path::Path;

use crate::config::Config;

/// The `OPENHUMAN_STORAGE_URL` / `[storage] url` value that opts out of the
/// default and keeps the pure legacy layout.
pub const CLASSIC: &str = "classic";

/// The value spellings that mean [`CLASSIC`].
const CLASSIC_ALIASES: [&str; 2] = [CLASSIC, "legacy"];

/// What the process's configuration asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageMode {
    /// A backend named by `OPENHUMAN_STORAGE_URL` or `[storage] url`.
    Url(String),
    /// Nothing configured: SQLite document tables inside the small stores'
    /// own database files, everything else as it was.
    Default,
    /// Opted out ([`CLASSIC`]): the pure legacy layout.
    Classic,
}

/// The mode `env` and `config` ask for. Blank values count as unset, a blank
/// override falls through to the file, and [`CLASSIC`] in either is the
/// opt-out.
pub fn mode_from(env: Option<String>, config: &Config) -> StorageMode {
    let chosen = env
        .into_iter()
        .chain(config.storage.url.clone())
        .map(|url| url.trim().to_string())
        .find(|url| !url.is_empty());
    match chosen {
        None => StorageMode::Default,
        Some(url) if CLASSIC_ALIASES.iter().any(|a| url.eq_ignore_ascii_case(a)) => {
            StorageMode::Classic
        }
        Some(url) => StorageMode::Url(url),
    }
}

/// [`mode_from`] with the environment read, and a SaaS process (which has no
/// workspace-local files to default to, and whose storage is always an
/// explicit backend) never in [`StorageMode::Default`].
pub fn mode(config: &Config) -> StorageMode {
    let mode = mode_from(std::env::var(super::STORAGE_URL_VAR).ok(), config);
    if mode == StorageMode::Default && crate::core::runtime::mode::is_saas() {
        return StorageMode::Classic;
    }
    mode
}

/// The default URL for `workspace_dir`: SQLite in directory mode, one file
/// per named database.
pub fn default_url(workspace_dir: &Path) -> String {
    format!("sqlite:{}", workspace_dir.display())
}

/// The URL in effect for `mode`: the configured one, the default under
/// `workspace_dir`, or none for the legacy layout.
pub fn effective_url(mode: &StorageMode, workspace_dir: &Path) -> Option<String> {
    match mode {
        StorageMode::Url(url) => Some(url.clone()),
        StorageMode::Default => Some(default_url(workspace_dir)),
        StorageMode::Classic => None,
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
