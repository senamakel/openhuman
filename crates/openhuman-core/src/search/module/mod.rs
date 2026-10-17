//! OpenHuman's host policy and configuration for the TinySearch module.
//!
//! Provider implementations, tool schemas, role dispatch, and configuration
//! execution live in TinySearch. This module translates OpenHuman's resolved
//! search policy into the private TinySearch configuration and applies the
//! host's profile boundary to its tool catalogue and requests. Bus loading and
//! calls remain in [`crate::modules::search`].

use std::collections::BTreeMap;

use tinysearch_bus::{
    BackendAuthMode, BackendConfig, PresentationConfig, ProviderConfig, ProviderRoute,
    SearchConfig as ModuleSearchConfig,
};

use crate::config::Config;
use crate::search::providers::{self, ResolvedProvider};
use tinysearch_bus::ROLES;

pub(crate) mod policy;

/// Private direct-route settings for one provider: key, base URL, limits.
fn direct_settings(config: &Config, provider: &str) -> ProviderConfig {
    let search = &config.search;
    let mut settings = ProviderConfig {
        enabled: true,
        route: ProviderRoute::Direct,
        max_results: Some(search.max_results as u64),
        timeout_secs: Some(search.timeout_secs),
        ..Default::default()
    };
    match provider {
        "seltz" => {
            settings.credential = config
                .seltz
                .api_key
                .clone()
                .filter(|k| !k.trim().is_empty());
            settings.base_url = config.seltz.api_url.clone();
            settings.max_results = Some(config.seltz.max_results as u64);
            settings.timeout_secs = Some(config.seltz.timeout_secs);
        }
        "searxng" => {
            settings.base_url = Some(config.searxng.base_url.clone());
            settings.max_results = Some(config.searxng.max_results as u64);
            settings.timeout_secs = Some(config.searxng.timeout_secs);
            settings.default_language = Some(config.searxng.default_language.clone());
        }
        other => {
            // A key pinned on this agent's config wins over the stored one.
            settings.credential = search.direct_key(other).map(str::to_owned);
            settings.base_url = search
                .host_credential(other)
                .and_then(|pinned| pinned.base_url())
                .or_else(|| {
                    search
                        .credentials(other)
                        .and_then(|c| c.base_url.as_deref())
                })
                .map(str::to_owned);
        }
    }
    settings
}

fn provider_config(config: &Config, provider: &ResolvedProvider) -> ProviderConfig {
    let mut settings = match provider.route {
        ProviderRoute::Backend => ProviderConfig {
            enabled: true,
            route: ProviderRoute::Backend,
            max_results: Some(config.search.max_results as u64),
            timeout_secs: Some(config.search.timeout_secs),
            ..Default::default()
        },
        ProviderRoute::Direct => direct_settings(config, provider.id),
    };
    settings.enabled = provider.usable;
    settings
}

/// The complete private configuration for TinySearch.
pub(crate) fn module_config(config: &Config) -> ModuleSearchConfig {
    let private = config
        .profile_providers
        .as_ref()
        .map(|binding| binding.effective_search(config));
    let config = private.as_ref().unwrap_or(config);
    let direct_profile = config.profile_providers.is_some();
    let credential = if direct_profile {
        None
    } else {
        crate::security::credentials::session_support::resolve_backend_credential(config).ok()
    };
    let resolved = providers::resolve_with(config, credential.is_some());
    let backend = if direct_profile {
        BackendConfig::default()
    } else {
        BackendConfig {
            base_url: crate::backend::base_url(&config.api_url).ok(),
            auth_mode: if credential.as_ref().is_some_and(|c| c.is_api_key()) {
                BackendAuthMode::ApiKey
            } else {
                BackendAuthMode::Session
            },
            credential: credential.map(|c| c.into_secret()),
            sdk_name: crate::backend::product_identity(),
        }
    };

    let mut module_providers: BTreeMap<String, ProviderConfig> = resolved
        .iter()
        .filter(|provider| provider.enabled)
        .map(|provider| (provider.id.to_string(), provider_config(config, provider)))
        .collect();
    // Deep research runs on the direct Gemini API and needs the user's key,
    // even when grounded Gemini answers go through the managed route.
    if !direct_profile
        && module_providers.contains_key("gemini")
        && !module_providers.contains_key("gemini_deep_research")
    {
        let mut deep = direct_settings(config, "gemini_deep_research");
        deep.enabled = crate::search::is_active(config) && deep.credential.is_some();
        module_providers.insert("gemini_deep_research".into(), deep);
    }

    let roles = ROLES
        .into_iter()
        .map(|role| (role, providers::role_order(config, role)))
        .collect();
    let presentation = PresentationConfig {
        mode: config.search.presentation,
        provider: config.search.presentation_provider.clone(),
        roles,
    };

    tracing::debug!(
        enabled = crate::search::is_active(config),
        providers = ?module_providers
            .iter()
            .map(|(name, p)| format!("{name}:{:?}:{}", p.route, p.enabled))
            .collect::<Vec<_>>(),
        presentation = config.search.presentation.as_str(),
        "[search][module] built module configuration"
    );

    ModuleSearchConfig {
        enabled: crate::search::is_active(config),
        backend,
        providers: module_providers,
        presentation,
    }
}

/// Deterministic declarations for synchronous tool registration, computed
/// from the same configuration the module will receive.
pub(crate) fn configured_tool_specs(config: &Config) -> Vec<tinysearch_bus::ToolSpec> {
    let module = module_config(config);
    if !module.enabled {
        return Vec::new();
    }
    let available =
        tinysearch_bus::configured_provider_tools(&module, &tinysearch_bus::provider_tool_specs());
    let mut specs = tinysearch_bus::select_tools(&available, &module.presentation).tools;
    if let Some(binding) = &config.profile_providers {
        binding.present(&mut specs);
    }
    specs
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod config_tests;
