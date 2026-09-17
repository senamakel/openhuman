//! Storing and clearing a provider's embedding API key.

use std::collections::HashMap;

use crate::config::Config;
use crate::rpc::RpcOutcome;
use crate::security::credentials::AuthService;

use super::LOG_PREFIX;

/// Stores an API key for a specific embedding provider.
pub async fn set_api_key(
    config: &Config,
    provider_slug: &str,
    api_key: &str,
) -> Result<RpcOutcome<serde_json::Value>, String> {
    if provider_slug.is_empty() {
        return Err("provider slug is required".into());
    }
    if api_key.trim().is_empty() {
        return Err("api_key cannot be empty".into());
    }

    let cred_provider = format!("embeddings:{provider_slug}");
    let auth = AuthService::from_config(config);
    auth.store_provider_token(&cred_provider, "default", api_key, HashMap::new(), true)
        .map_err(|e| format!("failed to store embedding API key: {e}"))?;

    // #5324: supplying a BYO key does NOT change the embedding signature, so
    // `ensure_reembed_backfill` has nothing to enqueue — but it is precisely
    // the action that unblocks jobs parked on `budget_exhausted` /
    // `auth_missing`. Requeue them here or they stay dead until the user
    // separately discovers the "Retry failed" button. A store failure is
    // surfaced (not reported as `0`) so the key-stored response can't imply the
    // parked queue was recovered when it wasn't.
    let requeue_result = crate::memory::ops::maintenance::retry_failed(config).await;
    let requeued_count = *requeue_result.as_ref().unwrap_or(&0);
    let requeue_error = requeue_result.as_ref().err().cloned();
    let requeued_note = match &requeue_error {
        None => requeued_count.to_string(),
        Some(e) => format!("error ({e})"),
    };

    tracing::info!(
        provider = provider_slug,
        requeued = requeued_count,
        requeue_error = requeue_error.as_deref().unwrap_or(""),
        "{LOG_PREFIX} set_api_key stored"
    );

    Ok(RpcOutcome::new(
        serde_json::json!({ "stored": true, "provider": provider_slug, "requeued_failed_jobs": requeued_count, "requeue_error": requeue_error }),
        vec![format!(
            "embedding API key stored for {provider_slug} (requeued_failed={requeued_note})"
        )],
    ))
}

/// Removes the API key for a specific embedding provider.
pub async fn clear_api_key(
    config: &Config,
    provider_slug: &str,
) -> Result<RpcOutcome<serde_json::Value>, String> {
    if provider_slug.is_empty() {
        return Err("provider slug is required".into());
    }

    let cred_provider = format!("embeddings:{provider_slug}");
    let auth = AuthService::from_config(config);
    let removed = auth
        .remove_profile(&cred_provider, "default")
        .map_err(|e| format!("failed to clear embedding API key: {e}"))?;

    tracing::info!(
        provider = provider_slug,
        removed,
        "{LOG_PREFIX} clear_api_key"
    );

    Ok(RpcOutcome::new(
        serde_json::json!({ "cleared": removed, "provider": provider_slug }),
        vec![format!("embedding API key cleared for {provider_slug}")],
    ))
}
