//! Diagnostics collected while reading and migrating `config.toml`.

use std::path::PathBuf;

use super::super::Config;

/// Source used to recover after the configured file failed to parse.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ConfigLoadFallback {
    /// The primary config parsed successfully, or no config existed.
    #[default]
    None,
    /// The primary config was invalid and its `.bak` file parsed successfully.
    Backup,
    /// Neither the primary config nor its backup could be used.
    Defaults,
}

/// The original config parse failure when the loader recovered from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigParseError {
    /// File whose contents failed to parse.
    pub(crate) path: PathBuf,
    /// Parse error including any TOML line or typed-field path.
    ///
    /// The parser's raw message can contain source context. Doctor findings
    /// should report a safe path/line/classification instead of copying this
    /// message verbatim.
    pub(crate) message: String,
}

/// Non-persisted information collected as a config is loaded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LoadDiagnostics {
    /// Dotted paths to keys not represented by the current config schema.
    pub(crate) unknown_keys: Vec<String>,
    /// Parse error that caused the loader to choose a fallback.
    pub(crate) parse_error: Option<ConfigParseError>,
    /// Fallback chosen after a parse failure.
    pub(crate) fallback: ConfigLoadFallback,
    /// Legacy config paths rewritten by an in-memory migration.
    pub(crate) migrated_keys: Vec<String>,
    unknown_keys_logged: bool,
}

impl LoadDiagnostics {
    pub(crate) fn warn_unknown_keys_once(&mut self) {
        if self.unknown_keys_logged || self.unknown_keys.is_empty() {
            return;
        }
        tracing::warn!(
            keys = %self.unknown_keys.join(","),
            "[config:load] unknown config keys ignored"
        );
        self.unknown_keys_logged = true;
    }

    pub(crate) fn record_migrated_key(&mut self, key: impl Into<String>) {
        let key = key.into();
        if !self.migrated_keys.contains(&key) {
            self.migrated_keys.push(key);
        }
    }
}

pub(super) fn record_non_utf8_recovery(
    config: &mut Config,
    path: &std::path::Path,
    fallback: ConfigLoadFallback,
) {
    let diagnostics = config.load_diagnostics_mut();
    if diagnostics.fallback != ConfigLoadFallback::None {
        return;
    }
    diagnostics.parse_error = Some(ConfigParseError {
        path: path.to_path_buf(),
        message: "config file could not be decoded as UTF-8".to_string(),
    });
    diagnostics.fallback = fallback;
}
