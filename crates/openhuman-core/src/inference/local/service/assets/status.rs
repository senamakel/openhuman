//! Reporting which local model assets are present, missing, or downloading.

use tracing::{debug, trace};

use crate::config::Config;
use crate::inference::local::provider::{provider_from_config, LocalAiProvider};
use crate::inference::local::service::LocalAiService;
use crate::inference::model_ids;
use crate::inference::paths::resolve_tts_voice_path;
use crate::inference::presets::{self, VisionMode};
use crate::inference::types::{LocalAiAssetStatus, LocalAiAssetsStatus};

impl LocalAiService {
    pub async fn assets_status(&self, config: &Config) -> Result<LocalAiAssetsStatus, String> {
        let chat_model = model_ids::effective_chat_model_id(config);
        let vision_model = model_ids::effective_vision_model_id(config);
        let embedding_model = model_ids::effective_embedding_model_id(config);
        let tts_voice = model_ids::effective_tts_voice_id(config);

        let provider = provider_from_config(config);
        let correlation_id = uuid::Uuid::new_v4().to_string();
        trace!(
            target: "local_ai::assets",
            %correlation_id,
            provider = %provider.as_str(),
            chat_model = %chat_model,
            vision_model = %vision_model,
            embedding_model = %embedding_model,
            "[local_ai:assets:provider_routing] entry"
        );

        // External-runtime precondition: OpenHuman no longer installs or
        // starts Ollama itself, so the interesting question is whether the
        // user-managed runtime is reachable right now.
        let uses_ollama_assets = matches!(
            provider,
            LocalAiProvider::Ollama | LocalAiProvider::LmStudio
        );
        let ollama_available = if uses_ollama_assets {
            let base_url = crate::inference::local::ollama_base_url_from_config(config);
            let present = self.ollama_healthy_at(&base_url).await;
            debug!(
                target: "local_ai::assets",
                %correlation_id,
                provider = %provider.as_str(),
                ollama_available = present,
                "[local_ai:assets:provider_routing] ollama runtime check"
            );
            present
        } else {
            true
        };
        let (chat_ready, vision_ready, embedding_ready) = if provider == LocalAiProvider::LmStudio {
            trace!(
                target: "local_ai::assets",
                %correlation_id,
                branch = "lm_studio",
                "[local_ai:assets:provider_routing] selected provider branch"
            );
            let chat_ready = match self.has_lm_studio_model(config, &chat_model).await {
                Ok(ready) => {
                    debug!(
                        target: "local_ai::assets",
                        %correlation_id,
                        provider = "lm_studio",
                        model = %chat_model,
                        ready,
                        "[local_ai:assets:provider_routing] lm studio chat model check"
                    );
                    ready
                }
                Err(err) => {
                    debug!(
                        target: "local_ai::assets",
                        %correlation_id,
                        provider = "lm_studio",
                        model = %chat_model,
                        error = %err,
                        "[local_ai:assets:provider_routing] lm studio chat model check failed"
                    );
                    false
                }
            };
            let embedding_ready = if ollama_available {
                match self.has_model_for_config(config, &embedding_model).await {
                    Ok(ready) => {
                        debug!(
                            target: "local_ai::assets",
                            %correlation_id,
                            provider = "ollama",
                            model = %embedding_model,
                            ready,
                            "[local_ai:assets:provider_routing] lm studio embedding ollama model check"
                        );
                        ready
                    }
                    Err(err) => {
                        debug!(
                            target: "local_ai::assets",
                            %correlation_id,
                            provider = "ollama",
                            model = %embedding_model,
                            error = %err,
                            "[local_ai:assets:provider_routing] lm studio embedding ollama model check failed"
                        );
                        false
                    }
                }
            } else {
                debug!(
                    target: "local_ai::assets",
                    %correlation_id,
                    provider = "ollama",
                    model = %embedding_model,
                    "[local_ai:assets:provider_routing] lm studio embedding check skipped; ollama runtime unavailable"
                );
                false
            };
            (chat_ready, false, embedding_ready)
        } else if ollama_available {
            trace!(
                target: "local_ai::assets",
                %correlation_id,
                branch = "ollama",
                "[local_ai:assets:provider_routing] selected provider branch"
            );
            let chat_ready = match self.has_model_for_config(config, &chat_model).await {
                Ok(ready) => {
                    debug!(
                        target: "local_ai::assets",
                        %correlation_id,
                        provider = "ollama",
                        capability = "chat",
                        model = %chat_model,
                        ready,
                        "[local_ai:assets:provider_routing] ollama model check"
                    );
                    ready
                }
                Err(err) => {
                    debug!(
                        target: "local_ai::assets",
                        %correlation_id,
                        provider = "ollama",
                        capability = "chat",
                        model = %chat_model,
                        error = %err,
                        "[local_ai:assets:provider_routing] ollama model check failed"
                    );
                    false
                }
            };
            let vision_ready = match self.has_model_for_config(config, &vision_model).await {
                Ok(ready) => {
                    debug!(
                        target: "local_ai::assets",
                        %correlation_id,
                        provider = "ollama",
                        capability = "vision",
                        model = %vision_model,
                        ready,
                        "[local_ai:assets:provider_routing] ollama model check"
                    );
                    ready
                }
                Err(err) => {
                    debug!(
                        target: "local_ai::assets",
                        %correlation_id,
                        provider = "ollama",
                        capability = "vision",
                        model = %vision_model,
                        error = %err,
                        "[local_ai:assets:provider_routing] ollama model check failed"
                    );
                    false
                }
            };
            let embedding_ready = match self.has_model_for_config(config, &embedding_model).await {
                Ok(ready) => {
                    debug!(
                        target: "local_ai::assets",
                        %correlation_id,
                        provider = "ollama",
                        capability = "embedding",
                        model = %embedding_model,
                        ready,
                        "[local_ai:assets:provider_routing] ollama model check"
                    );
                    ready
                }
                Err(err) => {
                    debug!(
                        target: "local_ai::assets",
                        %correlation_id,
                        provider = "ollama",
                        capability = "embedding",
                        model = %embedding_model,
                        error = %err,
                        "[local_ai:assets:provider_routing] ollama model check failed"
                    );
                    false
                }
            };
            (chat_ready, vision_ready, embedding_ready)
        } else {
            trace!(
                target: "local_ai::assets",
                %correlation_id,
                branch = "ollama_runtime_unavailable",
                "[local_ai:assets:provider_routing] selected provider branch"
            );
            (false, false, false)
        };
        trace!(
            target: "local_ai::assets",
            %correlation_id,
            chat_ready,
            vision_ready,
            embedding_ready,
            ollama_available,
            "[local_ai:assets:provider_routing] exit"
        );
        let tts_resolve = resolve_tts_voice_path(config);

        let tts_path = tts_resolve.as_ref().ok().cloned();

        // TTS is downloaded on demand (first synthesis). When the model file
        // is not yet on disk but a download URL is configured, report
        // "ondemand" instead of "missing" so the UI can treat it as
        // non-blocking.
        let has_tts_url = config
            .local_ai
            .tts_download_url
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty());

        let tts_state = if tts_path.is_some() {
            "ready"
        } else if has_tts_url {
            "ondemand"
        } else {
            "missing"
        };

        if let Err(ref err) = tts_resolve {
            debug!("[local_ai::assets_status] TTS resolve failed (state={tts_state}): {err}");
        }

        let tts_warning = match tts_state {
            "ondemand" => Some("TTS voice will download on first synthesis request.".to_string()),
            _ => None,
        };

        let vision_mode = presets::vision_mode_for_config(&config.local_ai);
        let embedding_path = Some(format!("ollama://{embedding_model}"));
        Ok(LocalAiAssetsStatus {
            chat: LocalAiAssetStatus {
                state: if chat_ready { "ready" } else { "missing" }.to_string(),
                id: chat_model,
                provider: provider.as_str().to_string(),
                path: None,
                warning: (provider == LocalAiProvider::LmStudio && !chat_ready).then(|| {
                    "Load this model in LM Studio or update local_ai.chat_model_id.".to_string()
                }),
            },
            vision: LocalAiAssetStatus {
                state: if provider == LocalAiProvider::LmStudio {
                    "disabled".to_string()
                } else {
                    match vision_mode {
                        VisionMode::Disabled => "disabled",
                        VisionMode::Ondemand if vision_ready => "ready",
                        VisionMode::Ondemand => "ondemand",
                        VisionMode::Bundled if vision_ready => "ready",
                        VisionMode::Bundled => "missing",
                    }
                    .to_string()
                },
                id: vision_model,
                provider: provider.as_str().to_string(),
                path: None,
                warning: if provider == LocalAiProvider::LmStudio {
                    Some("Vision is not part of the first LM Studio provider slice.".to_string())
                } else {
                    match vision_mode {
                        VisionMode::Disabled => {
                            Some("Vision is disabled for this RAM tier.".to_string())
                        }
                        VisionMode::Ondemand if !vision_ready => {
                            Some("Vision model will download on first vision request.".to_string())
                        }
                        _ => None,
                    }
                },
            },
            embedding: LocalAiAssetStatus {
                state: if embedding_ready { "ready" } else { "missing" }.to_string(),
                id: embedding_model,
                provider: if provider == LocalAiProvider::LmStudio {
                    "ollama".to_string()
                } else {
                    provider.as_str().to_string()
                },
                path: embedding_path,
                warning: (provider == LocalAiProvider::LmStudio).then(|| {
                    "Embeddings still use the existing Ollama path in this first LM Studio slice."
                        .to_string()
                }),
            },
            tts: LocalAiAssetStatus {
                state: tts_state.to_string(),
                id: tts_voice,
                provider: "piper".to_string(),
                path: tts_path,
                warning: tts_warning,
            },
            quantization: model_ids::effective_quantization(config),
            ollama_available,
        })
    }
}
