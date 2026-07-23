//! Business logic + RPC handlers for the `medulla_local` namespace.
//!
//! Thin surface over [`super::server`]: `status` reports the supervised serve
//! child's handshake state; `instruct` enqueues one instruction and returns the
//! synchronous receipt (§4.1). The subconscious tick path (§5.2) calls
//! [`instruct_tick`] directly rather than over RPC.

use serde::Deserialize;
use serde_json::{json, Value};
use tracing::warn;

use super::server;
use super::types::{InstructReceipt, MedullaLocalStatus};
use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::config::Config;
use crate::rpc::RpcOutcome;

#[derive(Debug, Deserialize)]
pub(crate) struct InstructParams {
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) meta: Value,
}

/// Enqueue one instruction against the supervised serve child.
pub async fn instruct_tick(
    config: &Config,
    message: &str,
    meta: Value,
) -> anyhow::Result<InstructReceipt> {
    let supervisor = server::ensure_started(config).await?;
    supervisor.instruct(message, meta).await
}

/// Snapshot the supervised serve child's status without forcing a spawn on the
/// failure path (a failed `ensure_started` still yields a well-formed
/// unavailable status).
pub async fn status(config: &Config) -> MedullaLocalStatus {
    match server::ensure_started(config).await {
        Ok(supervisor) => supervisor.snapshot().await,
        Err(error) => {
            warn!("[medulla_local] medulla_local.status: serve unavailable: {error:#}");
            MedullaLocalStatus {
                enabled: true,
                running: false,
                serve_version: None,
                session_id: None,
                ports: Vec::new(),
                message: Some(error.to_string()),
            }
        }
    }
}

pub(crate) async fn status_handler() -> Result<Value, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let status = status(&config).await;
    let payload = json!(status);
    let log = vec![format!(
        "medulla_local.status: running={} ports={}",
        status.running,
        status.ports.len()
    )];
    RpcOutcome::new(payload, log).into_cli_compatible_json()
}

pub(crate) async fn instruct_handler(params: InstructParams) -> Result<Value, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let meta = if params.meta.is_null() {
        json!({ "origin": "rpc" })
    } else {
        params.meta
    };
    let receipt = instruct_tick(&config, &params.message, meta)
        .await
        .map_err(|error| {
            warn!("[medulla_local] medulla_local.instruct failed: {error:#}");
            format!("{error:#}")
        })?;
    let payload = json!(receipt);
    let log = vec![format!(
        "medulla_local.instruct: instruction_id={} cycle_id={}",
        receipt.instruction_id, receipt.cycle_id
    )];
    RpcOutcome::new(payload, log).into_cli_compatible_json()
}
