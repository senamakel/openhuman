//! Resolve operator-supplied master keys from environment variables.

use super::*;

/// Only `NotPresent` means unset. Invalid Unicode must not fall through to the
/// OS keychain, where a new key could orphan the encrypted secrets.
pub(super) fn env_value(
    name: &str,
    raw: Result<String, std::env::VarError>,
) -> Result<Option<String>, String> {
    match raw {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(format!("{name} is set but is not valid Unicode"))
        }
    }
}

/// Resolve an operator-supplied master key from either supported environment
/// source. Empty values fall through to the OS keychain; errors never include
/// secret values or paths. `read_file` keeps the decision testable.
pub(super) fn master_key_from_env(
    inline: Option<&str>,
    file: Option<&str>,
    read_file: impl FnOnce(&Path) -> Result<String, String>,
) -> Result<Option<([u8; KEY_LEN], String)>, String> {
    let inline = inline.map(str::trim).filter(|value| !value.is_empty());
    let file = file.map(str::trim).filter(|value| !value.is_empty());
    match (inline, file) {
        (None, None) => Ok(None),
        (Some(_), Some(_)) => Err(format!(
            "{MASTER_KEY_ENV} and {MASTER_KEY_FILE_ENV} are both set; set exactly one"
        )),
        (Some(hex), None) => parse_master_key_hex(hex)
            .map(|key| Some((key, MASTER_KEY_ENV.to_string())))
            .map_err(|e| format!("{MASTER_KEY_ENV}: {e}")),
        (None, Some(path)) => {
            let contents =
                read_file(Path::new(path)).map_err(|e| format!("{MASTER_KEY_FILE_ENV}: {e}"))?;
            parse_master_key_hex(contents.trim())
                .map(|key| Some((key, MASTER_KEY_FILE_ENV.to_string())))
                .map_err(|e| format!("{MASTER_KEY_FILE_ENV}: {e}"))
        }
    }
}
