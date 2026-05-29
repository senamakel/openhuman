//! RPC-facing operations for the `task_sources` domain.
//!
//! Each function returns an [`RpcOutcome`] so the controller layer can
//! surface logs alongside the value. Errors are `String` to match the
//! `ControllerFuture` boundary. Business logic stays here; `schemas.rs`
//! only parses params and delegates.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::openhuman::config::Config;
use crate::openhuman::memory_sync::composio::providers::{get_provider, NormalizedTask, ProviderContext};
use crate::rpc::RpcOutcome;

use super::types::{
    FetchReason, FilterSpec, ProviderSlug, SourceTarget, TaskSource, TaskSourcePatch,
};
use super::{filter, pipeline, store};

/// List all configured task sources.
pub async fn list(config: &Config) -> Result<RpcOutcome<Vec<TaskSource>>, String> {
    let sources = store::list_sources(config).map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        sources.clone(),
        format!("task_sources.list returned {} source(s)", sources.len()),
    ))
}

/// Fetch a single source by id.
pub async fn get(config: &Config, id: &str) -> Result<RpcOutcome<TaskSource>, String> {
    let source = store::get_source(config, id).map_err(|e| e.to_string())?;
    Ok(RpcOutcome::new(source, vec![]))
}

/// Create a new source. Missing schedule / target / cap fields fall back
/// to the `[task_sources]` config defaults.
pub async fn add(
    config: &Config,
    provider: ProviderSlug,
    connection_id: Option<String>,
    name: Option<String>,
    filter: FilterSpec,
    interval_secs: Option<u64>,
    target: Option<SourceTarget>,
    max_tasks_per_fetch: Option<u32>,
) -> Result<RpcOutcome<TaskSource>, String> {
    let defaults = &config.task_sources;
    let interval_secs = interval_secs.unwrap_or(defaults.default_interval_secs);
    let max = max_tasks_per_fetch.unwrap_or(defaults.max_tasks_per_fetch);
    let target = target.unwrap_or(if defaults.auto_proactive {
        SourceTarget::AgentTodoProactive
    } else {
        SourceTarget::TodoOnly
    });

    let source = store::add_source(
        config,
        provider,
        connection_id.filter(|s| !s.trim().is_empty()),
        name.filter(|s| !s.trim().is_empty()),
        filter,
        interval_secs,
        target,
        max,
    )
    .map_err(|e| e.to_string())?;

    Ok(RpcOutcome::single_log(
        source.clone(),
        format!(
            "task_sources.add created '{}' for provider '{}'",
            source.id,
            source.provider.as_str()
        ),
    ))
}

/// Apply a partial update to a source.
pub async fn update(
    config: &Config,
    id: &str,
    patch: TaskSourcePatch,
) -> Result<RpcOutcome<TaskSource>, String> {
    let source = store::update_source(config, id, patch).map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        source,
        format!("task_sources.update applied to '{id}'"),
    ))
}

/// Remove a source by id.
pub async fn remove(config: &Config, id: &str) -> Result<RpcOutcome<Value>, String> {
    store::remove_source(config, id).map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        json!({ "id": id, "removed": true }),
        format!("task_sources.remove deleted '{id}'"),
    ))
}

/// Manually fetch one source now (`FetchReason::Manual`).
pub async fn fetch(config: &Config, id: &str) -> Result<RpcOutcome<super::FetchOutcome>, String> {
    let source = store::get_source(config, id).map_err(|e| e.to_string())?;
    let outcome = pipeline::run_source_once(config, &source, FetchReason::Manual).await;
    let log = format!(
        "task_sources.fetch '{id}': fetched {} routed {} dupes {}",
        outcome.fetched, outcome.routed, outcome.skipped_dupe
    );
    Ok(RpcOutcome::single_log(outcome, log))
}

/// Recently ingested tasks for a source (newest first).
pub async fn list_tasks(
    config: &Config,
    id: &str,
    limit: Option<usize>,
) -> Result<RpcOutcome<Vec<NormalizedTask>>, String> {
    let limit = limit.unwrap_or(50);
    let tasks = store::list_ingested(config, id, limit).map_err(|e| e.to_string())?;
    Ok(RpcOutcome::single_log(
        tasks.clone(),
        format!("task_sources.list_tasks '{id}' returned {}", tasks.len()),
    ))
}

/// Dry-run a filter: fetch matching tasks WITHOUT routing or recording
/// anything. Lets the UI validate a filter before saving a source.
pub async fn preview_filter(
    config: &Config,
    provider: ProviderSlug,
    filter_spec: FilterSpec,
    connection_id: Option<String>,
    max: Option<u32>,
) -> Result<RpcOutcome<Vec<NormalizedTask>>, String> {
    if filter_spec.provider() != provider {
        return Err(format!(
            "filter provider '{}' does not match requested provider '{}'",
            filter_spec.provider().as_str(),
            provider.as_str()
        ));
    }
    let provider_impl = get_provider(provider.as_str()).ok_or_else(|| {
        format!("no native provider registered for '{}'", provider.as_str())
    })?;
    let ctx = ProviderContext {
        config: Arc::new(config.clone()),
        toolkit: provider.as_str().to_string(),
        connection_id: connection_id.filter(|s| !s.trim().is_empty()),
    };
    let max = max.unwrap_or(config.task_sources.max_tasks_per_fetch);
    let fetch_filter = filter::to_fetch_filter(&filter_spec, max);
    let tasks = provider_impl
        .fetch_tasks(&ctx, &fetch_filter)
        .await
        .map_err(|e| format!("preview fetch failed: {e}"))?;
    Ok(RpcOutcome::single_log(
        tasks.clone(),
        format!("task_sources.preview_filter returned {}", tasks.len()),
    ))
}

/// Domain status: enabled flag + source counts.
pub async fn status(config: &Config) -> Result<RpcOutcome<Value>, String> {
    let sources = store::list_sources(config).map_err(|e| e.to_string())?;
    let enabled_count = sources.iter().filter(|s| s.enabled).count();
    Ok(RpcOutcome::new(
        json!({
            "enabled": config.task_sources.enabled,
            "defaultIntervalSecs": config.task_sources.default_interval_secs,
            "sourceCount": sources.len(),
            "enabledSourceCount": enabled_count,
        }),
        vec![],
    ))
}
