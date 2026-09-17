//! Triggering model downloads: everything the profile needs, or one asset.

use crate::config::Config;
use crate::inference::local::provider::{provider_from_config, LocalAiProvider};
use crate::inference::local::service::LocalAiService;
use crate::inference::model_ids;
use crate::inference::presets::{self, VisionMode};
use crate::inference::types::LocalAiAssetsStatus;

impl LocalAiService {
    pub async fn download_all_models(&self, config: &Config) -> Result<(), String> {
        if !config.local_ai.runtime_enabled {
            return Err("local ai is disabled".to_string());
        }
        let _guard = self.bootstrap_lock.lock().await;

        if provider_from_config(config) == LocalAiProvider::LmStudio {
            self.ensure_lm_studio_available(config).await?;
            let mut embedding_state = None;
            if config.local_ai.preload_embedding_model {
                let model_id = model_ids::effective_embedding_model_id(config);
                {
                    let mut status = self.status.lock();
                    status.state = "downloading".to_string();
                    status.embedding_state = "downloading".to_string();
                    status.warning = Some(format!(
                        "Downloading embedding model via Ollama: `{model_id}`"
                    ));
                }
                if let Err(err) = async {
                    self.ensure_ollama_server(config).await?;
                    self.ensure_ollama_model_available(config, &model_id, "embedding")
                        .await
                }
                .await
                {
                    log::warn!(
                        "[local_ai] LM Studio download_all_models embedding preload failed: {err}"
                    );
                    self.finalize_lm_studio_download_status(config, Some("missing"), None, None);
                    return Err(err);
                }
                embedding_state = Some("ready");
            }
            let mut tts_warning = None;
            let mut tts_state = None;
            if config.local_ai.preload_tts_voice {
                if let Err(err) = self.ensure_tts_asset_available(config).await {
                    log::warn!(
                        "[local_ai] LM Studio download_all_models TTS preload failed: {err}"
                    );
                    tts_state = Some("missing");
                    tts_warning = Some(err);
                } else {
                    tts_state = Some("ready");
                }
            }
            let warning = tts_warning;
            self.finalize_lm_studio_download_status(config, embedding_state, tts_state, warning);
            return Ok(());
        }

        self.ensure_ollama_server(config).await?;

        let mut steps = vec![
            ("chat", model_ids::effective_chat_model_id(config)),
            ("embedding", model_ids::effective_embedding_model_id(config)),
        ];
        if matches!(
            presets::vision_mode_for_config(&config.local_ai),
            VisionMode::Bundled
        ) {
            steps.insert(1, ("vision", model_ids::effective_vision_model_id(config)));
        }

        let total = steps.len();
        for (index, (label, model_id)) in steps.into_iter().enumerate() {
            {
                let mut status = self.status.lock();
                status.state = "downloading".to_string();
                status.warning = Some(format!(
                    "Downloading {} model {}/{}: `{}`",
                    label,
                    index + 1,
                    total,
                    model_id
                ));
                match label {
                    "vision" => status.vision_state = "downloading".to_string(),
                    "embedding" => status.embedding_state = "downloading".to_string(),
                    _ => {}
                }
            }
            self.ensure_ollama_model_available(config, &model_id, label)
                .await?;
        }

        let mut tts_warning = None;
        if let Err(err) = self.ensure_tts_asset_available(config).await {
            self.status.lock().tts_state = "missing".to_string();
            tts_warning = Some(err);
        }

        {
            let mut status = self.status.lock();
            status.state = "ready".to_string();
            status.vision_state = match presets::vision_mode_for_config(&config.local_ai) {
                VisionMode::Disabled => "disabled".to_string(),
                VisionMode::Ondemand => "idle".to_string(),
                VisionMode::Bundled => "ready".to_string(),
            };
            status.download_progress = Some(1.0);
            status.downloaded_bytes = None;
            status.total_bytes = None;
            status.download_speed_bps = None;
            status.eta_seconds = None;
            status.warning = tts_warning;
        }

        Ok(())
    }

    pub async fn download_asset(
        &self,
        config: &Config,
        capability: &str,
    ) -> Result<LocalAiAssetsStatus, String> {
        if !config.local_ai.runtime_enabled {
            return Err("local ai is disabled".to_string());
        }
        let _guard = self.bootstrap_lock.lock().await;

        let capability = capability.trim().to_ascii_lowercase();
        if provider_from_config(config) == LocalAiProvider::LmStudio
            && matches!(capability.as_str(), "chat" | "vision")
        {
            return Err(
                "LM Studio manages chat and vision model downloads. Load the model in LM Studio, then retry."
                    .to_string(),
            );
        }
        match capability.as_str() {
            "chat" => {
                self.ensure_ollama_server(config).await?;
                let model = model_ids::effective_chat_model_id(config);
                self.ensure_ollama_model_available(config, &model, "chat")
                    .await?;
            }
            "vision" => {
                if matches!(
                    presets::vision_mode_for_config(&config.local_ai),
                    VisionMode::Disabled
                ) {
                    return Err(
                        "Vision is disabled for this RAM tier. Switch to the 4-8 GB tier or above to enable it."
                            .to_string(),
                    );
                }
                // Resolve rather than take the effective id: the latter blanks
                // a chat-only model, and a blank id makes
                // `ensure_ollama_model_available` answer "no vision model is
                // configured" to a user who configured one. Naming the model
                // that cannot accept images is the whole point of #5146 P1.
                //
                // Before probing Ollama, too: a misconfigured id is answerable
                // without the network, and reaching the server first would hand
                // back a connection error for a problem that is purely local.
                let model = model_ids::resolve_vision_model_id(config)?;
                self.ensure_ollama_server(config).await?;
                self.ensure_ollama_model_available(config, &model, "vision")
                    .await?;
            }
            "embedding" | "embeddings" => {
                self.ensure_ollama_server(config).await?;
                let model = model_ids::effective_embedding_model_id(config);
                self.ensure_ollama_model_available(config, &model, "embedding")
                    .await?;
            }
            "tts" => {
                self.ensure_tts_asset_available(config).await?;
            }
            _ => {
                return Err(
                    "Unknown capability. Use one of: chat, vision, embedding, tts.".to_string(),
                )
            }
        }

        self.assets_status(config).await
    }
}
