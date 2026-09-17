//! Handlers for the Claude Code provider: version and auth probes, and its
//! full-access setting.

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{deserialize_params, to_json};
use crate::config::rpc as config_rpc;
use crate::core::all::ControllerFuture;
use crate::rpc::RpcOutcome;

#[derive(Debug, Deserialize)]
pub(super) struct InferenceClaudeCodeSetFullAccessParams {
    /// true → full access (`bypassPermissions` + full toolset); false → the
    /// default `acceptEdits` posture (file edits only).
    enabled: bool,
}

pub(super) fn handle_inference_claude_code_status(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let status = tokio::task::spawn_blocking(
            crate::inference::provider::claude_code::version_check::probe,
        )
        .await
        .map_err(|e| format!("claude_code_status join error: {e}"))?;
        to_json(RpcOutcome::new(status, vec![]))
    })
}

pub(super) fn handle_inference_claude_code_auth_status(
    _params: Map<String, Value>,
) -> ControllerFuture {
    Box::pin(async move {
        let auth = tokio::task::spawn_blocking(
            crate::inference::provider::claude_code::auth_status::probe,
        )
        .await
        .map_err(|e| format!("claude_code_auth_status join error: {e}"))?;
        to_json(RpcOutcome::new(auth, vec![]))
    })
}

pub(super) fn handle_inference_claude_code_settings(
    _params: Map<String, Value>,
) -> ControllerFuture {
    use crate::inference::provider::claude_code::settings;
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let settings = settings::load_for_config(&config);
        log::debug!(
            "[rpc][inference.claude_code_settings] full_access={}",
            settings.full_access
        );
        to_json(RpcOutcome::new(settings, vec![]))
    })
}

pub(super) fn handle_inference_claude_code_set_full_access(
    params: Map<String, Value>,
) -> ControllerFuture {
    use crate::inference::provider::claude_code::settings;
    Box::pin(async move {
        let p = deserialize_params::<InferenceClaudeCodeSetFullAccessParams>(params)?;
        let config = config_rpc::load_config_with_timeout().await?;
        let settings = settings::save_full_access_for_config(&config, p.enabled)
            .map_err(|e| format!("failed to persist claude code settings: {e}"))?;
        log::info!(
            "[rpc][inference.claude_code_set_full_access] persisted full_access={}",
            settings.full_access
        );
        to_json(RpcOutcome::new(settings, vec![]))
    })
}
