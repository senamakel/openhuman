//! Call path to the loaded TinySearch module: lazy load, private
//! reconfiguration when the resolved configuration changes, and serialized
//! `ListTools` / `ExecuteTool` calls.

use std::{
    future::Future,
    hash::{Hash, Hasher},
    sync::OnceLock,
};

use tinysearch_bus::{names, ExecuteToolRequest, ExecuteToolResponse, ListToolsResponse};

use super::{module_config, MODULE_ID};
use crate::config::Config;

/// Bus deadline for one `ExecuteTool` call.
///
/// The bus default ([`tinybus::connection::DEFAULT_TIMEOUT`], 30 s) is sized
/// for control calls. `web_answer_tool` waits on a provider to search *and*
/// synthesize an answer, and `web_search_tool` can fall back across providers
/// one after another; both routinely exceeded 30 s in production and came back
/// as a bus timeout although the module was still working. 90 s stays under
/// the harness's own default tool deadline
/// ([`crate::tools::timeout::DEFAULT_TIMEOUT_SECS`], 120 s), so the module's
/// answer, not the harness kill, decides the result.
pub(super) const EXECUTE_TOOL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

fn last_config() -> &'static tokio::sync::Mutex<Option<u64>> {
    static LAST: OnceLock<tokio::sync::Mutex<Option<u64>>> = OnceLock::new();
    LAST.get_or_init(|| tokio::sync::Mutex::new(None))
}

fn fingerprint(value: &serde_json::Value) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.to_string().hash(&mut hasher);
    hasher.finish()
}

async fn proxy(config: &Config) -> Result<tinybus::Proxy, String> {
    crate::modules::ops::ensure_loaded_within(
        config,
        MODULE_ID,
        Some(std::time::Duration::from_secs(8)),
    )
    .await
    .map_err(crate::modules::ops::LoadError::into_message)?;
    let runtime = crate::modules::host::runtime()
        .await
        .map_err(|error| format!("search module bus unavailable: {error}"))?;
    let configuration =
        serde_json::to_value(module_config(config)).map_err(|error| error.to_string())?;
    let current = fingerprint(&configuration);
    // Caller holds the module call lock through the subsequent bus call.
    let mut previous = last_config().lock().await;
    match *previous {
        // The first call after a lazy load: `ensure_loaded_within` initialized
        // the module with `module_config(config)` for this same config, so it
        // already holds this configuration. Reinitializing now would race the
        // module's own initialization.
        None => {
            tracing::debug!(
                "[modules][search] first call; module holds the load-time configuration"
            );
        }
        Some(last) if last == current => {}
        Some(_) => {
            tracing::debug!("[modules][search] configuration changed; reinitializing module");
            reinitialize(runtime, configuration).await?;
        }
    }
    *previous = Some(current);
    runtime
        .proxy(names::INTERFACE, names::OBJECT_PATH)
        .map_err(|error| format!("search module proxy unavailable: {error}"))
}

/// Reinitialize, waiting out a module that is still finishing its previous
/// (re)initialization instead of failing the caller's search.
async fn reinitialize(
    runtime: &crate::modules::host::ModuleRuntime,
    configuration: serde_json::Value,
) -> Result<(), String> {
    const ATTEMPTS: u32 = 40;
    for attempt in 1..=ATTEMPTS {
        match runtime
            .connection()
            .reinitialize_module(MODULE_ID, configuration.clone())
            .await
        {
            Ok(_) => return Ok(()),
            Err(error) if attempt < ATTEMPTS && error.to_string().contains("initializing") => {
                tracing::debug!(
                    attempt,
                    "[modules][search] module still initializing; retrying"
                );
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(error) => {
                return Err(format!(
                    "search module configuration refresh failed: {error}"
                ));
            }
        }
    }
    Err("search module configuration refresh failed: module never became ready".into())
}

pub(super) async fn with_module_lock<T, F, Fut>(operation: F) -> Result<T, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    static CALL_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    let guard = CALL_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let result = operation().await;
    drop(guard);
    result
}

async fn current_config(config: &Config) -> Result<Config, String> {
    crate::config::ops::reload_config_from_paths(&config.config_path, &config.workspace_dir).await
}

/// Refresh a loaded module after a settings or credential change. An unloaded
/// module gets the current configuration during its first initialization.
pub async fn refresh_loaded(config: &Config) -> Result<(), String> {
    if matches!(
        crate::modules::ops::state_of(MODULE_ID),
        crate::modules::types::ModuleState::Ready
    ) {
        let current = current_config(config).await?;
        with_module_lock(|| async {
            let _ = proxy(&current).await?;
            Ok(())
        })
        .await?;
    }
    Ok(())
}

pub async fn list_tools(config: &Config) -> Result<ListToolsResponse, String> {
    let current = current_config(config).await?;
    with_module_lock(|| async {
        proxy(&current)
            .await?
            .call(names::methods::LIST_TOOLS, ())
            .await
            .map_err(|error| format!("search ListTools failed: {error}"))
    })
    .await
}

pub async fn execute_tool(
    config: &Config,
    request: ExecuteToolRequest,
) -> Result<ExecuteToolResponse, String> {
    let current = current_config(config).await?;
    let tool = request.name.clone();
    with_module_lock(|| async {
        let proxy = proxy(&current).await?;
        // Keys travel only in the private module configuration; a call
        // carries the model's arguments, which are not secrets. An ordinary
        // call also works with a developer override, which is never attested.
        call_execute_tool(proxy, request).await.map_err(|error| {
            tracing::debug!(tool = %tool, "[modules][search] ExecuteTool failed");
            format!("search ExecuteTool failed: {error}")
        })
    })
    .await
}

/// `ExecuteTool` through `proxy`, under [`EXECUTE_TOOL_TIMEOUT`] rather than
/// the bus default.
pub(super) async fn call_execute_tool<R: serde::de::DeserializeOwned>(
    proxy: tinybus::Proxy,
    request: ExecuteToolRequest,
) -> tinybus::Result<R> {
    tracing::debug!(
        tool = %request.name,
        timeout_secs = EXECUTE_TOOL_TIMEOUT.as_secs(),
        "[modules][search] ExecuteTool"
    );
    proxy
        .with_timeout(EXECUTE_TOOL_TIMEOUT)
        .call(names::methods::EXECUTE_TOOL, (request,))
        .await
}

#[cfg(test)]
#[path = "proxy_tests.rs"]
mod proxy_tests;
