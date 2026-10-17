//! Request-time configuration and profile enforcement for TinySearch.

use tinysearch_bus::ExecuteToolRequest;

use crate::{
    config::Config,
    core::runtime::DispatchCtx,
};

/// The config a search call runs under.
///
/// A config carrying host-pinned search credentials is authoritative: it is
/// one agent's in-memory config, its keys exist nowhere else, and a reload of
/// the shared `config_path` would drop them. Any other config is reloaded so a
/// settings or login change is honoured on the next call.
pub(crate) async fn current_config(
    ctx: &DispatchCtx,
    config: &Config,
) -> Result<Config, String> {
    if config.profile_providers.is_some() {
        let context = ctx
            .core()
            .ok_or_else(|| "profile search requires its authenticated context".to_owned())?;
        let current = context
            .embedder_config()
            .filter(|current| current.config_path == config.config_path)
            .filter(|_| context.profile().is_some())
            .ok_or_else(|| "profile search configuration belongs to another context".to_owned())?;
        return Ok(current.clone());
    }
    if config.search.is_host_pinned() {
        tracing::debug!("[search][module] host-pinned search credentials; using the agent's config");
        return Ok(config.clone());
    }
    crate::config::ops::reload_config_from_paths(&config.config_path, &config.workspace_dir).await
}

/// Keep only tools admitted by the caller's profile after `ListTools` returns.
pub(crate) fn filter_profile_tools(
    ctx: &DispatchCtx,
    config: &Config,
    tools: &mut Vec<tinysearch_bus::ToolSpec>,
) {
    let Some(binding) = &config.profile_providers else {
        return;
    };
    binding.present(tools);
    let capabilities = crate::profiles::capabilities::ProfileCapabilities::in_ctx(ctx);
    tools.retain(|tool| {
        capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.admits(ctx, &tool.name))
    });
}

/// Enforce the profile's advertised tool, route, role and argument boundary.
pub(crate) fn canonical_request(
    ctx: &DispatchCtx,
    config: &Config,
    mut request: ExecuteToolRequest,
) -> Result<ExecuteToolRequest, String> {
    let Some(binding) = &config.profile_providers else {
        return Ok(request);
    };
    if !crate::profiles::capabilities::ProfileCapabilities::in_ctx(ctx)
        .is_some_and(|capabilities| capabilities.admits(ctx, &request.name))
    {
        return Err("search tool is outside the profile capability boundary".into());
    }
    let canonical = binding
        .canonical_role_name(&request.name)
        .ok_or_else(|| "search tool was not advertised by this profile".to_owned())?;
    if !super::configured_tool_specs(config)
        .iter()
        .any(|tool| tool.name == request.name)
    {
        return Err("search tool is unavailable in this profile".into());
    }
    let role = tinysearch_bus::role_for_tool(canonical)
        .ok_or_else(|| "search alias has no canonical role".to_owned())?;
    let module = super::module_config(config);
    if let Some(provider) = request
        .arguments
        .get("provider")
        .and_then(serde_json::Value::as_str)
    {
        if !module
            .presentation
            .roles
            .get(&role)
            .is_some_and(|order| order.iter().any(|allowed| allowed == provider))
        {
            return Err("search provider is outside the operator role policy".into());
        }
    }
    if request
        .arguments
        .get("depth")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|depth| depth != "standard")
        || [
            "api_key",
            "credential",
            "endpoint",
            "base_url",
            "route",
            "provider_url",
            "backend",
        ]
        .into_iter()
        .any(|key| request.arguments.get(key).is_some())
    {
        return Err("search arguments cannot override operator routes or capabilities".into());
    }
    request.name = canonical.to_owned();
    Ok(request)
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
