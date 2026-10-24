use crate::config::Config;
use crate::core::Outcome;
use serde_json::{json, Value};
use tracing::{debug, warn};

const LOG_PREFIX: &str = "[inference::ops]";

pub async fn inference_provider_auth_errors() -> Result<Outcome<Value>, String> {
    let errors = crate::inference::auth_error_registry::snapshot();
    debug!(count = errors.len(), "{LOG_PREFIX} provider_auth_errors:ok");
    Ok(Outcome::single_log(
        json!({ "errors": errors }),
        "inference provider auth errors fetched",
    ))
}

pub async fn inference_openai_oauth_start(config: &Config) -> Result<Outcome<Value>, String> {
    debug!("{LOG_PREFIX} openai_oauth_start:start");
    let result = crate::security::credentials::openai_oauth::start_openai_oauth(config)
        .await
        .map(|start| {
            Outcome::single_log(
                json!({
                    "authUrl": start.auth_url,
                    "state": start.state,
                    "redirectUri": start.redirect_uri,
                }),
                "openai oauth authorize url ready",
            )
        });
    match &result {
        Ok(_) => debug!("{LOG_PREFIX} openai_oauth_start:ok"),
        Err(err) => warn!(error = %err, "{LOG_PREFIX} openai_oauth_start:error"),
    }
    result
}

pub async fn inference_openai_oauth_complete(
    config: &Config,
    callback_url: &str,
) -> Result<Outcome<Value>, String> {
    debug!(
        callback_len = callback_url.len(),
        "{LOG_PREFIX} openai_oauth_complete:start"
    );
    let result =
        crate::security::credentials::openai_oauth::complete_openai_oauth(config, callback_url)
            .await
            .map(|payload| Outcome::single_log(payload, "openai oauth connected"));
    match &result {
        Ok(_) => debug!("{LOG_PREFIX} openai_oauth_complete:ok"),
        Err(err) => warn!(error = %err, "{LOG_PREFIX} openai_oauth_complete:error"),
    }
    result
}

pub async fn inference_openai_oauth_import_codex_cli(
    config: &Config,
) -> Result<Outcome<Value>, String> {
    debug!("{LOG_PREFIX} openai_oauth_import_codex_cli:start");
    let result =
        crate::security::credentials::openai_oauth::import_openai_oauth_from_codex_cli(config)
            .map(|payload| Outcome::single_log(payload, "openai oauth imported from codex cli"));
    match &result {
        Ok(_) => debug!("{LOG_PREFIX} openai_oauth_import_codex_cli:ok"),
        // Most failures here are expected user-state (no `~/.codex/auth.json`,
        // user never ran `codex login`, stale/empty file) — the UI already
        // surfaces the actionable error, so route through the observability
        // classifier to keep that flood out of Sentry (TAURI-RUST-83A) while a
        // genuine keyring/persist defect still falls through to a real event.
        Err(err) => crate::core::observability::report_error_or_expected(
            err,
            "inference",
            "openai_oauth_import_codex_cli",
            &[],
        ),
    }
    result
}

pub async fn inference_openai_oauth_status(config: &Config) -> Result<Outcome<Value>, String> {
    debug!("{LOG_PREFIX} openai_oauth_status:start");
    let result =
        crate::security::credentials::openai_oauth::openai_oauth_status(config).map(|status| {
            Outcome::single_log(
                json!({
                    "connected": status.connected,
                    "profileId": status.profile_id,
                    "expiresAt": status.expires_at,
                    "authMethod": status.auth_method,
                }),
                "openai oauth status",
            )
        });
    match &result {
        Ok(_) => debug!("{LOG_PREFIX} openai_oauth_status:ok"),
        Err(err) => warn!(error = %err, "{LOG_PREFIX} openai_oauth_status:error"),
    }
    result
}

pub async fn inference_openai_oauth_disconnect(config: &Config) -> Result<Outcome<Value>, String> {
    debug!("{LOG_PREFIX} openai_oauth_disconnect:start");
    let result = crate::security::credentials::openai_oauth::disconnect_openai_oauth(config)
        .await
        .map(|payload| Outcome::single_log(payload, "openai oauth disconnected"));
    match &result {
        Ok(_) => debug!("{LOG_PREFIX} openai_oauth_disconnect:ok"),
        Err(err) => warn!(error = %err, "{LOG_PREFIX} openai_oauth_disconnect:error"),
    }
    result
}
