//! One-time provisioning of the TokenJuice ML compressor ("Kompress") into a
//! dedicated virtualenv under the managed Python runtime.
//!
//! Mirrors `memory_tree::nlp::provision`: resolve a managed/system Python via
//! [`PythonBootstrap`], create an isolated venv, `pip install` torch (CPU
//! wheels) + transformers, write the worker script, and drop a marker so
//! subsequent launches skip straight to spawning. Network + disk heavy, but
//! guarded by the marker so it happens at most once per host.
//!
//! Any failure propagates as an error so the caller degrades to a native
//! compressor — the agent loop never fails because ML compression is missing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::process::Command;

use crate::openhuman::config::Config;
use crate::openhuman::runtime_python::PythonBootstrap;

/// Embedded worker script, written to disk at provision time.
const WORKER_PY: &str = include_str!("worker.py");

/// Timeouts for the one-time install. torch + transformers wheels are large
/// (CPU torch ~150 MB); give pip generous headroom on a cold cache.
const VENV_TIMEOUT: Duration = Duration::from_secs(120);
const PIP_TIMEOUT: Duration = Duration::from_secs(1800);

/// A provisioned Kompress runtime: the venv interpreter, worker script, and the
/// HuggingFace cache home for model weights.
#[derive(Debug, Clone)]
pub struct KompressRuntime {
    /// Python executable inside the dedicated venv (has torch + transformers).
    pub python_bin: PathBuf,
    /// Path to the written `worker.py` stdio server.
    pub worker_script: PathBuf,
    /// HF_HOME / model cache directory.
    pub hf_home: PathBuf,
}

/// Ensure torch + transformers are installed and return a ready-to-spawn runtime.
///
/// Idempotent: once the marker exists we skip venv creation and pip entirely.
pub async fn ensure_kompress(config: &Config) -> Result<KompressRuntime> {
    if !config.runtime_python.enabled {
        bail!("runtime_python disabled — cannot provision Kompress");
    }
    if !config.tokenjuice.ml_compression_enabled {
        bail!("tokenjuice.ml_compression_enabled is false");
    }

    let root = kompress_cache_root(config);
    tokio::fs::create_dir_all(&root)
        .await
        .with_context(|| format!("creating kompress cache dir {}", root.display()))?;

    let venv_dir = root.join("venv");
    let marker = venv_dir.join(".openhuman-kompress-ready");
    let worker_script = root.join("worker.py");
    let hf_home = root.join("hf");

    // Always (re)write the worker so an upgraded binary ships the latest protocol.
    tokio::fs::write(&worker_script, WORKER_PY)
        .await
        .with_context(|| format!("writing kompress worker {}", worker_script.display()))?;

    let venv_python = venv_python_path(&venv_dir);

    if marker.exists() && venv_python.exists() {
        log::debug!(
            "[tokenjuice::ml] kompress already provisioned at {}",
            venv_dir.display()
        );
        return Ok(KompressRuntime {
            python_bin: venv_python,
            worker_script,
            hf_home,
        });
    }

    log::info!(
        "[tokenjuice::ml] provisioning Kompress (one-time): venv={} model={}",
        venv_dir.display(),
        config.tokenjuice.ml_model_id
    );

    let bootstrap = PythonBootstrap::new(config.runtime_python.clone());
    let base = bootstrap
        .resolve()
        .await
        .context("resolving base python for kompress venv")?;

    run_step(
        &base.python_bin,
        &["-m", "venv", &venv_dir.to_string_lossy()],
        VENV_TIMEOUT,
        "create venv",
    )
    .await?;

    if !venv_python.exists() {
        bail!("venv created but interpreter missing at {}", venv_python.display());
    }

    run_step(
        &venv_python,
        &["-m", "pip", "install", "--upgrade", "pip"],
        PIP_TIMEOUT,
        "pip upgrade",
    )
    .await?;

    // CPU-only torch wheel to avoid pulling multi-GB CUDA builds.
    run_step(
        &venv_python,
        &[
            "-m",
            "pip",
            "install",
            "--index-url",
            "https://download.pytorch.org/whl/cpu",
            "torch",
        ],
        PIP_TIMEOUT,
        "pip install torch (cpu)",
    )
    .await?;

    run_step(
        &venv_python,
        &["-m", "pip", "install", "transformers", "tokenizers"],
        PIP_TIMEOUT,
        "pip install transformers",
    )
    .await?;

    tokio::fs::create_dir_all(&hf_home)
        .await
        .with_context(|| format!("creating hf home {}", hf_home.display()))?;

    tokio::fs::write(&marker, base.version.as_bytes())
        .await
        .with_context(|| format!("writing kompress ready marker {}", marker.display()))?;

    log::info!("[tokenjuice::ml] kompress provisioning complete");
    Ok(KompressRuntime {
        python_bin: venv_python,
        worker_script,
        hf_home,
    })
}

/// Cheap, network-free probe: is Kompress already provisioned on this host?
pub fn kompress_provisioned(config: &Config) -> bool {
    let venv_dir = kompress_cache_root(config).join("venv");
    let marker = venv_dir.join(".openhuman-kompress-ready");
    marker.exists() && venv_python_path(&venv_dir).exists()
}

async fn run_step(python_bin: &Path, args: &[&str], timeout: Duration, label: &str) -> Result<()> {
    log::debug!("[tokenjuice::ml] step `{label}`: {} {:?}", python_bin.display(), args);
    let mut cmd = Command::new(python_bin);
    cmd.args(args);
    cmd.kill_on_drop(true);

    let output = match tokio::time::timeout(timeout, cmd.output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(e).with_context(|| format!("spawning step `{label}`")),
        Err(_) => bail!("step `{label}` timed out after {:?}", timeout),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr
            .chars()
            .rev()
            .take(800)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        bail!("step `{label}` failed (status {}): {tail}", output.status);
    }
    Ok(())
}

fn venv_python_path(venv_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        venv_dir.join("Scripts").join("python.exe")
    } else {
        venv_dir.join("bin").join("python")
    }
}

/// Cache root for ML artefacts. Honours `runtime_python.cache_dir`, else the
/// user cache dir, else a workspace-relative fallback.
fn kompress_cache_root(config: &Config) -> PathBuf {
    let configured = config.runtime_python.cache_dir.trim();
    if !configured.is_empty() {
        return PathBuf::from(configured).join("tokenjuice-ml");
    }
    if let Some(user_cache) = dirs::cache_dir() {
        return user_cache.join("openhuman").join("tokenjuice-ml");
    }
    config.workspace_dir.join("tokenjuice").join("ml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn venv_python_path_is_platform_specific() {
        let p = venv_python_path(Path::new("/tmp/venv"));
        if cfg!(windows) {
            assert!(p.ends_with("Scripts/python.exe") || p.ends_with("Scripts\\python.exe"));
        } else {
            assert_eq!(p, PathBuf::from("/tmp/venv/bin/python"));
        }
    }

    #[test]
    fn cache_root_honours_configured_dir() {
        let mut cfg = Config::default();
        cfg.runtime_python.cache_dir = "/custom/py".to_string();
        assert_eq!(
            kompress_cache_root(&cfg),
            PathBuf::from("/custom/py").join("tokenjuice-ml")
        );
    }
}
