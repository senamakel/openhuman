//! A profile's backend credential.
//!
//! The gateway hands the core each user's TinyHumans credential — a session
//! JWT or an API key — through the operator plane. It is stored where every
//! backend caller already looks: the auth-profile store beside the profile's
//! `config_path` (`<root>/users/<id>/`). Work running under that profile's
//! context loads the profile's config, so `resolve_backend_credential` finds
//! that user's credential and no other.
//!
//! Unlike `auth.set_credential`, this changes nothing process-wide: no
//! `active_user.toml`, no rebinding of the default context, no global identity
//! or Sentry user, no scheduler gate. The core still never validates the
//! credential.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::security::credentials::session_support::{
    has_backend_credential, SESSION_EXPIRES_AT_META,
};
use crate::security::credentials::{
    api_key, AuthService, APP_SESSION_PROVIDER, DEFAULT_AUTH_PROFILE_NAME,
};

/// The kind of credential the gateway installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserCredentialKind {
    /// A TinyHumans session JWT.
    Session,
    /// A TinyHumans API key.
    ApiKey,
}

/// Store `token` as profile `config`'s credential of `kind`, replacing any
/// credential of the other kind.
///
/// `expires_at` (RFC 3339) lets the core reject an expired session locally
/// instead of sending a doomed request.
pub fn store(
    config: &Config,
    kind: UserCredentialKind,
    token: &str,
    expires_at: Option<&str>,
) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("credential is blank".to_string());
    }
    let _serial = replacing();
    match kind {
        UserCredentialKind::ApiKey => {
            api_key::store_api_key(config, token).map_err(|e| e.to_string())?;
        }
        UserCredentialKind::Session => {
            let mut metadata = HashMap::new();
            if let Some(exp) = expires_at {
                let exp = chrono::DateTime::parse_from_rfc3339(exp)
                    .map_err(|e| format!("expires_at is not RFC 3339: {e}"))?;
                metadata.insert(SESSION_EXPIRES_AT_META.to_string(), exp.to_rfc3339());
            }
            AuthService::from_config(config)
                .store_provider_token(
                    APP_SESSION_PROVIDER,
                    DEFAULT_AUTH_PROFILE_NAME,
                    token,
                    metadata,
                    true,
                )
                .map_err(|e| e.to_string())?;
        }
    }
    // Then drop the other kind, which would otherwise keep winning (an API
    // key is resolved before a session). The new credential is written first,
    // so a failure here never leaves the profile with none; it is reported so
    // the gateway can retry.
    let replaced = match kind {
        UserCredentialKind::Session => remove_provider(config, api_key::API_KEY_PROVIDER),
        UserCredentialKind::ApiKey => remove_provider(config, APP_SESSION_PROVIDER),
    };
    if let Err(error) = replaced {
        log::warn!(
            "[profiles][credentials] stored {kind:?} but could not remove the other kind: {error}"
        );
        return Err(format!(
            "stored the new credential but could not remove the previous one: {error}"
        ));
    }
    log::debug!(
        "[profiles][credentials] stored {kind:?} credential in {}",
        config
            .config_path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    );
    Ok(())
}

/// Remove every credential profile `config` holds, under any profile name.
/// Returns whether there was one.
pub fn clear(config: &Config) -> Result<bool, String> {
    let _serial = replacing();
    let had_key = remove_provider(config, api_key::API_KEY_PROVIDER)?;
    let had_session = remove_provider(config, APP_SESSION_PROVIDER)?;
    Ok(had_key || had_session)
}

/// Serialises credential changes, so two replacements of different kinds
/// cannot interleave their write and their removal and leave the profile with
/// neither. Operator credential changes are rare; one process-wide lock is
/// enough.
fn replacing() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Remove every profile of `provider` in profile `config`'s store, not just the
/// default one: the resolver would pick up any active profile left behind.
fn remove_provider(config: &Config, provider: &str) -> Result<bool, String> {
    let auth = AuthService::from_config(config);
    let provider_id =
        crate::security::credentials::normalize_provider(provider).map_err(|e| e.to_string())?;
    let names: Vec<String> = auth
        .load_profiles()
        .map_err(|e| e.to_string())?
        .profiles
        .values()
        .filter(|profile| profile.provider == provider_id)
        .map(|profile| profile.profile_name.clone())
        .collect();
    let mut removed = false;
    for name in names {
        removed |= auth
            .remove_profile(provider, &name)
            .map_err(|e| e.to_string())?;
    }
    // The default profile under its legacy key, if any.
    removed |= auth
        .remove_profile(provider, DEFAULT_AUTH_PROFILE_NAME)
        .map_err(|e| e.to_string())?;
    Ok(removed)
}

/// Whether profile `config` holds a credential.
pub fn has(config: &Config) -> bool {
    has_backend_credential(config)
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod tests;
