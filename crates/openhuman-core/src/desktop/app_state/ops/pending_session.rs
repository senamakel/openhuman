//! Pending-session revalidation: activating (or rejecting) an app session
//! whose backend user id was not yet confirmed, moving it into its own
//! user directory, and rebinding the login-gated services and memory
//! context that go with it.

use super::current_user_generation::{
    current_user_generation, forget_current_user_caches, CURRENT_USER_SESSION_MUTATION_LOCK,
};
use super::LOG_PREFIX;
use crate::api::rest::user_id_from_profile_payload;
use crate::config::Config;
use crate::security::credentials::{AuthService, APP_SESSION_PROVIDER, DEFAULT_AUTH_PROFILE_NAME};
use log::{debug, warn};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// Marks a stored `current_user` payload as not yet confirmed against the
/// backend — set on a locally-created session, cleared once `auth_get_me`
/// confirms the user id.
pub(super) const PENDING_BACKEND_VALIDATION_FIELD: &str = "pendingBackendValidation";

pub(super) fn snapshot_user_pending_backend_validation(user: Option<&Value>) -> bool {
    user.and_then(Value::as_object)
        .and_then(|obj| obj.get(PENDING_BACKEND_VALIDATION_FIELD))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(super) fn clear_pending_backend_validation_flag(mut user: Value) -> Value {
    if let Value::Object(ref mut map) = user {
        map.remove(PENDING_BACKEND_VALIDATION_FIELD);
    }
    user
}

pub(super) fn pending_session_user_id_for_cleanup(
    stored_user: Option<&Value>,
    metadata: &BTreeMap<String, String>,
) -> Option<String> {
    stored_user
        .and_then(user_id_from_profile_payload)
        .or_else(|| {
            metadata
                .get("user_id")
                .map(String::as_str)
                .map(str::trim)
                .filter(|user_id| !user_id.is_empty())
                .map(str::to_string)
        })
}

pub(super) fn config_state_dir(config: &Config) -> Option<PathBuf> {
    config.config_path.parent().map(Path::to_path_buf)
}

pub(super) fn same_config_state_dir(a: &Config, b: &Config) -> bool {
    config_state_dir(a) == config_state_dir(b)
}

pub(super) fn config_dir_for_workspace_env() -> Option<PathBuf> {
    let workspace = std::env::var_os("OPENHUMAN_WORKSPACE")?;
    if workspace.as_os_str().is_empty() {
        return None;
    }

    // Resolve through the SAME workspace→config-dir mapping `config::load` uses
    // (`resolve_config_dir_for_workspace`), not a private reimplementation.
    // A copy here drifts from the loader: it independently doubled
    // `~/.openhuman/workspace` into `~/.openhuman/.openhuman`, so
    // `config_is_workspace_env_scoped` compared that against the loader's real
    // `~/.openhuman` and returned false, mis-scoping credentials on session
    // revalidation (#6079). Delegating keeps the two in lockstep, including the
    // modern-layout recognition that fixes the doubling.
    let workspace_dir = PathBuf::from(workspace);
    let (config_dir, _workspace_dir) =
        crate::config::resolve_config_dir_for_workspace(&workspace_dir);
    Some(config_dir)
}

pub(super) fn config_is_workspace_env_scoped(config: &Config) -> bool {
    let Some(config_dir) = config_state_dir(config) else {
        return false;
    };
    config_dir_for_workspace_env()
        .as_deref()
        .is_some_and(|env_config_dir| env_config_dir == config_dir)
}

pub(super) async fn activate_revalidated_user_dir(user_id: &str) -> Result<Config, String> {
    let root_dir = crate::config::default_root_openhuman_dir()
        .map_err(|error| format!("failed to locate default root: {error}"))?;
    let previous_active = crate::config::read_active_user_id(&root_dir);
    let user_dir = crate::config::user_openhuman_dir(&root_dir, user_id);
    fs::create_dir_all(&user_dir).map_err(|error| {
        format!("failed to create user directory for revalidated pending session user_id={user_id}: {error}")
    })?;
    crate::config::write_active_user_id(&root_dir, user_id).map_err(|error| {
        format!("failed to write active_user.toml for revalidated pending session user_id={user_id}: {error}")
    })?;

    debug!(
        "{LOG_PREFIX} activated user directory for revalidated pending session user_id={user_id}"
    );
    if previous_active.is_none() {
        let pre_ws = crate::config::pre_login_user_dir(&root_dir).join("workspace");
        if let Err(error) = crate::memory::conversations::purge_threads(pre_ws) {
            debug!(
                "{LOG_PREFIX} pre-login conversation purge skipped after pending session revalidation: {error}"
            );
        }
    }

    let config = Config::load_from_default_paths().await.map_err(|error| {
        format!("failed to reload config after pending session user activation: {error}")
    })?;

    // The marker write above cleared the cached active workspace, and
    // `load_from_default_paths` deliberately bypasses the runtime resolver, so
    // nothing has refilled it. Resolve once through the authoritative path,
    // which republishes the cache and announces the switch — otherwise a
    // connected Event Log or notification client keeps the previous
    // workspace's handle until some unrelated later config load happens
    // (#5966). Publishing from `load_from_default_paths` itself would be
    // wrong: it ignores `OPENHUMAN_WORKSPACE`, so under that override it does
    // not know the runtime answer.
    if let Err(error) = crate::config::active_workspace_dir().await {
        warn!(
            "{LOG_PREFIX} could not refresh the active workspace after pending session activation: {error}"
        );
    }

    Ok(config)
}

pub(super) async fn finish_revalidated_user_activation(
    target_config: &Config,
    user_id: &str,
    service_rebind_source: Option<&Config>,
    generation: u64,
) {
    if let Err(error) = crate::cron::seed::prune_retired_jobs(target_config) {
        warn!("{LOG_PREFIX} failed to prune retired cron jobs after pending session revalidation: {error}");
    }

    // ── No explicit memory re-point here any more (#5560) ──────────────────
    //
    // This was `tinymemory_core::global::init(...)`, the in-process engine's
    // process-global slot, which had to be re-pointed by hand at every
    // activation site or it kept writing into the previous workspace.
    // `memory::binding` is keyed on (workspace, `[subsystems.memory]`), and the
    // context rebind immediately below re-points both — so memory follows it by
    // construction. `CoreContext::memory_binding`'s docs state this property as
    // the reason these sites need no memory call of their own.
    if let Err(error) = crate::core::runtime::context::CoreContext::rebind_default_workspace(
        &target_config.workspace_dir,
        target_config.subsystems.memory.clone(),
    ) {
        warn!("{LOG_PREFIX} failed to rebind core context after pending session revalidation: {error}");
    }
    // No people-store rebind: people is served by the bound memory driver, and
    // the core-context rebind above already moved that binding to the activated
    // user's workspace.
    crate::memory::conversations::register_conversation_persistence_subscriber(
        target_config.workspace_dir.clone(),
    );
    if current_user_generation() != generation {
        debug!("{LOG_PREFIX} skipping stale activation after pending session revalidation");
        return;
    }
    if let Some(source_config) = service_rebind_source {
        if current_user_generation() != generation {
            debug!(
                "{LOG_PREFIX} skipping stale login-gated service activation after pending session revalidation"
            );
            return;
        }
        crate::security::credentials::stop_login_gated_services(source_config).await;
        if current_user_generation() != generation {
            debug!(
                "{LOG_PREFIX} skipping stale login-gated service restart after pending session revalidation"
            );
            return;
        }
        crate::security::credentials::start_login_gated_services(target_config).await;
    } else {
        debug!(
            "{LOG_PREFIX} pending session revalidation left login-gated services running without restart"
        );
    }
    crate::cron::scheduler_gate::set_signed_out(false);
    crate::security::credentials::sentry_scope::bind(user_id);
}

pub(super) async fn remove_revalidated_source_profile(config: &Config) -> Result<(), String> {
    let config = config.clone();
    tokio::task::spawn_blocking(move || {
        AuthService::from_config(&config)
            .remove_profile(APP_SESSION_PROVIDER, DEFAULT_AUTH_PROFILE_NAME)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .unwrap_or_else(|e| {
        Err(format!(
            "{LOG_PREFIX} revalidated source profile remove task panicked: {e}"
        ))
    })
}

pub(super) async fn persist_revalidated_session_user(
    config: &Config,
    token: &str,
    base_metadata: BTreeMap<String, String>,
    user: Value,
    generation: u64,
) -> Result<Box<Config>, String> {
    let (target_config, user_id, source_config, source_moved) = {
        let _session_mutation_lock = CURRENT_USER_SESSION_MUTATION_LOCK.lock().await;
        if current_user_generation() != generation {
            return Err("pending session persistence became stale after sign-out".to_string());
        }
        let user_id = user_id_from_profile_payload(&user).ok_or_else(|| {
            "backend user id required before clearing pending validation".to_string()
        })?;
        let workspace_env_scoped = config_is_workspace_env_scoped(config);
        let target_config = if !workspace_env_scoped {
            activate_revalidated_user_dir(&user_id).await?
        } else {
            debug!(
                "{LOG_PREFIX} keeping revalidated pending session in OPENHUMAN_WORKSPACE-scoped config"
            );
            config.clone()
        };
        let source_config = config.clone();
        let source_moved = !same_config_state_dir(config, &target_config);
        let token = token.to_string();
        let mut metadata: HashMap<String, String> = base_metadata.into_iter().collect();
        metadata.insert("user_id".to_string(), user_id.clone());
        metadata.insert("user_json".to_string(), user.to_string());

        let config_for_store = target_config.clone();
        tokio::task::spawn_blocking(move || {
            AuthService::from_config(&config_for_store)
                .store_provider_token(
                    APP_SESSION_PROVIDER,
                    DEFAULT_AUTH_PROFILE_NAME,
                    &token,
                    metadata,
                    true,
                )
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .await
        .unwrap_or_else(|e| {
            Err(format!(
                "{LOG_PREFIX} revalidated session persist task panicked: {e}"
            ))
        })?;

        if source_moved {
            if let Err(error) = remove_revalidated_source_profile(&source_config).await {
                warn!(
                    "{LOG_PREFIX} failed to remove source pending session profile after user activation: {error}"
                );
            }
        }

        (target_config, user_id, source_config, source_moved)
    };

    finish_revalidated_user_activation(
        &target_config,
        &user_id,
        source_moved.then_some(&source_config),
        generation,
    )
    .await;

    Ok(Box::new(target_config))
}

pub(super) async fn clear_deferred_session_after_backend_rejection(
    config: &Config,
    pending_user_id: Option<&str>,
) -> Result<(), String> {
    let workspace_env_scoped = config_is_workspace_env_scoped(config);
    let config_for_remove = config.clone();
    let clear_result = tokio::task::spawn_blocking(move || {
        AuthService::from_config(&config_for_remove)
            .remove_profile(APP_SESSION_PROVIDER, DEFAULT_AUTH_PROFILE_NAME)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .unwrap_or_else(|e| {
        Err(format!(
            "{LOG_PREFIX} deferred session clear task panicked: {e}"
        ))
    });

    forget_current_user_caches();
    crate::cron::scheduler_gate::set_signed_out(true);

    match crate::config::default_root_openhuman_dir() {
        Ok(root_dir) => {
            let active_user = crate::config::read_active_user_id(&root_dir);
            let should_clear_active_user = if workspace_env_scoped {
                pending_user_id.is_some_and(|pending| active_user.as_deref() == Some(pending))
            } else {
                true
            };
            if should_clear_active_user {
                if let Err(error) = crate::config::clear_active_user(&root_dir) {
                    warn!(
                        "{LOG_PREFIX} failed to clear active_user.toml for rejected pending session: {error}"
                    );
                }
            } else {
                debug!(
                    "{LOG_PREFIX} preserving default active_user.toml for rejected OPENHUMAN_WORKSPACE-scoped pending session"
                );
            }
        }
        Err(error) if !workspace_env_scoped => {
            warn!(
                "{LOG_PREFIX} failed to locate default root while clearing rejected pending session: {error}"
            );
        }
        Err(_) => {}
    }
    crate::security::credentials::stop_login_gated_services(config).await;
    crate::security::credentials::sentry_scope::clear();

    clear_result
}
