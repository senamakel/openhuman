//! Python bootstrap orchestrator.
//!
//! Today the bootstrap resolves a compatible system Python 3.12+ interpreter.
//! The type surface already reserves room for a managed distribution so callers
//! do not need to change once bundled/downloaded CPython lands.

use anyhow::{bail, Result};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::resolver::{detect_system_python, SystemPython};
use crate::openhuman::config::schema::RuntimePythonConfig;

/// Origin of the resolved interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonSource {
    /// Reused a compatible Python already available on the host.
    System,
    /// Reserved for a future managed CPython distribution.
    Managed,
}

/// Fully-resolved Python interpreter.
#[derive(Debug, Clone)]
pub struct ResolvedPython {
    /// Absolute path to the Python executable.
    pub python_bin: std::path::PathBuf,
    /// Normalized interpreter version, e.g. `3.12.4`.
    pub version: String,
    /// Where the interpreter came from.
    pub source: PythonSource,
}

/// Serialised bootstrap entrypoint for Python runtime resolution.
pub struct PythonBootstrap {
    config: RuntimePythonConfig,
    cached: Arc<Mutex<Option<ResolvedPython>>>,
}

impl PythonBootstrap {
    pub fn new(config: RuntimePythonConfig) -> Self {
        Self {
            config,
            cached: Arc::new(Mutex::new(None)),
        }
    }

    /// Peek at the memoized interpreter without triggering a probe.
    pub fn try_cached(&self) -> Option<ResolvedPython> {
        self.cached.try_lock().ok().and_then(|g| g.clone())
    }

    /// Resolve a Python 3.12+ interpreter. The first successful result is
    /// memoized for subsequent callers.
    pub async fn resolve(&self) -> Result<ResolvedPython> {
        let mut guard = self.cached.lock().await;
        if let Some(existing) = guard.as_ref() {
            tracing::debug!(
                version = %existing.version,
                source = ?existing.source,
                "[runtime_python::bootstrap] returning cached ResolvedPython"
            );
            return Ok(existing.clone());
        }

        if !self.config.enabled {
            bail!(
                "runtime_python is disabled (set runtime_python.enabled = true to use Python-backed integrations)"
            );
        }

        if self.config.prefer_system {
            if let Some(system) = detect_system_python(
                &self.config.minimum_version,
                empty_to_none(&self.config.preferred_command),
            ) {
                let resolved = resolve_from_system(system);
                *guard = Some(resolved.clone());
                return Ok(resolved);
            }
        }

        bail!(
            "no compatible Python interpreter found (need Python >= {}); managed runtime installation is not implemented yet",
            self.config.minimum_version
        );
    }

    /// Build a preconfigured child-process launcher for stdio-oriented Python
    /// workloads such as MCP servers.
    pub async fn spawn_stdio(
        &self,
        spec: &crate::openhuman::runtime_python::process::PythonLaunchSpec,
    ) -> Result<tokio::process::Child> {
        let resolved = self.resolve().await?;
        crate::openhuman::runtime_python::process::spawn_stdio_process(&resolved, spec)
    }
}

fn resolve_from_system(system: SystemPython) -> ResolvedPython {
    tracing::info!(
        path = %system.path.display(),
        version = %system.version,
        "[runtime_python::bootstrap] reusing compatible system python"
    );
    ResolvedPython {
        python_bin: system.path,
        version: system.version,
        source: PythonSource::System,
    }
}

fn empty_to_none(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}
