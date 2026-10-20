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
    /// Safe dotted field path from the typed-deserialization error, if any.
    pub(crate) field_path: Option<String>,
    /// TOML syntax line, when the parser reports one.
    pub(crate) line: Option<usize>,
}

impl ConfigParseError {
    pub(crate) fn new(path: PathBuf, message: String) -> Self {
        let field_path = safe_field_path(&message);
        let line = syntax_error_line(&message);
        Self {
            path,
            message,
            field_path,
            line,
        }
    }

    /// Operator-facing summary that excludes parser text and source values.
    pub(crate) fn safe_summary(&self) -> String {
        if let Some(field_path) = &self.field_path {
            format!("The config field `{field_path}` has an invalid value.")
        } else if let Some(line) = self.line {
            format!("The config file has invalid TOML syntax at line {line}.")
        } else {
            "The config file is invalid.".into()
        }
    }
}

fn safe_field_path(message: &str) -> Option<String> {
    let (path, detail) = message.split_once(": ")?;
    if !detail.starts_with("invalid type:")
        && !detail.starts_with("missing field ")
        && !detail.starts_with("unknown variant ")
    {
        return None;
    }
    if path.is_empty()
        || !path
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '[' | ']'))
    {
        return None;
    }
    Some(path.to_owned())
}

fn syntax_error_line(message: &str) -> Option<usize> {
    let rest = message.strip_prefix("TOML parse error at line ")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
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
        field_path: None,
        line: None,
    });
    diagnostics.fallback = fallback;
}
