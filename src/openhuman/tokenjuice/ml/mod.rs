//! TokenJuice ML plain-text compressor ("Kompress") — Python/ModernBERT sidecar.
//!
//! Opt-in behind the `tokenjuice_ml` cargo feature **and** the
//! `tokenjuice.ml_compression_enabled` config flag. Provisions torch +
//! transformers into a managed venv on first use, runs a long-lived stdio
//! worker, and degrades gracefully (returns `Ok(None)` / `Err`) whenever
//! anything is unavailable — the agent loop never fails because ML compression
//! is missing.
//!
//! The compressor entry point [`crate::openhuman::tokenjuice::compressors::ml_text`]
//! calls [`compress`]; runtime config (model id, device, etc.) is installed once
//! via [`configure`] from the core's startup with the full [`Config`].

pub mod protocol;
pub mod provision;
pub mod sidecar;

#[cfg(test)]
#[path = "ml_tests.rs"]
mod ml_tests;

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::OnceCell;

use crate::openhuman::config::Config;
use crate::openhuman::tokenjuice::types::CompressOptions;
use provision::{ensure_kompress, KompressRuntime};
use sidecar::{MlSidecarOpts, PythonCompressorSidecar};

/// Global config snapshot, installed once at startup (the sidecar needs the
/// full [`Config`] for `runtime_python` provisioning + the ML settings).
static CONFIG: std::sync::OnceLock<Config> = std::sync::OnceLock::new();

/// Install the config snapshot the ML sidecar provisions against. Called from
/// the core startup when the feature is compiled in.
pub fn configure(config: Config) {
    let _ = CONFIG.set(config);
}

/// Bundle a live sidecar with the runtime + opts needed to respawn it on crash.
struct SidecarHandle {
    sidecar: PythonCompressorSidecar,
    runtime: KompressRuntime,
    opts: MlSidecarOpts,
}

/// Process-global memoised sidecar. `None` ⇒ init was attempted and failed;
/// callers fall back to a native compressor for the rest of the process.
static SIDECAR: OnceCell<Option<Arc<SidecarHandle>>> = OnceCell::const_new();

/// Compress `text` with the ML sidecar. Returns `Ok(Some(compacted))` on
/// success, `Ok(None)` when the result wouldn't help or the input is too large,
/// and `Err` when the sidecar is unavailable (caller degrades to native).
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

    let Some(handle) = shared_sidecar(config).await else {
        anyhow::bail!("kompress sidecar unavailable");
    };

    let result = handle
        .sidecar
        .compress(text, &handle.runtime, &handle.opts)
        .await?;
    Ok(Some(result.text))
}

/// Get the shared sidecar, provisioning + spawning on first call. Returns `None`
/// when unavailable; never errors so callers branch cleanly to fallback.
async fn shared_sidecar(config: &Config) -> Option<Arc<SidecarHandle>> {
    SIDECAR
        .get_or_init(|| async {
            match init_sidecar(config).await {
                Ok(h) => Some(Arc::new(h)),
                Err(e) => {
                    log::warn!("[tokenjuice::ml] kompress unavailable, using native fallback: {e:#}");
                    None
                }
            }
        })
        .await
        .clone()
}

async fn init_sidecar(config: &Config) -> Result<SidecarHandle> {
    let runtime = ensure_kompress(config).await?;
    let tj = &config.tokenjuice;
    let opts = MlSidecarOpts {
        model_id: tj.ml_model_id.clone(),
        device: tj.ml_device.clone(),
        target_ratio: tj.ml_target_ratio,
        max_input_chars: tj.ml_max_input_chars,
    };
    let sidecar = PythonCompressorSidecar::spawn(&runtime, &opts).await?;
    Ok(SidecarHandle {
        sidecar,
        runtime,
        opts,
    })
}
