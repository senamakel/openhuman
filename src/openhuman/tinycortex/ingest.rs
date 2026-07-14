//! Host adapters for tinycortex on-demand ingestion.

use rusqlite::Transaction;
use tinycortex::memory::ingest::{QueueJobSink, TreeJobSink};
use tinycortex::memory::score::extract::{LlmEntityExtractor, LlmExtractorConfig};
use tinycortex::memory::score::ScoringConfig;

use crate::openhuman::config::Config;
pub struct HostTreeJobSink;

impl HostTreeJobSink {
    pub fn new(_config: Config) -> Self {
        Self
    }
}

impl TreeJobSink for HostTreeJobSink {
    fn enqueue_extract_tx(
        &self,
        tx: &Transaction<'_>,
        chunk_id: &str,
        default_max_attempts: u32,
    ) -> anyhow::Result<bool> {
        tracing::trace!(
            chunk_id,
            default_max_attempts,
            "[memory:ingest] enqueue extract job in chunk transaction"
        );
        QueueJobSink.enqueue_extract_tx(tx, chunk_id, default_max_attempts)
    }
}

fn scoring_config(config: &Config) -> ScoringConfig {
    match super::build_chat_provider(config) {
        Ok(provider) => {
            let mut extractor = LlmExtractorConfig::default();
            extractor.output_language = config.output_language.clone();
            ScoringConfig::with_llm_extractor(std::sync::Arc::new(LlmEntityExtractor::new(
                extractor, provider,
            )))
        }
        Err(error) => {
            tracing::warn!(%error, "[memory:ingest] chat provider unavailable; using regex scoring");
            ScoringConfig::default_regex_only()
        }
    }
}

pub fn context(
    config: &Config,
) -> (
    tinycortex::memory::MemoryConfig,
    HostTreeJobSink,
    ScoringConfig,
) {
    (
        super::memory_config_from(config, config.workspace_dir.clone()),
        HostTreeJobSink::new(config.clone()),
        scoring_config(config),
    )
}
