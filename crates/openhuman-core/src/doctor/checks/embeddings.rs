//! Static validation for the selected embedding provider and vector settings.

use async_trait::async_trait;

use crate::config::Config;
use crate::doctor::{DoctorCheck, DoctorCtx, DoctorTier, Finding};

const CONFIG_ID: &str = "embeddings/config";

pub(super) struct EmbeddingConfigCheck;

#[async_trait]
impl DoctorCheck for EmbeddingConfigCheck {
    fn id(&self) -> &str {
        CONFIG_ID
    }

    fn category(&self) -> &str {
        "embedding configuration"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Static
    }

    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding> {
        validate(ctx.config())
    }
}

fn validate(config: &Config) -> Vec<Finding> {
    if config
        .embeddings_provider
        .as_deref()
        .is_some_and(|provider| provider.trim() == "none")
    {
        return Vec::new();
    }

    let local_model = config.workload_local_model("embeddings");
    let active_route_index = if local_model.is_none() {
        config
            .memory
            .embedding_model
            .trim()
            .strip_prefix("hint:")
            .and_then(|hint| {
                config
                    .embedding_routes
                    .iter()
                    .enumerate()
                    .find(|(_, route)| route.hint.trim() == hint.trim())
                    .map(|(index, _)| index)
            })
    } else {
        None
    };
    let active_route = active_route_index.and_then(|index| config.embedding_routes.get(index));
    let active_hint = if local_model.is_none() {
        config
            .memory
            .embedding_model
            .trim()
            .strip_prefix("hint:")
            .map(str::trim)
    } else {
        None
    };
    let (effective_provider, effective_model, effective_dimensions) =
        tinyinference_embeddings::catalog::effective_embedding_settings(
            &config.memory.embedding_provider,
            &config.memory.embedding_model,
            config.memory.embedding_dimensions,
            local_model.as_deref(),
        );
    let (provider, model, dimensions, provider_path, model_path, dimensions_path) =
        if let Some(route) = active_route {
            let route_path = format!(
                "embedding_routes[{}]",
                active_route_index.expect("active route has an index")
            );
            (
                route.provider.clone(),
                route.model.clone(),
                route
                    .dimensions
                    .unwrap_or(config.memory.embedding_dimensions),
                format!("{route_path}.provider"),
                format!("{route_path}.model"),
                if route.dimensions.is_some() {
                    format!("{route_path}.dimensions")
                } else {
                    "memory.embedding_dimensions".to_owned()
                },
            )
        } else {
            (
                effective_provider,
                effective_model,
                effective_dimensions,
                "memory.embedding_provider".to_owned(),
                "memory.embedding_model".to_owned(),
                if local_model.is_some() {
                    "embeddings_provider".to_owned()
                } else {
                    "memory.embedding_dimensions".to_owned()
                },
            )
        };
    let provider_slug = normalized_provider_slug(&provider);
    let mut findings = Vec::new();

    if active_hint.is_some()
        && active_route.is_none()
        && !config
            .embedding_routes
            .iter()
            .any(|route| route.hint.trim() == active_hint.unwrap())
    {
        findings.push(invalid(
            "memory.embedding_model",
            "The selected embedding model hint has no matching route.",
            "Add the matching embedding route or select a concrete embedding model.",
        ));
    }

    if tinyinference_embeddings::catalog::find_provider(provider_slug).is_none() {
        findings.push(invalid(
            &provider_path,
            "The configured embedding provider is not supported by this build.",
            "Select a supported embedding provider and rerun doctor.",
        ));
    }

    if model.trim().is_empty() {
        findings.push(invalid(
            &model_path,
            "The configured embedding model is missing.",
            "Set an embedding model for the selected provider and rerun doctor.",
        ));
    }

    if dimensions == 0 {
        findings.push(invalid(
            &dimensions_path,
            "The selected embedding model has no usable vector dimensions.",
            "Choose a model with known dimensions or configure a positive vector width.",
        ));
    } else if let Some(preset) =
        tinyinference_embeddings::catalog::find_model(provider_slug, model.trim())
        && !preset.allowed_dimensions.contains(&dimensions)
    {
        findings.push(invalid(
            &dimensions_path,
            "The configured vector dimensions do not match the selected embedding model.",
            "Set a dimension supported by the selected embedding model and rerun doctor.",
        ));
    }

    if provider_slug == "custom" && !custom_endpoint_is_configured(config, &provider) {
        findings.push(invalid(
            "custom_embeddings.endpoint",
            "The custom embedding provider has no configured endpoint.",
            "Set and verify the custom embedding endpoint in Settings, then rerun doctor.",
        ));
    }

    for (index, route) in config.embedding_routes.iter().enumerate() {
        let path = format!("embedding_routes[{index}]");
        if route.hint.trim().is_empty() {
            findings.push(invalid(
                &format!("{path}.hint"),
                "An embedding route has no workload hint.",
                "Set a non-empty hint for this embedding route.",
            ));
        }
        if route.model.trim().is_empty() {
            findings.push(invalid(
                &format!("{path}.model"),
                "An embedding route has no model.",
                "Set a model for this embedding route.",
            ));
        }
        let route_dimensions = route
            .dimensions
            .unwrap_or(config.memory.embedding_dimensions);
        let route_dimensions_path = if route.dimensions.is_some() {
            format!("{path}.dimensions")
        } else {
            "memory.embedding_dimensions".to_owned()
        };
        if route_dimensions == 0 {
            findings.push(invalid(
                &route_dimensions_path,
                "An embedding route has a zero vector dimension.",
                "Set a positive dimension supported by the route's model.",
            ));
        }
        let route_provider = normalized_provider_slug(&route.provider);
        if tinyinference_embeddings::catalog::find_provider(route_provider).is_none() {
            findings.push(invalid(
                &format!("{path}.provider"),
                "An embedding route uses an unsupported provider.",
                "Select a supported embedding provider for this route.",
            ));
        }
        if route_provider == "custom" && !custom_endpoint_is_configured(config, &route.provider) {
            findings.push(invalid(
                &format!("{path}.provider"),
                "A custom embedding route has no configured endpoint.",
                "Set a custom endpoint for this embedding route.",
            ));
        }
        if let Some(preset) =
            tinyinference_embeddings::catalog::find_model(route_provider, route.model.trim())
            && route_dimensions > 0
            && !preset.allowed_dimensions.contains(&route_dimensions)
        {
            findings.push(invalid(
                &format!("{path}.dimensions"),
                "An embedding route's dimensions do not match its model.",
                "Set a dimension supported by the embedding route's model.",
            ));
        }
    }

    if active_route.is_none() {
        if let Some(route) = config.embeddings_provider.as_deref() {
            if route.trim() != "none" {
                if let Some((route_provider, route_model)) = provider_route_parts(route) {
                    if normalize_provider(route_provider) != normalize_provider(&provider) {
                        findings.push(invalid(
                            "embeddings_provider",
                            "The embedding workload route and memory settings select different providers.",
                            "Use the same embedding provider in both settings and rerun doctor.",
                        ));
                    } else if !route_model.is_empty()
                        && route_model != model.trim()
                        && !route.starts_with("custom:")
                    {
                        findings.push(invalid(
                            "embeddings_provider",
                            "The embedding workload route and memory settings select different models.",
                            "Use the same embedding model in both settings and rerun doctor.",
                        ));
                    }
                }
            }
        }
    }

    let mut seen_paths = std::collections::HashSet::new();
    findings.retain(|finding| {
        finding
            .config_path
            .as_ref()
            .is_none_or(|path| seen_paths.insert(path.clone()))
    });
    findings
}

fn normalized_provider_slug(provider: &str) -> &str {
    match provider.trim() {
        "cloud" | "openhuman" => "managed",
        provider if provider.starts_with("custom:") => "custom",
        provider if provider.starts_with("ollama:") => "ollama",
        other => other,
    }
}

fn custom_endpoint_is_configured(config: &Config, provider: &str) -> bool {
    provider
        .strip_prefix("custom:")
        .is_some_and(|endpoint| !endpoint.trim().is_empty())
        || config
            .custom_embeddings
            .as_ref()
            .is_some_and(|custom| !custom.endpoint.trim().is_empty())
}

fn normalize_provider(provider: &str) -> &str {
    match provider.trim() {
        "cloud" | "openhuman" => "managed",
        provider if provider.starts_with("custom:") => "custom",
        provider if provider.starts_with("ollama:") => "ollama",
        other => other,
    }
}

fn provider_route_parts(provider: &str) -> Option<(&str, &str)> {
    let provider = provider.trim();
    if let Some(endpoint) = provider.strip_prefix("custom:") {
        return Some(("custom", endpoint));
    }
    if let Some(model) = provider.strip_prefix("ollama:") {
        return Some(("ollama", model.trim()));
    }
    match provider.split_once(':') {
        Some((slug, model)) => Some((slug.trim(), model.trim())),
        None if !provider.is_empty() => Some((provider, "")),
        None => None,
    }
}

fn invalid(path: &str, message: &str, fix_hint: &str) -> Finding {
    let mut finding = Finding::error(CONFIG_ID, message, fix_hint);
    finding.config_path = Some(path.to_owned());
    finding
}
