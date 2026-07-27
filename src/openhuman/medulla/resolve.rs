//! Resolve a configured [`MedullaClient`] from ambient config and credentials.
//!
//! # Why there is no `[medulla]` config section yet
//!
//! The Medulla backend base URL is read from `OPENHUMAN_MEDULLA_BASE_URL`,
//! falling back to the OpenHuman `api_url`. That fallback is a **provisional
//! assumption, not a verified fact**: `api_url` addresses the TinyHumans
//! backend, and whether the Medulla orchestration API is the same deployment
//! has not been confirmed. A dedicated `[medulla]` config section belongs with
//! the auth migration, where that question gets answered properly; declaring
//! one now would bake the assumption into a user-facing schema.
//!
//! The bearer is the existing OpenHuman session token, for the same reason —
//! one credential store, resolved through `credentials::session_support`,
//! rather than a second one that could drift out of sync.

use crate::openhuman::config::Config;
use crate::openhuman::credentials::session_support::get_session_token;

use super::client::MedullaClient;

/// Environment override for the Medulla backend base URL.
pub const MEDULLA_BASE_URL_ENV: &str = "OPENHUMAN_MEDULLA_BASE_URL";

/// Why a Medulla client could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotConfigured {
    /// Neither the env override nor `api_url` yielded a base URL.
    NoBaseUrl,
    /// No session token is available — the user is signed out.
    NoSessionToken,
}

impl NotConfigured {
    /// A message safe to surface to an operator.
    ///
    /// Deliberately free of URLs and tokens: this text reaches logs and the RPC
    /// error channel.
    pub fn message(&self) -> &'static str {
        match self {
            NotConfigured::NoBaseUrl => {
                "no Medulla backend configured; set OPENHUMAN_MEDULLA_BASE_URL or api_url"
            }
            NotConfigured::NoSessionToken => "not signed in; no session token available",
        }
    }

    /// Stable discriminator for the structured RPC error `data.kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            NotConfigured::NoBaseUrl => "MedullaNoBaseUrl",
            NotConfigured::NoSessionToken => "MedullaNoSessionToken",
        }
    }
}

/// The configured base URL, if any.
///
/// Precedence: `OPENHUMAN_MEDULLA_BASE_URL`, then `config.api_url`. Empty or
/// whitespace-only values count as unset, so an exported-but-blank env var does
/// not shadow a working config value.
pub fn base_url(config: &Config) -> Option<String> {
    let from_env = std::env::var(MEDULLA_BASE_URL_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty());
    from_env
        .or_else(|| config.api_url.clone())
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
}

/// Build a client from ambient config + credentials.
///
/// Returns [`NotConfigured`] rather than an opaque error so callers can tell
/// "signed out" (an expected user state the host should render as a notice)
/// from "misconfigured".
pub fn client(config: &Config) -> Result<MedullaClient, NotConfigured> {
    let Some(base) = base_url(config) else {
        log::debug!("[medulla] resolve_client outcome=no_base_url");
        return Err(NotConfigured::NoBaseUrl);
    };

    let token = get_session_token(config)
        .ok()
        .flatten()
        .filter(|t| !t.trim().is_empty());
    let Some(token) = token else {
        log::debug!("[medulla] resolve_client outcome=no_session_token");
        return Err(NotConfigured::NoSessionToken);
    };

    log::debug!("[medulla] resolve_client outcome=ok");
    Ok(MedullaClient::new(base, token))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize env mutation: `base_url` reads a process-global.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn config_with_api_url(url: Option<&str>) -> Config {
        let mut config = Config::default();
        config.api_url = url.map(str::to_string);
        config
    }

    #[test]
    fn env_override_wins_over_api_url() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(MEDULLA_BASE_URL_ENV, "https://medulla.example");
        let resolved = base_url(&config_with_api_url(Some("https://api.example")));
        std::env::remove_var(MEDULLA_BASE_URL_ENV);
        assert_eq!(resolved.as_deref(), Some("https://medulla.example"));
    }

    #[test]
    fn falls_back_to_api_url_when_env_unset() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(MEDULLA_BASE_URL_ENV);
        let resolved = base_url(&config_with_api_url(Some("https://api.example/")));
        assert_eq!(resolved.as_deref(), Some("https://api.example"));
    }

    #[test]
    fn blank_env_does_not_shadow_api_url() {
        // An exported-but-empty var is a common shell accident; treating it as
        // "configured" would break an otherwise-working setup.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(MEDULLA_BASE_URL_ENV, "   ");
        let resolved = base_url(&config_with_api_url(Some("https://api.example")));
        std::env::remove_var(MEDULLA_BASE_URL_ENV);
        assert_eq!(resolved.as_deref(), Some("https://api.example"));
    }

    #[test]
    fn none_when_neither_is_set() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(MEDULLA_BASE_URL_ENV);
        assert_eq!(base_url(&config_with_api_url(None)), None);
    }

    #[test]
    fn trailing_slashes_are_trimmed_so_paths_do_not_double_up() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(MEDULLA_BASE_URL_ENV, "https://medulla.example///");
        let resolved = base_url(&config_with_api_url(None));
        std::env::remove_var(MEDULLA_BASE_URL_ENV);
        assert_eq!(resolved.as_deref(), Some("https://medulla.example"));
    }

    #[test]
    fn not_configured_messages_carry_no_secrets() {
        for reason in [NotConfigured::NoBaseUrl, NotConfigured::NoSessionToken] {
            let msg = reason.message();
            assert!(!msg.contains("http"), "message must not leak a URL: {msg}");
            assert!(!reason.kind().is_empty());
        }
    }
}
