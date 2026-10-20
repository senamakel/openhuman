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
    let (provider, model, dimensions) =
        tinyinference_embeddings::catalog::effective_embedding_settings(
            &config.memory.embedding_provider,
            &config.memory.embedding_model,
            config.memory.embedding_dimensions,
            local_model.as_deref(),
        );
    let provider_slug = normalized_provider_slug(&provider);
    let mut findings = Vec::new();

    if tinyinference_embeddings::catalog::find_provider(provider_slug).is_none() {
        findings.push(invalid(
            "memory.embedding_provider",
            "The configured embedding provider is not supported by this build.",
            "Select a supported embedding provider and rerun doctor.",
        ));
    }

    if model.trim().is_empty() {
        findings.push(invalid(
            "memory.embedding_model",
            "The configured embedding model is missing.",
            "Set an embedding model for the selected provider and rerun doctor.",
        ));
    }

    if dimensions == 0 {
        findings.push(invalid(
            if local_model.is_some() {
                "embeddings_provider"
            } else {
                "memory.embedding_dimensions"
            },
            "The selected embedding model has no usable vector dimensions.",
            "Choose a model with known dimensions or configure a positive vector width.",
        ));
    } else if let Some(preset) =
        tinyinference_embeddings::catalog::find_model(provider_slug, model.trim())
        && !preset.allowed_dimensions.contains(&dimensions)
    {
        findings.push(invalid(
            if local_model.is_some() {
                "embeddings_provider"
            } else {
                "memory.embedding_dimensions"
            },
            "The configured vector dimensions do not match the selected embedding model.",
            "Set a dimension supported by the selected embedding model and rerun doctor.",
        ));
    }

    if provider_slug == "custom"
        && !provider.starts_with("custom:")
        && config
            .custom_embeddings
            .as_ref()
            .is_none_or(|custom| custom.endpoint.trim().is_empty())
    {
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
        if route.dimensions == Some(0) {
            findings.push(invalid(
                &format!("{path}.dimensions"),
                "An embedding route has a zero vector dimension.",
                "Set a positive dimension supported by the route's model.",
            ));
        }

        if config.memory.embedding_model.trim() == format!("hint:{}", route.hint.trim()) {
            if let Some(route_dimensions) = route.dimensions
                && route_dimensions != config.memory.embedding_dimensions
            {
                findings.push(invalid(
                    &format!("{path}.dimensions"),
                    "The active embedding route dimensions disagree with memory configuration.",
                    "Set matching dimensions on the embedding route and memory configuration.",
                ));
            }
            if normalize_provider(&route.provider)
                != normalize_provider(&config.memory.embedding_provider)
            {
                findings.push(invalid(
                    &format!("{path}.provider"),
                    "The active embedding route provider disagrees with memory configuration.",
                    "Use the same provider in the embedding route and memory configuration.",
                ));
            }
        }
    }

    if let Some(route) = config.embeddings_provider.as_deref() {
        if route.trim() != "none"
            && provider_route_slug(route).is_some_and(|route_provider| {
                normalize_provider(route_provider) != normalize_provider(&provider)
            })
        {
            findings.push(invalid(
                "embeddings_provider",
                "The embedding workload route and memory provider select different providers.",
                "Use the same embedding provider in both settings and rerun doctor.",
            ));
        }
    }

    findings
}

fn normalized_provider_slug(provider: &str) -> &str {
    match provider.trim() {
        "cloud" | "openhuman" => "managed",
        provider if provider.starts_with("custom:") => "custom",
        other => other,
    }
}

fn normalize_provider(provider: &str) -> &str {
    match provider.trim() {
        "cloud" | "openhuman" => "managed",
        provider if provider.starts_with("custom:") => "custom",
        provider if provider.starts_with("ollama:") => "ollama",
        other => other,
    }
}

fn provider_route_slug(provider: &str) -> Option<&str> {
    let provider = provider.trim();
    if provider.starts_with("custom:") {
        return Some("custom");
    }
    if provider.starts_with("ollama:") {
        return Some("ollama");
    }
    provider
        .split_once(':')
        .map(|(slug, _)| slug.trim())
        .or_else(|| (!provider.is_empty()).then_some(provider))
}

fn invalid(path: &str, message: &str, fix_hint: &str) -> Finding {
    let mut finding = Finding::error(CONFIG_ID, message, fix_hint);
    finding.config_path = Some(path.to_owned());
    finding
}
