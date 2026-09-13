//! Session teardown (`clear_session`) and read-only session state lookups.

use serde_json::json;

use crate::api::config::effective_backend_api_url;
use crate::api::jwt::get_session_token;
use crate::api::rest::BackendOAuthClient;
use crate::config::{default_root_openhuman_dir, Config};
use crate::rpc::RpcOutcome;
use crate::security::credentials::session_support::build_session_state;
use crate::security::credentials::{AuthService, APP_SESSION_PROVIDER, DEFAULT_AUTH_PROFILE_NAME};

use super::login_services::stop_login_gated_services;

pub async fn clear_session(config: &Config) -> Result<RpcOutcome<serde_json::Value>, String> {
    let mut logs = Vec::new();
    let removed = {
        let _session_mutation_lock = crate::desktop::app_state::CURRENT_USER_SESSION_MUTATION_LOCK
            .lock()
            .await;
        // Flip the scheduler-gate override first so any background worker that
        // is mid-iteration (or wakes up while we tear down) stalls at its next
        // `wait_for_capacity()` call instead of firing requests at a backend
        // we're about to invalidate. Idempotent.
        crate::cron::scheduler_gate::set_signed_out(true);

        // Invalidate before removing the profile so a pending revalidation cannot
        // recreate it after logout has finished the removal.
        crate::desktop::app_state::forget_current_user_caches();

        let auth = AuthService::from_config(config);
        auth.remove_profile(APP_SESSION_PROVIDER, DEFAULT_AUTH_PROFILE_NAME)
            .map_err(|e| e.to_string())?
    };

    // The core process stays alive on logout. Tear down its authenticated
    // Socket.IO transport and the user-pinned workflow bridge so neither can
    // keep serving the signed-out account until a later reconnect.
    if let Some(manager) = crate::platform::socket::global_socket_manager() {
        if let Err(error) = manager.disconnect().await {
            tracing::warn!(%error, "failed to disconnect backend socket on logout");
        }
    }
    crate::platform::socket::medulla::workflows::clear_workflow_bridge();

    // Clear the active user marker so subsequent config loads fall back to the
    // default (unauthenticated) openhuman directory.
    if let Ok(root_dir) = default_root_openhuman_dir() {
        if let Err(e) = crate::config::clear_active_user(&root_dir) {
            tracing::warn!(error = %e, "failed to clear active_user.toml on logout");
        }
    }

    // Stop all login-gated services (voice and local AI) so
    // they don't run as orphan processes after logout, consuming RAM/CPU with
    // no user context to operate against.
    stop_login_gated_services(config).await;

    // The process stays alive after desktop/TUI logout, so every process-global
    // store must follow the now-active pre-login workspace. Without this, a
    // signed-out caller can keep reading the previous account's context until
    // the process restarts.
    match crate::config::load_config_with_timeout().await {
        Ok(signed_out_config) => {
            let workspace = signed_out_config.workspace_dir.clone();
            // No `memory::global::init` twin here either — see the login site.
            // The context rebind below carries the signed-out workspace and its
            // `[subsystems.memory]` block, and the memory binding is keyed on
            // exactly that pair, so it follows without a second call.
            if let Err(error) = crate::core::runtime::context::CoreContext::rebind_default_workspace(
                &workspace,
                signed_out_config.subsystems.memory.clone(),
            ) {
                tracing::warn!(%error, "failed to rebind core context after logout");
            }
            crate::memory::conversations::register_conversation_persistence_subscriber(
                workspace.clone(),
            );
            logs.push(format!(
                "process globals rebound to signed-out workspace {}",
                workspace.display()
            ));
        }
        Err(error) => {
            tracing::warn!(%error, "failed to resolve signed-out workspace after logout");
            logs.push(format!("signed-out workspace rebind warning: {error}"));
        }
    }

    // Drop the Sentry scope user so events surfaced during/after teardown
    // (and before the next login) are no longer attributed to the
    // signed-out account — issue #3135.
    crate::security::credentials::sentry_scope::clear();

    logs.push("session cleared".to_string());
    Ok(RpcOutcome::new(json!({ "removed": removed }), logs))
}

pub async fn auth_get_state(
    config: &Config,
) -> Result<RpcOutcome<super::super::responses::AuthStateResponse>, String> {
    let state = build_session_state(config)?;
    Ok(RpcOutcome::single_log(state, "session state fetched"))
}

pub async fn auth_get_session_token_json(
    config: &Config,
) -> Result<RpcOutcome<serde_json::Value>, String> {
    let token = get_session_token(config)?;
    Ok(RpcOutcome::single_log(
        json!({ "token": token }),
        "session token fetched",
    ))
}

pub async fn auth_get_me(config: &Config) -> Result<RpcOutcome<serde_json::Value>, String> {
    let api_url = effective_backend_api_url(&config.api_url);
    let token = get_session_token(config)?.ok_or_else(|| "session JWT required".to_string())?;
    let client = BackendOAuthClient::new(&api_url).map_err(|e| e.to_string())?;
    let user = client
        .fetch_current_user(&token)
        .await
        // `flatten_authed_error` maps the typed `BackendApiError::Unauthorized`
        // onto the `SESSION_EXPIRED:` sentinel and falls through to `{e:#}` for
        // everything else, so both properties this call site needs are kept:
        //
        // * Non-401s still render the full anyhow context chain, so the
        //   underlying reqwest transport error (timeout / connection reset /
        //   TLS / DNS) reaches `observability::is_transient_message_failure`.
        //   Bare `e.to_string()` renders only the top context layer
        //   ("GET /auth/me") and collapsed every transient transport failure
        //   into Sentry TAURI-RUST-10.
        // * A 401 is recognised by `jsonrpc::is_session_expired_error`, which
        //   skips the Sentry report AND publishes `DomainEvent::SessionExpired`
        //   so `SessionExpiredSubscriber` clears the dead JWT.
        //
        // Until #5232 routed `fetch_current_user` through `authed_json`, a 401
        // here surfaced as `"GET /auth/me failed (401 Unauthorized): …"`, which
        // `is_session_expired_error` matched on its HTTP-verb prefix. The typed
        // error renders as `"backend rejected session token on GET /auth/me"`,
        // which matches neither classifier — so on 0.63.9 every lapsed session
        // reported to Sentry as a code defect (TAURI-RUST-RYD) and, because the
        // stale token was never cleared, re-fired the same 401 on the next
        // revalidation: the forced sign-out loop in #5307.
        .map_err(crate::api::flatten_authed_error)?;

    Ok(RpcOutcome::single_log(user, "current user fetched"))
}
