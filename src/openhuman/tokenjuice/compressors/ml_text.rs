//! ML plain-text compressor ("Kompress") — trait slot.
//!
//! Plain text has no structural skeleton to exploit, so high-quality
//! compression needs a learned model (Headroom uses ModernBERT token
//! classification to drop low-salience spans). That path runs a Python sidecar
//! and is **opt-in** behind the `tokenjuice.ml_compression_enabled` config flag
//! plus the `tokenjuice_ml` cargo feature.
//!
//! This module is the [`Compressor`] slot. The async sidecar bridge is wired in
//! the `ml/` submodule in a later slice; until then (and whenever the feature/
//! flag/runtime is unavailable) `compress` declines so the router falls back to
//! the generic compressor — never an error in the agent loop.

use async_trait::async_trait;

use super::Compressor;
use crate::openhuman::tokenjuice::types::{
    CompressInput, CompressOptions, CompressOutput, CompressorKind,
};

pub struct MlTextCompressor;

#[async_trait]
impl Compressor for MlTextCompressor {
    fn kind(&self) -> CompressorKind {
        CompressorKind::MlText
    }

    async fn compress(
        &self,
        input: &CompressInput<'_>,
        opts: &CompressOptions,
    ) -> Option<CompressOutput> {
        if !opts.ml_text_enabled {
            return None;
        }
        #[cfg(feature = "tokenjuice_ml")]
        {
            match crate::openhuman::tokenjuice::ml::compress(input.content, opts).await {
                Ok(Some(text)) if text.len() < input.content.len() => {
                    return Some(CompressOutput::lossy(text, CompressorKind::MlText));
                }
                Ok(_) => return None,
                Err(e) => {
                    log::debug!("[tokenjuice][ml] unavailable, falling back: {e:#}");
                    return None;
                }
            }
        }
        #[cfg(not(feature = "tokenjuice_ml"))]
        {
            let _ = input;
            None
        }
    }
}
