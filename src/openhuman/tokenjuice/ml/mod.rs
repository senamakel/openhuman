//! TokenJuice ML plain-text compressor ("Kompress").
//!
//! Plain text has no structural skeleton to exploit, so high-quality
//! compression needs a learned model (ModernBERT token/sentence salience). That
//! runs inside the shared [`crate::openhuman::runtime_python_server`] as the
//! `kompress` backend — this module is just the thin Rust entry the
//! [`crate::openhuman::tokenjuice::compressors::ml_text`] compressor calls.
//!
//! Opt-in at runtime via `config.tokenjuice.ml_compression_enabled` (default
//! off) — there is no build-time feature gate, since torch is provisioned at
//! runtime (pip), never linked. Degrades gracefully (`Ok(None)` / `Err`) when
//! the flag is off, the runtime python server is unavailable, or the input is
//! too large — the agent loop never fails because ML compression is missing.

use std::sync::OnceLock;

use anyhow::Result;

use crate::openhuman::config::Config;
use crate::openhuman::tokenjuice::types::CompressOptions;

/// Global config snapshot, installed once at startup. The runtime python server
/// provisions/launches against the full [`Config`], so the deep compressor call
/// site (which has no `Config`) reads it from here.
static CONFIG: OnceLock<Config> = OnceLock::new();

/// Install the config snapshot the Kompress backend runs against. Called from
/// the core startup.
pub fn configure(config: Config) {
    let _ = CONFIG.set(config);
}

/// Compress `text` via the Kompress backend of the runtime python server.
///
/// Returns `Ok(Some(compacted))` on a useful result, `Ok(None)` when the flag
/// is off / input too large / output wouldn't help, and `Err` when the backend
/// is unavailable (caller degrades to a native compressor).
pub async fn compress(text: &str, _opts: &CompressOptions) -> Result<Option<String>> {
    let Some(config) = CONFIG.get() else {
        anyhow::bail!("tokenjuice ml not configured");
    };
    let tj = &config.tokenjuice;
    if !tj.ml_compression_enabled {
        return Ok(None);
    }
    if text.len() > tj.ml_max_input_chars {
        // Too large for the model — let a native compressor handle it.
        return Ok(None);
    }

    let resp = crate::openhuman::runtime_python_server::request_kompress(config, text).await?;
    if resp.compressed_text.is_empty() || resp.compressed_text.len() >= text.len() {
        return Ok(None);
    }
    log::debug!(
        "[tokenjuice::ml] kompress {} -> {} chars",
        resp.input_chars,
        resp.output_chars
    );
    Ok(Some(resp.compressed_text))
}
