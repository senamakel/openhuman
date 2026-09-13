//! Download progress reporting, and the status reset once an LM Studio
//! download completes.

use crate::config::Config;
use crate::inference::local::service::LocalAiService;
use crate::inference::types::{LocalAiDownloadProgressItem, LocalAiDownloadsProgress};

impl LocalAiService {
    pub async fn downloads_progress(
        &self,
        config: &Config,
    ) -> Result<LocalAiDownloadsProgress, String> {
        let assets = self.assets_status(config).await?;
        let status = self.status();

        let mut chat = LocalAiDownloadProgressItem {
            id: assets.chat.id,
            provider: assets.chat.provider,
            state: assets.chat.state,
            progress: None,
            downloaded_bytes: None,
            total_bytes: None,
            speed_bps: None,
            eta_seconds: None,
            warning: assets.chat.warning,
            path: assets.chat.path,
        };
        let mut vision = LocalAiDownloadProgressItem {
            id: assets.vision.id,
            provider: assets.vision.provider,
            state: assets.vision.state,
            progress: None,
            downloaded_bytes: None,
            total_bytes: None,
            speed_bps: None,
            eta_seconds: None,
            warning: assets.vision.warning,
            path: assets.vision.path,
        };
        let mut embedding = LocalAiDownloadProgressItem {
            id: assets.embedding.id,
            provider: assets.embedding.provider,
            state: assets.embedding.state,
            progress: None,
            downloaded_bytes: None,
            total_bytes: None,
            speed_bps: None,
            eta_seconds: None,
            warning: assets.embedding.warning,
            path: assets.embedding.path,
        };
        let mut tts = LocalAiDownloadProgressItem {
            id: assets.tts.id,
            provider: assets.tts.provider,
            state: assets.tts.state,
            progress: None,
            downloaded_bytes: None,
            total_bytes: None,
            speed_bps: None,
            eta_seconds: None,
            warning: assets.tts.warning,
            path: assets.tts.path,
        };

        if status.state == "downloading" {
            let active = if status.tts_state == "downloading" {
                "tts"
            } else if status.vision_state == "downloading" {
                "vision"
            } else if status.embedding_state == "downloading" {
                "embedding"
            } else {
                "chat"
            };

            let apply = |item: &mut LocalAiDownloadProgressItem| {
                item.state = "downloading".to_string();
                item.progress = status.download_progress;
                item.downloaded_bytes = status.downloaded_bytes;
                item.total_bytes = status.total_bytes;
                item.speed_bps = status.download_speed_bps;
                item.eta_seconds = status.eta_seconds;
                item.warning = status.warning.clone();
            };

            match active {
                "tts" => apply(&mut tts),
                "vision" => apply(&mut vision),
                "embedding" => apply(&mut embedding),
                _ => apply(&mut chat),
            }
        }

        Ok(LocalAiDownloadsProgress {
            state: status.state,
            warning: status.warning,
            progress: status.download_progress,
            downloaded_bytes: status.downloaded_bytes,
            total_bytes: status.total_bytes,
            speed_bps: status.download_speed_bps,
            eta_seconds: status.eta_seconds,
            chat,
            vision,
            embedding,
            tts,
            ollama_available: assets.ollama_available,
        })
    }

    pub(super) fn finalize_lm_studio_download_status(
        &self,
        config: &Config,
        embedding_state: Option<&'static str>,
        tts_state: Option<&'static str>,
        warning: Option<String>,
    ) {
        let mut status = self.status.lock();
        status.state = "ready".to_string();
        status.vision_state = "disabled".to_string();
        if let Some(state) = embedding_state {
            status.embedding_state = state.to_string();
        } else if !config.local_ai.preload_embedding_model {
            status.embedding_state = "idle".to_string();
        } else if status.embedding_state != "ready" {
            status.embedding_state = "missing".to_string();
        }
        if let Some(state) = tts_state {
            status.tts_state = state.to_string();
        } else if !config.local_ai.preload_tts_voice {
            status.tts_state = "idle".to_string();
        }
        status.warning = warning;
        status.error_detail = None;
        status.error_category = None;
        status.download_progress = None;
        status.downloaded_bytes = None;
        status.total_bytes = None;
        status.download_speed_bps = None;
        status.eta_seconds = None;
    }
}
