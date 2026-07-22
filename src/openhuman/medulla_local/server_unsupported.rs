//! Non-unix stub for the medulla-local supervisor surface.
//!
//! The local serve transport is a unix domain socket, which `tokio::net`
//! only provides on unix targets. On other targets (Windows) the
//! `medulla-local` feature still compiles: this stub exposes the same public
//! surface `ops.rs` drives, with every entry point reporting a typed
//! unsupported-platform error instead of breaking the build. A portable
//! transport (e.g. stdio) can replace this stub later without touching the
//! callers.

use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use super::types::{HarnessStatus, InstructReceipt, MedullaLocalStatus};
use crate::openhuman::config::Config;

/// Typed marker for "this build's target has no unix-socket transport", so
/// callers can distinguish a platform limitation from a runtime failure.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("medulla-local is unavailable on this platform: the local serve transport requires unix domain sockets")]
pub struct UnsupportedPlatformError;

/// Stub supervisor: never connectable on this platform. [`ensure_started`]
/// always fails first, so these methods exist only to keep the call sites in
/// `ops.rs` compiling identically on every target.
pub struct MedullaSupervisor;

impl MedullaSupervisor {
    /// Always fails with [`UnsupportedPlatformError`].
    pub async fn instruct(&self, _message: &str, _meta: Value) -> Result<InstructReceipt> {
        Err(UnsupportedPlatformError.into())
    }

    /// Always fails with [`UnsupportedPlatformError`].
    pub async fn harness_status(&self) -> Result<HarnessStatus> {
        Err(UnsupportedPlatformError.into())
    }

    /// A well-formed not-running snapshot carrying the platform message.
    pub async fn snapshot(&self) -> MedullaLocalStatus {
        MedullaLocalStatus {
            enabled: true,
            running: false,
            serve_version: None,
            session_id: None,
            ports: Vec::new(),
            message: Some(UnsupportedPlatformError.to_string()),
        }
    }
}

/// Always reports the platform as unsupported (typed, downcastable): `status`
/// folds it into a well-formed not-running snapshot and `instruct` fails
/// cleanly, exactly like an unconfigured serve entry does on unix.
pub async fn ensure_started(_config: &Config) -> Result<Arc<MedullaSupervisor>> {
    Err(UnsupportedPlatformError.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ensure_started_reports_unsupported_platform() {
        let error = ensure_started(&Config::default())
            .await
            .expect_err("non-unix targets must not report a live supervisor");
        assert!(
            error.downcast_ref::<UnsupportedPlatformError>().is_some(),
            "error must be the typed platform marker: {error:#}"
        );
    }

    #[tokio::test]
    async fn stub_supervisor_snapshot_is_well_formed_and_not_running() {
        let snapshot = MedullaSupervisor.snapshot().await;
        assert!(snapshot.enabled);
        assert!(!snapshot.running);
        assert!(snapshot
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("unavailable on this platform"));
    }
}
