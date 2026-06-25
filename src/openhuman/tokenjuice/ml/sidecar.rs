//! Long-lived stdio sidecar for the TokenJuice ML ("Kompress") compressor.
//!
//! Spawns `worker.py` under the provisioned venv interpreter, reads handshake
//! lines until `{"ready":true}` (tolerating `{"status":…}` progress keepalives
//! during the slow first-run model download), then issues one request per call
//! over a mutex-guarded stdin/stdout pair. The model loads once for the life of
//! the process; calls are line round-trips.
//!
//! A single worker + request queue (the mutex) — ModernBERT inference isn't
//! safe to call concurrently on one model instance, and one CPU process
//! saturates cores anyway. On a detected crash the sidecar respawns once, then
//! fails to the caller (which degrades to a native compressor).

use std::process::Stdio;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use super::protocol::{MlCompressResult, ReadyLine, WorkerRequest, WorkerResponse};
use super::provision::KompressRuntime;

/// Hard cap on the cold-start handshake (model download + load). Reset on every
/// progress line, so this bounds time-without-progress, not total download time.
const HANDSHAKE_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// Per-request timeout once the model is warm.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Runtime knobs the sidecar needs per request, mapped from `[tokenjuice]`.
#[derive(Debug, Clone)]
pub struct MlSidecarOpts {
    pub model_id: String,
    pub device: String,
    pub target_ratio: f64,
    pub max_input_chars: usize,
}

struct Inner {
    _child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

/// Handle to a running Kompress worker.
pub struct PythonCompressorSidecar {
    inner: Mutex<Inner>,
}

impl PythonCompressorSidecar {
    /// Spawn the worker and complete the readiness handshake.
    pub async fn spawn(runtime: &KompressRuntime, opts: &MlSidecarOpts) -> Result<Self> {
        let inner = Self::spawn_inner(runtime, opts).await?;
        Ok(Self {
            inner: Mutex::new(inner),
        })
    }

    async fn spawn_inner(runtime: &KompressRuntime, opts: &MlSidecarOpts) -> Result<Inner> {
        log::debug!(
            "[tokenjuice::ml] spawning kompress worker python={} script={}",
            runtime.python_bin.display(),
            runtime.worker_script.display()
        );
        let mut child = Command::new(&runtime.python_bin)
            .arg("-u")
            .arg(&runtime.worker_script)
            .env("TOKENJUICE_ML_MODEL_ID", &opts.model_id)
            .env("TOKENJUICE_ML_DEVICE", &opts.device)
            .env("HF_HOME", &runtime.hf_home)
            .env("HF_HUB_DISABLE_TELEMETRY", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| "spawning kompress worker process")?;

        let stdin = child.stdin.take().context("kompress child stdin missing")?;
        let stdout = child.stdout.take().context("kompress child stdout missing")?;
        let mut lines = BufReader::new(stdout).lines();

        // Handshake: read until ready, tolerating progress keepalives. Each line
        // resets the idle timer, so a long-but-progressing download survives.
        loop {
            let line = match tokio::time::timeout(HANDSHAKE_IDLE_TIMEOUT, lines.next_line()).await {
                Ok(Ok(Some(line))) => line,
                Ok(Ok(None)) => bail!("kompress worker exited before readiness handshake"),
                Ok(Err(e)) => return Err(e).context("reading kompress readiness line"),
                Err(_) => bail!("kompress worker handshake stalled (no progress)"),
            };
            let parsed: ReadyLine = match serde_json::from_str(&line) {
                Ok(p) => p,
                Err(_) => continue, // ignore stray stdout noise
            };
            if parsed.is_progress() {
                log::debug!(
                    "[tokenjuice::ml] handshake progress: {}",
                    parsed.status.as_deref().unwrap_or("")
                );
                continue;
            }
            if !parsed.ready {
                bail!(
                    "kompress worker failed to load model: {}",
                    parsed.error.unwrap_or_else(|| "unknown".into())
                );
            }
            log::info!(
                "[tokenjuice::ml] kompress worker ready model={} device={}",
                parsed.model.as_deref().unwrap_or("?"),
                parsed.device.as_deref().unwrap_or("?")
            );
            break;
        }

        Ok(Inner {
            _child: child,
            stdin,
            stdout: lines,
            next_id: 0,
        })
    }

    /// Compress `text`. On a detected crash, respawn once and retry; a second
    /// failure returns `Err` so the caller degrades to a native compressor.
    pub async fn compress(
        &self,
        text: &str,
        runtime: &KompressRuntime,
        opts: &MlSidecarOpts,
    ) -> Result<MlCompressResult> {
        match self.try_compress(text, opts).await {
            Ok(r) => Ok(r),
            Err(e) => {
                log::warn!("[tokenjuice::ml] worker error ({e:#}); respawning once");
                let fresh = Self::spawn_inner(runtime, opts).await?;
                *self.inner.lock().await = fresh;
                self.try_compress(text, opts).await
            }
        }
    }

    async fn try_compress(&self, text: &str, opts: &MlSidecarOpts) -> Result<MlCompressResult> {
        let mut guard = self.inner.lock().await;
        let id = guard.next_id;
        guard.next_id += 1;
        let id_str = id.to_string();

        let req = WorkerRequest::Compress {
            id: id_str.clone(),
            text,
            target_ratio: opts.target_ratio,
            max_input_chars: opts.max_input_chars,
        };
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');
        guard
            .stdin
            .write_all(line.as_bytes())
            .await
            .context("writing kompress request")?;
        guard.stdin.flush().await.context("flushing kompress request")?;

        loop {
            let next = tokio::time::timeout(REQUEST_TIMEOUT, guard.stdout.next_line()).await;
            let line = match next {
                Ok(Ok(Some(l))) => l,
                Ok(Ok(None)) => bail!("kompress worker closed mid-request"),
                Ok(Err(e)) => return Err(e).context("reading kompress response"),
                Err(_) => bail!("kompress request timed out"),
            };
            let resp: WorkerResponse = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("[tokenjuice::ml] unparseable worker line skipped: {e}");
                    continue;
                }
            };
            if resp.id.as_deref() != Some(id_str.as_str()) {
                continue;
            }
            if let Some(err) = resp.error {
                bail!("kompress error: {err}");
            }
            let text = resp
                .compressed_text
                .context("kompress response missing compressed_text")?;
            return Ok(MlCompressResult {
                text,
                stats: resp.stats,
            });
        }
    }
}
