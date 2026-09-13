//! Handler for the `tools_composio_execute` controller schema.

use serde_json::{json, Map, Value};

use crate::config::rpc as config_rpc;
use crate::core::all::ControllerFuture;
use crate::rpc::RpcOutcome;

pub(super) fn handle_composio_execute(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "missing required `action`".to_string())?;
        let action_args = params.get("params").cloned();

        let config = config_rpc::load_config_with_timeout().await?;
        // Route through the mode-aware factory so direct-mode users
        // hit their personal Composio tenant when the Tauri shell
        // calls `tools.composio_execute` (e.g. onboarding-driven
        // flows). Pre-fix, the controller hard-bound to the
        // backend-only `build_composio_client` and silently 4xx'd for
        // direct-mode users (#1710). Mirrors
        // `composio::ops::composio_execute`.
        use crate::integrations::composio::client::{
            create_composio_client, direct_execute, ComposioClientKind,
        };
        let kind =
            create_composio_client(&config).map_err(|e| format!("tools.composio_execute: {e}"))?;
        tracing::debug!(
            action = %action,
            mode = %config.composio.mode,
            "[tools][composio_execute] executing action"
        );
        let resp = match kind {
            ComposioClientKind::Backend(client) => {
                tracing::debug!(action = %action, "[tools][composio_execute] branch=backend");
                client
                    .execute_tool(&action, action_args)
                    .await
                    .map_err(|e| format!("composio execute_tool (backend) failed: {e:#}"))?
            }
            ComposioClientKind::Direct(direct) => {
                tracing::debug!(action = %action, "[tools][composio_execute] branch=direct");
                direct_execute(
                    &direct,
                    &action,
                    action_args,
                    &config.composio.entity_id,
                    None,
                )
                .await
                .map_err(|e| format!("composio execute_tool (direct) failed: {e:#}"))?
            }
        };
        tracing::debug!(
            action = %action,
            successful = resp.successful,
            "[tools][composio_execute] complete"
        );

        let payload = json!({
            "successful": resp.successful,
            "data": resp.data,
            "error": resp.error,
            "cost_usd": resp.cost_usd,
            "markdown_formatted": resp.markdown_formatted,
        });
        let log = vec![format!(
            "tools.composio_execute: action={action} successful={}",
            resp.successful
        )];
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}
